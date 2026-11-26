#![no_std]
//! Eyes-free PIN entry shared by the Pi and the front-panel controller.
use zeroize::Zeroizing;
pub mod token_pin;

/// Prototype sequence length.
pub const PIN_LENGTH: usize = 9;
const MODE: u8 = 1 << 5;

pub type Sequence = Zeroizing<[u8; PIN_LENGTH]>;

pub enum EntryEvent {
    Pending,
    Restart,
    Complete(Sequence),
}

/// Receives debounced switch levels. Bits 0 through 6 are up, right, down,
/// left, centre, Mode and Confirm. Zero means all controls are released.
#[derive(Default)]
pub struct PinEntry {
    ready: bool,
    contacts: u8,
    sequence: Sequence,
    used: usize,
}

impl PinEntry {
    /// Ignore anything held on entry. Collect all contacts until the next
    /// neutral sample so a hold counts once and a chord counts zero times.
    pub fn sample(&mut self, down: u8) -> EntryEvent {
        if !self.ready {
            self.ready = down == 0;
            return EntryEvent::Pending;
        }
        if down != 0 {
            self.contacts |= down;
            return EntryEvent::Pending;
        }
        match core::mem::take(&mut self.contacts) {
            MODE => {
                *self = Self::default();
                return EntryEvent::Restart;
            }
            symbol @ (1 | 2 | 4 | 8 | 16) => {
                self.sequence[self.used] = symbol;
                self.used += 1;
                if self.used == PIN_LENGTH {
                    let sequence = core::mem::take(&mut self.sequence);
                    *self = Self::default();
                    return EntryEvent::Complete(sequence);
                }
            }
            _ => {}
        }
        EntryEvent::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(entry: &mut PinEntry, samples: &[u8]) {
        for &sample in samples {
            assert!(matches!(entry.sample(sample), EntryEvent::Pending));
        }
    }

    fn complete(entry: &mut PinEntry) {
        let expected = [16, 16, 1, 2, 4, 8, 1, 4, 16];
        for &symbol in &expected[..PIN_LENGTH - 1] {
            pending(entry, &[symbol, symbol, 0, 0]);
        }
        pending(entry, &[expected[PIN_LENGTH - 1]]);
        match entry.sample(0) {
            EntryEvent::Complete(sequence) => assert_eq!(*sequence, expected),
            _ => panic!("expected full PIN on release"),
        }
    }

    #[test]
    fn held_on_entry_chords_and_confirm_do_not_enter_symbols() {
        let mut entry = PinEntry::default();
        pending(&mut entry, &[16, 0, 1, 3, 2, 0, 64, 0, 32 | 4, 0]);
        complete(&mut entry);
    }

    #[test]
    fn restart_and_completion_clear_the_previous_entry() {
        let mut entry = PinEntry::default();
        pending(&mut entry, &[0, 4, 0, 4, 0, 32]);
        assert!(matches!(entry.sample(0), EntryEvent::Restart));
        pending(&mut entry, &[8, 0]);
        complete(&mut entry);
        pending(&mut entry, &[1, 0]);
        complete(&mut entry);
    }
}
