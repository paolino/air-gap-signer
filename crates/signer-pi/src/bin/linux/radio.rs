use super::*;
pub(super) fn run_bluetooth(
    hal: &mut Hal,
    usb: &mut PiUsb,
) -> Result<(), Box<dyn std::error::Error>> {
    use signer_hal::HalError;
    use signer_pi::bluetooth::{self, Event};
    let keys = Path::new("/var/lib/signer-pi/practice");
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let radio = runtime.block_on(bluetooth::start(keys))?;
    let panel = radio.panel.clone();
    loop {
        if let Some(event) = panel.take() {
            match event {
                Event::Compare(code, reply) => {
                    if !reply.is_closed() {
                        let accepted = signer_pi::key_trial::confirm(
                            hal,
                            &format!("Pair laptop? Compare this code on both screens: {code:06}"),
                        )?;
                        let _ = reply.send(accepted);
                    }
                }
                Event::Review(data) => {
                    let signed = review_wire(hal, keys, &data);
                    panel.finish(
                        signer_link::id(&data),
                        signed.as_ref().copied().map_err(|_| ()),
                    );
                    if let Err(error) = signed {
                        hal.show_message(&format!("Review stopped. {error}"))?;
                        hal.wait_event()?;
                    }
                }
            }
            continue;
        }
        if usb.poll_insert() {
            round(hal, usb)?;
            continue;
        }
        let pairing = panel.pairing();
        let text = if pairing {
            "Pairing open (60s). Run signer pair.\n#6 Cancel"
        } else {
            "Bluetooth ready\n#5 Pair laptop\n#6 Keys"
        };
        hal.show_message(text)?;
        let wake = panel.clone();
        hal.buttons.set_abort(Some(Box::new(move || {
            (wake.ready()
                || (pairing && !wake.pairing())
                || find_stick(Path::new(SYSFS_BLOCK)).is_some())
            .then(|| HalError::Usb("radio or stick ready".into()))
        })));
        let event = hal.wait_event();
        hal.buttons.set_abort(None);
        match event {
            Err(HalError::Usb(s)) if s == "radio or stick ready" => (),
            Ok(ButtonEvent::Confirm) => panel.open_pairing(),
            Ok(ButtonEvent::Mode) if pairing => panel.close_pairing(),
            Ok(ButtonEvent::Mode) => {
                let mut menu = KeyMenu::new(PiKeyInventory.public_slots().ok(), hal.text_grid().1);
                local_keys(hal, &mut menu)?;
            }
            Ok(_) => (),
            Err(e) => return Err(e.into()),
        }
    }
}

fn review_wire(
    hal: &mut Hal,
    keys: &Path,
    data: &[u8],
) -> Result<Option<[u8; 64]>, Box<dyn std::error::Error>> {
    let (tx, plan) = signer_link::decode(data).ok_or("invalid request")?;
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("plan.json"), plan)?;
    std::fs::write(dir.path().join("payment.cbor"), tx)?;
    signer_pi::key_trial::preprod::run(hal, keys, dir.path())?;
    match std::fs::read(dir.path().join("payment.signature")) {
        Ok(bytes) => Ok(Some(
            bytes.try_into().map_err(|_| "invalid signature length")?,
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
