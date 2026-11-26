# Recognizing the signer with its boot card installed

The [secure project definition](secure-project.md) records the selected architecture, operating procedure and release criteria.

Investigation of upstream main at 6d232fac6144ef49c0769571e5d6672a337e0f4f. Evidence comes from source inspection, issue history and manufacturer documentation. The acceptance cases below still need hardware tests.

## What the owner should experience

Leave the microSD in the case. A verified copy of the approved image should work as a recovery spare. Before entering a PIN, the owner checks on a separately enrolled verifier that their signer has answered a fresh challenge and is running approved review software. An offline phone is one possible verifier. Enrollment must happen while the owner already trusts the hardware. After wallet recovery on replacement hardware, enroll its new device identity deliberately. Restoring a wallet seed must never restore the old device identity.

The earlier issue #32 decision required a RAM-only boot and owner custody of the SD. That addresses an unattended boot medium on an otherwise trusted machine. A replacement machine can still ask for the PIN. The installed-card direction supersedes the daily removal workflow for this local enclosure draft. [Issue #32 and its recorded decision](https://github.com/paolino/air-gap-signer/issues/32)

## What the Pi 3B prototype does

The user-facing binary reads a transaction stick and enters the review session with NoKeys. There is no device-recognition or PIN step in this entry point. The separate SE051 bench tool can sign with a populated slot. Current key creation handles the seed on the Pi and imports it without an explicit object policy. The project tracks authenticated chip sessions and encrypted key import separately. A recoverable wallet key would also identify any replacement device that restored the same wallet. [User-facing binary](https://github.com/paolino/air-gap-signer/blob/6d232fac6144ef49c0769571e5d6672a337e0f4f/crates/signer-pi/src/bin/signer-pi.rs#L110) · [Key driver](https://github.com/paolino/air-gap-signer/blob/6d232fac6144ef49c0769571e5d6672a337e0f4f/crates/signer-pi/src/se051.rs#L177) · [Encrypted channel #38](https://github.com/paolino/air-gap-signer/issues/38) · [Chip PIN #39](https://github.com/paolino/air-gap-signer/issues/39)

The USB stick supplies both the interpreter that explains a transaction and the rules selecting bytes to sign. The Pi computes these independently. An authentic display can therefore present an untrustworthy description even after a successful boot check. Require an approved decoder that derives the review and the signing operation from the same immutable request. Issue #21 also tracks panel transfer completion and accepting a fresh button press. [Review code](https://github.com/paolino/air-gap-signer/blob/6d232fac6144ef49c0769571e5d6672a337e0f4f/crates/signer-pi/src/flow.rs#L28) · [Review binding #10](https://github.com/paolino/air-gap-signer/issues/10) · [Panel and button #21](https://github.com/paolino/air-gap-signer/issues/21)

## Two hardware routes

**Verified Linux host.** A non-wireless CM4 Lite can use internal microSD on its carrier. Raspberry Pi documents a hardware-enforced chain to a signed boot image. Place the signer and approved interpreters inside the authenticated image, or authenticate the root filesystem from it. Define a minimum accepted version for rollback and recovery. The CM4 has two high-density connectors and requires a carrier redesign. It preserves the existing Linux application. To satisfy whole-device recognition, the design still needs a demonstrated way to bind the enrolled identity and trusted boot state to this particular host, protect its credentials, and restrict use of the SE051. A signed boot image alone gives the phone no fresh evidence. The Pi 3B lacks this documented production boot path. [Raspberry Pi boot chain](https://www.raspberrypi.com/documentation/security/security-architecture.html) · [Supported provisioning targets](https://www.raspberrypi.com/documentation/security/secure-boot-provisioner.html) · [CM4 datasheet](https://pip-assets.raspberrypi.com/categories/634-raspberry-pi-compute-module-4/documents/RP-008168-DS-4-cm4-datasheet.pdf)

**Trusted front board.** Prototype an STM32H573 with Secure Manager on a development board. ST documents a chip-specific attestation key, challenge responses, lifecycle state and software measurements. Verify exactly which measurements the chosen configuration reports for the review firmware. Place the display bus, input switches, PIN handling and SE051 on the controller side. Let the Pi deliver transaction bytes over a bounded interface. The controller must validate the transaction semantics, drive the screen, accept a fresh physical confirmation, and authorize the SE051 operation for those exact bytes. The USB-WASM review design therefore needs a trusted decoder on the controller or an equivalent independently verified representation. Memory, decoder coverage, provisioning and display throughput need bench evidence before selecting the part. This route authenticates the signing front board. If identifying a replacement Pi itself is required, it also needs an enrolled host identity. [ST manual UM3254, sections on initial attestation and device authentication](https://www.st.com/resource/en/user_manual/um3254-secure-manager-for-stm32h573xx-microcontrollers-stmicroelectronics.pdf) · [NXP SE051 configurations](https://www.nxp.com/docs/en/application-note/AN12973.pdf)

## First bench experiment

Use the front-board route to test the hardest claim first. Enroll the attestation public key of development board A on a trusted offline verifier, separately from any wallet key. Carry a fresh verifier challenge in on the existing USB-stick path and return the signed response on the stick or screen. A screen QR output uses the phone camera only. Limit and validate the message format. The verifier checks the challenge, enrolled key, production lifecycle and allowlisted measurement covering the review program, and reports the result before the user enters any secret. Start with test credentials. Development lifecycle evidence should be shown as development evidence. Verify a deliberate update and explicit device replacement without silently trusting a new key. [PSA attestation claims](https://www.rfc-editor.org/rfc/rfc9783.html)

| Condition | Required result |
|---|---|
| Enrolled board A, approved firmware, fresh challenge | Verifier recognizes A and the approved review build |
| Board B with identical firmware and restored wallet | Verifier reports a different signer before PIN entry |
| Recorded response from A | Verifier refuses the stale challenge |
| Review firmware modified or a previously approved release revoked | Boot refuses the image or the verifier refuses authorization before PIN entry |
| Authentic signer with an exact copy of the approved boot card | Normal operation |
| Pi replaced or card contents altered on the controller design | Untrusted Pi input cannot drive the trusted screen, read the PIN or authorize a signature |
| Trustworthy hardware with an unsupported interpreter or a mismatch between displayed transaction and signing request | Refuse to sign |
| Independent verifier receives no valid evidence | No positive identification. Owner withholds the PIN |

A fresh challenge prevents replay. Someone who can relay a live exchange to the genuine board can still obtain a genuine answer. It also does not authenticate the glass, wiring or outer case against physical implants. If this stronger threat is in scope, pre-PIN recognition alone is insufficient. Consider keeping PIN entry and transaction approval on a user-held trusted device and binding its approval to the specific transaction. That would change the product interaction and needs a separate design decision.
