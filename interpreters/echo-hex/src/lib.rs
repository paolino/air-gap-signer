use core::sync::atomic::{AtomicUsize, Ordering};

extern "C" {
    /// Linker-provided symbol marking the start of the heap in WASM linear memory.
    static __heap_base: u8;
}

static HEAP_PTR: AtomicUsize = AtomicUsize::new(0);
static HEAP_BASE: AtomicUsize = AtomicUsize::new(0);

fn heap_base() -> usize {
    let base = HEAP_BASE.load(Ordering::Relaxed);
    if base != 0 {
        return base;
    }
    let base = unsafe { &__heap_base as *const u8 as usize };
    HEAP_BASE.store(base, Ordering::Relaxed);
    HEAP_PTR.store(base, Ordering::Relaxed);
    base
}

#[no_mangle]
pub extern "C" fn alloc(size: i32) -> i32 {
    heap_base(); // ensure initialized
    let size = size as usize;
    let ptr = HEAP_PTR.fetch_add(size, Ordering::SeqCst);
    // Check against WASM memory size (in pages of 64 KiB)
    let mem_size = core::arch::wasm32::memory_size(0) * 65536;
    if ptr + size > mem_size {
        // Try to grow memory
        let pages_needed = ((ptr + size - mem_size) + 65535) / 65536;
        if core::arch::wasm32::memory_grow(0, pages_needed) == usize::MAX {
            HEAP_PTR.store(ptr, Ordering::SeqCst); // rollback
            return 0;
        }
    }
    ptr as i32
}

fn nibble_to_hex(n: u8) -> u8 {
    if n < 10 {
        b'0' + n
    } else {
        b'a' + (n - 10)
    }
}

/// interpret(ptr, len) -> ptr to length-prefixed JSON string.
///
/// Output JSON: `{"hex":"<hex-encoded payload>","length":<n>}`
#[no_mangle]
pub extern "C" fn interpret(ptr: i32, len: i32) -> i32 {
    let payload = unsafe { core::slice::from_raw_parts(ptr as *const u8, len as usize) };

    let hex_len = payload.len() * 2;
    let prefix = b"{\"hex\":\"";
    let middle = b"\",\"length\":";
    let suffix = b"}";

    let mut len_buf = [0u8; 20];
    let len_str = fmt_usize(payload.len(), &mut len_buf);

    let total_len = prefix.len() + hex_len + middle.len() + len_str.len() + suffix.len();

    let out_ptr = alloc((4 + total_len) as i32);
    if out_ptr == 0 {
        return 0;
    }

    let out = unsafe { core::slice::from_raw_parts_mut(out_ptr as *mut u8, 4 + total_len) };

    // Length prefix (LE u32)
    out[0..4].copy_from_slice(&(total_len as u32).to_le_bytes());
    let mut offset = 4;

    out[offset..offset + prefix.len()].copy_from_slice(prefix);
    offset += prefix.len();

    for &b in payload {
        out[offset] = nibble_to_hex(b >> 4);
        out[offset + 1] = nibble_to_hex(b & 0x0f);
        offset += 2;
    }

    out[offset..offset + middle.len()].copy_from_slice(middle);
    offset += middle.len();

    out[offset..offset + len_str.len()].copy_from_slice(len_str);
    offset += len_str.len();

    out[offset..offset + suffix.len()].copy_from_slice(suffix);

    out_ptr
}

fn fmt_usize(mut n: usize, buf: &mut [u8; 20]) -> &[u8] {
    if n == 0 {
        buf[19] = b'0';
        return &buf[19..];
    }
    let mut i = 20;
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    &buf[i..]
}
