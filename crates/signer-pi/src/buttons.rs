//! `Buttons` on the TFT bonnet's GPIO lines.

use signer_hal::{ButtonEvent, HalError};
use std::time::Duration;

/// fbtft refreshes the panel at most every 1/fps (40 ms at the overlay's
/// 25 fps) and a full 240×240 transfer takes about as long again; 100 ms
/// is the existing settling allowance, without panel acknowledgement.
pub const SETTLE: Duration = Duration::from_millis(100);
/// A switch must settle for this long before a press counts.
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
        6 => Some(ButtonEvent::Mode),
        17 => Some(ButtonEvent::Up),
        22 => Some(ButtonEvent::Down),
        23 => Some(ButtonEvent::Open),
        27 => Some(ButtonEvent::Back),
        _ => None,
    }
}

/// A raw switch transition on the kernel's monotonic clock.
#[derive(Clone, Copy)]
struct Edge {
    line: u32,
    down: bool,
    at: Duration,
}

trait EdgeSource {
    fn now(&self) -> Result<Duration, HalError>;
    fn edge_within(&mut self, timeout: Duration) -> Result<Option<Edge>, HalError>;
    fn released(&self) -> Result<bool, HalError>;
    fn pressed(&self, line: u32) -> Result<bool, HalError>;
}

pub const REPEAT_DELAY: Duration = Duration::from_millis(400);
pub const REPEAT_EVERY: Duration = Duration::from_millis(120);
const ABORT_POLL: Duration = Duration::from_millis(200);

#[derive(Clone, Copy)]
struct Held {
    line: u32,
    delay: Duration,
}

/// Qualify a neutral interval before opening input. Recheck the levels after
/// taking the timestamp so a control held across the boundary cannot qualify.
fn arm(source: &mut impl EdgeSource) -> Result<Duration, HalError> {
    loop {
        if !source.released()? {
            source.edge_within(ABORT_POLL)?;
            continue;
        }
        if source.edge_within(DEBOUNCE)?.is_none() {
            let at = source.now()?;
            if source.released()? {
                return Ok(at);
            }
        }
    }
}

fn wait_fresh(source: &mut impl EdgeSource) -> Result<(u32, ButtonEvent), HalError> {
    loop {
        let armed = arm(source)?;
        let mut candidate = None;
        loop {
            let timeout = if candidate.is_some() {
                DEBOUNCE
            } else {
                ABORT_POLL
            };
            match source.edge_within(timeout)? {
                // The kernel can deliver a transition after the screen change.
                // Its occurrence time determines which screen owns it.
                Some(edge) if edge.at <= armed => break,
                Some(edge) => {
                    if candidate.is_some_and(|line| line != edge.line) {
                        break; // Two controls overlapped. Wait for neutral.
                    }
                    candidate = edge.down.then_some(edge.line);
                }
                None => {
                    if let Some(line) = candidate {
                        if let Some(event) = event_for(line) {
                            if source.pressed(line)? {
                                return Ok((line, event));
                            }
                        }
                        break;
                    }
                }
            }
        }
    }
}

fn wait_with_repeat<S: EdgeSource>(
    source: &mut S,
    held: &mut Option<Held>,
    settle: impl FnOnce(&mut S, Duration),
) -> Result<ButtonEvent, HalError> {
    if let Some(previous) = held.take() {
        if source.pressed(previous.line)?
            && source.edge_within(previous.delay)?.is_none()
            && source.pressed(previous.line)?
        {
            *held = Some(Held {
                delay: REPEAT_EVERY,
                ..previous
            });
            return Ok(if previous.line == 17 {
                ButtonEvent::UpHeld
            } else {
                ButtonEvent::DownHeld
            });
        }
    }
    settle(source, SETTLE);
    let (line, event) = wait_fresh(source)?;
    if matches!(event, ButtonEvent::Up | ButtonEvent::Down) {
        *held = Some(Held {
            line,
            delay: REPEAT_DELAY,
        });
    }
    Ok(event)
}

#[cfg(target_os = "linux")]
pub use gpio::{Abort, GpioButtons};

#[cfg(target_os = "linux")]
mod gpio {
    use super::*;
    use gpiocdev::line::{Bias, EdgeDetection, EdgeKind, EventClock, Value, Values};
    use gpiocdev::Request;
    use rustix::time::{clock_gettime, ClockId};
    use std::time::Instant;

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
                .with_event_clock(EventClock::Monotonic)
                .with_edge_detection(EdgeDetection::BothEdges)
                .request()
                .map_err(|e| HalError::Button(format!("{chip}: {e}")))?;
            Ok(Self {
                request,
                held: None,
                abort: None,
            })
        }

        /// Snapshot all active-low contacts for the timed recovery keyboard.
        pub fn pin_contacts(&mut self) -> Result<u8, HalError> {
            if let Some(error) = self.abort.as_mut().and_then(|check| check()) {
                return Err(error);
            }
            let mut values = Values::from_offsets(&LINES);
            self.request.values(&mut values).map_err(gpio_error)?;
            Ok(LINES.iter().enumerate().fold(0, |mask, (bit, line)| {
                mask | (u8::from(values.get(*line) == Some(Value::Inactive)) << bit)
            }))
        }

        pub fn set_abort(&mut self, abort: Option<Abort>) {
            self.abort = abort;
        }
    }

    fn gpio_error(e: gpiocdev::Error) -> HalError {
        HalError::Button(e.to_string())
    }

    impl EdgeSource for GpioButtons {
        fn now(&self) -> Result<Duration, HalError> {
            Duration::try_from(clock_gettime(ClockId::Monotonic))
                .map_err(|_| HalError::Button("invalid monotonic clock".into()))
        }

        fn edge_within(&mut self, timeout: Duration) -> Result<Option<Edge>, HalError> {
            let start = Instant::now();
            loop {
                if let Some(error) = self.abort.as_mut().and_then(|check| check()) {
                    return Err(error);
                }
                let remaining = timeout.saturating_sub(start.elapsed());
                if self
                    .request
                    .wait_edge_event(remaining.min(ABORT_POLL))
                    .map_err(gpio_error)?
                {
                    let event = self.request.read_edge_event().map_err(gpio_error)?;
                    return Ok(Some(Edge {
                        line: event.offset,
                        at: Duration::from_nanos(event.timestamp_ns),
                        down: event.kind == EdgeKind::Falling,
                    }));
                }
                if start.elapsed() >= timeout {
                    return Ok(None);
                }
            }
        }

        fn pressed(&self, line: u32) -> Result<bool, HalError> {
            let mut values = Values::from_offsets(&[line]);
            self.request.values(&mut values).map_err(gpio_error)?;
            Ok(values.get(line) == Some(Value::Inactive))
        }

        fn released(&self) -> Result<bool, HalError> {
            let mut values = Values::from_offsets(&LINES);
            self.request.values(&mut values).map_err(gpio_error)?;
            Ok(LINES
                .iter()
                .all(|&line| values.get(line) == Some(Value::Active)))
        }
    }

    impl signer_hal::Buttons for GpioButtons {
        fn wait_event(&mut self) -> Result<ButtonEvent, HalError> {
            let mut held = self.held.take();
            let event =
                wait_with_repeat(self, &mut held, |_, duration| std::thread::sleep(duration));
            self.held = held;
            event
        }
    }
}

#[cfg(test)]
mod tests;
