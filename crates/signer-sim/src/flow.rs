use signer_core::crypto::extract_signable;
use signer_core::display::json_to_lines;
use signer_core::spec::{OutputSpec, SigningSpec};
use signer_core::wasm_sandbox::Sandbox;
use signer_hal::{ButtonEvent, Buttons, Display, HalError, SecureElement, UsbMount};

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
