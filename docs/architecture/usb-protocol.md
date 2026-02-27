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
  --interpreter cardano-cbor.wasm \
  --output /mnt/usb \
  --label "Cardano Transaction" \
  --algorithm ed25519 \
  --key-id payment-0 \
  --signable hash-blake2b \
  --output-mode wasm-assemble
```

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
