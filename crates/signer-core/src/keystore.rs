use std::collections::HashMap;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum KeystoreError {
    #[error("key not found: {0}")]
    KeyNotFound(String),
    #[error("decryption failed")]
    DecryptionFailed,
}

/// Plaintext keystore for Phase 0/1 development.
/// Phase 3 replaces this with AES-256-GCM encrypted storage.
pub struct PlaintextKeystore {
    keys: HashMap<String, Vec<u8>>,
}

impl PlaintextKeystore {
    pub fn new() -> Self {
        Self {
            keys: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key_id: String, secret: Vec<u8>) {
        self.keys.insert(key_id, secret);
    }

    pub fn get(&self, key_id: &str) -> Result<&[u8], KeystoreError> {
        self.keys
            .get(key_id)
            .map(|v| v.as_slice())
            .ok_or_else(|| KeystoreError::KeyNotFound(key_id.into()))
    }
}

impl Default for PlaintextKeystore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_retrieve() {
        let mut ks = PlaintextKeystore::new();
        ks.insert("key-0".into(), vec![1, 2, 3]);
        assert_eq!(ks.get("key-0").unwrap(), &[1, 2, 3]);
    }

    #[test]
    fn missing_key() {
        let ks = PlaintextKeystore::new();
        assert!(ks.get("nope").is_err());
    }
}
