use super::*;
pub(super) fn run_bluetooth(
    hal: &mut Hal,
    usb: &mut PiUsb,
    radio: signer_pi::bluetooth::Radio,
) -> Result<(), Box<dyn std::error::Error>> {
    use signer_hal::HalError;
    use signer_pi::bluetooth::Event;
    let keys = Path::new("/var/lib/signer-pi/practice");
    let panel = radio.panel.clone();
    loop {
        if let Some(event) = panel.take() {
            match event {
                Event::Compare(code, reply) => {
                    if reply.is_closed() {
                        continue;
                    }
                    const ENDED: &str = "Bluetooth comparison ended";
                    let pending = reply.clone();
                    hal.buttons.set_abort(Some(Box::new(move || {
                        pending.is_closed().then(|| HalError::Button(ENDED.into()))
                    })));
                    let result = signer_pi::key_trial::confirm(
                        hal,
                        &format!("Pair laptop?\nCompare both screens\n{code:06}"),
                    );
                    hal.buttons.set_abort(None);
                    match result {
                        Ok(accepted) => {
                            let _ = reply.send(accepted);
                        }
                        Err(HalError::Button(reason)) if reason == ENDED => (),
                        Err(e) => return Err(e.into()),
                    }
                }
                Event::Clock { unix, received } => {
                    const EXPIRED: &str = "Clock request expired";
                    let window = std::time::Duration::from_secs(60);
                    if received.elapsed() >= window {
                        continue;
                    }
                    let formatted = std::process::Command::new("date")
                        .args(["-u", "-d", &format!("@{unix}"), "+%F %T UTC"])
                        .output()?;
                    if !formatted.status.success() {
                        return Err("cannot display proposed time".into());
                    }
                    let utc = std::str::from_utf8(&formatted.stdout)?.trim();
                    hal.buttons.set_abort(Some(Box::new(move || {
                        (received.elapsed() >= window).then(|| HalError::Button(EXPIRED.into()))
                    })));
                    let accepted = signer_pi::key_trial::confirm(
                        hal,
                        &format!("Set panel clock?\n{utc}\nCheck date and time."),
                    );
                    hal.buttons.set_abort(None);
                    match accepted {
                        Ok(true) if received.elapsed() < window => {
                            use rustix::time::{clock_gettime, clock_settime, ClockId, Timespec};
                            let target = (unix + received.elapsed().as_secs()) as i64;
                            if target > clock_gettime(ClockId::Realtime).tv_sec {
                                clock_settime(
                                    ClockId::Realtime,
                                    Timespec {
                                        tv_sec: target,
                                        tv_nsec: 0,
                                    },
                                )?;
                            }
                        }
                        Ok(_) => (),
                        Err(HalError::Button(reason)) if reason == EXPIRED => (),
                        Err(e) => return Err(e.into()),
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
            "Pairing open (60s)\nRun signer pair.\n\n#6 Cancel"
        } else {
            "Bluetooth ready\n\n#5 Pair laptop\n#6 Keys"
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
