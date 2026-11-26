//! A two-dimensional recovery-token keyboard. Its public layout contains
//! only the selected key and the active layer, never the entered PIN.
use zeroize::{Zeroize, Zeroizing};

const LOWER: [&str; 4] = ["1234567890", "qwertyuiop", "asdfghjkl", "zxcvbnm"];
const UPPER: [&str; 4] = ["1234567890", "QWERTYUIOP", "ASDFGHJKL", "ZXCVBNM"];
const SYMBOLS: [&str; 4] = ["1234567890", "!@#$%^&*()", "-_=+[]{}\\|", ";:'\",.<>/?`~"];
const ACCENTS: [&str; 4] = ["1234567890", "áàâãäéèêë", "íìîïóòôõö", "úùûüçñ"];
const UPPER_ACCENTS: [&str; 4] = ["1234567890", "ÁÀÂÃÄÉÈÊË", "ÍÌÎÏÓÒÔÕÖ", "ÚÙÛÜÇÑ"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Character(char),
    Shift,
    Symbols,
    Accents,
    Space,
    Delete,
    Send,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Layer {
    #[default]
    Letters,
    Symbols,
    Accents,
}

pub const ROWS: usize = 6;

pub struct Keyboard {
    pub row: usize,
    pub column: usize,
    layer: Layer,
    upper: bool,
    // Horizontal position in units of 1/240 of the keyboard width. Preserve
    // it on vertical moves so a trip through a shorter row is reversible.
    x: usize,
}

impl Default for Keyboard {
    fn default() -> Self {
        Self {
            row: 1,
            column: 0,
            layer: Layer::Letters,
            upper: false,
            x: 12,
        }
    }
}

impl Keyboard {
    fn chars(&self) -> &[&'static str; 4] {
        match (self.layer, self.upper) {
            (Layer::Symbols, _) => &SYMBOLS,
            (Layer::Accents, false) => &ACCENTS,
            (Layer::Accents, true) => &UPPER_ACCENTS,
            (Layer::Letters, true) => &UPPER,
            (Layer::Letters, false) => &LOWER,
        }
    }
    pub fn width(&self, row: usize) -> usize {
        if row < 4 {
            self.chars()[row].chars().count()
        } else {
            3
        }
    }
    pub fn key(&self, row: usize, col: usize) -> Key {
        match row {
            4 => [Key::Shift, Key::Symbols, Key::Accents][col],
            5 => [Key::Space, Key::Delete, Key::Send][col],
            _ => Key::Character(self.chars()[row].chars().nth(col).unwrap()),
        }
    }
    pub fn action_label(&self, key: Key) -> &'static str {
        match key {
            Key::Shift if self.upper => "abc",
            Key::Shift => "ABC",
            Key::Symbols if self.layer == Layer::Symbols => "Letters",
            Key::Symbols => "#+=",
            Key::Accents if self.layer == Layer::Accents => "Letters",
            Key::Accents => "Accents",
            Key::Space => "Space",
            Key::Delete => "Del",
            Key::Send => "Send",
            Key::Character(_) => "",
        }
    }
    pub fn horizontal(&mut self, right: bool) {
        let n = self.width(self.row);
        self.column = (self.column + if right { 1 } else { n - 1 }) % n;
        self.x = (2 * self.column + 1) * 240 / (2 * n);
    }
    pub fn vertical(&mut self, down: bool) {
        self.row = if down {
            (self.row + 1).min(ROWS - 1)
        } else {
            self.row.saturating_sub(1)
        };
        self.column = self.x * self.width(self.row) / 240;
    }
}

pub struct Picker {
    pub keyboard: Keyboard,
    bytes: Zeroizing<[u8; 63]>,
    used: usize,
}

impl Default for Picker {
    fn default() -> Self {
        Self {
            keyboard: Keyboard::default(),
            bytes: Zeroizing::new([0; 63]),
            used: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Selected {
    Editing,
    TooLong,
    TooShort,
    Submit,
}

impl Picker {
    pub fn length(&self) -> usize {
        self.pin().chars().count()
    }
    pub fn pin(&self) -> &str {
        core::str::from_utf8(&self.bytes[..self.used]).unwrap()
    }
    pub fn select(&mut self) -> Selected {
        let board = &mut self.keyboard;
        match board.key(board.row, board.column) {
            Key::Shift => {
                board.upper = !board.upper;
                if board.layer == Layer::Symbols {
                    board.layer = Layer::Letters;
                }
            }
            Key::Symbols => {
                board.layer = if board.layer == Layer::Symbols {
                    Layer::Letters
                } else {
                    Layer::Symbols
                }
            }
            Key::Accents => {
                board.layer = if board.layer == Layer::Accents {
                    Layer::Letters
                } else {
                    Layer::Accents
                }
            }
            Key::Delete => {
                if let Some(c) = self.pin().chars().next_back() {
                    let end = self.used;
                    self.used -= c.len_utf8();
                    self.bytes[self.used..end].zeroize();
                }
            }
            Key::Send => {
                return if self.length() >= 4 {
                    Selected::Submit
                } else {
                    Selected::TooShort
                }
            }
            key => {
                let c = match key {
                    Key::Character(c) => c,
                    Key::Space => ' ',
                    _ => unreachable!(),
                };
                if self.used + c.len_utf8() > self.bytes.len() {
                    return Selected::TooLong;
                }
                self.used += c.encode_utf8(&mut self.bytes[self.used..]).len();
            }
        }
        Selected::Editing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn arrows_follow_visible_rows_and_columns_and_the_layout_covers_printable_ascii() {
        let mut board = Keyboard::default();
        board.horizontal(true);
        assert_eq!(board.key(board.row, board.column), Key::Character('w'));
        board.vertical(true);
        assert_eq!(board.key(board.row, board.column), Key::Character('s'));
        board.vertical(false);
        assert_eq!(board.key(board.row, board.column), Key::Character('w'));
        for c in '!'..='~' {
            assert!([LOWER, UPPER, SYMBOLS]
                .iter()
                .flatten()
                .any(|row| row.contains(c)));
        }
    }
    #[test]
    fn selected_character_stays_put_and_limit_and_delete_respect_utf8() {
        let mut p = Picker::default();
        p.keyboard.layer = Layer::Accents;
        for _ in 0..31 {
            assert_eq!(p.select(), Selected::Editing);
        }
        assert_eq!(p.length(), 31);
        assert_eq!(p.select(), Selected::TooLong);
        p.keyboard.row = 5;
        p.keyboard.column = 1;
        p.select();
        assert_eq!(p.length(), 30);
        assert_eq!(&p.bytes[60..62], &[0, 0]);
    }
}

/// Sampled recovery-keyboard contacts, in bonnet line order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    Select,
    Cancel,
    Up,
    Down,
    Right,
    Left,
}

/// Debounce the six contacts. A held direction repeats; selection and
/// cancellation require a new press after a stable neutral sample.
#[derive(Default)]
pub struct KeyRepeat {
    raw: Option<u8>,
    stable: Option<u8>,
    changed: core::time::Duration,
    ready: bool,
    repeat_at: Option<core::time::Duration>,
}

impl KeyRepeat {
    pub fn sample(&mut self, raw: u8, now: core::time::Duration) -> Option<Stroke> {
        use core::time::Duration;
        const DEBOUNCE: Duration = Duration::from_millis(20);
        const DELAY: Duration = Duration::from_millis(400);
        const PERIOD: Duration = Duration::from_millis(120);
        if self.raw != Some(raw) {
            self.raw = Some(raw);
            self.changed = now;
            self.repeat_at = None;
            return None;
        }
        if now.saturating_sub(self.changed) < DEBOUNCE {
            return None;
        }
        let key = match raw {
            1 => Some(Stroke::Select),
            2 => Some(Stroke::Cancel),
            4 => Some(Stroke::Up),
            8 => Some(Stroke::Down),
            16 => Some(Stroke::Right),
            32 => Some(Stroke::Left),
            _ => None,
        };
        if self.stable != Some(raw) {
            self.stable = Some(raw);
            if raw == 0 {
                self.ready = true;
                return None;
            }
            if !core::mem::take(&mut self.ready) {
                return None;
            }
            if matches!(
                key,
                Some(Stroke::Up | Stroke::Down | Stroke::Right | Stroke::Left)
            ) {
                self.repeat_at = Some(now + DELAY);
            }
            return key;
        }
        if self.repeat_at.is_some_and(|at| now >= at) {
            self.repeat_at = Some(now + PERIOD);
            return key;
        }
        None
    }
}

#[cfg(test)]
mod repeat_tests {
    use super::*;
    use core::time::Duration;
    #[test]
    fn only_the_four_directions_repeat_and_release_stops_them() {
        for (mask, stroke, moves) in [
            (1, Stroke::Select, false),
            (2, Stroke::Cancel, false),
            (4, Stroke::Up, true),
            (8, Stroke::Down, true),
            (16, Stroke::Right, true),
            (32, Stroke::Left, true),
        ] {
            let mut r = KeyRepeat::default();
            let mut sample = |raw, t| r.sample(raw, Duration::from_millis(t));
            assert_eq!(sample(0, 0), None);
            assert_eq!(sample(0, 20), None);
            assert_eq!(sample(mask, 30), None);
            assert_eq!(sample(mask, 50), Some(stroke));
            assert_eq!(sample(mask, 449), None);
            assert_eq!(sample(mask, 450), moves.then_some(stroke));
            assert_eq!(sample(mask, 569), None);
            assert_eq!(sample(mask, 570), moves.then_some(stroke));
            assert_eq!(sample(0, 571), None);
            assert_eq!(sample(0, 1000), None);
        }
    }
    #[test]
    fn held_on_entry_and_chords_do_not_type() {
        let mut r = KeyRepeat::default();
        for (t, mask) in [
            (0, 1),
            (100, 1),
            (200, 0),
            (220, 0),
            (300, 17),
            (340, 17),
            (400, 1),
            (450, 1),
        ] {
            assert_eq!(r.sample(mask, Duration::from_millis(t)), None);
        }
    }
}
