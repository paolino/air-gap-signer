//! Interpreter for Cardano transactions: decodes the transaction CBOR on
//! the USB stick into the JSON the signer reviews. Same ABI as
//! `echo-hex`: `alloc` and `interpret`, no imports.

use pallas_addresses::Address;
use pallas_primitives::conway::DatumOption;
use pallas_traverse::{MultiEraInput, MultiEraOutput, MultiEraPolicyAssets, MultiEraTx};
use serde_json::{json, Map, Value};

/// The review JSON for a transaction's CBOR.
pub fn describe(cbor: &[u8]) -> Result<Value, String> {
    let tx = MultiEraTx::decode(cbor).map_err(|e| format!("not a Cardano transaction: {e}"))?;
    let mut out = Map::new();
    out.insert(
        "era".into(),
        json!(format!("{:?}", tx.era()).to_lowercase()),
    );
    if let Some(network) = tx.network_id() {
        out.insert(
            "network".into(),
            json!(format!("{network:?}").to_lowercase()),
        );
    }
    out.insert("fee".into(), json!(ada(tx.fee().unwrap_or(0))));
    if let Some(slot) = tx.validity_start() {
        out.insert("valid_from_slot".into(), json!(slot));
    }
    if let Some(slot) = tx.ttl() {
        out.insert("valid_until_slot".into(), json!(slot));
    }
    out.insert("inputs".into(), inputs(&tx.inputs()));
    out.insert(
        "outputs".into(),
        Value::Array(tx.outputs().iter().map(output).collect::<Result<_, _>>()?),
    );
    let mint = tx.mints();
    if !mint.is_empty() {
        out.insert(
            "mint".into(),
            assets(&mint, |a| a.mint_coin().map(|c| c as i128)),
        );
    }
    let withdrawals = tx.withdrawals_sorted_set();
    if !withdrawals.is_empty() {
        let mut w = Map::new();
        for (account, amount) in withdrawals {
            let name = Address::from_bytes(account)
                .ok()
                .and_then(|a| a.to_bech32().ok())
                .unwrap_or_else(|| hex::encode(account));
            w.insert(name, json!(ada(amount)));
        }
        out.insert("withdrawals".into(), Value::Object(w));
    }
    let certs = tx.certs();
    if !certs.is_empty() {
        let list = certs.iter().map(|c| json!(cert_name(c))).collect();
        out.insert("certificates".into(), Value::Array(list));
    }
    let collateral = tx.collateral();
    if !collateral.is_empty() {
        out.insert("collateral".into(), inputs(&collateral));
    }
    let refs = tx.reference_inputs();
    if !refs.is_empty() {
        out.insert("reference_inputs".into(), inputs(&refs));
    }
    let signers = tx.required_signers();
    if let Some(signers) = signers.as_alonzo().filter(|s| !s.is_empty()) {
        let list = signers.iter().map(|h| json!(hex::encode(h))).collect();
        out.insert("required_signers".into(), Value::Array(list));
    }
    Ok(Value::Object(out))
}

fn inputs(list: &[MultiEraInput]) -> Value {
    list.iter()
        .map(|i| json!(format!("{}#{}", i.hash(), i.index())))
        .collect()
}

fn output(o: &MultiEraOutput) -> Result<Value, String> {
    // Shelley addresses as bech32, Byron addresses as base58.
    let address = o
        .address()
        .map_err(|e| format!("output address: {e}"))?
        .to_string();
    let value = o.value();
    let mut out = Map::new();
    out.insert("address".into(), json!(address));
    out.insert("ada".into(), json!(ada(value.coin())));
    let multi = value.assets();
    if !multi.is_empty() {
        out.insert(
            "assets".into(),
            assets(&multi, |a| a.output_coin().map(|c| c as i128)),
        );
    }
    match o.datum() {
        Some(DatumOption::Hash(h)) => {
            out.insert("datum_hash".into(), json!(hex::encode(h)));
        }
        Some(DatumOption::Data(d)) => {
            out.insert("inline_datum".into(), json!(hex::encode(d.0.raw_cbor())));
        }
        None => {}
    }
    if o.script_ref().is_some() {
        out.insert("reference_script".into(), json!("present"));
    }
    Ok(Value::Object(out))
}

/// Assets as `{policy: {name: quantity}}`; a name that is printable
/// ASCII is shown as text followed by its hex.
fn assets(
    policies: &[MultiEraPolicyAssets],
    quantity: impl Fn(&pallas_traverse::MultiEraAsset) -> Option<i128>,
) -> Value {
    let mut out = Map::new();
    for p in policies {
        let mut names = Map::new();
        for a in p.assets() {
            let raw = a.name();
            let shown = match std::str::from_utf8(raw) {
                Ok(s) if !s.is_empty() && s.chars().all(|c| (' '..='~').contains(&c)) => {
                    format!("{s} ({})", hex::encode(raw))
                }
                _ => hex::encode(raw),
            };
            names.insert(shown, json!(quantity(&a).unwrap_or(0).to_string()));
        }
        out.insert(hex::encode(p.policy()), Value::Object(names));
    }
    Value::Object(out)
}

fn cert_name(cert: &pallas_traverse::MultiEraCert) -> String {
    format!("{cert:?}")
        .split(['(', ' ', '{'])
        .next()
        .unwrap_or("certificate")
        .to_string()
}

/// Lovelace as ADA with all six decimals: `1.500000 ADA`.
pub fn ada(lovelace: u64) -> String {
    format!("{}.{:06} ADA", lovelace / 1_000_000, lovelace % 1_000_000)
}

/// The interpreter ABI the signer's sandbox calls.
#[cfg(target_arch = "wasm32")]
mod abi {
    /// Reserve `size` bytes of linear memory for the host to write into.
    #[no_mangle]
    pub extern "C" fn alloc(size: i32) -> i32 {
        let mut buf = Vec::<u8>::with_capacity(size.max(0) as usize);
        let ptr = buf.as_mut_ptr();
        core::mem::forget(buf);
        ptr as i32
    }

    /// Decode the transaction at `ptr..ptr+len` and return a pointer to
    /// its review JSON, prefixed by its length as a little-endian u32.
    /// Anything that is not a transaction returns 0, which the sandbox
    /// reports as a failure: an error never reaches the review.
    #[no_mangle]
    pub extern "C" fn interpret(ptr: i32, len: i32) -> i32 {
        let cbor = unsafe { core::slice::from_raw_parts(ptr as *const u8, len.max(0) as usize) };
        let Ok(json) = super::describe(cbor) else {
            return 0;
        };
        let text = json.to_string().into_bytes();
        let mut out = Vec::with_capacity(4 + text.len());
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(&text);
        let ptr = out.as_mut_ptr();
        core::mem::forget(out);
        ptr as i32
    }
}
