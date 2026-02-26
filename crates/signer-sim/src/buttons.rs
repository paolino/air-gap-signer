use minifb::{Key, Window};
use signer_hal::{ButtonEvent, HalError};
use std::thread;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_millis(16); // ~60 fps

/// Poll a minifb window for key presses and map to ButtonEvents.
///
/// Takes a mutable reference to the window (shared with SimDisplay).
pub fn poll_event(window: &mut Window) -> Result<Option<ButtonEvent>, HalError> {
    window.update();

    if !window.is_open() {
        return Err(HalError::Button("window closed".into()));
    }

    if window.is_key_pressed(Key::Enter, minifb::KeyRepeat::No) {
        return Ok(Some(ButtonEvent::Confirm));
    }
    if window.is_key_pressed(Key::Escape, minifb::KeyRepeat::No) {
        return Ok(Some(ButtonEvent::Reject));
    }
    if window.is_key_pressed(Key::Up, minifb::KeyRepeat::Yes) {
        return Ok(Some(ButtonEvent::Up));
    }
    if window.is_key_pressed(Key::Down, minifb::KeyRepeat::Yes) {
        return Ok(Some(ButtonEvent::Down));
    }

    Ok(None)
}

/// Blocking wait: polls until an event occurs.
pub fn wait_event(window: &mut Window) -> Result<ButtonEvent, HalError> {
    loop {
        if let Some(ev) = poll_event(window)? {
            return Ok(ev);
        }
        thread::sleep(POLL_INTERVAL);
    }
}
