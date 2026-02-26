use ed25519_dalek::{Signer, SigningKey};
use rand::RngCore;
use sha2::{Digest, Sha256};
use signer_hal::HalError;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// JSON-serializable keystore format.
#[derive(serde::Serialize, serde::Deserialize)]
struct KeystoreFile {
    pin_hash: Option<String>,
    keys: HashMap<String, String>,
}

/// Simulated secure element backed by a JSON keystore on disk.
///
/// Tracks PIN hash, key slots, and per-session PIN verification state.
pub struct SimSecureElement {
    path: PathBuf,
    pin_hash: Option<Vec<u8>>,
    keys: HashMap<u8, [u8; 32]>,
    pin_verified: bool,
}

impl SimSecureElement {
    /// Load an existing keystore or create an empty one if the file doesn't exist.
    pub fn from_file_or_new(path: &Path) -> Self {
        if path.exists() {
            match Self::from_file(path) {
                Ok(se) => se,
                Err(e) => {
                    eprintln!("warning: failed to load keystore, starting fresh: {e}");
                    Self::create_empty(path)
                }
            }
        } else {
            Self::create_empty(path)
        }
    }

    /// Create a new empty (unprovisioned) keystore.
    fn create_empty(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            pin_hash: None,
            keys: HashMap::new(),
            pin_verified: false,
        }
    }

    /// Load keystore from a JSON file.
    fn from_file(path: &Path) -> Result<Self, String> {
        let data = fs::read_to_string(path)
            .map_err(|e| format!("failed to read keystore {}: {e}", path.display()))?;
        let kf: KeystoreFile = serde_json::from_str(&data)
            .map_err(|e| format!("failed to parse keystore JSON: {e}"))?;

        let pin_hash = kf
            .pin_hash
            .map(|h| hex::decode(&h).map_err(|e| format!("invalid pin_hash hex: {e}")))
            .transpose()?;

        let mut keys = HashMap::new();
        for (slot_str, hex_str) in kf.keys {
            let slot: u8 = slot_str
                .parse()
                .map_err(|e| format!("invalid slot number {slot_str}: {e}"))?;
            let bytes =
                hex::decode(&hex_str).map_err(|e| format!("invalid hex for slot {slot}: {e}"))?;
            let seed: [u8; 32] = bytes
                .try_into()
                .map_err(|_| format!("slot {slot}: key must be 32 bytes"))?;
            keys.insert(slot, seed);
        }

        Ok(Self {
            path: path.to_path_buf(),
            pin_hash,
            keys,
            pin_verified: false,
        })
    }

    /// Persist current state to disk.
    fn save(&self) -> Result<(), HalError> {
        let kf = KeystoreFile {
            pin_hash: self.pin_hash.as_ref().map(hex::encode),
            keys: self
                .keys
                .iter()
                .map(|(slot, seed)| (slot.to_string(), hex::encode(seed)))
                .collect(),
        };
        let json = serde_json::to_string_pretty(&kf)
            .map_err(|e| HalError::Storage(format!("failed to serialize keystore: {e}")))?;
        fs::write(&self.path, json)
            .map_err(|e| HalError::Storage(format!("failed to write keystore: {e}")))?;
        Ok(())
    }

    fn require_pin(&self) -> Result<(), HalError> {
        if !self.pin_verified {
            return Err(HalError::Storage("PIN not verified".into()));
        }
        Ok(())
    }
}

impl signer_hal::SecureElement for SimSecureElement {
    fn set_pin(&mut self, pin: &[u8]) -> Result<(), HalError> {
        if self.pin_hash.is_some() {
            return Err(HalError::Storage("PIN already set".into()));
        }
        let hash = Sha256::digest(pin).to_vec();
        self.pin_hash = Some(hash);
        self.save()
    }

    fn verify_pin(&mut self, pin: &[u8]) -> Result<(), HalError> {
        let stored = self
            .pin_hash
            .as_ref()
            .ok_or_else(|| HalError::Storage("no PIN set".into()))?;
        let hash = Sha256::digest(pin).to_vec();
        if hash != *stored {
            self.pin_verified = false;
            return Err(HalError::Storage("wrong PIN".into()));
        }
        self.pin_verified = true;
        Ok(())
    }

    fn is_provisioned(&self) -> bool {
        self.pin_hash.is_some()
    }

    fn generate_key(&mut self, slot: u8) -> Result<Vec<u8>, HalError> {
        self.require_pin()?;
        let mut seed = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut seed);
        self.keys.insert(slot, seed);
        self.save()?;
        let signing_key = SigningKey::from_bytes(&seed);
        Ok(signing_key.verifying_key().to_bytes().to_vec())
    }

    fn sign(&mut self, slot: u8, hash: &[u8]) -> Result<Vec<u8>, HalError> {
        self.require_pin()?;
        let seed = self
            .keys
            .get(&slot)
            .ok_or_else(|| HalError::Storage(format!("no key in slot {slot}")))?;
        let signing_key = SigningKey::from_bytes(seed);
        let signature = signing_key.sign(hash);
        Ok(signature.to_bytes().to_vec())
    }

    fn public_key(&self, slot: u8) -> Result<Vec<u8>, HalError> {
        let seed = self
            .keys
            .get(&slot)
            .ok_or_else(|| HalError::Storage(format!("no key in slot {slot}")))?;
        let signing_key = SigningKey::from_bytes(seed);
        Ok(signing_key.verifying_key().to_bytes().to_vec())
    }

    fn import_key(&mut self, slot: u8, seed: &[u8]) -> Result<Vec<u8>, HalError> {
        self.require_pin()?;
        let seed_arr: [u8; 32] = seed
            .try_into()
            .map_err(|_| HalError::Storage("seed must be 32 bytes".into()))?;
        self.keys.insert(slot, seed_arr);
        self.save()?;
        let signing_key = SigningKey::from_bytes(&seed_arr);
        Ok(signing_key.verifying_key().to_bytes().to_vec())
    }

    fn export_seed(&self, slot: u8) -> Result<Vec<u8>, HalError> {
        let seed = self
            .keys
            .get(&slot)
            .ok_or_else(|| HalError::Storage(format!("no key in slot {slot}")))?;
        Ok(seed.to_vec())
    }
}
