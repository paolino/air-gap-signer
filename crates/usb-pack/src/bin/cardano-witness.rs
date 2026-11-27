//! Verify the panel's public output before converting it to a Conway key witness.
use clap::Parser;
use std::{error::Error, fs, path::PathBuf};

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
    let key = fs::read(cli.public_key)?
        .try_into()
        .map_err(|_| "expected 32-byte verification key")?;
    usb_pack::witness::write_verified(
        fs::read(cli.payload)?,
        key,
        &fs::read(cli.signature)?,
        &cli.output,
    )
}
