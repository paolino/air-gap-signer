use super::*;
use std::collections::VecDeque;

type Transition = (u64, u32, bool);

/// The first time is when the contact moved; delivery may lag behind.
/// Level reads always reflect the physical timeline, even during a wait.
struct Rig {
    time: Duration,
    physical: Vec<Transition>,
    queue: VecDeque<(Duration, Edge)>,
}

fn ms(time: u64) -> Duration {
    Duration::from_millis(time)
}

impl Rig {
    fn new(events: &[Transition]) -> Self {
        let queue = events
            .iter()
            .map(|&(t, line, down)| {
                (
                    ms(t),
                    Edge {
                        at: ms(t),
                        line,
                        down,
                    },
                )
            })
            .collect();
        Self {
            time: Duration::ZERO,
            physical: events.to_vec(),
            queue,
        }
    }

    fn press(&mut self, held: &mut Option<Held>) -> ButtonEvent {
        wait_with_repeat(self, held, |rig, d| rig.time += d).unwrap()
    }
}

impl EdgeSource for Rig {
    fn now(&self) -> Result<Duration, HalError> {
        Ok(self.time)
    }

    fn edge_within(&mut self, wait: Duration) -> Result<Option<Edge>, HalError> {
        assert!(self.time < ms(5000), "timed out waiting for a fresh press");
        let until = self.time + wait;
        if self
            .queue
            .front()
            .is_some_and(|(delivery, _)| *delivery <= until)
        {
            let (delivery, edge) = self.queue.pop_front().unwrap();
            self.time = self.time.max(delivery);
            return Ok(Some(edge));
        }
        self.time = until;
        Ok(None)
    }

    fn pressed(&self, line: u32) -> Result<bool, HalError> {
        Ok(self
            .physical
            .iter()
            .rev()
            .find(|&&(t, l, _)| ms(t) <= self.time && l == line)
            .is_some_and(|&(_, _, down)| down))
    }

    fn released(&self) -> Result<bool, HalError> {
        Ok(LINES.iter().all(|&l| !self.pressed(l).unwrap()))
    }
}

#[test]
fn only_the_six_review_controls_are_mapped() {
    use ButtonEvent::*;
    assert_eq!(
        LINES.map(event_for),
        [Confirm, Mode, Up, Down, Open, Back].map(Some)
    );
    assert_eq!(event_for(4), None);
    assert_eq!(event_for(9), None);
}

#[test]
fn a_press_started_before_arming_and_delivered_late_cannot_confirm() {
    // Confirm starts and ends before arming at 120 ms. A short bounce after
    // arming would look like a new Confirm without the original timestamp.
    let mut rig = Rig::new(&[
        (114, 5, true),
        (118, 5, false),
        (125, 5, true),
        (160, 5, false),
        (220, 6, true),
    ]);
    rig.queue[0].0 = ms(123);
    rig.queue[1].0 = ms(124);
    assert_eq!(rig.press(&mut None), ButtonEvent::Mode);
}

#[test]
fn queued_presses_and_controls_held_across_the_screen_are_discarded() {
    for start in [0, 50, 115, 120] {
        let mut rig = Rig::new(&[(start, 5, true), (180, 5, false), (220, 23, true)]);
        assert_eq!(rig.press(&mut None), ButtonEvent::Open);
    }
}

#[test]
fn a_fresh_bouncing_press_is_accepted_once_after_settling() {
    let mut rig = Rig::new(&[
        (140, 5, true),
        (144, 5, false),
        (150, 5, true),
        (198, 5, false),
        (350, 6, true),
    ]);
    let mut held = None;
    assert_eq!(rig.press(&mut held), ButtonEvent::Confirm);
    assert_eq!(rig.time, ms(170));
    assert_eq!(rig.press(&mut held), ButtonEvent::Mode);
}

#[test]
fn brief_pulses_and_overlapping_presses_do_not_confirm() {
    let mut rig = Rig::new(&[
        (140, 5, true),
        (144, 5, false),
        (180, 5, true),
        (185, 23, true),
        (215, 5, false),
        (220, 23, false),
        (250, 6, true),
    ]);
    assert_eq!(rig.press(&mut None), ButtonEvent::Mode);
}

#[test]
fn only_held_up_and_down_repeat_and_stop_on_release() {
    use ButtonEvent::*;
    for (line, tap, repeat) in [(22, Down, DownHeld), (17, Up, UpHeld)] {
        let mut rig = Rig::new(&[(140, line, true), (705, line, false), (900, 6, true)]);
        let mut held = None;
        assert_eq!(rig.press(&mut held), tap);
        let pressed = rig.time;
        assert_eq!(rig.press(&mut held), repeat);
        assert_eq!(rig.time - pressed, REPEAT_DELAY);
        assert_eq!(rig.press(&mut held), repeat);
        assert_eq!(rig.time - pressed, REPEAT_DELAY + REPEAT_EVERY);
        assert_eq!(rig.press(&mut held), Mode);
    }
    for (line, expected) in [(5, Confirm), (6, Mode), (23, Open), (27, Back)] {
        let mut rig = Rig::new(&[(140, line, true), (800, line, false), (850, 17, true)]);
        let mut held = None;
        assert_eq!(rig.press(&mut held), expected);
        assert_eq!(rig.press(&mut held), Up);
    }
}

#[test]
fn a_quick_direction_tap_does_not_wait_through_the_repeat_delay() {
    let mut rig = Rig::new(&[(140, 22, true), (170, 22, false), (320, 6, true)]);
    let mut held = None;
    assert_eq!(rig.press(&mut held), ButtonEvent::Down);
    assert_eq!(rig.press(&mut held), ButtonEvent::Mode);
    assert_eq!(rig.time, ms(340));
}
