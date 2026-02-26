use crate::spec::{HashAlgorithm, SignAlgorithm, Signable, SignableSource};
use blake2::digest::consts::U32;
use blake2::{Blake2b, Digest};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CryptoError {
    #[error("signing error: {0}")]
    Signing(String),
    #[error("range {offset}..{end} out of bounds (payload length {payload_len})")]
    RangeOutOfBounds {
        offset: usize,
        end: usize,
        payload_len: usize,
    },
    #[error("unsupported algorithm: {0:?}")]
    Unsupported(SignAlgorithm),
}

/// Extract the bytes to sign from the payload according to the Signable spec.
pub fn extract_signable(payload: &[u8], signable: &Signable) -> Result<Vec<u8>, CryptoError> {
    match signable {
        Signable::Whole => Ok(payload.to_vec()),
        Signable::Range { offset, length } => {
            let end = offset + length;
            if end > payload.len() {
                return Err(CryptoError::RangeOutOfBounds {
                    offset: *offset,
                    end,
                    payload_len: payload.len(),
                });
            }
            Ok(payload[*offset..end].to_vec())
        }
        Signable::HashThenSign { hash, source } => {
            let source_bytes = match source {
                SignableSource::Whole => payload.to_vec(),
                SignableSource::Range { offset, length } => {
                    let end = offset + length;
                    if end > payload.len() {
                        return Err(CryptoError::RangeOutOfBounds {
                            offset: *offset,
                            end,
                            payload_len: payload.len(),
                        });
                    }
                    payload[*offset..end].to_vec()
                }
            };
            Ok(hash_bytes(*hash, &source_bytes))
        }
    }
}

/// Hash bytes with the given algorithm.
fn hash_bytes(algo: HashAlgorithm, data: &[u8]) -> Vec<u8> {
    match algo {
        HashAlgorithm::Blake2b256 => {
            let mut hasher = Blake2b::<U32>::new();
            hasher.update(data);
            hasher.finalize().to_vec()
        }
        HashAlgorithm::Sha256 => {
            use sha2::Sha256;
            let mut hasher = Sha256::new();
            hasher.update(data);
            hasher.finalize().to_vec()
        }
        HashAlgorithm::Sha3_256 => {
            use sha3::Sha3_256;
            let mut hasher = Sha3_256::new();
            hasher.update(data);
            hasher.finalize().to_vec()
        }
    }
}

/// Sign bytes with the given algorithm and secret key.
///
/// For Ed25519: `secret_key` is the 32-byte seed.
pub fn sign(
    algorithm: SignAlgorithm,
    secret_key: &[u8],
    message: &[u8],
) -> Result<Vec<u8>, CryptoError> {
    match algorithm {
        SignAlgorithm::Ed25519 => sign_ed25519(secret_key, message),
        SignAlgorithm::Secp256k1Ecdsa => {
            Err(CryptoError::Unsupported(SignAlgorithm::Secp256k1Ecdsa))
        }
        SignAlgorithm::Secp256k1Schnorr => {
            Err(CryptoError::Unsupported(SignAlgorithm::Secp256k1Schnorr))
        }
    }
}

fn sign_ed25519(secret_key: &[u8], message: &[u8]) -> Result<Vec<u8>, CryptoError> {
    use ed25519_dalek::{Signer, SigningKey};
    let key_bytes: [u8; 32] = secret_key
        .try_into()
        .map_err(|_| CryptoError::Signing("Ed25519 key must be 32 bytes".into()))?;
    let signing_key = SigningKey::from_bytes(&key_bytes);
    let signature = signing_key.sign(message);
    Ok(signature.to_bytes().to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signature, SigningKey, Verifier, VerifyingKey};

    #[test]
    fn ed25519_sign_verify() {
        let seed = [42u8; 32];
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key: VerifyingKey = (&signing_key).into();

        let message = b"hello air-gapped signer";
        let sig_bytes = sign(SignAlgorithm::Ed25519, &seed, message).unwrap();
        let signature = Signature::from_bytes(&sig_bytes.try_into().unwrap());
        verifying_key.verify(message, &signature).unwrap();
    }

    #[test]
    fn extract_whole() {
        let payload = b"test payload";
        let result = extract_signable(payload, &Signable::Whole).unwrap();
        assert_eq!(result, payload);
    }

    #[test]
    fn extract_range() {
        let payload = b"0123456789";
        let result = extract_signable(
            payload,
            &Signable::Range {
                offset: 2,
                length: 4,
            },
        )
        .unwrap();
        assert_eq!(result, b"2345");
    }

    #[test]
    fn extract_range_out_of_bounds() {
        let payload = b"short";
        let result = extract_signable(
            payload,
            &Signable::Range {
                offset: 2,
                length: 100,
            },
        );
        assert!(result.is_err());
    }

    #[test]
    fn extract_hash_then_sign_blake2b() {
        let payload = b"hash me";
        let result = extract_signable(
            payload,
            &Signable::HashThenSign {
                hash: HashAlgorithm::Blake2b256,
                source: SignableSource::Whole,
            },
        )
        .unwrap();
        assert_eq!(result.len(), 32);
    }
}
