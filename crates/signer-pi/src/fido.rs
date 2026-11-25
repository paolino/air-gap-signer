//! A minimal FIDO2 (CTAP 2.1) client over Linux `hidraw`, enough for the
//! backup: create a credential with `hmac-secret` and ask the token for
//! the secret of a salt. Pure Rust, so the signer stays one static
//! binary (the usual crates need libudev).
//!
//! Touch only: no user verification, so the token's PIN is not needed;
//! the secret is then the token's no-UV secret, the same every time.

use ciborium::Value;
use std::io::{Read, Write};

const REPORT: usize = 64;
const BROADCAST: [u8; 4] = [0xff; 4];
const CMD_INIT: u8 = 0x86;
const CMD_CBOR: u8 = 0x90;
const CMD_KEEPALIVE: u8 = 0xbb;
const CMD_ERROR: u8 = 0xbf;
const MAKE_CREDENTIAL: u8 = 0x01;
const GET_ASSERTION: u8 = 0x02;
const CLIENT_PIN: u8 = 0x06;
const RP_ID: &str = "air-gap-signer";

#[derive(Debug, thiserror::Error)]
pub enum FidoError {
    #[error("no FIDO2 token found")]
    NoToken,
    #[error("token I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("token protocol: {0}")]
    Protocol(String),
    #[error("token said {0:#04x}")]
    Status(u8),
}

fn protocol(what: impl Into<String>) -> FidoError {
    FidoError::Protocol(what.into())
}

/// Split `data` into CTAPHID reports for channel `cid`, each prefixed
/// with the hidraw report id 0.
pub fn frames(cid: [u8; 4], cmd: u8, data: &[u8]) -> Vec<[u8; REPORT + 1]> {
    let mut out = Vec::new();
    let mut first = [0u8; REPORT + 1];
    first[1..5].copy_from_slice(&cid);
    first[5] = cmd;
    first[6..8].copy_from_slice(&(data.len() as u16).to_be_bytes());
    let n = data.len().min(REPORT - 7);
    first[8..8 + n].copy_from_slice(&data[..n]);
    out.push(first);
    for (seq, chunk) in data[n..].chunks(REPORT - 5).enumerate() {
        let mut f = [0u8; REPORT + 1];
        f[1..5].copy_from_slice(&cid);
        f[5] = seq as u8;
        f[6..6 + chunk.len()].copy_from_slice(chunk);
        out.push(f);
    }
    out
}

/// Read one CTAPHID message on `cid`, skipping keepalives (the token
/// waiting for a touch). Returns the command and its data.
pub fn receive(dev: &mut impl Read, cid: [u8; 4]) -> Result<(u8, Vec<u8>), FidoError> {
    let mut report = [0u8; REPORT];
    loop {
        dev.read_exact(&mut report)?;
        if report[..4] != cid {
            continue;
        }
        let cmd = report[4];
        if cmd == CMD_KEEPALIVE {
            continue;
        }
        let len = u16::from_be_bytes([report[5], report[6]]) as usize;
        let mut data = report[7..].to_vec();
        let mut seq = 0u8;
        while data.len() < len {
            dev.read_exact(&mut report)?;
            if report[..4] != cid {
                continue;
            }
            if report[4] != seq {
                return Err(protocol("continuation out of order"));
            }
            seq += 1;
            data.extend_from_slice(&report[5..]);
        }
        data.truncate(len);
        if cmd == CMD_ERROR {
            return Err(FidoError::Status(data.first().copied().unwrap_or(0)));
        }
        return Ok((cmd, data));
    }
}

/// A token open on a CTAPHID channel.
pub struct Device<D> {
    dev: D,
    cid: [u8; 4],
}

impl<D: Read + Write> Device<D> {
    /// Allocate a channel on `dev`.
    pub fn init(mut dev: D) -> Result<Self, FidoError> {
        let nonce: [u8; 8] = rand::random();
        for f in frames(BROADCAST, CMD_INIT, &nonce) {
            dev.write_all(&f)?;
        }
        loop {
            let (cmd, data) = receive(&mut dev, BROADCAST)?;
            if cmd == CMD_INIT && data.len() >= 12 && data[..8] == nonce {
                let cid = data[8..12].try_into().unwrap();
                return Ok(Self { dev, cid });
            }
        }
    }

    /// Send a CTAP2 command; the response's CBOR map.
    fn cbor(&mut self, command: u8, request: Option<Value>) -> Result<Value, FidoError> {
        let mut data = vec![command];
        if let Some(r) = request {
            ciborium::into_writer(&r, &mut data).map_err(|e| protocol(e.to_string()))?;
        }
        for f in frames(self.cid, CMD_CBOR, &data) {
            self.dev.write_all(&f)?;
        }
        let (cmd, data) = receive(&mut self.dev, self.cid)?;
        if cmd != CMD_CBOR {
            return Err(protocol(format!("reply command {cmd:#04x}")));
        }
        match data.split_first() {
            Some((0, rest)) => ciborium::from_reader(rest).map_err(|e| protocol(e.to_string())),
            Some((status, _)) => Err(FidoError::Status(*status)),
            None => Err(protocol("empty reply")),
        }
    }

    /// Create a credential with `hmac-secret` (needs a touch); its id.
    pub fn enrol(&mut self) -> Result<Vec<u8>, FidoError> {
        let request = map(vec![
            (int(1), Value::Bytes(vec![0; 32])),
            (int(2), map(vec![(text("id"), text(RP_ID))])),
            (
                int(3),
                map(vec![
                    (
                        text("id"),
                        Value::Bytes(rand::random::<[u8; 16]>().to_vec()),
                    ),
                    (text("name"), text("backup")),
                ]),
            ),
            (
                int(4),
                Value::Array(vec![map(vec![
                    (text("alg"), Value::Integer((-7).into())),
                    (text("type"), text("public-key")),
                ])]),
            ),
            (int(6), map(vec![(text("hmac-secret"), Value::Bool(true))])),
        ]);
        let reply = self.cbor(MAKE_CREDENTIAL, Some(request))?;
        let auth = bytes(field(&reply, 2)?)?;
        // rpIdHash 32 | flags 1 | counter 4 | aaguid 16 | idLen 2 | id
        let len = u16::from_be_bytes(
            auth.get(53..55)
                .ok_or_else(|| protocol("short authData"))?
                .try_into()
                .unwrap(),
        ) as usize;
        Ok(auth
            .get(55..55 + len)
            .ok_or_else(|| protocol("short credential id"))?
            .to_vec())
    }

    /// The token's secret for `salt` under credential `id` (needs a touch).
    pub fn secret(&mut self, id: &[u8], salt: &[u8; 32]) -> Result<[u8; 32], FidoError> {
        let shared = self.key_agreement()?;
        let salt_enc = shared.encrypt(salt);
        let salt_auth = shared.authenticate(&salt_enc);
        let request = map(vec![
            (int(1), text(RP_ID)),
            (int(2), Value::Bytes(vec![0; 32])),
            (
                int(3),
                Value::Array(vec![map(vec![
                    (text("id"), Value::Bytes(id.to_vec())),
                    (text("type"), text("public-key")),
                ])]),
            ),
            (
                int(4),
                map(vec![(
                    text("hmac-secret"),
                    map(vec![
                        (int(1), shared.platform_key()),
                        (int(2), Value::Bytes(salt_enc)),
                        (int(3), Value::Bytes(salt_auth)),
                        (int(4), int(2)),
                    ]),
                )]),
            ),
        ]);
        let reply = self.cbor(GET_ASSERTION, Some(request))?;
        let auth = bytes(field(&reply, 2)?)?;
        if auth.len() < 37 || auth[32] & 0x80 == 0 {
            return Err(protocol("no extension output"));
        }
        let ext: Value = ciborium::from_reader(&auth[37..]).map_err(|e| protocol(e.to_string()))?;
        let out = ext
            .as_map()
            .and_then(|m| m.iter().find(|(k, _)| k.as_text() == Some("hmac-secret")))
            .and_then(|(_, v)| v.as_bytes())
            .ok_or_else(|| protocol("no hmac-secret output"))?;
        shared
            .decrypt(out)?
            .try_into()
            .map_err(|_| protocol("hmac-secret is not 32 bytes"))
    }

    /// PIN/UV protocol 2 key agreement.
    fn key_agreement(&mut self) -> Result<Shared, FidoError> {
        let reply = self.cbor(
            CLIENT_PIN,
            Some(map(vec![(int(1), int(2)), (int(2), int(2))])),
        )?;
        Shared::agree(field(&reply, 1)?)
    }
}

/// The PIN/UV protocol 2 shared secret with the token.
struct Shared {
    hmac_key: [u8; 32],
    aes_key: [u8; 32],
    platform: p256::PublicKey,
}

impl Shared {
    fn agree(cose: &Value) -> Result<Self, FidoError> {
        use p256::elliptic_curve::sec1::FromEncodedPoint;
        let coord = |k: i64| -> Result<[u8; 32], FidoError> {
            bytes(field(cose, k)?)?
                .try_into()
                .map_err(|_| protocol("bad coordinate"))
        };
        let point = p256::EncodedPoint::from_affine_coordinates(
            &coord(-2)?.into(),
            &coord(-3)?.into(),
            false,
        );
        let token = Option::<p256::PublicKey>::from(p256::PublicKey::from_encoded_point(&point))
            .ok_or_else(|| protocol("bad token key"))?;
        let secret = p256::ecdh::EphemeralSecret::random(&mut rand::rngs::OsRng);
        let z = secret.diffie_hellman(&token);
        let hk = hkdf::Hkdf::<sha2::Sha256>::new(Some(&[0u8; 32]), z.raw_secret_bytes());
        let mut hmac_key = [0u8; 32];
        let mut aes_key = [0u8; 32];
        hk.expand(b"CTAP2 HMAC key", &mut hmac_key).unwrap();
        hk.expand(b"CTAP2 AES key", &mut aes_key).unwrap();
        Ok(Self {
            hmac_key,
            aes_key,
            platform: secret.public_key(),
        })
    }

    fn platform_key(&self) -> Value {
        use p256::elliptic_curve::sec1::ToEncodedPoint;
        let p = self.platform.to_encoded_point(false);
        map(vec![
            (int(1), int(2)),
            (int(3), Value::Integer((-25).into())),
            (Value::Integer((-1).into()), int(1)),
            (
                Value::Integer((-2).into()),
                Value::Bytes(p.x().unwrap().to_vec()),
            ),
            (
                Value::Integer((-3).into()),
                Value::Bytes(p.y().unwrap().to_vec()),
            ),
        ])
    }

    /// AES-256-CBC, random IV prepended, no padding (whole blocks).
    fn encrypt(&self, plain: &[u8]) -> Vec<u8> {
        use cbc::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyIvInit};
        let iv: [u8; 16] = rand::random();
        let mut buf = plain.to_vec();
        let n = buf.len();
        cbc::Encryptor::<aes::Aes256>::new(&self.aes_key.into(), &iv.into())
            .encrypt_padded_mut::<NoPadding>(&mut buf, n)
            .expect("whole blocks");
        [iv.to_vec(), buf].concat()
    }

    fn decrypt(&self, data: &[u8]) -> Result<Vec<u8>, FidoError> {
        use cbc::cipher::{block_padding::NoPadding, BlockDecryptMut, KeyIvInit};
        if data.len() < 32 || !data.len().is_multiple_of(16) {
            return Err(protocol("bad ciphertext length"));
        }
        let (iv, ct) = data.split_at(16);
        let mut buf = ct.to_vec();
        cbc::Decryptor::<aes::Aes256>::new(&self.aes_key.into(), iv.into())
            .decrypt_padded_mut::<NoPadding>(&mut buf)
            .map_err(|_| protocol("decrypt"))?;
        Ok(buf)
    }

    fn authenticate(&self, msg: &[u8]) -> Vec<u8> {
        use hmac::Mac;
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(&self.hmac_key).unwrap();
        mac.update(msg);
        mac.finalize().into_bytes().to_vec()
    }
}

fn int(n: i64) -> Value {
    Value::Integer(n.into())
}

fn text(s: &str) -> Value {
    Value::Text(s.into())
}

/// A CBOR map, entries given in CTAP2 canonical order.
fn map(entries: Vec<(Value, Value)>) -> Value {
    Value::Map(entries)
}

fn field(v: &Value, key: i64) -> Result<&Value, FidoError> {
    v.as_map()
        .and_then(|m| m.iter().find(|(k, _)| k.as_integer() == Some(key.into())))
        .map(|(_, v)| v)
        .ok_or_else(|| protocol(format!("missing field {key}")))
}

fn bytes(v: &Value) -> Result<&[u8], FidoError> {
    v.as_bytes()
        .map(|b| b.as_slice())
        .ok_or_else(|| protocol("expected bytes"))
}

/// The first `hidraw` device whose report descriptor declares the FIDO
/// usage page (0xF1D0).
pub fn find_token(sys: &std::path::Path) -> Option<std::path::PathBuf> {
    let mut names: Vec<_> = std::fs::read_dir(sys)
        .ok()?
        .flatten()
        .map(|e| e.file_name())
        .collect();
    names.sort();
    names.into_iter().find_map(|n| {
        let desc = std::fs::read(sys.join(&n).join("device/report_descriptor")).ok()?;
        desc.windows(3)
            .any(|w| w == [0x06, 0xd0, 0xf1])
            .then(|| std::path::Path::new("/dev").join(n))
    })
}

/// Open the first FIDO2 token.
pub fn open() -> Result<Device<std::fs::File>, FidoError> {
    let path = find_token(std::path::Path::new("/sys/class/hidraw")).ok_or(FidoError::NoToken)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)?;
    Device::init(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_message_is_split_and_reassembled() {
        let cid = [1, 2, 3, 4];
        let data: Vec<u8> = (0..200u8).collect();
        let fs = frames(cid, CMD_CBOR, &data);
        assert_eq!(fs.len(), 4, "57 + 59 + 59 + 25");
        assert!(fs.iter().all(|f| f[0] == 0), "report id 0 first");
        // What the token would send back: the same reports without the id,
        // with a keepalive and another channel's report mixed in.
        let mut wire = Vec::new();
        let mut keepalive = [0u8; REPORT];
        keepalive[..4].copy_from_slice(&cid);
        keepalive[4] = CMD_KEEPALIVE;
        keepalive[6] = 1;
        wire.extend_from_slice(&keepalive);
        wire.extend_from_slice(&[9u8; REPORT]);
        for f in &fs {
            wire.extend_from_slice(&f[1..]);
        }
        let (cmd, back) = receive(&mut wire.as_slice(), cid).unwrap();
        assert_eq!((cmd, back), (CMD_CBOR, data));
    }

    #[test]
    fn a_token_error_is_reported() {
        let cid = [1, 2, 3, 4];
        let wire: Vec<u8> = frames(cid, CMD_ERROR, &[0x06])[0][1..].to_vec();
        assert!(matches!(
            receive(&mut wire.as_slice(), cid),
            Err(FidoError::Status(6))
        ));
    }

    #[test]
    fn the_fido_usage_page_finds_the_token() {
        let dir = std::env::temp_dir().join(format!("fido-sys-{}", std::process::id()));
        for (name, desc) in [
            ("hidraw0", vec![0x05, 0x01, 0x09, 0x06]),
            ("hidraw3", vec![0x06, 0xd0, 0xf1, 0x09, 0x01]),
        ] {
            std::fs::create_dir_all(dir.join(name).join("device")).unwrap();
            std::fs::write(dir.join(name).join("device/report_descriptor"), desc).unwrap();
        }
        assert_eq!(find_token(&dir), Some("/dev/hidraw3".into()));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
