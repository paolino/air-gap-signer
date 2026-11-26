//! Volatile, disposable PIN trial for the prototype bonnet.
use gpiocdev::line::{Bias, Value, Values};
use gpiocdev::Request;
use panel_input::{EntryEvent, PinEntry, Sequence};
use signer_hal::{Display, HalError};
use signer_pi::buttons::{DEBOUNCE, POLL, SETTLE};
use std::thread;

// BCM line numbers in panel_input contact order.
const LINES: [u32; 7] = [17, 23, 22, 27, 4, 6, 5];

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

    fn enter(&mut self, display: &mut impl Display, prompt: &str) -> Result<Sequence, HalError> {
        loop {
            display.show_message(prompt)?;
            thread::sleep(SETTLE);
            let mut entry = PinEntry::default();
            loop {
                match entry.sample(self.down()?) {
                    EntryEvent::Pending => thread::sleep(POLL),
                    EntryEvent::Restart => break,
                    EntryEvent::Complete(sequence) => return Ok(sequence),
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
    mut enter: impl FnMut(&str) -> Result<Sequence, HalError>,
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
    use panel_input::PIN_LENGTH;

    #[test]
    fn mismatch_requires_another_complete_entry() {
        let correct = [1, 1, 2, 4, 8, 16, 1, 2, 8];
        let mut entries = [correct, [2; PIN_LENGTH], correct].into_iter();
        let mut prompts = Vec::new();
        verify_practice_sequence(|prompt| {
            prompts.push(prompt.to_owned());
            Ok(Sequence::new(entries.next().unwrap()))
        })
        .unwrap();
        assert_eq!(prompts.len(), 3);
        assert!(prompts[2].starts_with("No match"));
        assert!(entries.next().is_none());
    }
}
