use signer_core::wasm_sandbox::Sandbox;

fn echo_hex_wasm() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/wasm32-unknown-unknown/release/echo_hex.wasm"
    );
    std::fs::read(path).expect("echo_hex.wasm not found — run `just build-wasm` first")
}

#[test]
fn interpret_echo_hex() {
    let sandbox = Sandbox::new().unwrap();
    let module = sandbox.load_module(&echo_hex_wasm()).unwrap();

    let payload = b"\xde\xad\xbe\xef";
    let json_str = module.interpret(payload).unwrap();

    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["hex"], "deadbeef");
    assert_eq!(parsed["length"], 4);
}

#[test]
fn interpret_empty_payload() {
    let sandbox = Sandbox::new().unwrap();
    let module = sandbox.load_module(&echo_hex_wasm()).unwrap();

    let json_str = module.interpret(b"").unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["hex"], "");
    assert_eq!(parsed["length"], 0);
}

#[test]
fn interpret_larger_payload() {
    let sandbox = Sandbox::new().unwrap();
    let module = sandbox.load_module(&echo_hex_wasm()).unwrap();

    let payload: Vec<u8> = (0..=255).collect();
    let json_str = module.interpret(&payload).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(parsed["length"], 256);

    let hex = parsed["hex"].as_str().unwrap();
    assert_eq!(hex.len(), 512);
    assert!(hex.starts_with("000102"));
    assert!(hex.ends_with("fdfeff"));
}
