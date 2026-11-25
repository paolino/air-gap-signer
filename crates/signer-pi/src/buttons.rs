//! `Buttons` on the TFT bonnet's GPIO lines.

use signer_hal::{ButtonEvent, HalError};
use std::time::Duration;

/// fbtft refreshes the panel at most every 1/fps (40 ms at the overlay's
/// 25 fps) and a full 240×240 transfer takes about as long again; 100 ms
/// covers both.
pub const SETTLE: Duration = Duration::from_millis(100);
/// The kernel's software debounce period for the lines.
pub const DEBOUNCE: Duration = Duration::from_millis(20);
/// How often to look again while a control is still held.
pub const POLL: Duration = Duration::from_millis(10);

/// The GPIO line of each mapped control. The joystick press (4) is
/// unmapped.
pub const LINES: [u32; 6] = [5, 6, 17, 22, 23, 27];

/// The event a falling edge on `line` produces, if any.
pub fn event_for(line: u32) -> Option<ButtonEvent> {
    match line {
        5 => Some(ButtonEvent::Confirm),
        6 => Some(ButtonEvent::Reject),
        17 => Some(ButtonEvent::Up),
        22 => Some(ButtonEvent::Down),
        23 => Some(ButtonEvent::Open),
        27 => Some(ButtonEvent::Back),
        _ => None,
    }
}

/// A queue of button edges, as the kernel buffers them.
pub trait EdgeSource {
    /// Whether an edge is queued, without blocking.
    fn pending(&mut self) -> Result<bool, HalError>;
    /// The line of the next edge, blocking until there is one.
    fn next(&mut self) -> Result<u32, HalError>;
    /// Whether every control is up right now.
    fn released(&mut self) -> Result<bool, HalError>;
}

/// Wait for a press that starts after this call.
///
/// First wait `SETTLE` for the panel to show what the caller just drew,
/// then until every control is up, then one more debounce period so the
/// kernel has reported any edge it was still filtering. Everything queued
/// by then is discarded: a press made before the screen changed, or held
/// across the change, can never answer it.
pub fn wait_fresh<S: EdgeSource>(
    source: &mut S,
    mut sleep: impl FnMut(&mut S, Duration),
) -> Result<ButtonEvent, HalError> {
    sleep(source, SETTLE);
    while !source.released()? {
        sleep(source, POLL);
    }
    sleep(source, DEBOUNCE + POLL);
    while source.pending()? {
        source.next()?;
    }
    loop {
        if let Some(event) = event_for(source.next()?) {
            return Ok(event);
        }
    }
}

#[cfg(target_os = "linux")]
pub use gpio::GpioButtons;

#[cfg(target_os = "linux")]
mod gpio {
    use super::{wait_fresh, EdgeSource, DEBOUNCE, LINES};
    use gpiocdev::line::{Bias, EdgeDetection, Value, Values};
    use gpiocdev::Request;
    use signer_hal::{ButtonEvent, HalError};

    /// The mapped controls, requested as pulled-up inputs that report
    /// debounced falling edges.
    pub struct GpioButtons {
        request: Request,
    }

    impl GpioButtons {
        pub fn open(chip: &str) -> Result<Self, HalError> {
            let request = Request::builder()
                .on_chip(chip)
                .with_consumer("signer-pi")
                .with_lines(&LINES)
                .as_input()
                .with_bias(Bias::PullUp)
                .with_edge_detection(EdgeDetection::FallingEdge)
                .with_debounce_period(DEBOUNCE)
                .request()
                .map_err(|e| HalError::Button(format!("{chip}: {e}")))?;
            Ok(Self { request })
        }
    }

    impl EdgeSource for GpioButtons {
        fn pending(&mut self) -> Result<bool, HalError> {
            self.request
                .has_edge_event()
                .map_err(|e| HalError::Button(e.to_string()))
        }

        fn next(&mut self) -> Result<u32, HalError> {
            self.request
                .read_edge_event()
                .map(|e| e.offset)
                .map_err(|e| HalError::Button(e.to_string()))
        }

        /// The lines are pulled up, so an untouched control reads high.
        fn released(&mut self) -> Result<bool, HalError> {
            let mut values = Values::from_offsets(&LINES);
            self.request
                .values(&mut values)
                .map_err(|e| HalError::Button(e.to_string()))?;
            Ok(LINES.iter().all(|&l| values.get(l) == Some(Value::Active)))
        }
    }

    impl signer_hal::Buttons for GpioButtons {
        fn wait_event(&mut self) -> Result<ButtonEvent, HalError> {
            wait_fresh(self, |_, d| std::thread::sleep(d))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[test]
    fn maps_buttons_and_joystick() {
        assert_eq!(event_for(23), Some(ButtonEvent::Open));
        assert_eq!(event_for(27), Some(ButtonEvent::Back));
        assert_eq!(event_for(5), Some(ButtonEvent::Confirm));
        assert_eq!(event_for(6), Some(ButtonEvent::Reject));
        assert_eq!(event_for(17), Some(ButtonEvent::Up));
        assert_eq!(event_for(22), Some(ButtonEvent::Down));
    }

    #[test]
    fn joystick_press_does_nothing() {
        assert_eq!(event_for(4), None);
    }

    #[test]
    fn every_requested_line_is_mapped() {
        assert!(LINES.iter().all(|&l| event_for(l).is_some()));
    }

    /// Edges queued before the wait, edges that arrive during it, and how
    /// many more polls a control stays held.
    struct Fake {
        queued: VecDeque<u32>,
        arriving: VecDeque<u32>,
        held_for: usize,
    }

    impl EdgeSource for Fake {
        fn pending(&mut self) -> Result<bool, HalError> {
            Ok(!self.queued.is_empty())
        }

        fn next(&mut self) -> Result<u32, HalError> {
            self.queued
                .pop_front()
                .or_else(|| self.arriving.pop_front())
                .ok_or_else(|| HalError::Button("no more edges".into()))
        }

        fn released(&mut self) -> Result<bool, HalError> {
            Ok(self.held_for == 0)
        }
    }

    fn fake(queued: &[u32], arriving: &[u32], held_for: usize) -> Fake {
        Fake {
            queued: queued.iter().copied().collect(),
            arriving: arriving.iter().copied().collect(),
            held_for,
        }
    }

    fn fresh(src: &mut Fake, sleep: impl FnMut(&mut Fake, Duration)) -> Option<ButtonEvent> {
        wait_fresh(src, sleep).ok()
    }

    #[test]
    fn a_press_queued_before_the_screen_changed_is_discarded() {
        let mut src = fake(&[5, 5], &[6], 0);
        assert_eq!(fresh(&mut src, |_, _| {}), Some(ButtonEvent::Reject));
    }

    #[test]
    fn a_press_during_settle_is_discarded() {
        let mut src = fake(&[], &[23], 0);
        let mut first = true;
        let press_while_settling = |s: &mut Fake, _| {
            if std::mem::take(&mut first) {
                s.queued.push_back(5);
            }
        };
        assert_eq!(
            fresh(&mut src, press_while_settling),
            Some(ButtonEvent::Open)
        );
    }

    #[test]
    fn a_held_control_must_be_released_and_its_late_edge_is_discarded() {
        // #5 is held across the screen change; the debounced edge only
        // arrives while we wait for the release.
        let mut src = fake(&[], &[6], 3);
        let mut polls = Vec::new();
        let hold = |s: &mut Fake, d: Duration| {
            polls.push(d);
            if s.held_for > 0 {
                s.held_for -= 1;
                if s.held_for == 0 {
                    s.queued.push_back(5);
                }
            }
        };
        assert_eq!(fresh(&mut src, hold), Some(ButtonEvent::Reject));
        assert_eq!(polls.first(), Some(&SETTLE));
        assert_eq!(polls.last(), Some(&(DEBOUNCE + POLL)));
    }

    #[test]
    fn unmapped_lines_are_skipped() {
        let mut src = fake(&[], &[4, 9, 17], 0);
        assert_eq!(fresh(&mut src, |_, _| {}), Some(ButtonEvent::Up));
    }
}
