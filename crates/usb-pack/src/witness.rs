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
    write_result(input, key, raw_signature, output, false)
}

/// Write a complete signed Conway transaction for the panel's supported self-payment.
pub fn write_signed_transaction(
    input: Vec<u8>,
    key: [u8; 32],
    signature: &[u8],
    output: &Path,
) -> Result<(), Box<dyn Error>> {
    write_result(input, key, signature, output, true)
}

fn write_result(
    input: Vec<u8>,
    key: [u8; 32],
    raw_signature: &[u8],
    output: &Path,
    assemble: bool,
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
    let envelope = if assemble {
        let (_, body) = cardano_tx::decode_conway(&payload)?;
        // The panel accepts a canonical four-item transaction with an empty
        // witness set, a true validity flag, and null auxiliary data.
        if payload.first() != Some(&0x84) || payload.get(body.end..) != Some(&[0xa0, 0xf5, 0xf6]) {
            return Err("expected an unsigned panel self-payment".into());
        }
        let mut signed = payload[..body.end].to_vec();
        let mut encoder = pallas_codec::minicbor::Encoder::new(&mut signed);
        encoder
            .map(1)?
            .u8(0)?
            .tag(pallas_codec::minicbor::data::Tag::new(258))?
            .array(1)?;
        signed.extend_from_slice(&witness);
        signed.extend_from_slice(&payload[body.end + 1..]);
        json!({"type":"Tx ConwayEra","description":"Signed preprod self-payment","cborHex": hex::encode(signed)})
    } else {
        json!({"type":"TxWitness ConwayEra","description":"Key Witness ShelleyEra","cborHex":hex::encode(witness)})
    };
    let dir = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer_pretty(file.as_file_mut(), &envelope)?;
    writeln!(file)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(output)?;
    println!("Verified signature for {}", hex::encode(digest));
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
        let signed_path = dir.path().join("payment.signed");
        assert!(write_signed_transaction(tx.clone(), [0; 32], &sig, &signed_path).is_err());
        assert!(!signed_path.exists());
        write_signed_transaction(tx.clone(), key, &sig, &signed_path).unwrap();
        let (signed, spec) = cardano::prepare(std::fs::read(&signed_path).unwrap()).unwrap();
        assert_eq!(extract_signable(&signed, &spec).unwrap(), digest);
        let (decoded, _) = cardano_tx::decode_conway(&signed).unwrap();
        let keys = decoded
            .transaction_witness_set
            .vkeywitness
            .as_ref()
            .unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].vkey.as_slice(), key);
        assert_eq!(keys[0].signature.as_slice(), sig);
        assert!(write_signed_transaction(tx.clone(), key, &sig, &signed_path).is_err());
        let saved = std::fs::read(&out).unwrap();
        assert!(write_verified(tx, key, &sig, &out).is_err());
        assert_eq!(std::fs::read(out).unwrap(), saved);
    }
}
