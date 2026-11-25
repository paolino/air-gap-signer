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
    /// Whether the control on `line` is down right now.
    fn pressed(&mut self, line: u32) -> Result<bool, HalError>;
}

/// Holding up or down repeats: after `REPEAT_DELAY`, then every
/// `REPEAT_EVERY`. Nothing else repeats, so a held button can never
/// confirm, reject, open or go back by itself.
pub const REPEAT_DELAY: Duration = Duration::from_millis(400);
pub const REPEAT_EVERY: Duration = Duration::from_millis(120);

/// The press just returned, and whether it was the first of a hold.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Held {
    pub event: ButtonEvent,
    pub first: bool,
}

/// The next press: a repeat of a held up/down, or else a fresh press
/// (`wait_fresh`).
pub fn wait_with_repeat<S: EdgeSource>(
    source: &mut S,
    held: &mut Option<Held>,
    mut sleep: impl FnMut(&mut S, Duration),
) -> Result<ButtonEvent, HalError> {
    if let Some(Held { event, first }) = *held {
        let line = match event {
            ButtonEvent::Up => Some(17),
            ButtonEvent::Down => Some(22),
            _ => None,
        };
        if let Some(line) = line {
            // Look every POLL, so a released control is noticed at once.
            let wait = if first { REPEAT_DELAY } else { REPEAT_EVERY };
            let mut waited = Duration::ZERO;
            let mut still = source.pressed(line)?;
            while still && waited < wait {
                sleep(source, POLL);
                waited += POLL;
                still = source.pressed(line)?;
            }
            if still {
                *held = Some(Held {
                    event,
                    first: false,
                });
                return Ok(event);
            }
        }
    }
    let event = wait_fresh(source, sleep)?;
    *held = Some(Held { event, first: true });
    Ok(event)
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
pub use gpio::{Abort, GpioButtons};

#[cfg(target_os = "linux")]
mod gpio {
    use super::{wait_with_repeat, EdgeSource, Held, DEBOUNCE, LINES};

    /// How often a wait checks its abort condition.
    const ABORT_POLL: std::time::Duration = std::time::Duration::from_millis(200);
    use gpiocdev::line::{Bias, EdgeDetection, Value, Values};
    use gpiocdev::Request;
    use signer_hal::{ButtonEvent, HalError};

    /// The mapped controls, requested as pulled-up inputs that report
    /// debounced falling edges.
    /// Checked while waiting for a press; an error ends the wait.
    pub type Abort = Box<dyn FnMut() -> Option<HalError>>;

    pub struct GpioButtons {
        request: Request,
        held: Option<Held>,
        abort: Option<Abort>,
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
            Ok(Self {
                request,
                held: None,
                abort: None,
            })
        }

        /// While set, `abort` is checked about five times a second during
        /// a wait, and its error ends the wait: e.g. the stick was pulled.
        pub fn set_abort(&mut self, abort: Option<Abort>) {
            self.abort = abort;
        }
    }

    impl EdgeSource for GpioButtons {
        fn pending(&mut self) -> Result<bool, HalError> {
            self.request
                .has_edge_event()
                .map_err(|e| HalError::Button(e.to_string()))
        }

        fn next(&mut self) -> Result<u32, HalError> {
            let err = |e: gpiocdev::Error| HalError::Button(e.to_string());
            loop {
                if let Some(error) = self.abort.as_mut().and_then(|check| check()) {
                    return Err(error);
                }
                if self.request.wait_edge_event(ABORT_POLL).map_err(err)? {
                    return self
                        .request
                        .read_edge_event()
                        .map(|e| e.offset)
                        .map_err(err);
                }
            }
        }

        fn pressed(&mut self, line: u32) -> Result<bool, HalError> {
            let mut values = Values::from_offsets(&[line]);
            self.request
                .values(&mut values)
                .map_err(|e| HalError::Button(e.to_string()))?;
            Ok(values.get(line) == Some(Value::Inactive))
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
            let mut held = self.held.take();
            let event = wait_with_repeat(self, &mut held, |_, d| std::thread::sleep(d));
            self.held = held;
            event
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
        /// The line held down, and for how many more `pressed` checks.
        holding: Option<(u32, usize)>,
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

        fn pressed(&mut self, line: u32) -> Result<bool, HalError> {
            match &mut self.holding {
                Some((l, n)) if *n > 0 => {
                    *n -= 1;
                    Ok(*l == line)
                }
                _ => Ok(false),
            }
        }
    }

    fn fake(queued: &[u32], arriving: &[u32], held_for: usize) -> Fake {
        Fake {
            holding: None,
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

    /// Checks needed to hold through the first delay and `n` repeats.
    fn checks_for(repeats: usize) -> usize {
        let per = |d: Duration| (d.as_millis() / POLL.as_millis()) as usize + 1;
        per(REPEAT_DELAY) + repeats.saturating_sub(1) * per(REPEAT_EVERY)
    }

    #[test]
    fn holding_down_repeats_after_a_delay_then_faster() {
        let mut src = fake(&[], &[22, 6], 0);
        src.holding = Some((22, checks_for(2)));
        let mut held = None;
        let mut events = Vec::new();
        let mut waited = Vec::new();
        for _ in 0..4 {
            let mut total = Duration::ZERO;
            events.push(wait_with_repeat(&mut src, &mut held, |_, d| total += d).unwrap());
            waited.push(total);
        }
        use ButtonEvent::{Down, Reject};
        assert_eq!(
            events,
            vec![Down, Down, Down, Reject],
            "two repeats, then a fresh press"
        );
        assert!(
            waited[1] >= REPEAT_DELAY,
            "first repeat waits the delay: {waited:?}"
        );
        assert!(
            waited[2] < REPEAT_DELAY,
            "later repeats are faster: {waited:?}"
        );
    }

    #[test]
    fn a_quick_tap_does_not_hold_up_the_next_press() {
        // Down is released right away; the wait for a repeat must notice
        // at once instead of sleeping through the whole delay.
        let mut src = fake(&[], &[22, 6], 0);
        src.holding = Some((22, 1));
        let mut held = None;
        assert_eq!(
            wait_with_repeat(&mut src, &mut held, |_, _| {}).ok(),
            Some(ButtonEvent::Down)
        );
        let mut total = Duration::ZERO;
        let next = wait_with_repeat(&mut src, &mut held, |_, d| total += d).ok();
        assert_eq!(next, Some(ButtonEvent::Reject));
        assert!(total < REPEAT_DELAY, "released early: {total:?}");
    }

    #[test]
    fn a_held_confirm_never_repeats() {
        let mut src = fake(&[], &[5, 6], 0);
        src.holding = Some((5, 1000));
        let mut held = None;
        assert_eq!(
            wait_with_repeat(&mut src, &mut held, |_, _| {}).ok(),
            Some(ButtonEvent::Confirm)
        );
        assert_eq!(
            wait_with_repeat(&mut src, &mut held, |_, _| {}).ok(),
            Some(ButtonEvent::Reject),
            "the next answer is a fresh press, not a repeated #5"
        );
    }

    #[test]
    fn open_and_back_never_repeat() {
        for line in [23, 27] {
            let mut src = fake(&[], &[line, 17], 0);
            src.holding = Some((line, 1000));
            let mut held = None;
            wait_with_repeat(&mut src, &mut held, |_, _| {}).unwrap();
            assert_eq!(
                wait_with_repeat(&mut src, &mut held, |_, _| {}).ok(),
                Some(ButtonEvent::Up)
            );
        }
    }
}
