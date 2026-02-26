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

- [x] `signer-sim` with minifb window + keyboard input
- [x] JSON rendering on simulated screen
- [x] End-to-end signing flow in simulator
- [x] Interactive setup flow: PIN entry, key generation/recovery, dual-USB export
- [x] Simulated secure element with PIN hash, session state, save/load to JSON
- [ ] `usb-pack` CLI for preparing test USB contents

## Phase 2: Cardano Interpreter

- [ ] `interpreters/cardano-cbor` — parse Cardano TX CBOR to JSON
- [ ] WASM `assemble()` — attach witness to transaction
- [ ] `HashThenSign` with Blake2b-256
- [ ] Test with real Cardano testnet transactions

## Phase 3: Secure Element Integration

- [ ] SE050 driver over I2C (HAL `SecureElement` trait)
- [ ] Key generation inside SE (private key never exported)
- [ ] PIN verification with hardware retry lockout
- [x] Public key export via USB (for on-chain registration)
- [ ] SE-side signing: Pi sends hash, SE returns signature
- [x] PIN entry UI (buttons + display)
- [x] Simulator mock SE (in-memory keys for desktop testing)
- [x] Seed backup to private USB + recovery from existing seed

## Phase 4: Raspberry Pi HAL

- [ ] `signer-pi` — framebuffer, GPIO buttons, USB mount, I2C secure element
- [ ] Cross-compile aarch64-unknown-linux-musl
- [ ] Test on physical Pi 4 or Pi Zero 2W with display + buttons + ATECC608B

## Phase 5: Buildroot Image

- [ ] External tree, kernel config, defconfig
- [ ] `just image` recipe
- [ ] Boot time optimization (target: <3s to PIN prompt)

## Phase 6: Hardening

- [ ] Security audit
- [ ] Secp256k1 ECDSA + Schnorr support
- [ ] Read-only rootfs (squashfs)
- [ ] Multiple key slots (up to 16 per SE)
- [ ] Key slot labelling (associate slot with blockchain/purpose)
