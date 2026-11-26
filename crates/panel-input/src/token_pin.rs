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
