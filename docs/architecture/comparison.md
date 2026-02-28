# Comparison with Keycard

[Keycard](https://keycard.tech) is a commercial open-source hardware wallet
by Status. It offers an NFC smartcard and the Keycard Shell (an air-gapped
device with QR scanning). This page compares the two projects.

## Shared goals

- Private key never leaves the secure element
- EAL6+ certified hardware
- Fully open-source (hardware and software)
- Air-gapped operation
- Clear signing — users see what they approve before confirming
- Seed backup and recovery
- PIN protection with hardware-enforced retry lockout
- Multi-chain support

## Where Air-Gap-Signer differs

- **Ed25519 support** — Keycard only supports secp256k1. Cardano
  requires Ed25519, so Keycard cannot sign Cardano transactions.
- **Blockchain-agnostic by design** — chain support is not hardcoded.
  A WASM interpreter on the USB stick parses any transaction format.
  Adding a new blockchain means writing a small WASM module, not
  modifying the device firmware.
- **USB-based air gap** — data travels on a USB stick (read-only mount,
  unmount before processing). Keycard uses NFC or QR codes via the
  Shell device. USB avoids wireless proximity risks.
- **DIY / open hardware** — runs on a Raspberry Pi with an off-the-shelf
  SE050 breakout board. No vendor dependency for hardware supply.
- **WASM sandbox** — the interpreter is fuel-metered (10M ops) and
  memory-capped (16 MB) with zero host imports. A malicious WASM
  module cannot escape the sandbox.

## Where Keycard differs

- **Credit card form factor** — fits in a wallet. The Pi-based device
  is bulkier.
- **NFC communication** — tap to sign. No USB stick handling.
- **Wallet ecosystem** — compatible with 10+ existing wallets
  out of the box.
- **Production-ready** — shipping product with established supply
  chain. Air-Gap-Signer is in early development (Phase 1).
- **Multi-card encrypted backup** — Keycard supports cloning keys
  across multiple cards as an alternative to seed phrases.

## Summary

Keycard validates the security model (secure element, PIN lockout,
clear signing, air gap). The key architectural difference is
flexibility: Air-Gap-Signer treats the transaction format as a
pluggable concern (WASM interpreters) rather than a fixed feature
set. This matters for Cardano and any future chain that uses
Ed25519 or non-standard transaction formats.
