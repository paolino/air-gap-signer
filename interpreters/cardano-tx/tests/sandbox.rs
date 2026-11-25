//! The built module, run inside the signer's sandbox, says the same as
//! `describe` on the host — and refuses what is not a transaction.

use signer_core::wasm_sandbox::Sandbox;
use std::fs;
use std::path::PathBuf;

fn module() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/wasm32-unknown-unknown/release/cardano_tx.wasm"
    );
    fs::read(path).expect("cardano_tx.wasm not found: run `just build-wasm` first")
}

#[test]
fn the_sandboxed_module_matches_the_host_decoding() {
    let sandbox = Sandbox::new().unwrap();
    let wasm = module();
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    for entry in fs::read_dir(dir).unwrap().flatten() {
        if entry.path().extension().is_none_or(|x| x != "cbor") {
            continue;
        }
        let cbor = fs::read(entry.path()).unwrap();
        let json = sandbox
            .load_module(&wasm)
            .unwrap()
            .interpret(&cbor)
            .unwrap();
        let sandboxed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            sandboxed,
            cardano_tx::describe(&cbor).unwrap(),
            "{:?}",
            entry.path()
        );
    }
}

#[test]
fn the_sandboxed_module_refuses_garbage() {
    let sandbox = Sandbox::new().unwrap();
    let wasm = module();
    assert!(sandbox
        .load_module(&wasm)
        .unwrap()
        .interpret(b"not a transaction")
        .is_err());
}
