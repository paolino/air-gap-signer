use ed25519_dalek::{Signer, SigningKey};
use signer_hal::HalError;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Simulated secure element backed by a plaintext JSON keystore.
///
/// The file maps slot numbers to hex-encoded Ed25519 seeds:
/// ```json
/// { "0": "abcd1234...", "1": "..." }
/// ```
pub struct SimSecureElement {
    keys: HashMap<u8, [u8; 32]>,
}

impl SimSecureElement {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let data = fs::read_to_string(path)
            .map_err(|e| format!("failed to read keystore {}: {e}", path.display()))?;
        let map: HashMap<String, String> = serde_json::from_str(&data)
            .map_err(|e| format!("failed to parse keystore JSON: {e}"))?;

        let mut keys = HashMap::new();
        for (slot_str, hex_str) in map {
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
        Ok(Self { keys })
    }
}

impl signer_hal::SecureElement for SimSecureElement {
    fn verify_pin(&mut self, _pin: &[u8]) -> Result<(), HalError> {
        Ok(())
    }

    fn generate_key(&mut self, _slot: u8) -> Result<Vec<u8>, HalError> {
        Err(HalError::Storage(
            "generate_key not supported in simulator".into(),
        ))
    }

    fn sign(&mut self, slot: u8, hash: &[u8]) -> Result<Vec<u8>, HalError> {
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
}
