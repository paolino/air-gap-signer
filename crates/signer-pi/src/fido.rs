//! A minimal FIDO2 (CTAP 2.1) client over Linux `hidraw`, enough for the
//! backup: create a credential with `hmac-secret` and ask the token for
//! the secret of a salt. Pure Rust, so the signer stays one static
//! binary (the usual crates need libudev).
//!
//! Every operation proves the token's PIN (a PIN/UV auth token with
//! make-credential and get-assertion permissions for this RP), so the
//! secret is the token's user-verified one: a token and a backup stick
//! without the PIN open nothing.

use ciborium::Value;
use std::io::{Read, Write};

const REPORT: usize = 64;
/// The longest CTAPHID message: an initial report and 128 continuations.
const MAX_MESSAGE: usize = (REPORT - 7) + 128 * (REPORT - 5);
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
    #[error("wrong token PIN")]
    WrongPin,
    #[error("too many wrong PINs: unplug the token and plug it back in")]
    PinAuthBlocked,
    #[error("token PIN blocked: only a token reset unblocks it, and a reset erases its credentials and the backup it holds")]
    PinBlocked,
    #[error("token said {0:#04x}")]
    Status(u8),
}

fn protocol(what: impl Into<String>) -> FidoError {
    FidoError::Protocol(what.into())
}

/// Split `data` into CTAPHID reports for channel `cid`, each prefixed
/// with the hidraw report id 0.
pub fn frames(cid: [u8; 4], cmd: u8, data: &[u8]) -> Result<Vec<[u8; REPORT + 1]>, FidoError> {
    if data.len() > MAX_MESSAGE {
        return Err(protocol(format!("message of {} bytes", data.len())));
    }
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
    Ok(out)
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
        if len > MAX_MESSAGE {
            return Err(protocol(format!("token message of {len} bytes")));
        }
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
    /// Large-blob fragment size: the token's message limit less 64.
    fragment: usize,
}

impl<D: Read + Write> Device<D> {
    /// Allocate a channel on `dev`.
    pub fn init(mut dev: D) -> Result<Self, FidoError> {
        let nonce: [u8; 8] = rand::random();
        for f in frames(BROADCAST, CMD_INIT, &nonce)? {
            dev.write_all(&f)?;
        }
        loop {
            let (cmd, data) = receive(&mut dev, BROADCAST)?;
            if cmd == CMD_INIT && data.len() >= 12 && data[..8] == nonce {
                let cid = data[8..12].try_into().unwrap();
                let mut device = Self {
                    dev,
                    cid,
                    fragment: fragment_size(None),
                };
                let info = device.cbor(GET_INFO, None)?;
                device.fragment = fragment_size(
                    field(&info, 5)
                        .ok()
                        .and_then(|v| v.as_integer())
                        .and_then(|n| usize::try_from(n).ok()),
                );
                return Ok(device);
            }
        }
    }

    /// Send a CTAP2 command; the response's CBOR map.
    fn cbor(&mut self, command: u8, request: Option<Value>) -> Result<Value, FidoError> {
        let mut data = vec![command];
        if let Some(r) = request {
            ciborium::into_writer(&r, &mut data).map_err(|e| protocol(e.to_string()))?;
        }
        for f in frames(self.cid, CMD_CBOR, &data)? {
            self.dev.write_all(&f)?;
        }
        let (cmd, data) = receive(&mut self.dev, self.cid)?;
        if cmd != CMD_CBOR {
            return Err(protocol(format!("reply command {cmd:#04x}")));
        }
        match data.split_first() {
            Some((0, rest)) => ciborium::from_reader(rest).map_err(|e| protocol(e.to_string())),
            Some((0x31, _)) => Err(FidoError::WrongPin),
            Some((0x32, _)) => Err(FidoError::PinBlocked),
            Some((0x34, _)) => Err(FidoError::PinAuthBlocked),
            Some((status, _)) => Err(FidoError::Status(*status)),
            None => Err(protocol("empty reply")),
        }
    }

    /// Send a CTAP2 command whose success reply has no body.
    fn cbor_empty(&mut self, command: u8, request: Value) -> Result<(), FidoError> {
        let mut data = vec![command];
        ciborium::into_writer(&request, &mut data).map_err(|e| protocol(e.to_string()))?;
        for f in frames(self.cid, CMD_CBOR, &data)? {
            self.dev.write_all(&f)?;
        }
        let (_, data) = receive(&mut self.dev, self.cid)?;
        match data.first() {
            Some(0) => Ok(()),
            Some(0x31) => Err(FidoError::WrongPin),
            Some(0x32) => Err(FidoError::PinBlocked),
            Some(0x34) => Err(FidoError::PinAuthBlocked),
            Some(status) => Err(FidoError::Status(*status)),
            None => Err(protocol("empty reply")),
        }
    }

    /// Create a resident credential with `hmac-secret` and a large-blob
    /// key, so the token can also hold the backup (needs the PIN and a
    /// touch); its id.
    pub fn enrol(&mut self, pin: &str) -> Result<Vec<u8>, FidoError> {
        self.enrol_backup(pin, true)
    }

    /// File-backed backups carry their credential handle, so they require
    /// neither a resident account nor storage in the token's large-blob array.
    pub fn enrol_file(&mut self, pin: &str) -> Result<Vec<u8>, FidoError> {
        self.enrol_backup(pin, false)
    }

    fn enrol_backup(&mut self, pin: &str, resident: bool) -> Result<Vec<u8>, FidoError> {
        let (_, auth) = self.pin_token(pin, PERMIT_MAKE_CREDENTIAL)?;
        let mut extensions = vec![(text("hmac-secret"), Value::Bool(true))];
        if resident {
            extensions.push((text("largeBlobKey"), Value::Bool(true)));
        }
        let request = map(vec![
            (int(1), Value::Bytes(CLIENT_DATA_HASH.to_vec())),
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
            (int(6), map(extensions)),
            (int(7), map(vec![(text("rk"), Value::Bool(resident))])),
            (int(8), Value::Bytes(auth.authenticate(&CLIENT_DATA_HASH))),
            (int(9), int(2)),
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

    /// The token's user-verified secret for `salt` under credential
    /// `id` (needs the PIN and a touch).
    pub fn secret(&mut self, id: &[u8], salt: &[u8; 32], pin: &str) -> Result<[u8; 32], FidoError> {
        self.request_assertions(Some(id), Some(salt), pin, false)?
            .remove(0)
            .secret
            .ok_or_else(|| protocol("no hmac-secret output"))
    }

    /// An assertion for credential `id`, or for this RP's resident
    /// credential when `id` is `None`; with the secret for `salt` if
    /// given, and the credential's large-blob key (needs the PIN and a
    /// touch).
    pub fn assert(
        &mut self,
        id: Option<&[u8]>,
        salt: Option<&[u8; 32]>,
        pin: &str,
    ) -> Result<Assertion, FidoError> {
        Ok(self.assertions(id, salt, pin)?.remove(0))
    }

    /// Assertions for credential `id`, or for every resident credential
    /// of this RP when `id` is `None` (needs the PIN and a touch).
    pub fn assertions(
        &mut self,
        id: Option<&[u8]>,
        salt: Option<&[u8; 32]>,
        pin: &str,
    ) -> Result<Vec<Assertion>, FidoError> {
        self.request_assertions(id, salt, pin, true)
    }

    fn request_assertions(
        &mut self,
        id: Option<&[u8]>,
        salt: Option<&[u8; 32]>,
        pin: &str,
        large_blob_key: bool,
    ) -> Result<Vec<Assertion>, FidoError> {
        let (shared, auth) = self.pin_token(pin, PERMIT_GET_ASSERTION)?;
        let mut ext = Vec::new();
        if let Some(salt) = salt {
            let salt_enc = shared.encrypt(salt);
            let salt_auth = shared.authenticate(&salt_enc);
            ext.push((
                text("hmac-secret"),
                map(vec![
                    (int(1), shared.platform_key()),
                    (int(2), Value::Bytes(salt_enc)),
                    (int(3), Value::Bytes(salt_auth)),
                    (int(4), int(2)),
                ]),
            ));
        }
        if large_blob_key {
            ext.push((text("largeBlobKey"), Value::Bool(true)));
        }
        let mut request = vec![
            (int(1), text(RP_ID)),
            (int(2), Value::Bytes(CLIENT_DATA_HASH.to_vec())),
        ];
        if let Some(id) = id {
            request.push((
                int(3),
                Value::Array(vec![map(vec![
                    (text("id"), Value::Bytes(id.to_vec())),
                    (text("type"), text("public-key")),
                ])]),
            ));
        }
        request.extend([
            (int(4), map(ext)),
            (int(6), Value::Bytes(auth.authenticate(&CLIENT_DATA_HASH))),
            (int(7), int(2)),
        ]);
        let reply = self.cbor(GET_ASSERTION, Some(map(request)))?;
        let first = Self::parse_assertion(&reply, salt.is_some(), &shared)?;
        // Without a credential id the token answers for each resident
        // credential of this RP, one at a time.
        let count = match id {
            Some(_) => 1,
            None => field(&reply, 5)
                .ok()
                .and_then(|v| v.as_integer())
                .and_then(|n| usize::try_from(n).ok())
                .unwrap_or(1),
        };
        let mut all = vec![first];
        for _ in 1..count.min(64) {
            let next = self.cbor(GET_NEXT_ASSERTION, None)?;
            all.push(Self::parse_assertion(&next, salt.is_some(), &shared)?);
        }
        Ok(all)
    }

    /// The credential, secret and large-blob key in a get-assertion reply.
    fn parse_assertion(
        reply: &Value,
        with_secret: bool,
        shared: &Shared,
    ) -> Result<Assertion, FidoError> {
        let auth = bytes(field(reply, 2)?)?;
        if auth.len() < 37 || auth[32] & 0x04 == 0 {
            return Err(protocol("the token did not verify the PIN"));
        }
        let credential_id = field(reply, 1)?
            .as_map()
            .and_then(|m| m.iter().find(|(k, _)| k.as_text() == Some("id")))
            .and_then(|(_, v)| v.as_bytes())
            .cloned()
            .ok_or_else(|| protocol("no credential in the reply"))?;
        let secret = match with_secret {
            false => None,
            true => {
                if auth[32] & 0x80 == 0 {
                    return Err(protocol("no extension output"));
                }
                let ext: Value =
                    ciborium::from_reader(&auth[37..]).map_err(|e| protocol(e.to_string()))?;
                let out = ext
                    .as_map()
                    .and_then(|m| m.iter().find(|(k, _)| k.as_text() == Some("hmac-secret")))
                    .and_then(|(_, v)| v.as_bytes())
                    .ok_or_else(|| protocol("no hmac-secret output"))?;
                Some(
                    shared
                        .decrypt(out)?
                        .try_into()
                        .map_err(|_| protocol("hmac-secret is not 32 bytes"))?,
                )
            }
        };
        let large_blob_key = match field(reply, 7) {
            Ok(v) => Some(
                bytes(v)?
                    .try_into()
                    .map_err(|_| protocol("bad large-blob key"))?,
            ),
            Err(_) => None,
        };
        Ok(Assertion {
            credential_id,
            secret,
            large_blob_key,
        })
    }

    /// The token's whole serialized large-blob array.
    pub fn read_blobs(&mut self) -> Result<Vec<u8>, FidoError> {
        let mut out = Vec::new();
        loop {
            let request = map(vec![
                (int(1), int(self.fragment as i64)),
                (int(3), int(out.len() as i64)),
            ]);
            let reply = self.cbor(LARGE_BLOBS, Some(request))?;
            let part = bytes(field(&reply, 1)?)?;
            if part.len() > self.fragment || out.len() + part.len() > MAX_BLOBS {
                return Err(protocol("the token's blob storage is larger than expected"));
            }
            out.extend_from_slice(part);
            if part.len() < self.fragment {
                return Ok(out);
            }
        }
    }

    /// Replace the token's large-blob array with `array` (needs the PIN).
    pub fn write_blobs(&mut self, array: &[u8], pin: &str) -> Result<(), FidoError> {
        use sha2::{Digest, Sha256};
        let (_, auth) = self.pin_token(pin, PERMIT_LARGE_BLOB_WRITE)?;
        for (i, part) in array.chunks(self.fragment).enumerate() {
            let offset = (i * self.fragment) as u32;
            let message = [
                &[0xffu8; 32][..],
                &[LARGE_BLOBS, 0x00],
                &offset.to_le_bytes(),
                &Sha256::digest(part),
            ]
            .concat();
            let mut request = vec![
                (int(2), Value::Bytes(part.to_vec())),
                (int(3), int(offset.into())),
            ];
            if offset == 0 {
                request.push((int(4), int(array.len() as i64)));
            }
            request.extend([
                (int(5), Value::Bytes(auth.authenticate(&message))),
                (int(6), int(2)),
            ]);
            self.cbor_empty(LARGE_BLOBS, map(request))?;
        }
        Ok(())
    }

    /// Store `backup` in the token, under its resident credential `id`
    /// (needs the PIN, a touch for the key, then the PIN again).
    pub fn store_backup(&mut self, id: &[u8], backup: &[u8], pin: &str) -> Result<(), FidoError> {
        let key = self
            .assert(Some(id), None, pin)?
            .large_blob_key
            .ok_or_else(|| protocol("the credential has no large-blob key"))?;
        let array = self.read_blobs()?;
        let array =
            crate::large_blob::write(&array, &key, backup).map_err(|e| protocol(e.to_string()))?;
        self.write_blobs(&array, pin)
    }

    /// The backup stored in the token, with the credential holding it
    /// (needs the PIN and a touch).
    pub fn load_backup(&mut self, pin: &str) -> Result<(Vec<u8>, Vec<u8>), FidoError> {
        // Several resident credentials can exist (an enrolment that was
        // interrupted before its backup was stored): use the first one
        // that has a backup.
        let all = self.assertions(None, None, pin)?;
        let array = self.read_blobs()?;
        all.into_iter()
            .find_map(|a| {
                let key = a.large_blob_key?;
                let backup = crate::large_blob::read(&array, &key).ok()?;
                Some((a.credential_id, backup))
            })
            .ok_or_else(|| protocol(crate::large_blob::BlobError::Missing.to_string()))
    }

    /// The file-backed trial uses hmac-secret and PIN/UV protocol 2.
    /// GetInfo reads capabilities without checking a PIN or creating a credential.
    pub fn check_file_backup_support(&mut self) -> Result<(), FidoError> {
        let info = self.cbor(GET_INFO, None)?;
        let has = |field_number, value| {
            field(&info, field_number)
                .ok()
                .and_then(Value::as_array)
                .is_some_and(|list| list.contains(&value))
        };
        if !has(2, text("hmac-secret")) {
            return Err(protocol("token lacks hmac-secret"));
        }
        if !has(6, int(2)) {
            return Err(protocol("token lacks PIN/UV protocol 2"));
        }
        Ok(())
    }

    /// Tries left before the token blocks its PIN.
    pub fn pin_retries(&mut self) -> Result<u8, FidoError> {
        let reply = self.cbor(
            CLIENT_PIN,
            Some(map(vec![(int(1), int(2)), (int(2), int(1))])),
        )?;
        field(&reply, 3)?
            .as_integer()
            .and_then(|n| u8::try_from(n).ok())
            .ok_or_else(|| protocol("bad retries"))
    }

    /// A PIN/UV auth token for `permissions` on this RP, proving `pin`;
    /// with the shared secret it was agreed under.
    fn pin_token(&mut self, pin: &str, permissions: u8) -> Result<(Shared, PinToken), FidoError> {
        let shared = self.key_agreement()?;
        let pin_hash = pin_hash(pin);
        let request = map(vec![
            (int(1), int(2)),
            (int(2), int(9)),
            (int(3), shared.platform_key()),
            (int(6), Value::Bytes(shared.encrypt(&pin_hash[..16]))),
            (int(9), int(permissions.into())),
            (int(10), text(RP_ID)),
        ]);
        let reply = self.cbor(CLIENT_PIN, Some(request))?;
        let token = shared.decrypt(bytes(field(&reply, 2)?)?)?;
        let token: [u8; 32] = token.try_into().map_err(|_| protocol("bad PIN token"))?;
        Ok((shared, PinToken(token)))
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

const PERMIT_MAKE_CREDENTIAL: u8 = 0x01;
const PERMIT_GET_ASSERTION: u8 = 0x02;
const PERMIT_LARGE_BLOB_WRITE: u8 = 0x10;
const LARGE_BLOBS: u8 = 0x0c;
const GET_INFO: u8 = 0x04;
const GET_NEXT_ASSERTION: u8 = 0x08;
/// The most large-blob data read from a token.
const MAX_BLOBS: usize = 64 * 1024;

/// Large-blob fragment size for a token whose message limit is
/// `max_msg_size` (CTAP 2.1: maxMsgSize − 64, 1024 when not advertised).
fn fragment_size(max_msg_size: Option<usize>) -> usize {
    max_msg_size
        .unwrap_or(1024)
        .clamp(128, MAX_MESSAGE)
        .saturating_sub(64)
}

/// The PIN as the token expects it hashed: Unicode NFC.
fn pin_hash(pin: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    use unicode_normalization::UnicodeNormalization;
    Sha256::digest(pin.nfc().collect::<String>().as_bytes()).into()
}

/// What a get-assertion returned.
pub struct Assertion {
    pub credential_id: Vec<u8>,
    pub secret: Option<[u8; 32]>,
    pub large_blob_key: Option<[u8; 32]>,
}
/// No client data: the assertion's signature is not used, only its
/// hmac-secret output.
const CLIENT_DATA_HASH: [u8; 32] = [0; 32];

/// A PIN/UV auth token (protocol 2): authenticates requests.
struct PinToken([u8; 32]);

impl PinToken {
    fn authenticate(&self, msg: &[u8]) -> Vec<u8> {
        use hmac::Mac;
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(&self.0).unwrap();
        mac.update(msg);
        mac.finalize().into_bytes().to_vec()
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
        let fs = frames(cid, CMD_CBOR, &data).unwrap();
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
        let wire: Vec<u8> = frames(cid, CMD_ERROR, &[0x06]).unwrap()[0][1..].to_vec();
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

    #[test]
    fn an_oversized_token_message_is_refused() {
        let cid = [1, 2, 3, 4];
        let mut first = [0u8; REPORT];
        first[..4].copy_from_slice(&cid);
        first[4] = CMD_CBOR;
        first[5..7].copy_from_slice(&((MAX_MESSAGE + 1) as u16).to_be_bytes());
        assert!(matches!(
            receive(&mut first.as_slice(), cid),
            Err(FidoError::Protocol(_))
        ));
        assert!(frames(cid, CMD_CBOR, &vec![0; MAX_MESSAGE + 1]).is_err());
        let longest = vec![7u8; MAX_MESSAGE];
        let fs = frames(cid, CMD_CBOR, &longest).unwrap();
        assert_eq!(fs.len(), 129, "an initial report and 128 continuations");
        let wire: Vec<u8> = fs.iter().flat_map(|f| f[1..].to_vec()).collect();
        assert_eq!(receive(&mut wire.as_slice(), cid).unwrap().1, longest);
    }

    #[test]
    fn the_pin_is_hashed_in_nfc() {
        // "é" precomposed and as "e" + combining acute accent.
        assert_eq!(pin_hash("caf\u{e9}1"), pin_hash("cafe\u{301}1"));
        assert_ne!(pin_hash("cafe1"), pin_hash("caf\u{e9}1"));
    }

    #[test]
    fn the_fragment_follows_the_tokens_message_limit() {
        assert_eq!(fragment_size(None), 960, "CTAP default 1024 - 64");
        assert_eq!(fragment_size(Some(2048)), 1984);
        assert_eq!(fragment_size(Some(0)), 64, "a silly value stays usable");
    }

    #[test]
    fn temporary_and_permanent_pin_locks_say_different_things() {
        let temporary = FidoError::PinAuthBlocked.to_string();
        let permanent = FidoError::PinBlocked.to_string();
        assert!(temporary.contains("plug it back in") && !temporary.contains("reset"));
        assert!(permanent.contains("erases"));
    }
}
