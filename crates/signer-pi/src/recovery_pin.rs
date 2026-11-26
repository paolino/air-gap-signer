//! Physical arrows move a visible cursor. The renderer never receives PIN bytes.
use panel_input::token_pin::{Keyboard, Picker, Selected};
use signer_core::review::ButtonEvent;
use signer_hal::{Buttons, HalError};

pub trait PinDisplay {
    fn draw_keyboard(
        &mut self,
        keyboard: &Keyboard,
        count: usize,
        status: Selected,
    ) -> Result<(), HalError>;
}

pub fn enter(hal: &mut (impl Buttons + PinDisplay)) -> Result<Option<Picker>, HalError> {
    let mut pin = Picker::default();
    let mut status = Selected::Editing;
    loop {
        hal.draw_keyboard(&pin.keyboard, pin.length(), status)?;
        status = Selected::Editing;
        use ButtonEvent::*;
        match hal.wait_event()? {
            Up | UpHeld => pin.keyboard.vertical(false),
            Down | DownHeld => pin.keyboard.vertical(true),
            Back => pin.keyboard.horizontal(false),
            Open => pin.keyboard.horizontal(true),
            Confirm => {
                status = pin.select();
                if status == Selected::Submit {
                    return Ok(Some(pin));
                }
            }
            Mode | Reject => return Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct Panel(VecDeque<ButtonEvent>);
    impl Buttons for Panel {
        fn wait_event(&mut self) -> Result<ButtonEvent, HalError> {
            self.0
                .pop_front()
                .ok_or_else(|| HalError::Button("script exhausted".into()))
        }
    }
    impl PinDisplay for Panel {
        fn draw_keyboard(&mut self, _: &Keyboard, _: usize, _: Selected) -> Result<(), HalError> {
            Ok(())
        }
    }
    #[test]
    fn four_directions_type_on_the_grid_then_send() {
        use ButtonEvent::*;
        // q, w, s, 2; down to the actions, then move to Send.
        let mut panel = Panel(
            [
                Confirm, Open, Confirm, Down, Confirm, Up, Up, Confirm, Down, Down, Down, Down,
                Down, Open, Open, Confirm,
            ]
            .into(),
        );
        assert_eq!(enter(&mut panel).unwrap().unwrap().pin(), "qws2");
    }
    #[test]
    fn cancel_discards_the_entry() {
        assert!(
            enter(&mut Panel([ButtonEvent::Confirm, ButtonEvent::Mode].into()))
                .unwrap()
                .is_none()
        );
    }
}
