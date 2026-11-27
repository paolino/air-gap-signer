//! Physical signing for the bench's one-input, ADA-only preprod self-payment.
//! The trusted bench host resolves the preprod input and pins the CLI transaction ID.
use super::*;
use blake2::{digest::consts::U28, Blake2b};
use pallas_codec::minicbor::{data::Tag, Encoder};
use serde::Deserialize;
use signer_core::display::json_to_lines;
use signer_core::review::Review;
use signer_core::session::{Finish, Key, Session, Signer};
use signer_core::spec::SignAlgorithm;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    input_tx: String,
    input_index: u32,
    input_lovelace: u64,
    fee: u64,
    expires_slot: u64,
    tip_slot: u64,
    tip_unix: u64,
    tx_id: String,
}

impl Plan {
    fn check_time(&self, unix: u64) -> Result<(), Error> {
        let elapsed = unix
            .checked_sub(self.tip_unix)
            .ok_or("clock behind preprod tip")?;
        let slot = self
            .tip_slot
            .checked_add(elapsed)
            .ok_or("invalid preprod slot")?;
        if slot >= self.expires_slot {
            return Err("preprod payment expired".into());
        }
        Ok(())
    }

    // Reconstruct the entire allowed transaction independently. An extra output,
    // script, certificate, network change or existing witness changes the bytes.
    fn review(
        &self,
        public: &[u8; 32],
        payload: &[u8],
    ) -> Result<(serde_json::Value, [u8; 32]), Error> {
        let amount = self
            .input_lovelace
            .checked_sub(self.fee)
            .filter(|n| *n > 0)
            .ok_or("fee exhausts input")?;
        let mut input = [0; 32];
        hex::decode_to_slice(&self.input_tx, &mut input)?;
        let mut address = [0; 29];
        address[0] = 0x60; // testnet enterprise address with a payment key
        address[1..].copy_from_slice(&Blake2b::<U28>::digest(public));
        let mut tx = Encoder::new(Vec::new());
        tx.array(4)?.map(4)?;
        tx.u8(0)?
            .tag(Tag::new(258))?
            .array(1)?
            .array(2)?
            .bytes(&input)?
            .u32(self.input_index)?;
        tx.u8(1)?.array(1)?.array(2)?.bytes(&address)?.u64(amount)?;
        tx.u8(2)?.u64(self.fee)?.u8(3)?.u64(self.expires_slot)?;
        tx.map(0)?.bool(true)?.null()?;
        if tx.into_writer() != payload {
            return Err("transaction differs from preprod self-payment to this key".into());
        }
        let (fields, _, digest) = cardano_tx::conway_preview(payload)?;
        if self.tx_id != hex::encode(digest) {
            return Err("transaction ID differs from the staged preprod plan".into());
        }
        Ok((fields, digest))
    }
}

fn now() -> Result<u64, Error> {
    Ok(SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs())
}

struct PanelSigner<'a, H> {
    hal: &'a mut H,
    se: Se051,
    public: [u8; 32],
    digest: [u8; 32],
    plan: Plan,
}

impl<H: Buttons + Display + recovery_pin::PinContacts> PanelSigner<'_, H> {
    fn approve(&mut self) -> Result<Vec<u8>, Error> {
        self.plan.check_time(now()?)?;
        if !confirm(
            self.hal,
            &format!(
                "Sign preprod self-payment? Fee {}. ID {}.",
                cardano_tx::ada(self.plan.fee),
                &self.plan.tx_id[..12]
            ),
        )? {
            return Err("Cancelled".into());
        }
        let pin = pin_enrollment::enter(self.hal, "Sign preprod payment. #6 cancel")?
            .ok_or("Cancelled")?;
        self.plan.check_time(now()?)?;
        Ok(pin_enrollment::sign_verified(&mut self.se, &pin, &self.public, &self.digest)?.to_vec())
    }
}

impl<H: Buttons + Display + recovery_pin::PinContacts> Signer for PanelSigner<'_, H> {
    fn keys(&self) -> Vec<Key> {
        vec![Key {
            slot: 0,
            algorithm: SignAlgorithm::Ed25519,
            label: "Practice key".into(),
        }]
    }
    fn sign(&mut self, slot: u8, message: &[u8]) -> Result<Vec<u8>, String> {
        if slot != 0 || message != self.digest {
            return Err("Payment changed".into());
        }
        self.approve().map_err(|e| e.to_string())
    }
}

/// Review the staged draft on the physical panel and export a verified detached
/// signature only after the operator finishes the Sign screen.
pub fn run(
    hal: &mut (impl Buttons + Display + recovery_pin::PinContacts),
    keys: &Path,
    dir: &Path,
) -> Result<(), Error> {
    use crate::usb::read_limited;
    let out = dir.join("payment.signature");
    if out.try_exists()? {
        return Err("payment signature already exists".into());
    }
    let plan: Plan = serde_json::from_slice(&read_limited(&dir.join("plan.json"), 4096)?)?;
    let payload = read_limited(&dir.join("payment.cbor"), 1024)?;
    let bus = File::open("/dev/i2c-1")?;
    bus.try_lock()?;
    let mut se = Se051::open("/dev/i2c-1")?;
    let (pin_enrollment::Phase::Protected, Some(public)) = pin_enrollment::inspect(&mut se, keys)?
    else {
        return Err("finish signing PIN setup in Keys first".into());
    };
    plan.check_time(now()?)?;
    let (fields, digest) = plan.review(&public, &payload)?;
    if !confirm(hal, "Review preprod self-payment?")? {
        return Ok(());
    }
    let (cols, rows) = hal.text_grid();
    let review = Review::new(json_to_lines(&fields), cols, rows)?;
    let mut signer = PanelSigner {
        hal,
        se,
        public,
        digest,
        plan,
    };
    let mut session = Session::new(
        review,
        digest.to_vec(),
        &[(0, SignAlgorithm::Ed25519)],
        &signer,
    );
    loop {
        signer.hal.show_review(&session.view())?;
        let event = signer.hal.wait_event()?;
        if let Finish::Done(signatures) = session.handle(event, &mut signer) {
            match signatures.as_slice() {
                [] => signer.hal.show_message("Payment cancelled.")?,
                [(0, signature)] => {
                    signer.plan.check_time(now()?)?;
                    save_new(&out, signature)?;
                    signer.hal.show_message("Preprod witness saved.")?;
                }
                _ => return Err("unexpected payment witnesses".into()),
            }
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Independently built with cardano-cli 11.7 conway transaction build-raw and txid.
    #[test]
    fn only_the_exact_cli_self_payment_can_reach_review() {
        let mut public = [0; 32];
        public[31] = 1;
        let payload = hex::decode("84a400d9010281825820000000000000000000000000000000000000000000000000000000000000000201018182581d603de39f24efebfb0c0b0fcb1b630549c87d26743c2e6d00dcd7cff3be1a0039fbc0021a00030d40031a00061a80a0f5f6").unwrap();
        let mut plan = Plan {
            input_tx: format!("{:064x}", 2),
            input_index: 1,
            input_lovelace: 4_000_000,
            fee: 200_000,
            expires_slot: 400_000,
            tip_slot: 399_900,
            tip_unix: 1_000_000,
            tx_id: "fa80d64fca87a285e51b56575fbd5c47a7b6b9fefd03ebeb6cfef2e03d88d0ae".into(),
        };
        let (review, digest) = plan.review(&public, &payload).unwrap();
        assert_eq!(review["fee"], "0.200000 ADA");
        assert_eq!(hex::encode(digest), plan.tx_id);
        assert!(plan.check_time(999_999).is_err());
        assert!(plan.check_time(1_000_099).is_ok());
        assert!(plan.check_time(1_000_100).is_err());
        assert!(plan.review(&[0; 32], &payload).is_err());
        for i in 0..payload.len() {
            let mut changed = payload.clone();
            changed[i] ^= 1;
            assert!(plan.review(&public, &changed).is_err(), "changed byte {i}");
        }
        plan.input_lovelace += 1;
        assert!(plan.review(&public, &payload).is_err());
        plan.input_lovelace -= 1;
        plan.tx_id = "00".repeat(32);
        assert!(plan.review(&public, &payload).is_err());
    }
}
