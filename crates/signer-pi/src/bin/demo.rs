//! Reviews a sample transaction on the prototype rig: the joystick moves
//! the cursor, pressing it opens a line in full, button #5 confirms once
//! every shortened line has been read, button #6 rejects.

#[cfg(target_os = "linux")]
#[path = "demo/joystick_pin.rs"]
mod joystick_pin;

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
    use signer_hal::{ButtonEvent, Buttons, Display, HalError};
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

        /// Try eyes-free entry of a disposable joystick PIN before one sample review
        #[arg(long, conflicts_with_all = ["script", "json"])]
        panel_trial: bool,

        /// Review this JSON file instead of the built-in sample
        #[arg(long)]
        json: Option<PathBuf>,

        /// Play presses from this file instead of reading the buttons:
        /// one step per line, `key | caption`, key one of up, down, left,
        /// right, ok, no, wait. Each step is logged to stdout as a JSON
        /// line with its wall-clock time, for captioning a recording.
        #[arg(long)]
        script: Option<PathBuf>,

        /// Pause before each scripted step, in milliseconds
        #[arg(long, default_value_t = 2000)]
        step_ms: u64,
    }

    /// Presses played back from a script; `None` when it is used up.
    struct Script {
        steps: std::collections::VecDeque<(Option<ButtonEvent>, String)>,
        pause: std::time::Duration,
    }

    impl Script {
        fn load(path: &Path, pause_ms: u64) -> Result<Self, String> {
            let text =
                std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
            let mut steps = std::collections::VecDeque::new();
            for (n, line) in text.lines().enumerate() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let (key, caption) = line.split_once('|').unwrap_or((line, ""));
                let event = match key.trim() {
                    "up" => Some(ButtonEvent::Up),
                    "down" => Some(ButtonEvent::Down),
                    "left" => Some(ButtonEvent::Back),
                    "right" => Some(ButtonEvent::Open),
                    "ok" => Some(ButtonEvent::Confirm),
                    "no" => Some(ButtonEvent::Reject),
                    "wait" => None,
                    other => return Err(format!("line {}: unknown key {other:?}", n + 1)),
                };
                steps.push_back((event, caption.trim().to_string()));
            }
            Ok(Self {
                steps,
                pause: std::time::Duration::from_millis(pause_ms),
            })
        }

        /// Wait, log the step, and return its press; `wait` steps only log.
        fn next(&mut self) -> Option<ButtonEvent> {
            loop {
                let (event, caption) = self.steps.pop_front()?;
                std::thread::sleep(self.pause);
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_millis());
                println!(
                    "{}",
                    serde_json::json!({"t_ms": ms as u64, "key": format!("{event:?}"), "caption": caption})
                );
                if let Some(event) = event {
                    return Some(event);
                }
            }
        }
    }

    impl Buttons for Script {
        fn wait_event(&mut self) -> Result<ButtonEvent, HalError> {
            self.next()
                .ok_or_else(|| HalError::Button("script finished".into()))
        }
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

    fn run<B: Buttons>(
        display: &mut FbDisplay,
        buttons: &mut B,
        value: &serde_json::Value,
        single_trial: bool,
    ) -> Result<(), HalError> {
        loop {
            let (cols, rows) = display.text_grid();
            let mut review = Review::new(json_to_lines(value), cols, rows)
                .map_err(|e| HalError::Display(e.to_string()))?;
            let outcome = loop {
                display.show_review(&review.view())?;
                match review.handle(buttons.wait_event()?) {
                    Outcome::Continue => {}
                    done => break done,
                }
            };
            if single_trial {
                return display.show_message(match outcome {
                    Outcome::Confirmed => "Practice review confirmed. Trial complete.",
                    _ => "Practice review rejected. Trial complete.",
                });
            }
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
        let value: serde_json::Value = match &cli.json {
            Some(path) => std::fs::read(path)
                .map_err(|e| e.to_string())
                .and_then(|b| serde_json::from_slice(&b).map_err(|e| e.to_string()))
                .unwrap_or_else(|e| fail(e)),
            None => serde_json::from_str(SAMPLE).expect("sample is valid JSON"),
        };
        let result = match &cli.script {
            Some(path) => {
                let mut script = Script::load(path, cli.step_ms).unwrap_or_else(|e| fail(e));
                match run(&mut display, &mut script, &value, false) {
                    Err(HalError::Button(m)) if m == "script finished" => Ok(()),
                    other => other,
                }
            }
            None => {
                if cli.panel_trial {
                    super::joystick_pin::trial(&cli.chip, &mut display).unwrap_or_else(|e| fail(e));
                }
                let mut buttons = GpioButtons::open(&cli.chip).unwrap_or_else(|e| fail(e));
                run(&mut display, &mut buttons, &value, cli.panel_trial)
            }
        };
        if let Err(e) = result {
            fail(e);
        }
    }
}
