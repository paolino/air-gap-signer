# WASM Sandbox

Interpreters run inside a Wasmtime sandbox with strict resource limits.

## Security properties

- **Zero imports** — the WASM module cannot call any host functions.
  No filesystem, no network, no clock, no randomness.
- **Fuel-metered** — 10 million operations budget. Prevents infinite loops
  and excessive computation.
- **Memory-capped** — 16 MB maximum linear memory. Prevents OOM on the device.
- **Stack-limited** — 512 KiB call stack.

## ABI contract

The WASM module must export:

### `memory`

The module's linear memory, accessible to the host for data transfer.

### `alloc(size: i32) -> i32`

Allocate `size` bytes in WASM memory. Returns a pointer (offset into linear memory), or 0 on failure.

### `interpret(ptr: i32, len: i32) -> i32`

Parse the payload bytes at `[ptr, ptr+len)` and return a pointer to a length-prefixed UTF-8 JSON string:

```
[4 bytes LE u32: length][length bytes: UTF-8 JSON]
```

### `assemble(payload_ptr: i32, payload_len: i32, sig_ptr: i32, sig_len: i32) -> i32`

*(Optional)* Combine the original payload and signature into a final signed artifact. Same length-prefixed output convention.

## Why WASM

- **Polyglot** — interpreters can be written in Rust, C, AssemblyScript, or any language targeting wasm32
- **Deterministic** — same input always produces same output (no I/O, no randomness)
- **Small** — the echo-hex interpreter compiles to ~1 KB
- **Auditable** — `wasm2wat` produces readable text format for review
