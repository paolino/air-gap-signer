# Getting Started

## Prerequisites

- [Nix](https://nixos.org/download/) with flakes enabled

## Setup

```bash
git clone https://github.com/paolino/air-gap-signer.git
cd air-gap-signer
nix develop
```

The nix shell provides Rust (with `wasm32-unknown-unknown` target), `just`, `wasmtime`, and `mkdocs`.

## Build

```bash
just build       # Build all host crates
just build-wasm  # Build WASM interpreters
```

## Test

```bash
just test        # Run all tests (builds WASM first)
```

## Desktop simulator

```bash
just sim         # Run simulator (creates keys.json on first run)
```

First run triggers the setup flow:

1. **SET PIN** — enter 4-digit PIN with Up/Down + Enter
2. **CONFIRM PIN** — re-enter to confirm
3. **INSERT PRIVATE USB** — if `seed.bin` exists in the USB directory, the key is recovered; otherwise a new key is generated and `seed.bin` is written
4. **INSERT PUBLIC USB** — `pubkey.bin` is written for on-chain registration
5. **SETUP COMPLETE** — device enters the signing loop

Subsequent runs skip setup and prompt for PIN entry directly.

## Full CI locally

```bash
just ci          # format-check, lint, build, build-wasm, test
```

## Documentation

```bash
just serve-docs  # Local preview at http://127.0.0.1:8000
just build-docs  # Build static site
```

## Project layout

```
crates/
  signer-core/     # Pure logic (spec, WASM sandbox, crypto, display)
  signer-hal/      # Hardware abstraction traits
  signer-pi/       # Raspberry Pi implementation (Phase 4)
  signer-bin/      # PID 1 binary (Phase 4)
  signer-sim/      # Desktop simulator (Phase 1)
  usb-pack/        # CLI to prepare USB sticks

interpreters/
  echo-hex/        # Trivial test interpreter (hex dump)
  cardano-cbor/    # Cardano TX parser (Phase 2)

buildroot/         # Minimal Linux image (Phase 5)
```
