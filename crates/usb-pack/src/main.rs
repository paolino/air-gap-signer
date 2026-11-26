use usb_pack::cardano;

use clap::Parser;
use signer_core::spec::{
    HashAlgorithm, OutputSpec, SignAlgorithm, Signable, SignableSource, SigningSpec,
};
use std::fs;
use std::path::PathBuf;

/// Prepare USB stick contents for air-gapped signing.
#[derive(Parser)]
#[command(name = "usb-pack")]
struct Cli {
    /// Raw transaction payload file
    #[arg(long)]
    payload: PathBuf,

    /// WASM interpreter module
    #[arg(long)]
    interpreter: PathBuf,

    /// Output directory (will contain payload.bin, interpreter.wasm, sign.cbor)
    #[arg(long)]
    output: PathBuf,

    /// Human-readable label for the transaction
    #[arg(long, default_value = "Transaction")]
    label: String,

    /// Signing algorithm
    #[arg(long, default_value = "ed25519")]
    algorithm: String,

    /// Secure element key slot (0–15)
    #[arg(long)]
    key_slot: u8,

    /// Signable mode: whole, hash-blake2b, hash-sha256, cardano-body (Conway CBOR or CLI envelope)
    #[arg(long, default_value = "whole")]
    signable: String,

    /// Output mode: signature-only, append, wasm-assemble
    #[arg(long, default_value = "signature-only")]
    output_mode: String,
}

fn parse_algorithm(s: &str) -> SignAlgorithm {
    match s {
        "ed25519" => SignAlgorithm::Ed25519,
        "secp256k1-ecdsa" => SignAlgorithm::Secp256k1Ecdsa,
        "secp256k1-schnorr" => SignAlgorithm::Secp256k1Schnorr,
        other => panic!("unknown algorithm: {other}"),
    }
}

fn parse_signable(s: &str) -> Signable {
    match s {
        "whole" => Signable::Whole,
        "hash-blake2b" => Signable::HashThenSign {
            hash: HashAlgorithm::Blake2b256,
            source: SignableSource::Whole,
        },
        "hash-sha256" => Signable::HashThenSign {
            hash: HashAlgorithm::Sha256,
            source: SignableSource::Whole,
        },
        other => panic!("unknown signable mode: {other}"),
    }
}

fn parse_output_mode(s: &str) -> OutputSpec {
    match s {
        "signature-only" => OutputSpec::SignatureOnly,
        "append" => OutputSpec::AppendToPayload,
        "wasm-assemble" => OutputSpec::WasmAssemble,
        other => panic!("unknown output mode: {other}"),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    let (payload, signable) = if cli.signable == "cardano-body" {
        if cli.algorithm != "ed25519" {
            return Err("Cardano payment keys use ed25519".into());
        }
        let (bytes, signable) = cardano::prepare(fs::read(&cli.payload)?)?;
        (Some(bytes), signable)
    } else {
        (None, parse_signable(&cli.signable))
    };

    let spec = SigningSpec {
        review_protocol: (cli.signable == "cardano-body")
            .then_some(signer_core::spec::ReviewProtocol::CardanoConway),
        label: cli.label,
        signable,
        algorithm: parse_algorithm(&cli.algorithm),
        key_slot: cli.key_slot,
        output: parse_output_mode(&cli.output_mode),
    };

    fs::create_dir_all(&cli.output).expect("failed to create output directory");

    // Copy payload
    let target = cli.output.join("payload.bin");
    if let Some(bytes) = payload {
        fs::write(target, bytes)?;
    } else {
        fs::copy(&cli.payload, target)?;
    }

    // Copy interpreter
    fs::copy(&cli.interpreter, cli.output.join("interpreter.wasm"))
        .expect("failed to copy interpreter");

    // Write signing spec
    let cbor = spec.to_cbor().expect("failed to serialize signing spec");
    fs::write(cli.output.join("sign.cbor"), cbor).expect("failed to write sign.cbor");

    eprintln!("USB stick contents written to {:?}", cli.output);
    Ok(())
}
