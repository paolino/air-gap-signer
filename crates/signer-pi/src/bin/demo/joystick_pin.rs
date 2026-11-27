//! Volatile, disposable PIN trial for the prototype bonnet.
use gpiocdev::line::{Bias, Value, Values};
use gpiocdev::Request;
use panel_input::{EntryEvent, PinEntry, Sequence};
use signer_hal::{Display, HalError};
use signer_pi::buttons::{DEBOUNCE, POLL, SETTLE};
use signer_pi::display::show_pin_count;
use std::thread;

// BCM line and corresponding panel_input bit. GPIO4 (centre) is unused.
const CONTACTS: [(u32, u8); 6] = [(17, 0), (23, 1), (22, 2), (27, 3), (6, 5), (5, 6)];

fn gpio_error(e: gpiocdev::Error) -> HalError {
    HalError::Button(e.to_string())
}

struct Pad {
    request: Request,
    values: Values,
}

impl Pad {
    fn open(chip: &str) -> Result<Self, HalError> {
        let lines = CONTACTS.map(|(line, _)| line);
        let request = Request::builder()
            .on_chip(chip)
            .with_consumer("signer-pi-panel-demo")
            .with_lines(&lines)
            .as_input()
            .with_bias(Bias::PullUp)
            .with_debounce_period(DEBOUNCE)
            .request()
            .map_err(gpio_error)?;
        Ok(Self {
            request,
            values: Values::from_offsets(&lines),
        })
    }

    fn down(&mut self) -> Result<u8, HalError> {
        self.request.values(&mut self.values).map_err(gpio_error)?;
        Ok(CONTACTS.iter().fold(0, |mask, &(line, bit)| {
            mask | (u8::from(self.values.get(line) == Some(Value::Inactive)) << bit)
        }))
    }

    fn enter(&mut self, display: &mut impl Display, prompt: &str) -> Result<Sequence, HalError> {
        loop {
            show_pin_count(display, prompt, 0)?;
            thread::sleep(SETTLE);
            let mut entry = PinEntry::default();
            loop {
                match entry.sample(self.down()?) {
                    EntryEvent::Pending => thread::sleep(POLL),
                    EntryEvent::Progress(count) => show_pin_count(display, prompt, count)?,
                    EntryEvent::Restart => break,
                    EntryEvent::Complete(sequence) => {
                        show_pin_count(display, prompt, panel_input::PIN_LENGTH)?;
                        return Ok(sequence);
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
        let correct = [1, 1, 2, 4, 8, 4, 1, 2, 8];
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
