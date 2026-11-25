//! The signing loop on the device, up to the decision: insert the stick,
//! interpret the transaction, review it, decide. Signing itself needs the
//! secure element and is not here yet.

use signer_core::display::json_to_lines;
use signer_core::review::{Outcome, Review};
use signer_core::spec::SigningSpec;
use signer_core::wasm_sandbox::Sandbox;
use signer_hal::{Buttons, Display, UsbMount};

/// What the user decided about the transaction on the stick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Decision {
    Approved,
    Rejected,
}

/// Read the stick (mounted only while reading), show the spec's label,
/// interpret the payload in the WASM sandbox and run the review.
pub fn review_stick<H: Display + Buttons, U: UsbMount>(
    hal: &mut H,
    usb: &mut U,
) -> Result<Decision, Box<dyn std::error::Error>> {
    usb.mount_readonly()?;
    let contents = usb.read_contents();
    usb.unmount()?;
    let contents = contents?;

    let spec = SigningSpec::from_cbor(&contents.signing_spec_cbor)?;
    hal.show_message(&spec.label)?;

    let sandbox = Sandbox::new()?;
    let json = sandbox
        .load_module(&contents.interpreter_wasm)?
        .interpret(&contents.payload)?;
    let lines = json_to_lines(&serde_json::from_str(&json)?);

    let (cols, rows) = hal.text_grid();
    let mut review = Review::new(lines, cols, rows)?;
    loop {
        hal.show_review(&review.view())?;
        match review.handle(hal.wait_event()?) {
            Outcome::Continue => {}
            Outcome::Confirmed => return Ok(Decision::Approved),
            Outcome::Rejected => return Ok(Decision::Rejected),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signer_core::display::DisplayLine;
    use signer_core::review::View;
    use signer_core::spec::{OutputSpec, SignAlgorithm, Signable, SigningSpec};
    use signer_hal::{ButtonEvent, HalError, UsbContents};
    use std::collections::VecDeque;

    #[derive(Default)]
    struct Hal {
        messages: Vec<String>,
        views: Vec<View>,
        presses: VecDeque<ButtonEvent>,
    }

    impl Display for Hal {
        fn clear(&mut self) -> Result<(), HalError> {
            Ok(())
        }
        fn show_message(&mut self, text: &str) -> Result<(), HalError> {
            self.messages.push(text.into());
            Ok(())
        }
        fn show_lines(&mut self, _: &[DisplayLine], _: usize) -> Result<(), HalError> {
            Ok(())
        }
        fn text_grid(&self) -> (usize, usize) {
            (21, 11)
        }
        fn show_review(&mut self, view: &View) -> Result<(), HalError> {
            self.views.push(view.clone());
            Ok(())
        }
    }

    impl Buttons for Hal {
        fn wait_event(&mut self) -> Result<ButtonEvent, HalError> {
            self.presses
                .pop_front()
                .ok_or_else(|| HalError::Button("no more presses".into()))
        }
    }

    #[derive(Default)]
    struct Stick {
        files: Option<(Vec<u8>, Vec<u8>, Vec<u8>)>,
        mounted: bool,
        mounts: usize,
    }

    impl UsbMount for Stick {
        fn wait_insert(&mut self) -> Result<(), HalError> {
            Ok(())
        }
        fn mount_readonly(&mut self) -> Result<(), HalError> {
            self.mounted = true;
            self.mounts += 1;
            Ok(())
        }
        fn read_contents(&self) -> Result<UsbContents, HalError> {
            assert!(self.mounted, "read while unmounted");
            let (payload, interpreter_wasm, signing_spec_cbor) = self
                .files
                .clone()
                .ok_or_else(|| HalError::Usb("no files".into()))?;
            Ok(UsbContents {
                payload,
                interpreter_wasm,
                signing_spec_cbor,
            })
        }
        fn write_output(&mut self, _: &[u8]) -> Result<(), HalError> {
            panic!("nothing may be written to the stick")
        }
        fn read_file(&self, _: &str) -> Result<Option<Vec<u8>>, HalError> {
            Ok(None)
        }
        fn write_file(&mut self, _: &str, _: &[u8]) -> Result<(), HalError> {
            panic!("nothing may be written to the stick")
        }
        fn unmount(&mut self) -> Result<(), HalError> {
            self.mounted = false;
            Ok(())
        }
    }

    fn echo_hex() -> Vec<u8> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/wasm32-unknown-unknown/release/echo_hex.wasm"
        );
        std::fs::read(path).expect("echo_hex.wasm not found: run `just build-wasm` first")
    }

    fn spec(label: &str) -> Vec<u8> {
        SigningSpec {
            label: label.into(),
            signable: Signable::Whole,
            algorithm: SignAlgorithm::Ed25519,
            key_slot: 0,
            output: OutputSpec::SignatureOnly,
        }
        .to_cbor()
        .unwrap()
    }

    fn stick(payload: &[u8], wasm: Vec<u8>, spec: Vec<u8>) -> Stick {
        Stick {
            files: Some((payload.to_vec(), wasm, spec)),
            ..Default::default()
        }
    }

    #[test]
    fn a_transaction_on_the_stick_is_interpreted_reviewed_and_approved() {
        let mut hal = Hal {
            presses: VecDeque::from([ButtonEvent::Confirm]),
            ..Default::default()
        };
        let mut usb = stick(b"hi", echo_hex(), spec("Test payment"));
        assert_eq!(
            review_stick(&mut hal, &mut usb).unwrap(),
            Decision::Approved
        );
        assert_eq!(hal.messages, vec!["Test payment"]);
        match &hal.views[0] {
            View::Overview { rows, .. } => {
                let texts: Vec<_> = rows.iter().map(|r| r.text.as_str()).collect();
                assert_eq!(texts, vec!["hex: 6869", "length: 2"]);
            }
            v => panic!("expected the review, got {v:?}"),
        }
        assert!(!usb.mounted, "the stick is unmounted before processing");
        assert_eq!(usb.mounts, 1);
    }

    #[test]
    fn a_cut_value_must_be_opened_before_approval() {
        // The payload on the test stick: its hex is too long for a row.
        let mut hal = Hal {
            presses: VecDeque::from([
                ButtonEvent::Confirm,
                ButtonEvent::Open,
                ButtonEvent::Back,
                ButtonEvent::Confirm,
            ]),
            ..Default::default()
        };
        let mut usb = stick(b"pay 5 ADA to Bob", echo_hex(), spec("Test payment"));
        assert_eq!(
            review_stick(&mut hal, &mut usb).unwrap(),
            Decision::Approved
        );
        let headers: Vec<String> = hal
            .views
            .iter()
            .map(|v| match v {
                View::Overview { header, .. } | View::Detail { header, .. } => header.clone(),
            })
            .collect();
        assert!(headers[0].ends_with("1 left"), "{:?}", headers);
        assert!(
            headers[1].ends_with("1 left"),
            "the first Confirm was refused: {:?}",
            headers
        );
        assert!(headers[3].ends_with("OK?"), "{:?}", headers);
    }

    #[test]
    fn reject_is_a_decision_too() {
        let mut hal = Hal {
            presses: VecDeque::from([ButtonEvent::Reject]),
            ..Default::default()
        };
        let mut usb = stick(b"hi", echo_hex(), spec("x"));
        assert_eq!(
            review_stick(&mut hal, &mut usb).unwrap(),
            Decision::Rejected
        );
    }

    #[test]
    fn a_bad_spec_a_bad_interpreter_and_a_missing_file_are_errors() {
        let mut hal = Hal::default();
        assert!(review_stick(
            &mut hal,
            &mut stick(b"hi", echo_hex(), b"not cbor".to_vec())
        )
        .is_err());
        assert!(
            review_stick(&mut hal, &mut stick(b"hi", b"not wasm".to_vec(), spec("x"))).is_err()
        );
        let mut empty = Stick::default();
        assert!(review_stick(&mut hal, &mut empty).is_err());
        assert!(!empty.mounted, "unmounted even when reading fails");
        assert!(hal.views.is_empty(), "nothing reaches the review");
    }
}
