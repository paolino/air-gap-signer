use serde::{Deserialize, Serialize};

/// What portion of the payload to sign.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Signable {
    /// Sign the entire payload as-is.
    Whole,
    /// Sign a byte range within the payload.
    Range { offset: usize, length: usize },
    /// Hash the source bytes first, then sign the hash.
    HashThenSign {
        hash: HashAlgorithm,
        source: SignableSource,
    },
}

/// Source selection for HashThenSign.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SignableSource {
    Whole,
    Range { offset: usize, length: usize },
}

/// Supported hash algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum HashAlgorithm {
    Blake2b256,
    Sha256,
    Sha3_256,
}

/// Supported signing algorithms.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum SignAlgorithm {
    Ed25519,
    Secp256k1Ecdsa,
    Secp256k1Schnorr,
}

/// How to produce the final output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum OutputSpec {
    /// Write just the raw signature bytes.
    SignatureOnly,
    /// Append signature to the original payload.
    AppendToPayload,
    /// Call the WASM interpreter's `assemble(payload, sig)` function.
    WasmAssemble,
}

/// Complete signing specification — deserialized from `sign.cbor` on the USB stick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SigningSpec {
    pub label: String,
    pub signable: Signable,
    pub algorithm: SignAlgorithm,
    pub key_slot: u8,
    pub output: OutputSpec,
}

impl SigningSpec {
    /// Deserialize from CBOR bytes.
    pub fn from_cbor(bytes: &[u8]) -> Result<Self, ciborium::de::Error<std::io::Error>> {
        ciborium::from_reader(bytes)
    }

    /// Serialize to CBOR bytes.
    pub fn to_cbor(&self) -> Result<Vec<u8>, ciborium::ser::Error<std::io::Error>> {
        let mut buf = Vec::new();
        ciborium::into_writer(self, &mut buf)?;
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_whole_ed25519() {
        let spec = SigningSpec {
            label: "Cardano Transaction".into(),
            signable: Signable::Whole,
            algorithm: SignAlgorithm::Ed25519,
            key_slot: 0,
            output: OutputSpec::SignatureOnly,
        };
        let cbor = spec.to_cbor().unwrap();
        let decoded = SigningSpec::from_cbor(&cbor).unwrap();
        assert_eq!(spec, decoded);
    }

    #[test]
    fn round_trip_hash_then_sign() {
        let spec = SigningSpec {
            label: "Bitcoin PSBT".into(),
            signable: Signable::HashThenSign {
                hash: HashAlgorithm::Sha256,
                source: SignableSource::Whole,
            },
            algorithm: SignAlgorithm::Secp256k1Ecdsa,
            key_slot: 1,
            output: OutputSpec::WasmAssemble,
        };
        let cbor = spec.to_cbor().unwrap();
        let decoded = SigningSpec::from_cbor(&cbor).unwrap();
        assert_eq!(spec, decoded);
    }

    #[test]
    fn round_trip_range() {
        let spec = SigningSpec {
            label: "Custom Format".into(),
            signable: Signable::Range {
                offset: 4,
                length: 32,
            },
            algorithm: SignAlgorithm::Secp256k1Schnorr,
            key_slot: 2,
            output: OutputSpec::AppendToPayload,
        };
        let cbor = spec.to_cbor().unwrap();
        let decoded = SigningSpec::from_cbor(&cbor).unwrap();
        assert_eq!(spec, decoded);
    }

    #[test]
    fn round_trip_hash_then_sign_range() {
        let spec = SigningSpec {
            label: "Partial Hash".into(),
            signable: Signable::HashThenSign {
                hash: HashAlgorithm::Blake2b256,
                source: SignableSource::Range {
                    offset: 10,
                    length: 64,
                },
            },
            algorithm: SignAlgorithm::Ed25519,
            key_slot: 3,
            output: OutputSpec::SignatureOnly,
        };
        let cbor = spec.to_cbor().unwrap();
        let decoded = SigningSpec::from_cbor(&cbor).unwrap();
        assert_eq!(spec, decoded);
    }
}
