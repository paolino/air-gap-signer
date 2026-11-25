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
    let network = network(&tx);
    if let Some(slot) = tx.validity_start() {
        out.insert("valid_from".into(), json!(validity(slot, network)));
    }
    if let Some(slot) = tx.ttl() {
        out.insert("valid_until".into(), json!(validity(slot, network)));
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

#[derive(Debug, Clone, Copy, PartialEq)]
enum Network {
    Mainnet,
    Testnet,
}

/// The network the outputs pay to: mainnet only if every Shelley-era
/// output address is a mainnet one.
fn network(tx: &MultiEraTx) -> Option<Network> {
    let mut seen = None;
    for o in tx.outputs() {
        let net = match o.address() {
            Ok(Address::Shelley(a)) => a.network(),
            Ok(Address::Stake(a)) => a.network(),
            _ => continue,
        };
        let net = if net.is_mainnet() {
            Network::Mainnet
        } else {
            Network::Testnet
        };
        if seen.is_some_and(|s| s != net) {
            return Some(Network::Testnet);
        }
        seen = Some(net);
    }
    seen
}

/// A validity bound as `2026-11-25 15:57:45 UTC (slot 204055974)`, or
/// just `slot N` when the time cannot be known for sure.
fn validity(slot: u64, network: Option<Network>) -> String {
    match slot_time(slot, network) {
        Some(t) => format!("{t} (slot {slot})"),
        None => format!("slot {slot}"),
    }
}

/// The UTC time of a mainnet Shelley-era slot (one-second slots from
/// slot 4 492 800 at 2020-07-29 21:44:51 UTC). Testnets are left out: a
/// test address does not say whether it is preprod or preview, whose
/// clocks differ.
fn slot_time(slot: u64, network: Option<Network>) -> Option<String> {
    const SHELLEY_START_SLOT: u64 = 4_492_800;
    const UNIX_AT_SLOT_ZERO: u64 = 1_591_566_291;
    (network == Some(Network::Mainnet) && slot >= SHELLEY_START_SLOT)
        .then(|| utc(slot + UNIX_AT_SLOT_ZERO))
}

/// Unix seconds as `YYYY-MM-DD hh:mm:ss UTC` (proleptic Gregorian).
fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_from_unix_seconds() {
        assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc(1596059091), "2020-07-29 21:44:51 UTC");
        assert_eq!(utc(951782400), "2000-02-29 00:00:00 UTC");
        assert_eq!(utc(4102444799), "2099-12-31 23:59:59 UTC");
    }

    #[test]
    fn shelley_start_is_the_known_date() {
        assert_eq!(
            slot_time(4_492_800, Some(Network::Mainnet)).unwrap(),
            "2020-07-29 21:44:51 UTC"
        );
        assert_eq!(
            slot_time(1, Some(Network::Testnet)),
            None,
            "testnets are ambiguous"
        );
        assert_eq!(slot_time(1, None), None);
    }
}
