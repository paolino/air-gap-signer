use signer_core::crypto::extract_signable;
use signer_core::display::{json_to_lines, DisplayLine};
use signer_core::review::Review;
use signer_core::session::{Finish, Key, Session, Signer};
use signer_core::spec::{OutputSpec, SignAlgorithm, SigningSpec};
use signer_core::wasm_sandbox::Sandbox;
use signer_hal::{ButtonEvent, Buttons, Display, HalError, SecureElement, UsbMount};

const PIN_LEN: usize = 4;

/// Digit-by-digit PIN entry using 4 buttons.
///
/// Up/Down cycles current digit 0–9, Confirm advances to next digit,
/// Reject goes back (or cancels if at first position).
/// Returns `None` if the user cancelled.
fn enter_pin<H: Display + Buttons>(hal: &mut H, prompt: &str) -> Result<Option<Vec<u8>>, HalError> {
    let mut digits = [0u8; PIN_LEN];
    let mut pos: usize = 0;

    loop {
        // Build display string: show entered digits as '*', current as digit, rest as '_'
        let mut display = String::new();
        for (i, d) in digits.iter().enumerate() {
            if i > 0 {
                display.push(' ');
            }
            if i < pos {
                display.push('*');
            } else if i == pos {
                display.push((b'0' + d) as char);
            } else {
                display.push('_');
            }
        }

        let lines = vec![
            DisplayLine {
                key: None,
                value: prompt.to_string(),
                indent: 0,
            },
            DisplayLine {
                key: None,
                value: String::new(),
                indent: 0,
            },
            DisplayLine {
                key: None,
                value: format!("  [ {display} ]"),
                indent: 0,
            },
            DisplayLine {
                key: None,
                value: String::new(),
                indent: 0,
            },
            DisplayLine {
                key: None,
                value: "Up/Down=digit  Enter=next  Esc=back".to_string(),
                indent: 0,
            },
        ];
        hal.show_lines(&lines, 0)?;

        match hal.wait_event()? {
            ButtonEvent::Up | ButtonEvent::UpHeld => {
                digits[pos] = (digits[pos] + 1) % 10;
            }
            ButtonEvent::Down | ButtonEvent::DownHeld => {
                digits[pos] = (digits[pos] + 9) % 10; // wrap around: 0 -> 9
            }
            ButtonEvent::Confirm => {
                pos += 1;
                if pos >= PIN_LEN {
                    // Convert digits to ASCII bytes
                    let pin: Vec<u8> = digits.iter().map(|d| b'0' + d).collect();
                    return Ok(Some(pin));
                }
            }
            ButtonEvent::Reject => {
                if pos == 0 {
                    return Ok(None);
                }
                pos -= 1;
            }
            ButtonEvent::Open | ButtonEvent::Back | ButtonEvent::Mode => {}
        }
    }
}

/// First-time setup: set PIN, provision key (generate or recover from USB), export to USBs.
fn run_setup<H: Display + Buttons>(
    hal: &mut H,
    usb: &mut dyn UsbMount,
    se: &mut dyn SecureElement,
) -> Result<(), HalError> {
    hal.show_message("SETUP")?;
    hal.wait_event()?;

    loop {
        let pin = match enter_pin(hal, "SET PIN")? {
            Some(p) => p,
            None => {
                hal.show_message("SETUP CANCELLED")?;
                hal.wait_event()?;
                return Err(HalError::Storage("setup cancelled".into()));
            }
        };

        let confirm = match enter_pin(hal, "CONFIRM PIN")? {
            Some(p) => p,
            None => continue,
        };

        if pin != confirm {
            hal.show_message("PIN MISMATCH")?;
            hal.wait_event()?;
            continue;
        }

        se.set_pin(&pin)?;
        se.verify_pin(&pin)?;

        // --- Private USB: read existing seed or generate new one ---
        hal.show_message("INSERT PRIVATE USB")?;
        hal.wait_event()?;

        let pubkey = match usb.read_file("seed.bin")? {
            Some(seed) => {
                hal.show_message("RECOVERING FROM SEED...")?;
                se.import_key(0, &seed)?
            }
            None => {
                hal.show_message("GENERATING NEW KEY...")?;
                let pubkey = se.generate_key(0)?;
                let seed = se.export_seed(0)?;
                usb.write_file("seed.bin", &seed)?;
                hal.show_message("SEED SAVED TO USB")?;
                hal.wait_event()?;
                pubkey
            }
        };

        // --- Swap to public USB ---
        hal.show_message("REMOVE PRIVATE USB")?;
        hal.wait_event()?;

        hal.show_message("INSERT PUBLIC USB")?;
        hal.wait_event()?;

        usb.write_file("pubkey.bin", &pubkey)?;

        hal.show_message("PUBKEY SAVED TO USB")?;
        hal.wait_event()?;

        hal.show_message("REMOVE USB - SETUP COMPLETE")?;
        hal.wait_event()?;
        return Ok(());
    }
}

/// Boot flow: run setup if needed, verify PIN, then enter signing loop.
pub fn run<H: Display + Buttons>(
    hal: &mut H,
    usb: &mut dyn UsbMount,
    se: &mut dyn SecureElement,
) -> Result<(), HalError> {
    if !se.is_provisioned() {
        run_setup(hal, usb, se)?;
    } else {
        // PIN verification on every boot
        loop {
            let pin = match enter_pin(hal, "ENTER PIN")? {
                Some(p) => p,
                None => {
                    hal.show_message("GOODBYE")?;
                    hal.wait_event()?;
                    return Ok(());
                }
            };
            match se.verify_pin(&pin) {
                Ok(()) => break,
                Err(_) => {
                    hal.show_message("WRONG PIN")?;
                    hal.wait_event()?;
                }
            }
        }
    }

    run_loop(hal, usb, se)
}

/// Run one signing cycle: read USB, interpret, display, sign, write output.
///
/// Returns `Ok(true)` on successful signing, `Ok(false)` on rejection.
pub fn run_once<H: Display + Buttons>(
    hal: &mut H,
    usb: &mut dyn UsbMount,
    se: &mut dyn SecureElement,
) -> Result<bool, Box<dyn std::error::Error>> {
    usb.mount_readonly()?;
    let contents = usb.read_contents()?;

    let spec = SigningSpec::from_cbor(&contents.signing_spec_cbor)?;
    hal.show_message(&spec.label)?;

    // Run WASM interpreter to produce display JSON
    let sandbox = Sandbox::new()?;
    let wasm_module = sandbox.load_module(&contents.interpreter_wasm)?;
    let json_str = wasm_module.interpret(&contents.payload)?;
    let json_val: serde_json::Value = serde_json::from_str(&json_str)?;
    let lines = json_to_lines(&json_val);

    // The same session as the device: review, then Sign (Tab) to add the
    // requested key's signature, then Done.
    let message = extract_signable(&contents.payload, &spec.signable)?;
    let (cols, rows) = hal.text_grid();
    let mut signer = SeSigner(se);
    let allowed = [(spec.key_slot, spec.algorithm)];
    let mut session = Session::new(Review::new(lines, cols, rows)?, message, &allowed, &signer);
    let signatures = loop {
        hal.show_review(&session.view())?;
        if let Finish::Done(signatures) = session.handle(hal.wait_event()?, &mut signer) {
            break signatures;
        }
    };
    let Some((_, sig)) = signatures.into_iter().next() else {
        hal.show_message("NOTHING SIGNED")?;
        usb.unmount()?;
        return Ok(false);
    };

    // Produce output
    let output = match &spec.output {
        OutputSpec::SignatureOnly => sig,
        OutputSpec::AppendToPayload => {
            let mut buf = contents.payload.clone();
            buf.extend_from_slice(&sig);
            buf
        }
        OutputSpec::WasmAssemble => wasm_module.assemble(&contents.payload, &sig)?,
    };

    usb.write_output(&output)?;
    usb.unmount()?;
    hal.show_message("DONE \u{2014} REMOVE USB")?;

    Ok(true)
}

/// The simulated secure element as the session's signer: its Ed25519
/// keys by slot.
struct SeSigner<'a>(&'a mut dyn SecureElement);

/// Slots the simulator looks for keys in.
const SIM_SLOTS: u8 = 4;

impl Signer for SeSigner<'_> {
    fn keys(&self) -> Vec<Key> {
        (0..SIM_SLOTS)
            .filter_map(|slot| {
                let public = self.0.public_key(slot).ok()?;
                Some(Key {
                    slot,
                    algorithm: SignAlgorithm::Ed25519,
                    label: format!(
                        "slot {slot} {}",
                        hex::encode(&public[..public.len().min(3)])
                    ),
                })
            })
            .collect()
    }

    fn sign(&mut self, slot: u8, message: &[u8]) -> Result<Vec<u8>, String> {
        self.0.sign(slot, message).map_err(|e| e.to_string())
    }
}

/// Main signing loop: idle -> insert -> sign -> repeat.
pub fn run_loop<H: Display + Buttons>(
    hal: &mut H,
    usb: &mut dyn UsbMount,
    se: &mut dyn SecureElement,
) -> Result<(), HalError> {
    loop {
        hal.show_message("INSERT USB")?;
        usb.wait_insert()?;

        match run_once(hal, usb, se) {
            Ok(_) => {}
            Err(e) => {
                let msg = format!("ERROR: {e}");
                let _ = hal.show_message(&msg);
                let _ = usb.unmount();
            }
        }

        // Wait for acknowledgment before returning to idle
        let _ = hal.wait_event();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use signer_core::review::View;
    use signer_core::spec::Signable;
    use signer_hal::UsbContents;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct Hal {
        presses: VecDeque<ButtonEvent>,
    }

    impl Display for Hal {
        fn clear(&mut self) -> Result<(), HalError> {
            Ok(())
        }
        fn show_message(&mut self, _: &str) -> Result<(), HalError> {
            Ok(())
        }
        fn show_lines(&mut self, _: &[DisplayLine], _: usize) -> Result<(), HalError> {
            Ok(())
        }
        fn text_grid(&self) -> (usize, usize) {
            (21, 11)
        }
        fn show_review(&mut self, _: &View) -> Result<(), HalError> {
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

    struct Usb {
        contents: UsbContents,
        written: Option<Vec<u8>>,
    }

    impl UsbMount for Usb {
        fn wait_insert(&mut self) -> Result<(), HalError> {
            Ok(())
        }
        fn mount_readonly(&mut self) -> Result<(), HalError> {
            Ok(())
        }
        fn read_contents(&self) -> Result<UsbContents, HalError> {
            Ok(UsbContents {
                payload: self.contents.payload.clone(),
                interpreter_wasm: self.contents.interpreter_wasm.clone(),
                signing_spec_cbor: self.contents.signing_spec_cbor.clone(),
            })
        }
        fn write_output(&mut self, data: &[u8]) -> Result<(), HalError> {
            self.written = Some(data.to_vec());
            Ok(())
        }
        fn read_file(&self, _: &str) -> Result<Option<Vec<u8>>, HalError> {
            Ok(None)
        }
        fn write_file(&mut self, _: &str, _: &[u8]) -> Result<(), HalError> {
            Ok(())
        }
        fn unmount(&mut self) -> Result<(), HalError> {
            Ok(())
        }
    }

    /// A key in slot 0 only; records what it signed.
    #[derive(Default)]
    struct Se {
        signed: Vec<(u8, Vec<u8>)>,
    }

    impl SecureElement for Se {
        fn set_pin(&mut self, _: &[u8]) -> Result<(), HalError> {
            Ok(())
        }
        fn verify_pin(&mut self, _: &[u8]) -> Result<(), HalError> {
            Ok(())
        }
        fn is_provisioned(&self) -> bool {
            true
        }
        fn generate_key(&mut self, _: u8) -> Result<Vec<u8>, HalError> {
            unreachable!()
        }
        fn sign(&mut self, slot: u8, message: &[u8]) -> Result<Vec<u8>, HalError> {
            self.signed.push((slot, message.to_vec()));
            Ok(vec![0xab; 4])
        }
        fn public_key(&self, slot: u8) -> Result<Vec<u8>, HalError> {
            match slot {
                0 => Ok(vec![1, 2, 3, 4]),
                _ => Err(HalError::Storage("empty".into())),
            }
        }
        fn import_key(&mut self, _: u8, _: &[u8]) -> Result<Vec<u8>, HalError> {
            unreachable!()
        }
        fn export_seed(&self, _: u8) -> Result<Vec<u8>, HalError> {
            unreachable!()
        }
    }

    fn usb() -> Usb {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/wasm32-unknown-unknown/release/echo_hex.wasm"
        );
        Usb {
            contents: UsbContents {
                payload: b"hi".to_vec(),
                interpreter_wasm: std::fs::read(path).expect("run `just build-wasm` first"),
                signing_spec_cbor: SigningSpec {
                    label: "x".into(),
                    signable: Signable::Whole,
                    algorithm: SignAlgorithm::Ed25519,
                    key_slot: 0,
                    output: OutputSpec::SignatureOnly,
                }
                .to_cbor()
                .unwrap(),
            },
            written: None,
        }
    }

    fn run(presses: &[ButtonEvent]) -> (bool, Usb, Se) {
        let mut hal = Hal {
            presses: presses.iter().copied().collect(),
        };
        let (mut usb, mut se) = (usb(), Se::default());
        let result = run_once(&mut hal, &mut usb, &mut se).map_err(|e| e.to_string());
        (result.unwrap_or(false), usb, se)
    }

    #[test]
    fn enter_in_review_does_not_sign() {
        let (signed, usb, se) = run(&[ButtonEvent::Confirm]);
        assert!(!signed);
        assert!(se.signed.is_empty() && usb.written.is_none());
    }

    #[test]
    fn signing_goes_through_the_sign_screen() {
        use ButtonEvent::{Confirm, Down, Mode};
        let (signed, usb, se) = run(&[Mode, Confirm, Down, Confirm]);
        assert!(signed);
        assert_eq!(se.signed, vec![(0, b"hi".to_vec())]);
        assert_eq!(usb.written, Some(vec![0xab; 4]));
    }
}
