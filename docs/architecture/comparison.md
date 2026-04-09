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

- **Ed25519 support** — Keycard's SIGN APDU
  [specification](https://keycard.tech/docs/apdu/sign.html) defines
  P2=0x01 for EdDSA/Ed25519, but the implementation only supports
  P2=0x00 (secp256k1) today. The underlying JavaCard chip
  (NXP J3H082/J3H145, JavaCard 3.0.4) lacks native Ed25519 —
  that API (`Signature.ALG_EDDSA_ED25519`) requires JavaCard 3.1+.
  Adding Ed25519 would need either a chip upgrade or a software
  implementation on the card, which is slow and exposes side-channel
  risk. Cardano requires Ed25519, so Keycard cannot sign Cardano
  transactions today.
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

## Ed25519 gap — technical details

Keycard's SIGN APDU protocol reserves P2 values for multiple
algorithms: secp256k1 ECDSA (0x00), Ed25519 (0x01), BLS12-381
(0x02), and BIP340 Schnorr (0x03). Only secp256k1 is implemented.

The smartcard chip (NXP JCOP3 — J3H082 or J3H145) runs JavaCard
3.0.4. The `Signature.ALG_EDDSA_ED25519` constant was introduced
in JavaCard 3.1. Even newer JCOP chips (J3R180, JavaCard 3.0.5) do
not expose Ed25519 in hardware.

The Keycard Shell (STM32H573 MCU) includes a software secp256k1
implementation but no Ed25519 code. Signing is delegated to the
smartcard via APDU — the Shell does not sign independently.

Possible paths for Keycard to add Ed25519:

- **Chip upgrade** to a JavaCard 3.1+ secure element with native
  Ed25519. This changes the BOM and may require re-certification.
- **Software Ed25519 on the card** in JavaCard bytecode. Feasible
  but slow (~seconds per signature) and harder to protect against
  timing side-channels on a constrained chip.
- **Software Ed25519 on the Shell MCU**. The STM32H573 has
  enough power, but the private key would leave the secure element,
  breaking the core security property.

Air-Gap-Signer uses the NXP SE050, which supports Ed25519 natively
in hardware (the chip signs internally — the private key never
reaches the host CPU).
