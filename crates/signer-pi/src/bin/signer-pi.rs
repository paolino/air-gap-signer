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
    use signer_hal::{Buttons, Display, UsbMount};
    use signer_pi::buttons::GpioButtons;
    use signer_pi::display::{find_framebuffer, FbDisplay};
    use signer_pi::flow::{review_stick, Decision};
    use signer_pi::usb::PiUsb;
    use std::path::{Path, PathBuf};

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
        let mut usb = PiUsb::new(PathBuf::from("/sys/block"), cli.mount_point);
        loop {
            if let Err(e) = round(&mut hal, &mut usb) {
                fail(e);
            }
        }
    }

    /// One stick: wait for it, review, show the outcome, wait for removal.
    /// Problems with the stick are shown on screen, not fatal.
    fn round(hal: &mut Hal, usb: &mut PiUsb) -> Result<(), signer_hal::HalError> {
        hal.show_message("Insert USB stick")?;
        usb.wait_insert()?;
        hal.show_message("Reading USB stick")?;
        let outcome = match review_stick(hal, usb) {
            Ok(Decision::Approved) => {
                "Approved. Signing is not available yet: nothing was signed.".to_string()
            }
            Ok(Decision::Rejected) => "Rejected. Nothing was signed.".to_string(),
            Err(e) => format!("Cannot review this stick: {e}"),
        };
        hal.show_message(&format!("{outcome} Remove the USB stick."))?;
        usb.wait_removed();
        Ok(())
    }
}
