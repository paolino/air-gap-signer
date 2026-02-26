# Development Phases

## Phase 0: Foundation :material-check:

- [x] Cargo workspace + flake.nix + justfile
- [x] `signer-core/spec.rs` — SigningSpec CBOR types
- [x] `signer-core/wasm_sandbox.rs` — Wasmtime with fuel/memory limits
- [x] `signer-core/crypto.rs` — Ed25519 signing + hash dispatch
- [x] `signer-core/keystore.rs` — Plaintext keystore (dev only)
- [x] `signer-core/display.rs` — JSON to display lines
- [x] `signer-hal` — Hardware abstraction traits
- [x] `echo-hex` WASM interpreter
- [x] 17 tests passing

## Phase 1: Desktop Simulator

- [ ] `signer-sim` with minifb window + keyboard input
- [ ] JSON rendering on simulated screen
- [ ] End-to-end signing flow in simulator
- [ ] `usb-pack` CLI for preparing test USB contents

## Phase 2: Cardano Interpreter

- [ ] `interpreters/cardano-cbor` — parse Cardano TX CBOR to JSON
- [ ] WASM `assemble()` — attach witness to transaction
- [ ] `HashThenSign` with Blake2b-256
- [ ] Test with real Cardano testnet transactions

## Phase 3: Key Management

- [ ] AES-256-GCM encrypted keystore
- [ ] Argon2id PIN derivation
- [ ] PIN entry UI
- [ ] Key provisioning flow
- [ ] Zeroize secrets on drop

## Phase 4: Raspberry Pi HAL

- [ ] `signer-pi` — framebuffer, GPIO buttons, USB mount, SD storage
- [ ] Cross-compile aarch64-unknown-linux-musl
- [ ] Test on physical Pi 4 with HDMI + buttons

## Phase 5: Buildroot Image

- [ ] External tree, kernel config, defconfig
- [ ] `just image` recipe
- [ ] Boot time optimization (target: <3s to PIN prompt)

## Phase 6: Hardening

- [ ] Security audit
- [ ] Secp256k1 ECDSA + Schnorr support
- [ ] Read-only rootfs (squashfs)
- [ ] Wipe-after-3-failures
- [ ] Multiple key slots
