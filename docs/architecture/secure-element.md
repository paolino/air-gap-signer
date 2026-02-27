# Secure Element

## Why

Private keys stored on an SD card (even encrypted) are vulnerable to offline
brute-force. A 6-digit PIN has ~20 bits of entropy — a GPU rig cracks
Argon2id-protected keys in hours.

A hardware secure element solves this: the chip holds the private key
internally and enforces PIN retry limits (e.g. lockout after 10 failures).
A stolen SD card contains no secrets. During provisioning the seed is
exported once to a private USB stick for offline backup — after that
the key lives only inside the chip.

## Hardware

**NXP SE050C1HQ1** — EdgeLock SE050, HX2QFN20 package (3×3 mm).

- I2C interface (SDA/SCL on Pi GPIO 2/3)
- Native Ed25519, secp256k1, and NIST P-256 support
- Hardware key generation and signing — private key never leaves the chip
- PIN retry policy enforced in hardware
- Secure key storage with multiple slots
- CC EAL6+ certified

!!! note "Why SE050 over ATECC608B"
    The ATECC608B (~2 EUR) only supports ECDSA P-256 natively. Cardano and
    Bitcoin require Ed25519 / secp256k1, which would force the Pi to handle
    raw key material in RAM. The SE050 (~5-10 EUR) signs natively with these
    curves — the private key never reaches the Pi.

## Breakout board

A custom 20×20 mm breakout board connects the SE050 to the Pi via an 8-pin
header. It includes decoupling caps, I2C pull-up resistors, and mounting
holes.

![SE050 breakout board schematic](../assets/se050-breakout-schematic.svg)

Generate Gerber files with `just gerbers` (see
`hardware/SE050_breakout/generate_gerbers.py` for the full specification in
`hardware/SE050_breakout/generate_gerbers.prompt.md`).

## Key lifecycle

```mermaid
sequenceDiagram
    participant User as Human
    participant Pi as Raspberry Pi
    participant SE as Secure Element

    Note over Pi,SE: First boot (provisioning)
    User->>Pi: Set PIN + Confirm PIN (buttons)
    Pi->>SE: set_pin(hash)
    Pi->>SE: verify_pin(hash)
    User->>Pi: Insert private USB
    alt seed.bin exists on USB (recovery)
        Pi->>SE: import_key(slot 0, seed)
    else no seed on USB (fresh)
        Pi->>SE: generate_key(slot 0)
        SE-->>Pi: public key
        Pi->>SE: export_seed(slot 0)
        Pi->>Pi: Write seed.bin to private USB
    end
    User->>Pi: Remove private USB, insert public USB
    Pi->>Pi: Write pubkey.bin to public USB
    User->>Pi: Remove public USB — store private USB safely

    Note over Pi,SE: Normal boot
    User->>Pi: Enter PIN (buttons, 4 digits)
    Pi->>SE: verify_pin(hash)
    SE-->>Pi: OK (or error if wrong PIN)

    Note over Pi,SE: Signing
    Pi->>Pi: Extract hash from payload (WASM + spec)
    Pi->>SE: sign(slot N, hash)
    SE-->>SE: Sign internally
    SE->>Pi: Signature bytes
    Pi->>Pi: Write signature to USB
```

## HAL trait

```rust
pub trait SecureElement {
    /// Set the initial PIN during first-time setup.
    fn set_pin(&mut self, pin: &[u8]) -> Result<(), HalError>;

    /// Verify the user PIN.
    fn verify_pin(&mut self, pin: &[u8]) -> Result<(), HalError>;

    /// Check whether the device has been provisioned (PIN set, key generated).
    fn is_provisioned(&self) -> bool;

    /// Generate a keypair in the given slot. Returns the public key.
    fn generate_key(&mut self, slot: u8) -> Result<Vec<u8>, HalError>;

    /// Sign a hash using the key in the given slot.
    /// Requires prior PIN verification in the same session.
    fn sign(&mut self, slot: u8, hash: &[u8]) -> Result<Vec<u8>, HalError>;

    /// Read the public key from a slot.
    fn public_key(&self, slot: u8) -> Result<Vec<u8>, HalError>;

    /// Import an existing seed into a slot (recovery from backup).
    fn import_key(&mut self, slot: u8, seed: &[u8]) -> Result<Vec<u8>, HalError>;

    /// Export the seed for backup during provisioning.
    fn export_seed(&self, slot: u8) -> Result<Vec<u8>, HalError>;
}
```

## Threat model

| Threat | Mitigation |
|--------|------------|
| Stolen SD card | No secrets on SD — SE holds all keys |
| Lost/destroyed device | Seed backup on private USB allows full recovery on a new device |
| Stolen device (powered off) | PIN required on every boot, SE locks after N failures |
| Stolen device (powered on) | Physical access to buttons required to confirm each signing |
| Stolen private USB | Contains raw seed — store offline in a safe, treat like a hardware wallet recovery phrase |
| Side-channel on Pi | Pi never handles raw key material — SE050 signs internally |
| Glitch attack on SE | SE050 CC EAL6+ certified, tamper-resistant |
| USB-borne malware | WASM sandbox: no host imports, fuel-limited, memory-capped |
