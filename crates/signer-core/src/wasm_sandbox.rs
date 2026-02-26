use thiserror::Error;
use wasmtime::{Config, Engine, Linker, Module, Store, StoreLimits, StoreLimitsBuilder};

/// Fuel budget: 10 million operations.
const FUEL_LIMIT: u64 = 10_000_000;

/// Memory cap: 16 MB.
const MAX_MEMORY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum SandboxError {
    #[error("WASM engine error: {0}")]
    Engine(#[from] wasmtime::Error),
    #[error("module has no '{0}' export")]
    MissingExport(String),
    #[error("interpret returned null pointer")]
    NullPointer,
    #[error("output length {0} exceeds sandbox memory")]
    OutputOverflow(usize),
    #[error("invalid UTF-8 in WASM output")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),
}

/// Sandboxed WASM interpreter engine.
///
/// Zero imports — the module cannot call the host.
/// Fuel-metered and memory-capped.
pub struct Sandbox {
    engine: Engine,
}

impl Sandbox {
    pub fn new() -> Result<Self, SandboxError> {
        let mut config = Config::new();
        config.consume_fuel(true);
        config.max_wasm_stack(512 * 1024); // 512 KiB call stack
        Ok(Self {
            engine: Engine::new(&config)?,
        })
    }

    /// Load a WASM module from bytes.
    pub fn load_module(&self, wasm_bytes: &[u8]) -> Result<SandboxModule<'_>, SandboxError> {
        let module = Module::new(&self.engine, wasm_bytes)?;
        Ok(SandboxModule {
            engine: &self.engine,
            module,
        })
    }
}

fn new_store(engine: &Engine) -> Result<Store<StoreLimits>, SandboxError> {
    let limits = StoreLimitsBuilder::new()
        .memory_size(MAX_MEMORY_BYTES)
        .build();
    let mut store = Store::new(engine, limits);
    store.limiter(|s| s);
    store.set_fuel(FUEL_LIMIT)?;
    Ok(store)
}

/// A loaded WASM module ready to execute.
pub struct SandboxModule<'a> {
    engine: &'a Engine,
    module: Module,
}

impl SandboxModule<'_> {
    /// Call `interpret(ptr, len) -> ptr` on the WASM module.
    ///
    /// The module must export:
    /// - `memory`: linear memory
    /// - `alloc(size) -> ptr`: allocate `size` bytes, return pointer
    /// - `interpret(ptr, len) -> ptr`: interpret payload, return pointer to
    ///   length-prefixed (4 bytes LE) UTF-8 JSON string
    pub fn interpret(&self, payload: &[u8]) -> Result<String, SandboxError> {
        let linker: Linker<StoreLimits> = Linker::new(self.engine);
        let mut store = new_store(self.engine)?;

        let instance = linker.instantiate(&mut store, &self.module)?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| SandboxError::MissingExport("memory".into()))?;

        // Allocate space in WASM memory for the payload
        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "alloc")
            .map_err(|_| SandboxError::MissingExport("alloc".into()))?;
        let payload_ptr = alloc.call(&mut store, payload.len() as i32)?;
        if payload_ptr == 0 {
            return Err(SandboxError::NullPointer);
        }

        // Copy payload into WASM memory
        memory.data_mut(&mut store)[payload_ptr as usize..payload_ptr as usize + payload.len()]
            .copy_from_slice(payload);

        // Call interpret
        let interpret = instance
            .get_typed_func::<(i32, i32), i32>(&mut store, "interpret")
            .map_err(|_| SandboxError::MissingExport("interpret".into()))?;
        let result_ptr = interpret.call(&mut store, (payload_ptr, payload.len() as i32))?;
        if result_ptr == 0 {
            return Err(SandboxError::NullPointer);
        }

        // Read length-prefixed result: 4 bytes LE length, then UTF-8 JSON
        let mem_data = memory.data(&store);
        let result_offset = result_ptr as usize;
        if result_offset + 4 > mem_data.len() {
            return Err(SandboxError::OutputOverflow(result_offset + 4));
        }
        let len = u32::from_le_bytes(
            mem_data[result_offset..result_offset + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        if result_offset + 4 + len > mem_data.len() {
            return Err(SandboxError::OutputOverflow(len));
        }
        let json_bytes = mem_data[result_offset + 4..result_offset + 4 + len].to_vec();
        Ok(String::from_utf8(json_bytes)?)
    }

    /// Call `assemble(payload_ptr, payload_len, sig_ptr, sig_len) -> ptr` on the WASM module.
    ///
    /// Returns length-prefixed output bytes (same convention as `interpret`).
    pub fn assemble(&self, payload: &[u8], signature: &[u8]) -> Result<Vec<u8>, SandboxError> {
        let linker: Linker<StoreLimits> = Linker::new(self.engine);
        let mut store = new_store(self.engine)?;

        let instance = linker.instantiate(&mut store, &self.module)?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| SandboxError::MissingExport("memory".into()))?;

        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "alloc")
            .map_err(|_| SandboxError::MissingExport("alloc".into()))?;

        // Allocate and copy payload
        let payload_ptr = alloc.call(&mut store, payload.len() as i32)?;
        memory.data_mut(&mut store)[payload_ptr as usize..payload_ptr as usize + payload.len()]
            .copy_from_slice(payload);

        // Allocate and copy signature
        let sig_ptr = alloc.call(&mut store, signature.len() as i32)?;
        memory.data_mut(&mut store)[sig_ptr as usize..sig_ptr as usize + signature.len()]
            .copy_from_slice(signature);

        let assemble = instance
            .get_typed_func::<(i32, i32, i32, i32), i32>(&mut store, "assemble")
            .map_err(|_| SandboxError::MissingExport("assemble".into()))?;
        let result_ptr = assemble.call(
            &mut store,
            (
                payload_ptr,
                payload.len() as i32,
                sig_ptr,
                signature.len() as i32,
            ),
        )?;
        if result_ptr == 0 {
            return Err(SandboxError::NullPointer);
        }

        let mem_data = memory.data(&store);
        let result_offset = result_ptr as usize;
        let len = u32::from_le_bytes(
            mem_data[result_offset..result_offset + 4]
                .try_into()
                .unwrap(),
        ) as usize;
        Ok(mem_data[result_offset + 4..result_offset + 4 + len].to_vec())
    }
}
