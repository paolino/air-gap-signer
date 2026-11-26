//! Keep the ledger's original transaction body bytes in the review payload.
use pallas_codec::minicbor::Decoder;
use pallas_primitives::conway::Tx;
use serde::Deserialize;
use signer_core::spec::{HashAlgorithm, Signable, SignableSource};

type Error = Box<dyn std::error::Error>;

#[derive(Deserialize)]
struct Envelope {
    #[serde(rename = "type")]
    kind: String,
    #[serde(rename = "cborHex")]
    cbor_hex: String,
}

pub fn prepare(input: Vec<u8>) -> Result<(Vec<u8>, Signable), Error> {
    let payload = if input.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{') {
        let envelope: Envelope = serde_json::from_slice(&input)?;
        if envelope.kind != "Tx ConwayEra" {
            return Err("expected a Tx ConwayEra text envelope".into());
        }
        hex::decode(envelope.cbor_hex)?
    } else {
        input
    };
    let mut decoder = Decoder::new(&payload);
    let tx: Tx = decoder.decode()?;
    if decoder.position() != payload.len() {
        return Err("unexpected bytes after Cardano transaction".into());
    }
    let body = tx.transaction_body.raw_cbor();
    let mut header = Decoder::new(&payload);
    header.array()?;
    let offset = header.position();
    let length = body.len();
    if payload.get(offset..offset + length) != Some(body) {
        return Err("Cardano body differs from the original payload".into());
    }
    let signable = Signable::HashThenSign {
        hash: HashAlgorithm::Blake2b256,
        source: SignableSource::Range { offset, length },
    };
    Ok((payload, signable))
}

#[cfg(test)]
mod tests {
    use super::*;
    use signer_core::crypto::extract_signable;

    const ENVELOPE: &[u8] = br#"{"type":"Tx ConwayEra","cborHex":"84a300d9010281825820000000000000000000000000000000000000000000000000000000000000000000018182581d6096d3060bc838cc8ed9471d456e886f60ac1976d9aecc6db6927e64c41a001e8480021a00029810a0f5f6"}"#;
    // cardano-cli 11.7, conway transaction txid --tx-body-file
    const TX_ID: &str = "0afc47022406ff1a056d92177b4a847ce6de8dd620c426ef3e0c72c563370248";

    #[test]
    fn packed_body_matches_the_ledger_id() {
        let (payload, signable) = prepare(ENVELOPE.to_vec()).unwrap();
        assert_eq!(
            hex::encode(extract_signable(&payload, &signable).unwrap()),
            TX_ID
        );
        assert_eq!(
            prepare(payload.clone()).unwrap(),
            (payload.clone(), signable)
        );
        let mut trailing = payload;
        trailing.push(0);
        assert!(prepare(trailing).is_err());
        assert!(prepare(br#"{"type":"Wrong","cborHex":"00"}"#.to_vec()).is_err());
    }
}
