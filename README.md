# Air-Gapped Transaction Signer

Blockchain-agnostic air-gapped signing device for Raspberry Pi (4 or Zero 2W).

Transactions arrive on a USB stick, get displayed for human review, and are signed by a hardware secure element (SE050) that never exposes private keys. The USB stick carries a WASM interpreter that knows how to parse the specific transaction format — the signer itself only knows how to run WASM, render JSON, and send hashes to the secure element.

## How it works

```
First boot (setup):
  SET PIN → CONFIRM PIN
  → Insert private USB → generate key (or recover from seed.bin)
  → Insert public USB → export pubkey.bin
  → Setup complete

Signing USB stick contains:
  payload.bin        — raw transaction bytes
  interpreter.wasm   — WASM module: parse payload → JSON for display
  sign.cbor          — signing spec: algorithm, key slot, what to sign

Normal boot:
  PIN entry → Secure element unlocks → Idle
  → Insert USB → Run WASM → Display transaction → Confirm/Reject
  → Sign → Write signed.bin → Remove USB
```

## Properties

- **Air-gapped** — no networking. Only USB mass storage for data transfer.
- **Secure element** — private keys live in an SE050 chip. PIN retry lockout in hardware. Stolen SD card is worthless. Seed backup via private USB for recovery.
- **Blockchain-agnostic** — WASM interpreters handle any transaction format.
- **Sandboxed** — interpreters run with zero host imports, fuel-metered, memory-capped.
- **Minimal** — Buildroot Linux, stripped kernel, binary as PID 1, read-only rootfs.

## Building

Requires [Nix](https://nixos.org/download/) with flakes enabled.

```bash
nix develop
just ci         # format, lint, build, test
just sim        # run desktop simulator (setup on first run)
just serve-docs # local documentation at http://127.0.0.1:8000
```

## Project structure

```
crates/
  signer-core/     Pure logic: spec types, WASM sandbox, hash extraction, display
  signer-hal/      Hardware abstraction traits
  signer-sim/      Desktop simulator with simulated SE, PIN, keystore
  usb-pack/        CLI to prepare USB sticks

interpreters/
  echo-hex/        Test interpreter (hex dump)
```

## Documentation

[paolino.github.io/air-gap-signer](https://paolino.github.io/air-gap-signer/)

## License

Apache-2.0
