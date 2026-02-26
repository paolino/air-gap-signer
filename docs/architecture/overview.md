# Architecture Overview

## Crate structure

```mermaid
graph TD
    BIN[signer-bin<br/>PID 1 binary] --> HAL[signer-hal<br/>HW abstraction traits]
    BIN --> CORE[signer-core<br/>Pure logic]
    PI[signer-pi<br/>Pi HAL impl] --> HAL
    SIM[signer-sim<br/>Desktop simulator] --> HAL
    SIM --> CORE
    PI --> CORE
    CORE --> WT[wasmtime]
    CORE --> CIB[ciborium]
    HAL --> SE[Secure Element<br/>SE050 via I2C]
```

| Crate | Purpose |
|-------|---------|
| `signer-core` | Pure logic: spec types, WASM sandbox, display, hash extraction |
| `signer-hal` | Trait definitions: `Display`, `Buttons`, `UsbMount`, `SecureElement` |
| `signer-pi` | Raspberry Pi implementation: linuxfb, gpiod, mount, I2C secure element |
| `signer-bin` | The PID 1 binary (state machine orchestrating everything) |
| `signer-sim` | Desktop simulator using minifb window + keyboard |
| `usb-pack` | CLI tool to prepare USB stick contents |

## Device flow

```mermaid
stateDiagram-v2
    [*] --> CheckProvisioned: Boot
    CheckProvisioned --> Setup: Not provisioned
    CheckProvisioned --> PinEntry: Already provisioned

    state Setup {
        [*] --> SetPin: SET PIN + CONFIRM
        SetPin --> PrivateUSB: Insert private USB
        PrivateUSB --> Recovery: seed.bin found
        PrivateUSB --> Generate: seed.bin missing
        Recovery --> PublicUSB: Import key
        Generate --> PublicUSB: Generate key + write seed.bin
        PublicUSB --> [*]: Write pubkey.bin
    }

    Setup --> PinEntry: Setup complete
    PinEntry --> Idle: PIN verified by secure element
    Idle --> Loading: USB inserted
    Loading --> Displaying: WASM interpret → JSON
    Displaying --> Signing: User confirms
    Displaying --> Idle: User rejects
    Signing --> Done: SE signs hash → write signed.bin
    Done --> Idle: USB removed
```

## Interpreters

WASM modules are carried on the USB stick alongside the transaction payload. Each blockchain ecosystem ships its own interpreter:

| Interpreter | Format | Output |
|-------------|--------|--------|
| `echo-hex` | Any | Hex dump (testing) |
| `cardano-cbor` | Cardano TX CBOR | Structured JSON (inputs, outputs, fee, metadata) |
| `bitcoin-psbt` | Bitcoin PSBT | Structured JSON (inputs, outputs, fee) |

Interpreters are compiled to `wasm32-unknown-unknown` and must export:

- `alloc(size) → ptr` — bump allocator
- `interpret(ptr, len) → ptr` — parse payload, return length-prefixed JSON
- `assemble(payload_ptr, payload_len, sig_ptr, sig_len) → ptr` — (optional) combine payload + signature
