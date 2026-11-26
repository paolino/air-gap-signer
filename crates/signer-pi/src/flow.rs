//! The signing loop on the device: insert the stick, interpret the
//! transaction, then the session — review, sign, keys on #6 — until Done.
//! Writing the signatures back to the stick is not here yet.

use signer_core::crypto::extract_signable;
use signer_core::display::json_to_lines;
use signer_core::review::Review;
use signer_core::session::{Finish, Key, Session, Signer};
use signer_core::spec::SigningSpec;
use signer_core::wasm_sandbox::Sandbox;
use signer_hal::{Buttons, Display, UsbMount};

/// Signatures added before Done, as `(slot, signature)`; empty
/// when nothing was signed.
pub type Signatures = Vec<(u8, Vec<u8>)>;

/// Read the stick (mounted only while reading), show the spec's label,
/// interpret the payload in the WASM sandbox and run the session.
pub fn review_stick<H: Display + Buttons, U: UsbMount>(
    hal: &mut H,
    usb: &mut U,
    signer: &mut dyn Signer,
) -> Result<Signatures, Box<dyn std::error::Error>> {
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
    let message = extract_signable(&contents.payload, &spec.signable)?;

    let (cols, rows) = hal.text_grid();
    // Only the key the request names, with its algorithm, is offered.
    let allowed = [(spec.key_slot, spec.algorithm)];
    let mut session = Session::new(Review::new(lines, cols, rows)?, message, &allowed, signer);
    loop {
        hal.show_review(&session.view())?;
        if let Finish::Done(signatures) = session.handle(hal.wait_event()?, signer) {
            return Ok(signatures);
        }
    }
}

/// Public inspection works independently of signing authorization.
pub struct PiKeyInventory;

impl Signer for PiKeyInventory {
    #[cfg(target_os = "linux")]
    fn public_slots(&mut self) -> Result<signer_core::key_menu::PublicSlots, String> {
        crate::se051::Se051::open("/dev/i2c-1")
            .and_then(|mut se| se.public_slots())
            .map_err(|error| error.to_string())
    }
    fn keys(&self) -> Vec<Key> {
        Vec::new()
    }
    fn sign(&mut self, _: u8, _: &[u8]) -> Result<Vec<u8>, String> {
        Err("no key".into())
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
        spec_signing(label, Signable::Whole)
    }

    fn spec_signing(label: &str, signable: Signable) -> Vec<u8> {
        SigningSpec {
            label: label.into(),
            signable,
            algorithm: SignAlgorithm::Ed25519,
            key_slot: 0,
            output: OutputSpec::SignatureOnly,
        }
        .to_cbor()
        .unwrap()
    }

    /// An Ed25519 key in slot 0; records the bytes it was asked to sign
    /// and returns a signature unrelated to them.
    #[derive(Default)]
    struct OneKey {
        received: Vec<Vec<u8>>,
    }

    const SIG: [u8; 4] = [0xab; 4];

    impl Signer for OneKey {
        fn keys(&self) -> Vec<Key> {
            vec![Key {
                slot: 0,
                algorithm: SignAlgorithm::Ed25519,
                label: "slot 0".into(),
            }]
        }
        fn sign(&mut self, _: u8, message: &[u8]) -> Result<Vec<u8>, String> {
            self.received.push(message.to_vec());
            Ok(SIG.to_vec())
        }
    }

    /// Run the stick through review and signing with slot 0; the bytes
    /// the key was asked to sign.
    fn signed_bytes(payload: &[u8], signable: Signable) -> Vec<Vec<u8>> {
        use ButtonEvent::{Confirm, Down, Mode};
        let mut hal = Hal {
            presses: VecDeque::from([Mode, Confirm, Down, Confirm]),
            ..Default::default()
        };
        let mut usb = stick(payload, echo_hex(), spec_signing("x", signable));
        let mut key = OneKey::default();
        assert_eq!(
            review_stick(&mut hal, &mut usb, &mut key).unwrap(),
            vec![(0, SIG.to_vec())]
        );
        key.received
    }

    #[test]
    fn the_key_signs_exactly_the_bytes_the_request_selects() {
        use signer_core::spec::{HashAlgorithm, SignableSource};
        assert_eq!(signed_bytes(b"hi", Signable::Whole), vec![b"hi".to_vec()]);
        assert_eq!(
            signed_bytes(
                b"012345",
                Signable::Range {
                    offset: 2,
                    length: 3
                }
            ),
            vec![b"234".to_vec()]
        );
        // sha256("234"), computed independently with sha256sum.
        assert_eq!(
            signed_bytes(
                b"012345",
                Signable::HashThenSign {
                    hash: HashAlgorithm::Sha256,
                    source: SignableSource::Range {
                        offset: 2,
                        length: 3
                    },
                }
            ),
            vec![
                hex::decode("114bd151f8fb0c58642d2170da4ae7d7c57977260ac2cc8905306cab6b2acabc")
                    .unwrap()
            ]
        );
    }

    #[test]
    fn a_key_the_request_does_not_name_is_not_offered() {
        use ButtonEvent::{Confirm, Mode};
        let mut hal = Hal {
            presses: VecDeque::from([Mode, Confirm]),
            ..Default::default()
        };
        let other_slot = SigningSpec {
            label: "x".into(),
            signable: Signable::Whole,
            algorithm: SignAlgorithm::Ed25519,
            key_slot: 1,
            output: OutputSpec::SignatureOnly,
        }
        .to_cbor()
        .unwrap();
        let mut usb = stick(b"hi", echo_hex(), other_slot);
        let mut key = OneKey::default();
        // Only Done is on the Sign screen: #5 finishes, nothing signed.
        assert_eq!(review_stick(&mut hal, &mut usb, &mut key).unwrap(), vec![]);
        assert!(key.received.is_empty());
    }

    fn stick(payload: &[u8], wasm: Vec<u8>, spec: Vec<u8>) -> Stick {
        Stick {
            files: Some((payload.to_vec(), wasm, spec)),
            ..Default::default()
        }
    }

    #[test]
    fn a_transaction_on_the_stick_is_interpreted_reviewed_and_signed() {
        use ButtonEvent::{Confirm, Down, Mode};
        let mut hal = Hal {
            presses: VecDeque::from([Mode, Confirm, Down, Confirm]),
            ..Default::default()
        };
        let mut usb = stick(b"hi", echo_hex(), spec("Test payment"));
        assert_eq!(
            review_stick(&mut hal, &mut usb, &mut OneKey::default()).unwrap(),
            vec![(0, SIG.to_vec())],
            "slot 0 signed"
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
    fn a_cut_value_is_read_or_accepted_before_approval() {
        // The payload on the test stick: its hex is too long for a row.
        let headers = |presses: &[ButtonEvent]| {
            let mut hal = Hal {
                presses: presses.iter().copied().collect(),
                ..Default::default()
            };
            let mut usb = stick(b"pay 5 ADA to Bob", echo_hex(), spec("Test payment"));
            let decision = review_stick(&mut hal, &mut usb, &mut OneKey::default()).unwrap();
            let headers: Vec<String> = hal
                .views
                .iter()
                .map(|v| match v {
                    View::Overview { header, .. } | View::Detail { header, .. } => header.clone(),
                })
                .collect();
            (decision, headers)
        };
        use ButtonEvent::{Back, Confirm, Down, Mode, Open};
        let signed = vec![(0, SIG.to_vec())];
        // Signing is refused while it is left.
        let (decision, _) = headers(&[Mode, Confirm, Down, Confirm]);
        assert_eq!(decision, vec![]);
        // Read it in full, then sign.
        let (decision, h) = headers(&[Open, Back, Mode, Confirm, Down, Confirm]);
        assert_eq!(decision, signed);
        assert!(h[0].ends_with("1 left"), "{h:?}");
        assert!(h[2].ends_with("done"), "{h:?}");
        // Accept it unread with #5, then sign.
        let (decision, h) = headers(&[Confirm, Mode, Confirm, Down, Confirm]);
        assert_eq!(decision, signed);
        assert!(h[1].ends_with(" 1 waived"), "{h:?}");
    }

    #[test]
    fn pulling_the_stick_mid_review_ends_it_with_nothing_decided() {
        struct Pulled(Hal);
        impl Display for Pulled {
            fn clear(&mut self) -> Result<(), HalError> {
                self.0.clear()
            }
            fn show_message(&mut self, t: &str) -> Result<(), HalError> {
                self.0.show_message(t)
            }
            fn show_lines(&mut self, l: &[DisplayLine], o: usize) -> Result<(), HalError> {
                self.0.show_lines(l, o)
            }
            fn text_grid(&self) -> (usize, usize) {
                self.0.text_grid()
            }
            fn show_review(&mut self, v: &View) -> Result<(), HalError> {
                self.0.show_review(v)
            }
        }
        impl Buttons for Pulled {
            fn wait_event(&mut self) -> Result<ButtonEvent, HalError> {
                Err(HalError::Usb(crate::usb::STICK_REMOVED.into()))
            }
        }
        let mut hal = Pulled(Hal::default());
        let mut usb = stick(b"hi", echo_hex(), spec("x"));
        let err =
            review_stick(&mut hal, &mut usb, &mut OneKey::default()).expect_err("no decision");
        assert!(err.to_string().contains(crate::usb::STICK_REMOVED), "{err}");
    }

    #[test]
    fn done_without_a_signature_signs_nothing() {
        use ButtonEvent::{Confirm, Down, Mode};
        let mut hal = Hal {
            presses: VecDeque::from([Mode, Down, Confirm]),
            ..Default::default()
        };
        let mut usb = stick(b"hi", echo_hex(), spec("x"));
        assert_eq!(
            review_stick(&mut hal, &mut usb, &mut OneKey::default()).unwrap(),
            vec![]
        );
    }
}
