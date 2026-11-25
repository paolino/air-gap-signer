//! Real mainnet transactions, checked against Koios's independent decoding
//! of the same transactions (fixtures/*.koios.json).

use cardano_tx::describe;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

fn fixtures() -> Vec<(String, Vec<u8>, Value)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut out: Vec<_> = fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "cbor"))
        .map(|e| {
            let stem = e.path().file_stem().unwrap().to_string_lossy().to_string();
            let cbor = fs::read(e.path()).unwrap();
            let koios =
                serde_json::from_slice(&fs::read(dir.join(format!("{stem}.koios.json"))).unwrap())
                    .unwrap();
            (stem, cbor, koios)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(out.len(), 4, "four fixtures");
    out
}

/// `1.500000 ADA` back to lovelace, refusing anything else.
fn lovelace(ada: &Value) -> u64 {
    let s = ada
        .as_str()
        .unwrap_or_else(|| panic!("not a string: {ada}"));
    let n = s
        .strip_suffix(" ADA")
        .unwrap_or_else(|| panic!("not ADA: {s}"));
    let (whole, frac) = n.split_once('.').unwrap();
    assert_eq!(frac.len(), 6, "{s}");
    whole.parse::<u64>().unwrap() * 1_000_000 + frac.parse::<u64>().unwrap()
}

fn num(v: &Value) -> u64 {
    v.as_u64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap()
}

#[test]
fn fee_matches_koios() {
    for (name, cbor, koios) in fixtures() {
        let tx = describe(&cbor).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(lovelace(&tx["fee"]), num(&koios["fee"]), "{name}");
    }
}

#[test]
fn inputs_match_koios() {
    for (name, cbor, koios) in fixtures() {
        let tx = describe(&cbor).unwrap();
        let mut ours: Vec<String> = tx["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i.as_str().unwrap().to_string())
            .collect();
        let mut theirs: Vec<String> = koios["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i.as_str().unwrap().to_string())
            .collect();
        ours.sort();
        theirs.sort();
        assert_eq!(ours, theirs, "{name}");
    }
}

#[test]
fn outputs_match_koios_address_ada_and_assets() {
    for (name, cbor, koios) in fixtures() {
        let tx = describe(&cbor).unwrap();
        let ours = tx["outputs"].as_array().unwrap();
        let theirs = koios["outputs"].as_array().unwrap();
        assert_eq!(ours.len(), theirs.len(), "{name}");
        for (o, k) in ours.iter().zip(theirs) {
            assert_eq!(o["address"], k["address"], "{name}");
            assert_eq!(lovelace(&o["ada"]), num(&k["lovelace"]), "{name}");
            let mut ours_assets: Vec<(String, String, u64)> = Vec::new();
            if let Some(policies) = o["assets"].as_object() {
                for (policy, names) in policies {
                    for (asset, qty) in names.as_object().unwrap() {
                        let hex_name = asset
                            .rsplit_once(" (")
                            .map_or(asset.as_str(), |(_, h)| h.trim_end_matches(')'));
                        ours_assets.push((policy.clone(), hex_name.to_string(), num(qty)));
                    }
                }
            }
            let mut theirs_assets: Vec<(String, String, u64)> = k["assets"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| {
                    (
                        a[0].as_str().unwrap().into(),
                        a[1].as_str().unwrap().into(),
                        num(&a[2]),
                    )
                })
                .collect();
            ours_assets.sort();
            theirs_assets.sort();
            assert_eq!(ours_assets, theirs_assets, "{name}");
        }
    }
}

#[test]
fn the_minting_transaction_shows_its_mint() {
    let (_, cbor, koios) = fixtures()
        .into_iter()
        .find(|f| num(&f.2["mint"]) > 0)
        .unwrap();
    let tx = describe(&cbor).unwrap();
    assert!(
        tx["mint"].as_object().is_some_and(|m| !m.is_empty()),
        "{tx}"
    );
    let _ = koios;
}

#[test]
fn garbage_is_refused() {
    assert!(describe(b"not a transaction").is_err());
}
