# Air-Gapped Transaction Signer

A blockchain-agnostic air-gapped signing device for Raspberry Pi 4.

The device never touches a network. Transactions arrive on a USB stick, get displayed for human review, and are signed with keys that never leave the device.

## How it works

```mermaid
sequenceDiagram
    participant USB as USB Stick
    participant Dev as Signer Device
    participant User as Human

    USB->>Dev: Insert (payload.bin, interpreter.wasm, sign.cbor)
    Dev->>Dev: Run WASM interpret(payload) → JSON
    Dev->>User: Display transaction details on screen
    User->>Dev: CONFIRM / REJECT
    Dev->>Dev: Sign payload bytes
    Dev->>USB: Write signed.bin
    Dev->>User: "DONE — REMOVE USB"
```

## Key properties

- **Blockchain-agnostic** — the USB stick carries a WASM interpreter that knows
  how to parse the specific transaction format. The device only knows how to
  run WASM, render JSON, and sign bytes.
- **Air-gapped** — no networking hardware enabled. No WiFi, no Bluetooth,
  no Ethernet. Only USB mass storage for data transfer.
- **Sandboxed interpreters** — WASM modules run with zero host imports,
  fuel-metered (10M ops), and memory-capped (16 MB).
- **Minimal attack surface** — Buildroot Linux with a stripped kernel,
  read-only rootfs, our binary as PID 1.

## Project status

Currently in **Phase 0** — foundation crate with signing spec types,
WASM sandbox, Ed25519 crypto, and a trivial echo interpreter.
See [Phases](dev/phases.md) for the roadmap.
