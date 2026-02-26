# Air-Gapped Transaction Signer

Blockchain-agnostic air-gapped signing device for Raspberry Pi 4.

Transactions arrive on a USB stick, get displayed for human review, and are signed with keys that never leave the device. The USB stick carries a WASM interpreter that knows how to parse the specific transaction format — the signer itself only knows how to run WASM, render JSON, and sign bytes.

## How it works

```
USB stick contains:
  payload.bin        — raw transaction bytes
  interpreter.wasm   — WASM module: parse payload → JSON for display
  sign.cbor          — signing spec: algorithm, key ID, what to sign

Device flow:
  Boot → PIN → Decrypt keystore → Idle
       → Insert USB → Run WASM → Display transaction → Confirm/Reject
       → Sign → Write signed.bin → Remove USB
```

## Properties

- **Air-gapped** — no networking. Only USB mass storage for data transfer.
- **Blockchain-agnostic** — WASM interpreters handle any transaction format.
- **Sandboxed** — interpreters run with zero host imports, fuel-metered, memory-capped.
- **Minimal** — Buildroot Linux, stripped kernel, binary as PID 1, read-only rootfs.

## Building

Requires [Nix](https://nixos.org/download/) with flakes enabled.

```bash
nix develop
just ci        # format, lint, build, test
just serve-docs # local documentation at http://127.0.0.1:8000
```

## Project structure

```
crates/
  signer-core/     Pure logic: spec types, WASM sandbox, crypto, display
  signer-hal/      Hardware abstraction traits
  signer-pi/       Raspberry Pi implementation
  signer-sim/      Desktop simulator
  signer-bin/      PID 1 binary
  usb-pack/        CLI to prepare USB sticks

interpreters/
  echo-hex/        Test interpreter (hex dump)

buildroot/         Minimal Linux image
```

## Documentation

[paolino.github.io/air-gap-signer](https://paolino.github.io/air-gap-signer/)

## License

Apache-2.0
