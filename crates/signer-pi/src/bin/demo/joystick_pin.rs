//! Volatile, disposable PIN trial for the prototype bonnet.
use gpiocdev::line::{Bias, Value, Values};
use gpiocdev::Request;
use signer_hal::{Display, HalError};
use signer_pi::buttons::{DEBOUNCE, POLL, SETTLE};
use std::thread;
use zeroize::Zeroizing;

// Up, right, down, left, centre, Mode, Confirm. Bit positions are local to this demo.
const LINES: [u32; 7] = [17, 23, 22, 27, 4, 6, 5];
const MODE: u8 = 1 << 5;
const LENGTH: usize = 9;

#[derive(Default)]
struct Gesture {
    contacts: u8,
}

// Collect all debounced contacts across one excursion, including rolls
// between contacts. Emit only once the joystick returns to neutral.
impl Gesture {
    fn sample(&mut self, down: u8) -> Option<u8> {
        if down != 0 {
            self.contacts |= down;
            return None;
        }
        let contacts = std::mem::take(&mut self.contacts);
        (contacts.is_power_of_two() && contacts <= MODE).then_some(contacts)
    }
}

fn gpio_error(e: gpiocdev::Error) -> HalError {
    HalError::Button(e.to_string())
}

struct Pad {
    request: Request,
    values: Values,
}

impl Pad {
    fn open(chip: &str) -> Result<Self, HalError> {
        let request = Request::builder()
            .on_chip(chip)
            .with_consumer("signer-pi-panel-demo")
            .with_lines(&LINES)
            .as_input()
            .with_bias(Bias::PullUp)
            .with_debounce_period(DEBOUNCE)
            .request()
            .map_err(gpio_error)?;
        Ok(Self {
            request,
            values: Values::from_offsets(&LINES),
        })
    }

    fn down(&mut self) -> Result<u8, HalError> {
        self.request.values(&mut self.values).map_err(gpio_error)?;
        Ok(LINES.iter().enumerate().fold(0, |mask, (bit, line)| {
            mask | (u8::from(self.values.get(*line) == Some(Value::Inactive)) << bit)
        }))
    }

    fn enter(
        &mut self,
        display: &mut impl Display,
        prompt: &str,
    ) -> Result<Zeroizing<[u8; LENGTH]>, HalError> {
        loop {
            display.show_message(prompt)?;
            thread::sleep(SETTLE);
            while self.down()? != 0 {
                thread::sleep(POLL);
            }
            let mut pin = Zeroizing::new([0; LENGTH]);
            let mut used = 0;
            let mut gesture = Gesture::default();
            loop {
                thread::sleep(POLL);
                if let Some(symbol) = gesture.sample(self.down()?) {
                    if symbol == MODE {
                        break;
                    }
                    pin[used] = symbol;
                    used += 1;
                    if used == LENGTH {
                        return Ok(pin);
                    }
                }
            }
        }
    }
}

pub(super) fn trial(chip: &str, display: &mut impl Display) -> Result<(), HalError> {
    let mut pad = Pad::open(chip)?;
    verify_practice_sequence(|prompt| pad.enter(display, prompt))
}

fn verify_practice_sequence(
    mut enter: impl FnMut(&str) -> Result<Zeroizing<[u8; LENGTH]>, HalError>,
) -> Result<(), HalError> {
    let enrolled = enter("Practice only. Set 9 joystick actions. Mode restarts.")?;
    let mut prompt = "Repeat the 9 actions to open the sample review.";
    loop {
        if enter(prompt)? == enrolled {
            return Ok(());
        }
        prompt = "No match. Repeat your 9 practice actions. Mode restarts.";
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mismatch_requires_another_complete_entry() {
        let correct = [1, 1, 2, 4, 8, 16, 1, 2, 8];
        let mut entries = [correct, [2; LENGTH], correct].into_iter();
        let mut prompts = Vec::new();
        verify_practice_sequence(|prompt| {
            prompts.push(prompt.to_owned());
            Ok(Zeroizing::new(entries.next().unwrap()))
        })
        .unwrap();
        assert_eq!(prompts.len(), 3);
        assert!(prompts[2].starts_with("No match"));
        assert!(entries.next().is_none());
    }

    #[test]
    fn held_and_repeated_inputs_require_neutral() {
        for symbol in [1, 2, 4, 8, 16, MODE] {
            let mut g = Gesture::default();
            let events: Vec<_> = [0, symbol, symbol, 0, 0, symbol, 0]
                .into_iter()
                .filter_map(|s| g.sample(s))
                .collect();
            assert_eq!(events, [symbol, symbol]);
        }
    }

    #[test]
    fn reject_overlapping_contacts_and_confirm_until_neutral() {
        let mut g = Gesture::default();
        let events: Vec<_> = [1, 3, 2, 0, 64, 0, MODE | 4, 0, 16, 0]
            .into_iter()
            .filter_map(|s| g.sample(s))
            .collect();
        assert_eq!(events, [16]);
    }
}
