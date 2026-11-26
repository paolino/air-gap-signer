//! The display sees the keyboard layout, cursor visibility and count only.
use panel_input::token_pin::{KeyRepeat, Keyboard, Picker, Selected, Stroke};
use signer_hal::HalError;
use std::time::{Duration, Instant};

const FLASH: Duration = Duration::from_millis(100);

pub trait PinDisplay {
    fn draw_keyboard(
        &mut self,
        keyboard: &Keyboard,
        count: usize,
        status: Selected,
        highlight: bool,
    ) -> Result<(), HalError>;
}

pub trait PinContacts {
    fn pin_contacts(&mut self) -> Result<u8, HalError>;
}

pub fn enter(hal: &mut (impl PinContacts + PinDisplay)) -> Result<Option<Picker>, HalError> {
    let start = Instant::now();
    enter_with_clock(
        hal,
        || start.elapsed(),
        || std::thread::sleep(crate::buttons::POLL),
    )
}

fn enter_with_clock(
    hal: &mut (impl PinContacts + PinDisplay),
    now: impl Fn() -> Duration,
    mut poll: impl FnMut(),
) -> Result<Option<Picker>, HalError> {
    let mut pin = Picker::default();
    let mut keys = KeyRepeat::default();
    let mut status = Selected::Editing;
    hal.draw_keyboard(&pin.keyboard, 0, status, true)?;
    let mut hide_at = Some(now() + FLASH);
    loop {
        let t = now();
        if let Some(stroke) = keys.sample(hal.pin_contacts()?, t) {
            status = Selected::Editing;
            match stroke {
                Stroke::Up => pin.keyboard.vertical(false),
                Stroke::Down => pin.keyboard.vertical(true),
                Stroke::Left => pin.keyboard.horizontal(false),
                Stroke::Right => pin.keyboard.horizontal(true),
                Stroke::Select => {
                    status = pin.select();
                    if status == Selected::Submit {
                        return Ok(Some(pin));
                    }
                }
                Stroke::Cancel => return Ok(None),
            }
            hal.draw_keyboard(&pin.keyboard, pin.length(), status, true)?;
            hide_at = Some(now() + FLASH);
        } else if hide_at.is_some_and(|deadline| t >= deadline) {
            hal.draw_keyboard(&pin.keyboard, pin.length(), status, false)?;
            hide_at = None;
        }
        poll();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    struct Panel<'a> {
        clock: &'a Cell<u64>,
        contacts: Vec<(u64, u8)>,
        frames: Vec<(u64, bool)>,
    }
    impl PinContacts for Panel<'_> {
        fn pin_contacts(&mut self) -> Result<u8, HalError> {
            assert!(self.clock.get() < 2500, "script exhausted");
            Ok(self
                .contacts
                .iter()
                .rev()
                .find(|(t, _)| *t <= self.clock.get())
                .map_or(0, |(_, raw)| *raw))
        }
    }
    impl PinDisplay for Panel<'_> {
        fn draw_keyboard(
            &mut self,
            _: &Keyboard,
            _: usize,
            _: Selected,
            highlight: bool,
        ) -> Result<(), HalError> {
            self.frames.push((self.clock.get(), highlight));
            Ok(())
        }
    }
    #[test]
    fn idle_clears_at_100ms_and_a_new_movement_restarts_the_flash() {
        let clock = Cell::new(0);
        let mut p = Panel {
            clock: &clock,
            contacts: vec![(150, 16), (180, 0), (210, 8), (240, 0), (500, 2)],
            frames: vec![],
        };
        assert!(enter_with_clock(
            &mut p,
            || Duration::from_millis(clock.get()),
            || clock.set(clock.get() + 10)
        )
        .unwrap()
        .is_none());
        assert_eq!(
            p.frames,
            [
                (0, true),
                (100, false),
                (170, true),
                (230, true),
                (330, false)
            ]
        );
    }
    #[test]
    fn four_directions_type_and_send_only_on_selection() {
        let clock = Cell::new(0);
        let mut contacts = vec![];
        for (i, raw) in [1, 16, 1, 8, 1, 4, 4, 1, 8, 8, 8, 8, 8, 16, 16, 1]
            .into_iter()
            .enumerate()
        {
            let t = 50 + i as u64 * 100;
            contacts.extend([(t, raw), (t + 40, 0)]);
        }
        let mut p = Panel {
            clock: &clock,
            contacts,
            frames: vec![],
        };
        assert_eq!(
            enter_with_clock(
                &mut p,
                || Duration::from_millis(clock.get()),
                || clock.set(clock.get() + 10)
            )
            .unwrap()
            .unwrap()
            .pin(),
            "qws2"
        );
    }
}
