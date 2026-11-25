//! The key's backup: a file on a USB stick that any one of several FIDO2
//! tokens can open.
//!
//! The 32-byte Ed25519 seed is encrypted once with a random file key; the
//! file key is stored once per token, encrypted with the secret that token
//! returns for a random salt (FIDO2 `hmac-secret`). Any enrolled token
//! restores the seed; adding a token needs one already enrolled, since
//! the chip never gives the seed back.

use serde::{Deserialize, Serialize};
use thiserror::Error;
use zeroize::Zeroizing;

/// An Ed25519 seed, wiped from memory when dropped.
pub type Seed = Zeroizing<[u8; 32]>;

/// A FIDO2 token enrolled for the backup.
pub trait Token {
    /// The credential the token created at enrolment.
    fn credential_id(&self) -> Vec<u8>;
    /// The token's secret for `salt` (needs a touch on a real token).
    fn secret(&mut self, salt: &[u8; 32]) -> Result<[u8; 32], String>;
}

#[derive(Debug, Error, PartialEq)]
pub enum BackupError {
    #[error("this token is not enrolled in the backup")]
    NotEnrolled,
    #[error("this token is already enrolled")]
    AlreadyEnrolled,
    #[error("the backup needs at least one token")]
    NoToken,
    #[error("the token refused: {0}")]
    Token(String),
    #[error("the backup is damaged or does not match its token")]
    Corrupt,
    #[error("unreadable backup file: {0}")]
    Format(String),
}

/// One token's copy of the file key.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Slot {
    credential_id: Vec<u8>,
    salt: [u8; 32],
    nonce: [u8; 12],
    wrapped: Vec<u8>,
}

/// The backup file's content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Backup {
    version: u8,
    /// The key's public half: the file says which key it backs up.
    pub public_key: [u8; 32],
    nonce: [u8; 12],
    sealed: Vec<u8>,
    slots: Vec<Slot>,
}

const VERSION: u8 = 1;

fn random<const N: usize>() -> [u8; N] {
    use rand::RngCore;
    let mut b = [0u8; N];
    rand::rngs::OsRng.fill_bytes(&mut b);
    b
}

fn seal(key: &[u8; 32], nonce: &[u8; 12], aad: &[u8], plain: &[u8]) -> Vec<u8> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    chacha20poly1305::ChaCha20Poly1305::new(key.into())
        .encrypt(nonce.into(), Payload { msg: plain, aad })
        .expect("in-memory encryption")
}

fn open(
    key: &[u8; 32],
    nonce: &[u8; 12],
    aad: &[u8],
    sealed: &[u8],
) -> Result<Zeroizing<Vec<u8>>, BackupError> {
    use chacha20poly1305::aead::{Aead, KeyInit, Payload};
    chacha20poly1305::ChaCha20Poly1305::new(key.into())
        .decrypt(nonce.into(), Payload { msg: sealed, aad })
        .map(Zeroizing::new)
        .map_err(|_| BackupError::Corrupt)
}

fn public_of(seed: &[u8; 32]) -> [u8; 32] {
    ed25519_dalek::SigningKey::from_bytes(seed)
        .verifying_key()
        .to_bytes()
}

impl Backup {
    /// Back up `seed` for each of `tokens` (at least one).
    pub fn create(seed: &Seed, tokens: &mut [&mut dyn Token]) -> Result<Self, BackupError> {
        if tokens.is_empty() {
            return Err(BackupError::NoToken);
        }
        let file_key = Zeroizing::new(random::<32>());
        let public_key = public_of(seed);
        let nonce = random();
        let mut backup = Self {
            version: VERSION,
            public_key,
            nonce,
            sealed: seal(&file_key, &nonce, &public_key, seed.as_slice()),
            slots: Vec::new(),
        };
        for token in tokens.iter_mut() {
            backup.enrol(&file_key, &mut **token)?;
        }
        Ok(backup)
    }

    /// Enrol `new`, opening the file with `enrolled`.
    pub fn add_token(
        &mut self,
        enrolled: &mut dyn Token,
        new: &mut dyn Token,
    ) -> Result<(), BackupError> {
        let file_key = self.file_key(enrolled)?;
        self.enrol(&file_key, new)
    }

    /// Drop the token with `credential_id`; the last one stays.
    pub fn remove_token(&mut self, credential_id: &[u8]) -> Result<(), BackupError> {
        let i = self
            .slots
            .iter()
            .position(|s| s.credential_id == credential_id)
            .ok_or(BackupError::NotEnrolled)?;
        if self.slots.len() == 1 {
            return Err(BackupError::NoToken);
        }
        self.slots.remove(i);
        Ok(())
    }

    /// The seed, opened with `token`.
    pub fn restore(&self, token: &mut dyn Token) -> Result<Seed, BackupError> {
        let file_key = self.file_key(token)?;
        let plain = open(&file_key, &self.nonce, &self.public_key, &self.sealed)?;
        let seed: Seed = Zeroizing::new(
            plain
                .as_slice()
                .try_into()
                .map_err(|_| BackupError::Corrupt)?,
        );
        if public_of(&seed) != self.public_key {
            return Err(BackupError::Corrupt);
        }
        Ok(seed)
    }

    fn enrol(&mut self, file_key: &[u8; 32], token: &mut dyn Token) -> Result<(), BackupError> {
        let credential_id = token.credential_id();
        if self.slots.iter().any(|s| s.credential_id == credential_id) {
            return Err(BackupError::AlreadyEnrolled);
        }
        let salt = random();
        let secret = Zeroizing::new(token.secret(&salt).map_err(BackupError::Token)?);
        let nonce = random();
        let wrapped = seal(&secret, &nonce, &self.slot_aad(&credential_id), file_key);
        self.slots.push(Slot {
            credential_id,
            salt,
            nonce,
            wrapped,
        });
        Ok(())
    }

    fn file_key(&self, token: &mut dyn Token) -> Result<Zeroizing<[u8; 32]>, BackupError> {
        let credential_id = token.credential_id();
        let slot = self
            .slots
            .iter()
            .find(|s| s.credential_id == credential_id)
            .ok_or(BackupError::NotEnrolled)?;
        let secret = Zeroizing::new(token.secret(&slot.salt).map_err(BackupError::Token)?);
        let key = open(
            &secret,
            &slot.nonce,
            &self.slot_aad(&credential_id),
            &slot.wrapped,
        )?;
        Ok(Zeroizing::new(
            key.as_slice()
                .try_into()
                .map_err(|_| BackupError::Corrupt)?,
        ))
    }

    /// A slot is bound to the key and to its own credential.
    fn slot_aad(&self, credential_id: &[u8]) -> Vec<u8> {
        [&self.public_key[..], credential_id].concat()
    }

    /// How many tokens can open the backup.
    pub fn tokens(&self) -> usize {
        self.slots.len()
    }

    pub fn to_cbor(&self) -> Vec<u8> {
        let mut out = Vec::new();
        ciborium::into_writer(self, &mut out).expect("in-memory CBOR");
        out
    }

    pub fn from_cbor(bytes: &[u8]) -> Result<Self, BackupError> {
        let backup: Self =
            ciborium::from_reader(bytes).map_err(|e| BackupError::Format(e.to_string()))?;
        if backup.version != VERSION {
            return Err(BackupError::Format(format!("version {}", backup.version)));
        }
        Ok(backup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A token whose secret is a keyed hash of the salt, like hmac-secret.
    struct Fake {
        id: u8,
        refuse: bool,
    }

    impl Token for Fake {
        fn credential_id(&self) -> Vec<u8> {
            vec![self.id; 16]
        }
        fn secret(&mut self, salt: &[u8; 32]) -> Result<[u8; 32], String> {
            use sha2::{Digest, Sha256};
            if self.refuse {
                return Err("not touched".into());
            }
            Ok(Sha256::new()
                .chain_update([self.id])
                .chain_update(salt)
                .finalize()
                .into())
        }
    }

    fn token(id: u8) -> Fake {
        Fake { id, refuse: false }
    }

    fn seed() -> Seed {
        Zeroizing::new([7u8; 32])
    }

    #[test]
    fn any_enrolled_token_restores_the_seed() {
        let (mut a, mut b) = (token(1), token(2));
        let backup = Backup::create(&seed(), &mut [&mut a, &mut b]).unwrap();
        assert_eq!(backup.tokens(), 2);
        assert_eq!(*backup.restore(&mut a).unwrap(), *seed());
        assert_eq!(*backup.restore(&mut b).unwrap(), *seed());
    }

    #[test]
    fn the_file_names_the_key_it_backs_up() {
        let backup = Backup::create(&seed(), &mut [&mut token(1)]).unwrap();
        let public = ed25519_dalek::SigningKey::from_bytes(&seed()).verifying_key();
        assert_eq!(backup.public_key, public.to_bytes());
    }

    #[test]
    fn a_stranger_token_cannot_restore() {
        let backup = Backup::create(&seed(), &mut [&mut token(1)]).unwrap();
        assert_eq!(backup.restore(&mut token(9)), Err(BackupError::NotEnrolled));
    }

    #[test]
    fn a_token_that_answers_wrong_is_caught() {
        let backup = Backup::create(&seed(), &mut [&mut token(1)]).unwrap();
        // Same credential, different secret: a swapped or faulty token.
        struct Liar;
        impl Token for Liar {
            fn credential_id(&self) -> Vec<u8> {
                vec![1; 16]
            }
            fn secret(&mut self, _: &[u8; 32]) -> Result<[u8; 32], String> {
                Ok([0; 32])
            }
        }
        assert_eq!(backup.restore(&mut Liar), Err(BackupError::Corrupt));
    }

    #[test]
    fn a_refusing_token_reports_it() {
        let backup = Backup::create(&seed(), &mut [&mut token(1)]).unwrap();
        let mut t = Fake {
            id: 1,
            refuse: true,
        };
        assert_eq!(
            backup.restore(&mut t),
            Err(BackupError::Token("not touched".into()))
        );
    }

    #[test]
    fn no_token_no_backup() {
        assert_eq!(Backup::create(&seed(), &mut []), Err(BackupError::NoToken));
    }

    #[test]
    fn a_token_is_added_with_one_already_enrolled() {
        let (mut a, mut b) = (token(1), token(2));
        let mut backup = Backup::create(&seed(), &mut [&mut a]).unwrap();
        assert_eq!(
            backup.add_token(&mut token(9), &mut b),
            Err(BackupError::NotEnrolled)
        );
        backup.add_token(&mut a, &mut b).unwrap();
        assert_eq!(*backup.restore(&mut b).unwrap(), *seed());
        assert_eq!(
            backup.add_token(&mut a, &mut token(2)),
            Err(BackupError::AlreadyEnrolled)
        );
    }

    #[test]
    fn a_token_is_removed_but_never_the_last() {
        let (mut a, mut b) = (token(1), token(2));
        let mut backup = Backup::create(&seed(), &mut [&mut a, &mut b]).unwrap();
        backup.remove_token(&a.credential_id()).unwrap();
        assert_eq!(backup.restore(&mut a), Err(BackupError::NotEnrolled));
        assert_eq!(*backup.restore(&mut b).unwrap(), *seed());
        assert_eq!(
            backup.remove_token(&b.credential_id()),
            Err(BackupError::NoToken)
        );
        assert_eq!(backup.remove_token(&[3; 16]), Err(BackupError::NotEnrolled));
    }

    #[test]
    fn the_file_round_trips_and_holds_no_plain_seed() {
        let backup = Backup::create(&seed(), &mut [&mut token(1)]).unwrap();
        let bytes = backup.to_cbor();
        assert!(!bytes.windows(32).any(|w| w == *seed()), "seed in clear");
        let back = Backup::from_cbor(&bytes).unwrap();
        assert_eq!(*back.restore(&mut token(1)).unwrap(), *seed());
    }

    #[test]
    fn a_tampered_file_is_refused() {
        let mut backup = Backup::create(&seed(), &mut [&mut token(1)]).unwrap();
        backup.sealed[0] ^= 1;
        assert_eq!(backup.restore(&mut token(1)), Err(BackupError::Corrupt));
        let mut backup = Backup::create(&seed(), &mut [&mut token(1)]).unwrap();
        backup.public_key[0] ^= 1;
        assert_eq!(
            backup.restore(&mut token(1)),
            Err(BackupError::Corrupt),
            "key bound to its file"
        );
    }
}
