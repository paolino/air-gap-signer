use crate::cardano;
use ed25519_dalek::{Signature, VerifyingKey};
use serde_json::json;
use signer_core::crypto::extract_signable;
use std::{error::Error, io::Write, path::Path};
pub fn write_verified(
    input: Vec<u8>,
    key: [u8; 32],
    raw_signature: &[u8],
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    let (payload, signable) = cardano::prepare(input)?;
    let digest = extract_signable(&payload, &signable)?;
    let signature = Signature::from_slice(raw_signature)?;
    VerifyingKey::from_bytes(&key)?.verify_strict(&digest, &signature)?;

    let mut witness = Vec::new();
    pallas_codec::minicbor::Encoder::new(&mut witness)
        .array(2)?
        .bytes(&key)?
        .bytes(&signature.to_bytes())?;
    let envelope = json!({
        "type": "TxWitness ConwayEra",
        "description": "Key Witness ShelleyEra",
        "cborHex": hex::encode(witness),
    });
    let dir = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer_pretty(file.as_file_mut(), &envelope)?;
    writeln!(file)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(output)?;
    println!("Verified witness for {}", hex::encode(digest));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};

    #[test]
    fn witness_requires_the_original_body_and_expected_key_and_preserves_existing_output() {
        let tx=hex::decode("84a300d9010281825820000000000000000000000000000000000000000000000000000000000000000000018182581d6096d3060bc838cc8ed9471d456e886f60ac1976d9aecc6db6927e64c41a001e8480021a00029810a0f5f6").unwrap();
        let (payload, signable) = cardano::prepare(tx.clone()).unwrap();
        let digest = extract_signable(&payload, &signable).unwrap();
        let signer = SigningKey::from_bytes(&[3; 32]);
        let key = signer.verifying_key().to_bytes();
        let sig = signer.sign(&digest).to_bytes();
        let dir = tempfile::tempdir().unwrap();
        let out = dir.path().join("payment.witness");
        let mut changed = tx.clone();
        changed[10] ^= 1;
        assert!(write_verified(changed, key, &sig, &out).is_err());
        assert!(write_verified(
            tx.clone(),
            SigningKey::from_bytes(&[4; 32]).verifying_key().to_bytes(),
            &sig,
            &out
        )
        .is_err());
        assert!(!out.exists());
        write_verified(tx.clone(), key, &sig, &out).unwrap();
        let saved = std::fs::read(&out).unwrap();
        assert!(write_verified(tx, key, &sig, &out).is_err());
        assert_eq!(std::fs::read(out).unwrap(), saved);
    }
}
