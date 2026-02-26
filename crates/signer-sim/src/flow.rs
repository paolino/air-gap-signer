use signer_core::crypto::extract_signable;
use signer_core::display::{json_to_lines, DisplayLine};
use signer_core::spec::{OutputSpec, SigningSpec};
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
            ButtonEvent::Up => {
                digits[pos] = (digits[pos] + 1) % 10;
            }
            ButtonEvent::Down => {
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

    // Scrollable review
    let mut scroll: usize = 0;
    let max_scroll = lines.len().saturating_sub(1);
    hal.show_lines(&lines, scroll)?;

    let confirmed = loop {
        match hal.wait_event()? {
            ButtonEvent::Up => {
                scroll = scroll.saturating_sub(1);
                hal.show_lines(&lines, scroll)?;
            }
            ButtonEvent::Down => {
                scroll = max_scroll.min(scroll + 1);
                hal.show_lines(&lines, scroll)?;
            }
            ButtonEvent::Confirm => break true,
            ButtonEvent::Reject => break false,
        }
    };

    if !confirmed {
        hal.show_message("REJECTED")?;
        usb.unmount()?;
        return Ok(false);
    }

    // Extract signable bytes and sign via secure element
    let message = extract_signable(&contents.payload, &spec.signable)?;
    let sig = se.sign(spec.key_slot, &message)?;

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
