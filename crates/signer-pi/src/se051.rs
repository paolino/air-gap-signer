//! The SE051 secure element on the Pi's I2C bus, through Nitrokey's
//! `se05x` driver (T=1 over I2C and the SE05x applet APDUs).

use embedded_hal::i2c::{Error as _, ErrorKind, ErrorType, I2c, NoAcknowledgeSource, Operation};
use linux_embedded_hal::{Delay, I2cdev};
use se05x::se05x::commands::{
    CheckObjectExists, DeleteSecureObject, EddsaSign, GetRandom, GetVersion, ReadObject, WriteEcKey,
};
use se05x::se05x::Se05X;
use se05x::se05x::{EcCurve, ObjectId, P1KeyType};
use signer_core::backup::Seed;
use signer_hal::HalError;

/// The SE051's I2C address.
pub const ADDRESS: u8 = 0x48;

/// Linux I2C with errors reduced to `embedded-hal` kinds, which the
/// driver needs to tell a busy chip (NACK) from a real failure.
pub struct Bus(I2cdev);

impl ErrorType for Bus {
    type Error = ErrorKind;
}

impl I2c for Bus {
    fn transaction(&mut self, address: u8, ops: &mut [Operation<'_>]) -> Result<(), ErrorKind> {
        self.0.transaction(address, ops).map_err(|e| kind(&e))
    }
}

/// The SE051 NACKs while it is busy and the driver retries. The Pi's I2C
/// controller reports that NACK as `EREMOTEIO` on a write and as `EIO` on
/// a read (its incomplete-transfer check overrides the NACK; seen in the
/// `i2c_bcm2835` debug trace: status `DONE ERR`, failure `0x400`), which
/// the Linux HAL turns into `Other` and `Bus`. Report both as the NACK
/// they are; the driver's retry limit still ends a real fault.
fn kind(e: &linux_embedded_hal::I2CError) -> ErrorKind {
    use i2cdev::linux::LinuxI2CError;
    const EIO: i32 = 5;
    const EREMOTEIO: i32 = 121;
    let errno = match e.inner() {
        LinuxI2CError::Errno(n) => Some(*n),
        LinuxI2CError::Io(io) => io.raw_os_error(),
    };
    match errno {
        Some(EIO | EREMOTEIO) => ErrorKind::NoAcknowledge(NoAcknowledgeSource::Address),
        _ => e.kind(),
    }
}

pub struct Se051 {
    chip: Se05X<se05x::embedded_hal::Hal10<Bus>, se05x::embedded_hal::Hal10<Delay>>,
}

fn err(what: &str, e: impl core::fmt::Debug) -> HalError {
    HalError::Storage(format!("SE051 {what}: {e:?}"))
}

impl Se051 {
    /// Open the chip on `bus` (e.g. `/dev/i2c-1`) and select its applet.
    pub fn open(bus: &str) -> Result<Self, HalError> {
        let dev = I2cdev::new(bus).map_err(|e| err("bus", e))?;
        let mut chip = Se05X::new_hal_10(Bus(dev), ADDRESS, Delay);
        // A session that failed mid-exchange leaves the chip out of step
        // for the next one; the handshake resynchronises it. The driver
        // can panic on a malformed answer (it divides by a timing value
        // the chip sends): a faulty chip is an error, not a crash.
        let mut last = None;
        for _ in 0..3 {
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| chip.enable())) {
                Ok(Ok(_)) => return Ok(Self { chip }),
                Ok(Err(e)) => last = Some(format!("{e:?}")),
                Err(_) => {
                    return Err(HalError::Storage(
                        "SE051 enable: malformed answer from the chip".into(),
                    ))
                }
            }
        }
        Err(err("enable", last))
    }

    /// The applet version as `major.minor.patch`.
    pub fn version(&mut self) -> Result<String, HalError> {
        let buf = &mut [0u8; 64];
        let v = self
            .chip
            .run_command(&GetVersion {}, buf)
            .map_err(|e| err("version", e))?
            .version_info;
        Ok(format!("{v:?}"))
    }

    /// The driver itself, for commands not wrapped here.
    pub fn chip(
        &mut self,
    ) -> &mut Se05X<se05x::embedded_hal::Hal10<Bus>, se05x::embedded_hal::Hal10<Delay>> {
        &mut self.chip
    }

    /// `n` random bytes from the chip's generator.
    pub fn random(&mut self, n: u16) -> Result<Vec<u8>, HalError> {
        let buf = &mut [0u8; 512];
        let r = self
            .chip
            .run_command(&GetRandom { length: n.into() }, buf)
            .map_err(|e| err("random", e))?;
        check_random(r.data, n)?;
        Ok(r.data.to_vec())
    }
}

/// The driver does not check that the chip returned as many random
/// bytes as asked; a short answer is an error.
fn check_random(data: &[u8], asked: u16) -> Result<(), HalError> {
    if data.len() != usize::from(asked) {
        return Err(HalError::Storage(format!(
            "SE051 random: {} bytes for {asked} asked",
            data.len()
        )));
    }
    Ok(())
}

/// The longest message the chip signs. Longer commands need T=1
/// chaining, which fails with the `se05x` driver on the rig (250 bytes
/// signed, 255 lost the chip); Cardano signs 32-byte hashes.
pub const MAX_MESSAGE: usize = 250;

/// Refuse what the chip cannot sign, before it is sent.
pub fn check_message(message: &[u8]) -> Result<(), HalError> {
    if message.len() > MAX_MESSAGE {
        return Err(HalError::Storage(format!(
            "message of {} bytes: the SE051 signs at most {MAX_MESSAGE}",
            message.len()
        )));
    }
    Ok(())
}

/// Key slots for signing keys.
pub const SLOTS: u8 = 4;

fn slot_id(slot: u8) -> ObjectId {
    ObjectId([0x7D, 0xA0, 0x01, slot])
}

/// A signature as the chip returns it — R and S each little-endian — in
/// RFC 8032 order.
pub fn from_chip(signature: &[u8]) -> Result<[u8; 64], HalError> {
    if signature.len() != 64 {
        return Err(HalError::Storage(format!(
            "SE051 signature of {} bytes",
            signature.len()
        )));
    }
    let mut out = [0u8; 64];
    for (half, chip) in out.chunks_mut(32).zip(signature.chunks(32)) {
        half.copy_from_slice(chip);
        half.reverse();
    }
    Ok(out)
}

impl Se051 {
    /// A fresh seed: the chip's random bytes mixed with the Pi's, so
    /// neither source alone decides the key.
    pub fn fresh_seed(&mut self) -> Result<Seed, HalError> {
        use rand::RngCore;
        let chip = self.random(32)?;
        let mut seed = Seed::new([0u8; 32]);
        rand::rngs::OsRng.fill_bytes(seed.as_mut());
        for (s, c) in seed.iter_mut().zip(chip) {
            *s ^= c;
        }
        Ok(seed)
    }

    /// Load `seed` into an empty `slot`; its public key (computed here:
    /// the chip stores whatever public key it is given).
    pub fn put_key(&mut self, slot: u8, seed: &Seed) -> Result<[u8; 32], HalError> {
        if slot >= SLOTS {
            return Err(HalError::Storage(format!("no key slot {slot}")));
        }
        if self.public_key(slot)?.is_some() {
            return Err(HalError::Storage(format!("key slot {slot} is in use")));
        }
        let public = ed25519_dalek::SigningKey::from_bytes(seed)
            .verifying_key()
            .to_bytes();
        let buf = &mut [0u8; 128];
        self.chip
            .run_command(
                &WriteEcKey {
                    transient: false,
                    is_auth: false,
                    key_type: Some(P1KeyType::KeyPair),
                    policy: None,
                    max_attempts: None,
                    object_id: slot_id(slot),
                    curve: Some(EcCurve::IdEccEd25519),
                    private_key: Some(seed.as_slice()),
                    public_key: Some(&public),
                },
                buf,
            )
            .map_err(|e| err("write key", e))?;
        Ok(public)
    }

    /// The public key stored with `slot`'s key, if the slot is in use.
    pub fn public_key(&mut self, slot: u8) -> Result<Option<[u8; 32]>, HalError> {
        let buf = &mut [0u8; 128];
        let exists = self
            .chip
            .run_command(
                &CheckObjectExists {
                    object_id: slot_id(slot),
                },
                buf,
            )
            .map_err(|e| err("check slot", e))?
            .result
            .is_success();
        if !exists {
            return Ok(None);
        }
        let read = ReadObject {
            object_id: slot_id(slot),
            offset: None,
            length: None,
            rsa_key_component: None,
        };
        let data = self
            .chip
            .run_command(&read, buf)
            .map_err(|e| err("read public key", e))?
            .data;
        Ok(Some(data.try_into().map_err(|_| {
            HalError::Storage("SE051 public key size".into())
        })?))
    }

    /// Sign `message` with `slot`'s key. The signature is checked against
    /// the slot's public key before it is returned.
    pub fn sign(&mut self, slot: u8, message: &[u8]) -> Result<[u8; 64], HalError> {
        check_message(message)?;
        let public = self
            .public_key(slot)?
            .ok_or_else(|| HalError::Storage(format!("key slot {slot} is empty")))?;
        let buf = &mut [0u8; 256];
        let chip = self
            .chip
            .run_command(
                &EddsaSign {
                    key_id: slot_id(slot),
                    data: message,
                },
                buf,
            )
            .map_err(|e| err("sign", e))?;
        let signature = from_chip(chip.signature)?;
        verify(&public, message, &signature)?;
        Ok(signature)
    }

    /// Erase `slot`'s key.
    pub fn delete_key(&mut self, slot: u8) -> Result<(), HalError> {
        let buf = &mut [0u8; 64];
        self.chip
            .run_command(
                &DeleteSecureObject {
                    object_id: slot_id(slot),
                },
                buf,
            )
            .map_err(|e| err("delete key", e))?;
        Ok(())
    }
}

/// Refuse a signature that does not verify under `public`.
pub fn verify(public: &[u8; 32], message: &[u8], signature: &[u8; 64]) -> Result<(), HalError> {
    let key = ed25519_dalek::VerifyingKey::from_bytes(public).map_err(|e| err("public key", e))?;
    key.verify_strict(message, &ed25519_dalek::Signature::from_bytes(signature))
        .map_err(|_| HalError::Storage("SE051 signature does not verify: refused".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // RFC 8032 test 1, and what the rig's SE051 returned for it.
    const PUBLIC: &str = "d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a";
    const RFC_SIG: &str = "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b";
    const CHIP_SIG: &str = "5501492265e073d874d9e5b81e7f87848a826e80cce2869072ac60c3004356e50b107a8e4341516524be5b59f0f55bd26bb4f91c70391ec6ac3ba3901582b85f";

    fn public() -> [u8; 32] {
        hex::decode(PUBLIC).unwrap().try_into().unwrap()
    }

    #[test]
    fn the_chips_signature_is_the_rfc_signature() {
        let sig = from_chip(&hex::decode(CHIP_SIG).unwrap()).unwrap();
        assert_eq!(hex::encode(sig), RFC_SIG);
        assert!(verify(&public(), b"", &sig).is_ok());
    }

    #[test]
    fn a_signature_that_does_not_verify_is_refused() {
        let mut sig = from_chip(&hex::decode(CHIP_SIG).unwrap()).unwrap();
        assert!(verify(&public(), b"other", &sig).is_err(), "other message");
        sig[0] ^= 1;
        assert!(verify(&public(), b"", &sig).is_err(), "damaged");
        let raw: [u8; 64] = hex::decode(CHIP_SIG).unwrap().try_into().unwrap();
        assert!(
            verify(&public(), b"", &raw).is_err(),
            "unconverted chip bytes"
        );
        assert!(from_chip(&[0; 63]).is_err());
    }

    #[test]
    fn a_short_random_answer_is_refused() {
        assert!(check_random(&[0; 16], 16).is_ok());
        assert!(check_random(&[0; 1], 16).is_err());
        assert!(check_random(&[0; 17], 16).is_err());
    }

    #[test]
    fn messages_the_chip_cannot_sign_are_refused_first() {
        assert!(check_message(&[0; MAX_MESSAGE]).is_ok());
        assert!(check_message(&[0; MAX_MESSAGE + 1]).is_err());
    }
}
