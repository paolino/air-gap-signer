//! Verify the panel's public output before converting it to a Conway key witness.
use clap::Parser;
use ed25519_dalek::{Signature, VerifyingKey};
use serde_json::json;
use signer_core::crypto::extract_signable;
use std::{error::Error, fs, io::Write, path::PathBuf};
use usb_pack::cardano;

#[derive(Parser)]
struct Cli {
    /// The transaction CBOR or cardano-cli text envelope used to prepare the stick.
    #[arg(long)]
    payload: PathBuf,
    /// The panel's exported 32-byte payment verification key.
    #[arg(long)]
    public_key: PathBuf,
    /// The 64-byte Ed25519 signature returned by the panel (RFC 8032 order).
    #[arg(long)]
    signature: PathBuf,
    /// New cardano-cli key witness file.
    #[arg(long)]
    output: PathBuf,
}

fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let (payload, signable) = cardano::prepare(fs::read(cli.payload)?)?;
    let digest = extract_signable(&payload, &signable)?;
    let key: [u8; 32] = fs::read(cli.public_key)?
        .try_into()
        .map_err(|_| "expected 32-byte verification key")?;
    let signature = Signature::from_slice(&fs::read(cli.signature)?)?;
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
    let dir = cli
        .output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer_pretty(file.as_file_mut(), &envelope)?;
    writeln!(file)?;
    file.as_file().sync_all()?;
    file.persist_noclobber(cli.output)?;
    println!("Verified witness for {}", hex::encode(digest));
    Ok(())
}
