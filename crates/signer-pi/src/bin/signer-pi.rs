//! The signer on the prototype rig, up to the decision: wait for a USB
//! stick, review the transaction on it, show the decision, wait for the
//! stick to be pulled out, repeat. Signing needs the secure element and
//! is not done yet.

#[cfg(target_os = "linux")]
fn main() {
    linux::main()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("signer-pi needs Linux framebuffer, GPIO and USB devices");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
mod linux {
    use clap::Parser;
    use signer_core::key_menu::KeyMenu;
    use signer_core::session::Signer;
    use signer_hal::ButtonEvent;
    use signer_hal::{Buttons, Display};
    use signer_pi::buttons::GpioButtons;
    use signer_pi::display::{find_framebuffer, FbDisplay};
    use signer_pi::flow::{review_stick, PiKeyInventory};
    use signer_pi::usb::{find_stick, PiUsb, STICK_REMOVED};
    use std::path::{Path, PathBuf};

    const SYSFS_BLOCK: &str = "/sys/block";
    const SYSFS: &str = "/sys/class/graphics";

    #[derive(Parser)]
    #[command(name = "signer-pi", about = "Air-gapped signer on the prototype rig")]
    struct Cli {
        /// Framebuffer device; found by driver name when omitted
        #[arg(long)]
        fb: Option<PathBuf>,

        /// Font size in pixels: 18 gives 22 characters per row
        #[arg(long, default_value_t = 18.0)]
        size: f32,

        /// GPIO character device
        #[arg(long, default_value = "/dev/gpiochip0")]
        chip: String,

        /// Run the disposable SE051/FIDO recovery trial; directory stores the
        /// encrypted backup and raw public key between runs.
        #[arg(long, value_name = "DIRECTORY")]
        practice_keys: Option<PathBuf>,

        /// Where the stick is mounted while it is read
        #[arg(long, default_value = "/run/signer-usb")]
        mount_point: PathBuf,
    }

    /// The device and the hardware it drives, as one HAL.
    struct Hal {
        display: FbDisplay,
        buttons: GpioButtons,
    }

    impl Display for Hal {
        fn clear(&mut self) -> Result<(), signer_hal::HalError> {
            self.display.clear()
        }
        fn show_message(&mut self, text: &str) -> Result<(), signer_hal::HalError> {
            self.display.show_message(text)
        }
        fn show_lines(
            &mut self,
            lines: &[signer_core::display::DisplayLine],
            offset: usize,
        ) -> Result<(), signer_hal::HalError> {
            self.display.show_lines(lines, offset)
        }
        fn text_grid(&self) -> (usize, usize) {
            self.display.text_grid()
        }
        fn show_review(
            &mut self,
            view: &signer_core::review::View,
        ) -> Result<(), signer_hal::HalError> {
            self.display.show_review(view)
        }
    }

    impl signer_pi::recovery_pin::PinContacts for Hal {
        fn pin_contacts(&mut self) -> Result<u8, signer_hal::HalError> {
            self.buttons.pin_contacts()
        }
    }

    impl signer_pi::recovery_pin::PinDisplay for Hal {
        fn draw_keyboard(
            &mut self,
            keyboard: &panel_input::token_pin::Keyboard,
            count: usize,
            status: panel_input::token_pin::Selected,
            highlight: bool,
        ) -> Result<(), signer_hal::HalError> {
            self.display
                .draw_keyboard(keyboard, count, status, highlight)
        }
    }

    impl Buttons for Hal {
        fn wait_event(&mut self) -> Result<signer_hal::ButtonEvent, signer_hal::HalError> {
            self.buttons.wait_event()
        }
    }

    fn fail(e: impl std::fmt::Display) -> ! {
        eprintln!("{e}");
        std::process::exit(1);
    }

    pub fn main() {
        let cli = Cli::parse();
        let fb = cli
            .fb
            .or_else(|| find_framebuffer(Path::new(SYSFS), "fb_st7789v"))
            .unwrap_or_else(|| fail("no fb_st7789v framebuffer; is dtoverlay=minipitft13 set?"));
        let display = FbDisplay::open(Path::new(SYSFS), &fb, cli.size).unwrap_or_else(|e| fail(e));
        let buttons = GpioButtons::open(&cli.chip).unwrap_or_else(|e| fail(e));
        let mut hal = Hal { display, buttons };
        if let Some(dir) = cli.practice_keys {
            if let Err(error) = signer_pi::key_trial::run(&mut hal, &dir) {
                let message = format!("Key trial stopped: {error}");
                let _ = hal.show_message(&message);
                fail(message);
            }
            return;
        }
        let mut usb = PiUsb::new(PathBuf::from("/sys/block"), cli.mount_point);
        loop {
            if let Err(e) = round(&mut hal, &mut usb) {
                fail(e);
            }
        }
    }

    /// Open the local inventory while idle. The user can inspect keys with
    /// no transaction stick attached. Finish an open menu before reading a stick.
    fn wait_for_stick(hal: &mut Hal, usb: &mut PiUsb) -> Result<(), signer_hal::HalError> {
        const INSERTED: &str = "transaction stick inserted";
        loop {
            if usb.poll_insert() {
                return Ok(());
            }
            hal.show_message("Insert USB stick. #6 Keys")?;
            hal.buttons.set_abort(Some(Box::new(|| {
                find_stick(Path::new(SYSFS_BLOCK))
                    .is_some()
                    .then(|| signer_hal::HalError::Usb(INSERTED.into()))
            })));
            let event = hal.wait_event();
            hal.buttons.set_abort(None);
            match event {
                Err(signer_hal::HalError::Usb(reason)) if reason == INSERTED => continue,
                Ok(ButtonEvent::Mode) => {
                    let mut menu =
                        KeyMenu::new(PiKeyInventory.public_slots().ok(), hal.text_grid().1);
                    loop {
                        hal.show_review(&menu.view())?;
                        let button = hal.wait_event()?;
                        if button == ButtonEvent::Mode {
                            break;
                        }
                        if menu.needs_retry() && button == ButtonEvent::Confirm {
                            menu.refresh(PiKeyInventory.public_slots().ok());
                        } else {
                            menu.handle(button);
                        }
                    }
                }
                Ok(_) => {}
                Err(error) => return Err(error),
            }
        }
    }

    /// One stick: wait for it, review, show the outcome, wait for removal.
    /// Problems with the stick are shown on screen, not fatal.
    fn round(hal: &mut Hal, usb: &mut PiUsb) -> Result<(), signer_hal::HalError> {
        wait_for_stick(hal, usb)?;
        hal.show_message("Reading USB stick")?;
        // Pulling the stick out ends the review at once.
        hal.buttons.set_abort(Some(Box::new(|| {
            find_stick(Path::new(SYSFS_BLOCK))
                .is_none()
                .then(|| signer_hal::HalError::Usb(STICK_REMOVED.into()))
        })));
        let result = review_stick(hal, usb, &mut PiKeyInventory);
        hal.buttons.set_abort(None);
        let outcome = match result {
            Err(e) if e.to_string().contains(STICK_REMOVED) => {
                hal.show_message("USB stick removed. Nothing was signed.")?;
                return Ok(());
            }
            Ok(signatures) if signatures.is_empty() => "Nothing was signed.".to_string(),
            Ok(signatures) => format!(
                "{} signatures. Writing them to the stick is not available yet: nothing was written.",
                signatures.len()
            ),
            Err(e) => format!("Cannot review this stick: {e}"),
        };
        hal.show_message(&format!("{outcome} Remove the USB stick."))?;
        usb.wait_removed();
        Ok(())
    }
}
