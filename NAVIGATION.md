# Navigation

Codebase overview for the **air-gap-signer** Rust workspace -- a blockchain-agnostic
air-gapped signing device. Transactions arrive on a USB stick, are interpreted by
a sandboxed WASM module, displayed for human review, and signed by a hardware secure
element that never exposes private keys.

## Workspace layout

| Crate | Kind | Description |
|-------|------|-------------|
| `crates/signer-core` | lib | Pure logic: signing spec types, WASM sandbox (wasmtime), hash extraction, JSON-to-display flattening |
| `crates/signer-hal`  | lib | Hardware abstraction layer -- traits for Display, Buttons, UsbMount, SecureElement |
| `crates/signer-sim`  | bin | Desktop simulator: minifb window, simulated SE with PIN/keystore, full setup + signing flow |
| `crates/usb-pack`    | bin | CLI to prepare a USB stick (copies payload, interpreter WASM, generates `sign.cbor`) |
| `interpreters/echo-hex` | cdylib (WASM) | Test WASM interpreter: echoes payload as `{"hex":"...","length":N}` |

## `crates/signer-core` -- pure logic (library)

| File | Description |
|------|-------------|
| [lib.rs](../../search?q=path:crates/signer-core/src/lib.rs) | Module re-exports: `crypto`, `display`, `spec`, `wasm_sandbox` |
| [spec.rs](../../search?q=path:crates/signer-core/src/spec.rs) | `SigningSpec` and supporting enums (`Signable`, `SignAlgorithm`, `HashAlgorithm`, `OutputSpec`). CBOR round-trip via ciborium. Deserialized from `sign.cbor` on the USB stick |
| [wasm_sandbox.rs](../../search?q=path:crates/signer-core/src/wasm_sandbox.rs) | Fuel-metered, memory-capped wasmtime `Sandbox`. Zero host imports. Exposes `interpret(payload) -> JSON string` and `assemble(payload, sig) -> bytes`. 10M fuel ops, 16 MB memory cap |
| [crypto.rs](../../search?q=path:crates/signer-core/src/crypto.rs) | `extract_signable` -- selects/hashes the byte range to sign per the `Signable` spec. Supports Blake2b-256, SHA-256, SHA3-256 |
| [display.rs](../../search?q=path:crates/signer-core/src/display.rs) | Flattens serde_json `Value` into `Vec<DisplayLine>` for rendering on a simple framebuffer. `json_to_lines` + `render_text` |
| [tests/wasm_integration.rs](../../search?q=path:crates/signer-core/tests/wasm_integration.rs) | Integration tests for the WASM sandbox using the echo-hex interpreter |

## `crates/signer-hal` -- hardware abstraction (library)

| File | Description |
|------|-------------|
| [lib.rs](../../search?q=path:crates/signer-hal/src/lib.rs) | Trait definitions and shared types. `Display` (clear, show_message, show_lines), `Buttons` (wait_event -> ButtonEvent), `UsbMount` (wait_insert, mount, read/write files, unmount), `SecureElement` (set_pin, verify_pin, generate_key, sign, import_key, export_seed). Also defines `HalError`, `ButtonEvent`, `UsbContents` |

## `crates/signer-sim` -- desktop simulator (binary)

Entry point: [main.rs](../../search?q=path:crates/signer-sim/src/main.rs)

| File | Description |
|------|-------------|
| [main.rs](../../search?q=path:crates/signer-sim/src/main.rs) | CLI (clap): `--usb-dir` and `--keystore`. Creates `SimHal` (wraps display + buttons), `SimUsb`, `SimSecureElement`, then calls `flow::run` |
| [flow.rs](../../search?q=path:crates/signer-sim/src/flow.rs) | Core state machine. `run` dispatches to setup or PIN-verify then `run_loop`. `run_setup` handles first-time provisioning (PIN entry, key generation or recovery from seed, public key export). `run_once` is a single signing cycle: read USB -> WASM interpret -> scrollable review -> sign -> write output. `enter_pin` does digit-by-digit PIN entry via Up/Down/Confirm/Reject |
| [display.rs](../../search?q=path:crates/signer-sim/src/display.rs) | `SimDisplay` -- 640x480 minifb window with an embedded 8x8 bitmap font. Renders `DisplayLine` slices with scroll offset. Implements `signer_hal::Display` |
| [buttons.rs](../../search?q=path:crates/signer-sim/src/buttons.rs) | Maps minifb key events to `ButtonEvent`. `poll_event` (non-blocking) and `wait_event` (blocking at ~60 fps). Enter=Confirm, Escape=Reject, Arrow keys=Up/Down |
| [usb.rs](../../search?q=path:crates/signer-sim/src/usb.rs) | `SimUsb` -- directory-based USB simulation. Polls for `payload.bin`, `interpreter.wasm`, `sign.cbor`. Writes `signed.bin`. Implements `signer_hal::UsbMount` |
| [keystore.rs](../../search?q=path:crates/signer-sim/src/keystore.rs) | `SimSecureElement` -- JSON-backed keystore on disk. Stores SHA-256 PIN hash, Ed25519 seeds per slot. Tracks per-session PIN verification. Implements `signer_hal::SecureElement` |

## `crates/usb-pack` -- USB preparation CLI (binary)

Entry point: [main.rs](../../search?q=path:crates/usb-pack/src/main.rs)

| File | Description |
|------|-------------|
| [main.rs](../../search?q=path:crates/usb-pack/src/main.rs) | CLI (clap): `--payload`, `--interpreter`, `--output`, `--label`, `--algorithm` (ed25519/secp256k1-ecdsa/secp256k1-schnorr), `--key-slot`, `--signable` (whole/hash-blake2b/hash-sha256), `--output-mode` (signature-only/append/wasm-assemble). Copies files and writes `sign.cbor` |

## `interpreters/echo-hex` -- test WASM module (cdylib)

| File | Description |
|------|-------------|
| [lib.rs](../../search?q=path:interpreters/echo-hex/src/lib.rs) | `#![no_std]` WASM module. Exports `alloc` (bump allocator over WASM linear memory) and `interpret` (returns `{"hex":"...","length":N}` as length-prefixed UTF-8). Uses `__heap_base` linker symbol for heap start |

## Build recipes (`justfile`)

| Recipe | What it does |
|--------|-------------|
| `just build` | Build all workspace crates (excluding echo-hex) |
| `just build-wasm` | Build WASM interpreters to `wasm32-unknown-unknown` |
| `just test` | Build WASM first, then run all workspace tests |
| `just ci` | format-check + lint + build + build-wasm + test + gerbers |
| `just sim` | Run the desktop simulator (`--usb-dir ./test-usb`) |
| `just format` | cargo fmt |
| `just lint` | clippy with `-D warnings` |

## Data flow (signing cycle)

```
USB stick              signer-core                    signer-hal
---------              -----------                    ----------
payload.bin  ------>   Sandbox::interpret(payload)     Display::show_lines
interpreter.wasm       -> JSON for human review  ---->  (scrollable)
sign.cbor    ------>   SigningSpec::from_cbor           Buttons::wait_event
                       extract_signable(payload, spec)  -> Confirm/Reject
                       -> hash bytes             ---->  SecureElement::sign
                       OutputSpec dispatch              UsbMount::write_output
                       -> signed.bin             <----
```
