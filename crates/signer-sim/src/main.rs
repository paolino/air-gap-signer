mod buttons;
mod display;
mod flow;
mod keystore;
mod usb;

use clap::Parser;
use display::SimDisplay;
use signer_core::display::DisplayLine;
use signer_hal::{ButtonEvent, HalError};
use std::path::PathBuf;
use usb::SimUsb;

#[derive(Parser)]
#[command(name = "signer-sim", about = "Air-gapped signer desktop simulator")]
struct Cli {
    /// Directory simulating USB stick contents
    #[arg(long)]
    usb_dir: PathBuf,

    /// Path to plaintext keystore JSON file (maps slot numbers to hex seeds)
    #[arg(long)]
    keystore: PathBuf,
}

/// Wraps SimDisplay to also implement the Buttons trait,
/// since both need access to the same minifb window.
struct SimHal {
    display: SimDisplay,
}

impl signer_hal::Display for SimHal {
    fn clear(&mut self) -> Result<(), HalError> {
        signer_hal::Display::clear(&mut self.display)
    }

    fn show_message(&mut self, text: &str) -> Result<(), HalError> {
        signer_hal::Display::show_message(&mut self.display, text)
    }

    fn show_lines(&mut self, lines: &[DisplayLine], scroll_offset: usize) -> Result<(), HalError> {
        signer_hal::Display::show_lines(&mut self.display, lines, scroll_offset)
    }
}

impl signer_hal::Buttons for SimHal {
    fn wait_event(&mut self) -> Result<ButtonEvent, HalError> {
        buttons::wait_event(self.display.window_mut())
    }
}

fn main() {
    let cli = Cli::parse();

    let mut se = keystore::SimSecureElement::from_file(&cli.keystore).unwrap_or_else(|e| {
        eprintln!("keystore error: {e}");
        std::process::exit(1);
    });

    let sim_display = SimDisplay::new().unwrap_or_else(|e| {
        eprintln!("display error: {e}");
        std::process::exit(1);
    });

    let mut hal = SimHal {
        display: sim_display,
    };
    let mut usb = SimUsb::new(cli.usb_dir);

    if let Err(e) = flow::run_loop(&mut hal, &mut usb, &mut se) {
        eprintln!("flow error: {e}");
        std::process::exit(1);
    }
}
