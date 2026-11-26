//! The host side of an SE05x AES-key session (GlobalPlatform SCP03,
//! Amendment D): session keys, cryptograms and secure messaging. This is
//! the part the paired phone would hold — the controller only relays the
//! bytes it produces, so it cannot change a command the phone protected.
//!
//! Security levels: C-MAC alone, or the full level (C-MAC, C-DEC, R-MAC,
//! R-ENC).

use aes::Aes128;
use cmac::{Cmac, Mac};

/// EXTERNAL AUTHENTICATE security level: command MAC only.
pub const C_MAC: u8 = 0x01;
/// Command MAC and encryption, response MAC and encryption.
pub const FULL: u8 = 0x33;

fn cmac(key: &[u8; 16], parts: &[&[u8]]) -> [u8; 16] {
    let mut mac = <Cmac<Aes128> as Mac>::new(key.into());
    for p in parts {
        mac.update(p);
    }
    mac.finalize().into_bytes().into()
}

/// SCP03 key derivation (NIST SP 800-108 counter mode with CMAC):
/// `label (11 zero bytes, constant) | 00 | L (2 bytes) | counter 01 |
/// context (host challenge | card challenge)`.
fn derive(key: &[u8; 16], constant: u8, bits: u16, host: &[u8; 8], card: &[u8; 8]) -> [u8; 16] {
    let mut input = [0u8; 32];
    input[11] = constant;
    input[13..15].copy_from_slice(&bits.to_be_bytes());
    input[15] = 0x01;
    input[16..24].copy_from_slice(host);
    input[24..32].copy_from_slice(card);
    cmac(key, &[&input])
}

/// An authenticated session, from the host's side.
#[derive(Clone)]
pub struct Session {
    s_enc: [u8; 16],
    s_mac: [u8; 16],
    s_rmac: [u8; 16],
    /// MAC chaining value: every command's MAC covers all before it.
    chain: [u8; 16],
    /// Encryption counter: 1 for the first command after authentication.
    counter: u128,
    level: u8,
}

/// What the host sends to finish authentication.
pub struct Authentication {
    pub session: Session,
    /// The EXTERNAL AUTHENTICATE command, MAC included.
    pub apdu: Vec<u8>,
}

#[derive(Debug, PartialEq)]
pub enum Scp03Error {
    /// The chip's cryptogram is wrong: it does not hold this key.
    CardCryptogram,
    /// A response whose MAC or padding is wrong.
    Response,
}

impl Session {
    /// Check the chip's answer to INITIALIZE UPDATE and produce the
    /// EXTERNAL AUTHENTICATE command for `level`.
    pub fn authenticate(
        key: &[u8; 16],
        host_challenge: &[u8; 8],
        card_challenge: &[u8; 8],
        card_cryptogram: &[u8; 8],
        level: u8,
    ) -> Result<Authentication, Scp03Error> {
        let s_mac = derive(key, 0x06, 128, host_challenge, card_challenge);
        let card = derive(&s_mac, 0x00, 64, host_challenge, card_challenge);
        if card[..8] != card_cryptogram[..] {
            return Err(Scp03Error::CardCryptogram);
        }
        let host = derive(&s_mac, 0x01, 64, host_challenge, card_challenge);
        let mut session = Session {
            s_enc: derive(key, 0x04, 128, host_challenge, card_challenge),
            s_mac,
            s_rmac: derive(key, 0x07, 128, host_challenge, card_challenge),
            chain: [0; 16],
            counter: 1,
            // EXTERNAL AUTHENTICATE itself is MACed only.
            level: C_MAC,
        };
        let apdu = session.wrap(&[0x80, 0x82, level, 0x00], &host[..8], None);
        session.counter = 1;
        session.level = level;
        Ok(Authentication { session, apdu })
    }

    /// `header` (CLA INS P1 P2) and `data` as a protected command: data
    /// encrypted when the level says so, CLA with the secure-messaging
    /// bit, Lc counting the 8 MAC bytes, and the MAC over the chaining
    /// value, the header, Lc and the (encrypted) data.
    pub fn wrap(&mut self, header: &[u8; 4], data: &[u8], le: Option<u8>) -> Vec<u8> {
        let data = if self.level & 0x02 != 0 && !data.is_empty() {
            self.encrypt(data)
        } else {
            data.to_vec()
        };
        let mut apdu = vec![header[0] | 0x04, header[1], header[2], header[3]];
        let lc = data.len() + 8;
        assert!(lc <= 255, "short APDUs only");
        apdu.push(lc as u8);
        apdu.extend_from_slice(&data);
        let mac = cmac(&self.s_mac, &[&self.chain, &apdu]);
        self.chain = mac;
        apdu.extend_from_slice(&mac[..8]);
        apdu.extend(le);
        self.counter += 1;
        apdu
    }

    /// Like `wrap`, in extended-length form: Lc as `00 hi lo` (covered by
    /// the MAC) and Le as two bytes.
    pub fn wrap_extended(&mut self, header: &[u8; 4], data: &[u8], le: Option<u16>) -> Vec<u8> {
        let data = if self.level & 0x02 != 0 && !data.is_empty() {
            self.encrypt(data)
        } else {
            data.to_vec()
        };
        let mut apdu = vec![header[0] | 0x04, header[1], header[2], header[3], 0];
        apdu.extend_from_slice(&((data.len() + 8) as u16).to_be_bytes());
        apdu.extend_from_slice(&data);
        let mac = cmac(&self.s_mac, &[&self.chain, &apdu]);
        self.chain = mac;
        apdu.extend_from_slice(&mac[..8]);
        if let Some(le) = le {
            apdu.extend_from_slice(&le.to_be_bytes());
        }
        self.counter += 1;
        apdu
    }

    /// The data of a response to the last wrapped command: its R-MAC
    /// checked (over the data and status 90 00), then decrypted.
    pub fn unwrap(&self, reply: &[u8]) -> Result<Vec<u8>, Scp03Error> {
        use cbc::cipher::{block_padding::Iso7816, BlockDecryptMut, KeyIvInit};
        if self.level & 0x10 == 0 {
            return Ok(reply.to_vec());
        }
        let cut = reply.len().checked_sub(8).ok_or(Scp03Error::Response)?;
        let (data, rmac) = reply.split_at(cut);
        let expected = cmac(&self.s_rmac, &[&self.chain, data, &[0x90, 0x00]]);
        if expected[..8] != *rmac {
            return Err(Scp03Error::Response);
        }
        if self.level & 0x20 == 0 || data.is_empty() {
            return Ok(data.to_vec());
        }
        // The response IV uses the counter of the command it answers.
        let iv = self.iv(self.counter - 1, true);
        cbc::Decryptor::<Aes128>::new(&self.s_enc.into(), &iv.into())
            .decrypt_padded_vec_mut::<Iso7816>(data)
            .map_err(|_| Scp03Error::Response)
    }

    /// The IV for `counter`: the counter block (first byte 80 for a
    /// response) encrypted under S-ENC.
    fn iv(&self, counter: u128, response: bool) -> [u8; 16] {
        use aes::cipher::{BlockEncrypt, KeyInit};
        let mut block = counter.to_be_bytes();
        if response {
            block[0] = 0x80;
        }
        let mut b = block.into();
        Aes128::new(&self.s_enc.into()).encrypt_block(&mut b);
        b.into()
    }

    /// Command data padded (80 00…) and encrypted under S-ENC.
    fn encrypt(&self, data: &[u8]) -> Vec<u8> {
        use cbc::cipher::{block_padding::Iso7816, BlockEncryptMut, KeyIvInit};
        cbc::Encryptor::<Aes128>::new(&self.s_enc.into(), &self.iv(self.counter, false).into())
            .encrypt_padded_vec_mut::<Iso7816>(data)
    }
}

/// A command APDU, short or extended, as `(header, data, le)` with `le`
/// in short form (`0` meaning up to 256 bytes): the form `wrap` sends.
/// `None` when the bytes are not one APDU or the data does not fit a
/// short command.
pub fn split(apdu: &[u8]) -> Option<([u8; 4], &[u8], Option<u8>)> {
    let header: [u8; 4] = apdu.get(..4)?.try_into().ok()?;
    let body = &apdu[4..];
    let short_le = |le: usize| Some(if le == 0 || le >= 256 { 0 } else { le as u8 });
    match body.len() {
        0 => Some((header, &[], None)),
        1 => Some((header, &[], Some(body[0]))),
        // Extended: 00 Lc(2) data [Le(2)], or 00 Le(2) with no data.
        _ if body[0] == 0 && body.len() >= 3 => {
            let n = u16::from_be_bytes([body[1], body[2]]) as usize;
            if body.len() == 3 {
                return Some((header, &[], short_le(n)));
            }
            let data = body.get(3..3 + n)?;
            if data.len() > 255 - 8 {
                return None;
            }
            match &body[3 + n..] {
                [] => Some((header, data, None)),
                [hi, lo] => Some((
                    header,
                    data,
                    short_le(u16::from_be_bytes([*hi, *lo]) as usize),
                )),
                _ => None,
            }
        }
        _ => {
            let lc = body[0] as usize;
            let data = body.get(1..1 + lc)?;
            match &body[1 + lc..] {
                [] => Some((header, data, None)),
                [le] => Some((header, data, Some(*le))),
                _ => None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmac_matches_rfc_4493() {
        // RFC 4493 example 2: AES-128 CMAC of a 16-byte message.
        let key: [u8; 16] = hex::decode("2b7e151628aed2a6abf7158809cf4f3c")
            .unwrap()
            .try_into()
            .unwrap();
        let msg = hex::decode("6bc1bee22e409f96e93d7e117393172a").unwrap();
        assert_eq!(
            hex::encode(cmac(&key, &[&msg])),
            "070a16b46b4d4144f79bdd9dd04a287c"
        );
    }

    #[test]
    fn a_wrong_card_cryptogram_is_refused() {
        let r = Session::authenticate(&[1; 16], &[2; 8], &[3; 8], &[0; 8], C_MAC);
        assert!(matches!(r, Err(Scp03Error::CardCryptogram)));
    }

    fn test_session(level: u8) -> Session {
        Session {
            s_enc: [6; 16],
            s_mac: [5; 16],
            s_rmac: [7; 16],
            chain: [0; 16],
            counter: 1,
            level,
        }
    }

    #[test]
    fn the_full_level_encrypts_the_command_data() {
        let mut s = test_session(FULL);
        let secret = b"a 32-byte digest to be signed...";
        let apdu = s.wrap(&[0x80, 0x01, 0, 0], secret, Some(0));
        assert!(
            !apdu.windows(secret.len()).any(|w| w == secret),
            "data in clear"
        );
        assert_eq!(apdu[4] as usize, 48 + 8, "padded to blocks, plus the MAC");
        let mut t = test_session(FULL);
        t.counter = 2;
        assert_ne!(
            apdu,
            t.wrap(&[0x80, 0x01, 0, 0], secret, Some(0)),
            "the counter changes the IV"
        );
    }

    #[test]
    fn a_response_with_a_wrong_mac_is_refused() {
        let s = test_session(FULL);
        assert_eq!(s.unwrap(&[0; 24]), Err(Scp03Error::Response));
        assert_eq!(s.unwrap(&[0; 3]), Err(Scp03Error::Response));
    }

    #[test]
    fn each_mac_depends_on_the_ones_before() {
        let mut a = test_session(C_MAC);
        let mut b = test_session(C_MAC);
        let first = a.wrap(&[0x80, 0x01, 0, 0], b"x", None);
        assert_eq!(first, b.wrap(&[0x80, 0x01, 0, 0], b"x", None));
        let again = a.wrap(&[0x80, 0x01, 0, 0], b"x", None);
        assert_ne!(first, again, "the same command twice gets a different MAC");
        assert_eq!(again[0], 0x84, "secure messaging bit");
        assert_eq!(again[4] as usize, 1 + 8, "Lc counts the MAC");
    }

    #[test]
    fn apdus_split_into_header_data_and_le() {
        assert_eq!(
            split(&[0x80, 1, 2, 3]),
            Some(([0x80, 1, 2, 3], &[][..], None))
        );
        assert_eq!(
            split(&[0x80, 1, 2, 3, 2, 9, 9, 0]),
            Some(([0x80, 1, 2, 3], &[9, 9][..], Some(0)))
        );
        assert_eq!(split(&[0x80, 1, 2, 3, 2, 9]), None);
        // Extended: 00 Lc=0002 data Le=0000 → short with Le 0.
        assert_eq!(
            split(&[0x80, 1, 2, 3, 0, 0, 2, 9, 9, 0, 0]),
            Some(([0x80, 1, 2, 3], &[9, 9][..], Some(0)))
        );
        assert_eq!(
            split(&[0x80, 1, 2, 3, 0, 0, 2, 9, 9]),
            Some(([0x80, 1, 2, 3], &[9, 9][..], None))
        );
        assert_eq!(
            split(&[0x80, 1, 2, 3, 0, 0, 3, 9, 9]),
            None,
            "short of data"
        );
    }
}
