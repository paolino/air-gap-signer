//! The token's large-blob array (CTAP 2.1 §6.10): a CBOR array of
//! encrypted entries followed by the first 16 bytes of its SHA-256. Each
//! entry is DEFLATE-compressed data sealed with AES-256-GCM under one
//! credential's `largeBlobKey`; entries of other credentials (other
//! apps) are kept untouched.

use ciborium::Value;
use sha2::{Digest, Sha256};

#[derive(Debug, PartialEq, thiserror::Error)]
pub enum BlobError {
    #[error("the token's blob storage is damaged")]
    Corrupt,
    #[error("no backup stored in this token")]
    Missing,
}

/// The serialized array with its check.
pub fn serialize(entries: &[Value]) -> Vec<u8> {
    let mut out = Vec::new();
    ciborium::into_writer(&Value::Array(entries.to_vec()), &mut out).expect("in-memory CBOR");
    let check = Sha256::digest(&out);
    out.extend_from_slice(&check[..16]);
    out
}

/// The entries of a serialized array, after checking it.
pub fn parse(bytes: &[u8]) -> Result<Vec<Value>, BlobError> {
    if bytes.len() < 17 {
        return Err(BlobError::Corrupt);
    }
    let (array, check) = bytes.split_at(bytes.len() - 16);
    if Sha256::digest(array)[..16] != *check {
        return Err(BlobError::Corrupt);
    }
    match ciborium::from_reader(array) {
        Ok(Value::Array(entries)) => Ok(entries),
        _ => Err(BlobError::Corrupt),
    }
}

fn aad(size: usize) -> Vec<u8> {
    [&b"blob"[..], &(size as u64).to_le_bytes()].concat()
}

/// An entry holding `data` for the credential with `key`.
pub fn seal(key: &[u8; 32], data: &[u8]) -> Value {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    let nonce: [u8; 12] = rand::random();
    let packed = miniz_oxide::deflate::compress_to_vec(data, 9);
    let sealed = aes_gcm::Aes256Gcm::new(key.into())
        .encrypt(
            &nonce.into(),
            Payload {
                msg: &packed,
                aad: &aad(data.len()),
            },
        )
        .expect("in-memory encryption");
    Value::Map(vec![
        (Value::Integer(1.into()), Value::Bytes(sealed)),
        (Value::Integer(2.into()), Value::Bytes(nonce.to_vec())),
        (Value::Integer(3.into()), Value::Integer(data.len().into())),
    ])
}

/// `entry`'s data, if it belongs to the credential with `key`.
pub fn open(key: &[u8; 32], entry: &Value) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit, Payload};
    let get = |k: i64| {
        entry
            .as_map()?
            .iter()
            .find(|(f, _)| f.as_integer() == Some(k.into()))
            .map(|(_, v)| v)
    };
    let sealed = get(1)?.as_bytes()?;
    let nonce: [u8; 12] = get(2)?.as_bytes()?.as_slice().try_into().ok()?;
    let size = usize::try_from(get(3)?.as_integer()?).ok()?;
    let packed = aes_gcm::Aes256Gcm::new(key.into())
        .decrypt(
            &nonce.into(),
            Payload {
                msg: sealed,
                aad: &aad(size),
            },
        )
        .ok()?;
    let data = miniz_oxide::inflate::decompress_to_vec_with_limit(&packed, size).ok()?;
    (data.len() == size).then_some(data)
}

/// The data stored for `key` in the serialized array `bytes`.
pub fn read(bytes: &[u8], key: &[u8; 32]) -> Result<Vec<u8>, BlobError> {
    parse(bytes)?
        .iter()
        .find_map(|e| open(key, e))
        .ok_or(BlobError::Missing)
}

/// `bytes` with the entry for `key` replaced by `data`, other entries
/// kept. Damaged storage starts over empty.
pub fn write(bytes: &[u8], key: &[u8; 32], data: &[u8]) -> Vec<u8> {
    let mut entries: Vec<Value> = parse(bytes)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| open(key, e).is_none())
        .collect();
    entries.push(seal(key, data));
    serialize(&entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_empty_array_matches_the_spec() {
        // CTAP 2.1: the initial serialized large-blob array.
        assert_eq!(
            hex::encode(serialize(&[])),
            "8076be8b528d0075f7aae98d6fa57a6d3c"
        );
        assert_eq!(parse(&serialize(&[])), Ok(vec![]));
    }

    #[test]
    fn a_backup_is_written_and_read_back() {
        let key = [3u8; 32];
        let bytes = write(&serialize(&[]), &key, b"the backup");
        assert_eq!(read(&bytes, &key), Ok(b"the backup".to_vec()));
        assert_eq!(read(&bytes, &[4u8; 32]), Err(BlobError::Missing));
    }

    #[test]
    fn writing_again_replaces_ours_and_keeps_the_others() {
        let (ours, theirs) = ([3u8; 32], [4u8; 32]);
        let bytes = write(&serialize(&[]), &theirs, b"another app");
        let bytes = write(&bytes, &ours, b"v1");
        let bytes = write(&bytes, &ours, b"v2");
        assert_eq!(parse(&bytes).unwrap().len(), 2);
        assert_eq!(read(&bytes, &ours), Ok(b"v2".to_vec()));
        assert_eq!(read(&bytes, &theirs), Ok(b"another app".to_vec()));
    }

    #[test]
    fn a_damaged_array_is_refused() {
        let mut bytes = write(&serialize(&[]), &[3u8; 32], b"x");
        bytes[2] ^= 1;
        assert_eq!(read(&bytes, &[3u8; 32]), Err(BlobError::Corrupt));
        assert_eq!(parse(&[0x80]), Err(BlobError::Corrupt));
    }
}
