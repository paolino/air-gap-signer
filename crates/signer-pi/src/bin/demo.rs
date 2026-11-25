//! Reviews a sample transaction on the prototype rig: the joystick moves
//! the cursor, pressing it opens a line in full, button #5 confirms once
//! every shortened line has been read, button #6 rejects.

#[cfg(target_os = "linux")]
fn main() {
    linux::main()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("signer-pi-demo needs Linux framebuffer and GPIO devices");
    std::process::exit(1);
}

#[cfg(target_os = "linux")]
mod linux {
    use clap::Parser;
    use signer_core::display::json_to_lines;
    use signer_core::review::{Outcome, Review};
    use signer_hal::{Buttons, Display, HalError};
    use signer_pi::buttons::GpioButtons;
    use signer_pi::display::{find_framebuffer, FbDisplay};
    use std::path::{Path, PathBuf};

    const SYSFS: &str = "/sys/class/graphics";

    #[derive(Parser)]
    #[command(
        name = "signer-pi-demo",
        about = "Transaction review demo on the prototype rig"
    )]
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
    }

    const SAMPLE: &str = r#"{
  "network": "preprod",
  "fee": "171485",
  "outputs": [
    {"address": "addr_test1qz2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3jcu5d8ps7zex2k2xt3uqxgjqnnj83ws8lhrn648jjxtwq2ytjqp", "lovelace": "5000000"},
    {"address": "addr_test1vrgp3znqfq5lr4qyq0vcsy7vjdv5sr2ywdcgfgq2yqgfvjqj5ndw7", "lovelace": "94828515"}
  ],
  "validity_end": 87654321
}"#;

    fn run(display: &mut FbDisplay, buttons: &mut GpioButtons) -> Result<(), HalError> {
        let value: serde_json::Value = serde_json::from_str(SAMPLE).expect("sample is valid JSON");
        loop {
            let (cols, rows) = display.text_grid();
            let mut review = Review::new(json_to_lines(&value), cols, rows)
                .map_err(|e| HalError::Display(e.to_string()))?;
            let outcome = loop {
                display.show_review(&review.view())?;
                match review.handle(buttons.wait_event()?) {
                    Outcome::Continue => {}
                    done => break done,
                }
            };
            let message = match outcome {
                Outcome::Confirmed => "Confirmed. Any button to restart.",
                _ => "Rejected. Any button to restart.",
            };
            display.show_message(message)?;
            buttons.wait_event()?;
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
        let mut display =
            FbDisplay::open(Path::new(SYSFS), &fb, cli.size).unwrap_or_else(|e| fail(e));
        let mut buttons = GpioButtons::open(&cli.chip).unwrap_or_else(|e| fail(e));
        if let Err(e) = run(&mut display, &mut buttons) {
            fail(e);
        }
    }
}
