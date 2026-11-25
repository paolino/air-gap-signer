//! Turns display lines into screen rows of a fixed width.
//!
//! Values are wrapped, never truncated: what the signer asks the user
//! to approve must be visible in full.

use signer_core::display::DisplayLine;

/// Render each line as `indent key: value` and wrap it into rows of at
/// most `cols` characters. Continuation rows keep the line's indent.
pub fn wrap_lines(lines: &[DisplayLine], cols: usize) -> Vec<String> {
    let mut rows = Vec::new();
    for line in lines {
        // Cap the indent so deep nesting still leaves room for text.
        let pad = "  "
            .repeat(line.indent)
            .chars()
            .take(cols / 2)
            .collect::<String>();
        let width = cols - pad.chars().count();
        let text = match &line.key {
            Some(k) if line.value.is_empty() => format!("{k}:"),
            Some(k) => format!("{k}: {}", line.value),
            None => line.value.clone(),
        };
        for chunk in chunks(&text, width) {
            rows.push(format!("{pad}{chunk}"));
        }
    }
    rows
}

/// Wrap a message into rows of at most `cols` characters, breaking at
/// spaces where possible.
pub fn wrap_message(text: &str, cols: usize) -> Vec<String> {
    let mut rows: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        for piece in chunks(word, cols) {
            let needed = current.chars().count() + piece.chars().count() + 1;
            if current.is_empty() {
                current = piece;
            } else if needed <= cols {
                current.push(' ');
                current.push_str(&piece);
            } else {
                rows.push(std::mem::replace(&mut current, piece));
            }
        }
    }
    if !current.is_empty() {
        rows.push(current);
    }
    rows
}

/// Split `text` into pieces of at most `width` characters (at least one
/// piece, even for empty text).
fn chunks(text: &str, width: usize) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return vec![String::new()];
    }
    chars
        .chunks(width.max(1))
        .map(|c| c.iter().collect())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(indent: usize, key: Option<&str>, value: &str) -> DisplayLine {
        DisplayLine {
            indent,
            key: key.map(Into::into),
            value: value.into(),
        }
    }

    #[test]
    fn short_line_is_one_row() {
        let rows = wrap_lines(&[line(0, Some("fee"), "170000")], 15);
        assert_eq!(rows, vec!["fee: 170000"]);
    }

    #[test]
    fn long_value_wraps_without_losing_characters() {
        let addr = "addr1qxy2kgdygjrsqtzq2n0yrf2493p83kkfjhx0wlh";
        let rows = wrap_lines(&[line(0, Some("to"), addr)], 15);
        assert!(rows.iter().all(|r| r.chars().count() <= 15));
        let joined: String = rows.concat().chars().filter(|c| *c != ' ').collect();
        assert_eq!(joined, format!("to:{addr}"));
    }

    #[test]
    fn continuation_rows_keep_indent() {
        let rows = wrap_lines(&[line(1, Some("k"), "abcdefghijklmnop")], 10);
        assert!(rows.len() > 1);
        assert!(rows.iter().all(|r| r.starts_with("  ")));
    }

    #[test]
    fn key_without_value_ends_with_colon() {
        assert_eq!(
            wrap_lines(&[line(0, Some("outputs"), "")], 15),
            vec!["outputs:"]
        );
    }

    #[test]
    fn deep_indent_still_makes_progress() {
        let rows = wrap_lines(&[line(20, None, "xyz")], 10);
        let joined: String = rows.concat().chars().filter(|c| *c != ' ').collect();
        assert_eq!(joined, "xyz");
    }

    #[test]
    fn message_breaks_at_spaces() {
        assert_eq!(
            wrap_message("Insert USB stick", 10),
            vec!["Insert USB", "stick"]
        );
    }

    #[test]
    fn message_splits_overlong_words() {
        assert_eq!(wrap_message("abcdefghij", 4), vec!["abcd", "efgh", "ij"]);
    }
}
