# USB Stick Protocol

The USB stick is the only data channel between the outside world and the signing device. It carries three files:

| File | Purpose |
|------|---------|
| `payload.bin` | Raw transaction bytes |
| `interpreter.wasm` | WASM module that parses the payload into human-readable JSON |
| `sign.cbor` | Signing specification: algorithm, key ID, what bytes to sign |

## Preparing a USB stick

Use the `usb-pack` CLI:

```bash
usb-pack \
  --payload tx.raw \
  --interpreter cardano_tx.wasm \
  --output /mnt/usb \
  --label "Cardano Transaction" \
  --algorithm ed25519 \
  --key-slot 0 \
  --signable cardano-body \
  --output-mode signature-only
```

For Conway transactions, `--signable cardano-body` accepts raw transaction
CBOR or the `Tx ConwayEra` text envelope from `cardano-cli`. It writes the full
transaction CBOR for review and selects the original body bytes for Blake2b-256
hashing. The resulting digest is the transaction ID. The package itself carries
no signing authority. A Cardano witness requires both the payment verification
key and its signature over that digest.

## Mount protocol

1. Device detects USB insertion via udev/poll
2. Mounts the first VFAT partition **read-only**
3. Reads the three files into memory
4. Unmounts before processing (minimizes USB exposure)
5. After signing, remounts **read-write** to write `signed.bin`
6. Unmounts and signals completion

During signing, the device only reads these three files — nothing else.

## Setup USB protocol

During first-time provisioning, the device uses **two separate USB sticks**:

### Private USB (seed backup)

| File | Purpose |
|------|---------|
| `seed.bin` | 32-byte Ed25519 seed (raw bytes) |

- If `seed.bin` exists when inserted, the device **recovers** the key from it (import).
- If `seed.bin` does not exist, the device **generates** a new key and writes `seed.bin`.
- **Store this USB offline in a safe place.** It is the only way to recover funds if the device is lost or stolen.

### Public USB (key export)

| File | Purpose |
|------|---------|
| `pubkey.bin` | 32-byte Ed25519 public key (raw bytes) |

- Written during setup after key generation/recovery.
- Used to register the public key on-chain.

## Assemble a Conway key witness on the host

Given a raw 32-byte panel public key and a 64-byte signature, the host can check
the signature and write a witness file for the same transaction.

```bash
cardano-witness --payload tx.raw --public-key practice-public.key \
  --signature signed.bin --output payment.witness
cardano-cli conway transaction assemble --tx-body-file tx.raw \
  --witness-file payment.witness --out-file signed.tx
```

The example assumes `tx.raw` is a Conway text envelope. The packer and witness
tool also accept raw transaction CBOR. The regular Pi service currently offers
public key inventory and transaction review. Its signing and signature export
are pending.
