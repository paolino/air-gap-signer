# Signing Spec

The `sign.cbor` file on the USB stick tells the device how to sign the transaction.

## Structure

```rust
struct SigningSpec {
    label: String,          // Human-readable label ("Cardano Transaction")
    signable: Signable,     // What bytes to sign
    algorithm: SignAlgorithm, // Which signing algorithm
    key_id: String,         // Key identifier in the device keystore
    output: OutputSpec,     // How to produce the output
}
```

## Signable

Determines which bytes from the payload get signed:

| Variant | Description |
|---------|-------------|
| `Whole` | Sign the entire payload as-is |
| `Range { offset, length }` | Sign a byte range within the payload |
| `HashThenSign { hash, source }` | Hash first (Blake2b-256, SHA-256, or SHA3-256), then sign the hash |

`HashThenSign` is the most common mode — Cardano signs the Blake2b-256 hash of the transaction body, not the raw bytes.

## Algorithms

| Algorithm | Key size | Signature size | Use case |
|-----------|----------|----------------|----------|
| Ed25519 | 32 bytes | 64 bytes | Cardano, Solana |
| Secp256k1 ECDSA | 32 bytes | 64-72 bytes | Bitcoin, Ethereum |
| Secp256k1 Schnorr | 32 bytes | 64 bytes | Bitcoin Taproot |

## Output modes

| Mode | Behavior |
|------|----------|
| `SignatureOnly` | Write raw signature bytes to `signed.bin` |
| `AppendToPayload` | Concatenate payload + signature |
| `WasmAssemble` | Call the interpreter's `assemble()` function to produce chain-specific format |

## Encoding

The spec is CBOR-encoded (via `ciborium` / serde) for compact binary representation. The `usb-pack` CLI generates it from command-line flags.
