//! A signing session: the review of one transaction plus the modes the
//! user moves between with #6 — Review, Sign, Keys.
//!
//! In Sign mode #5 on a key adds that key's signature at once and #5
//! again removes it: the toggle is the undo, there is no confirmation and
//! no separate reject (a session finished with no signature signs
//! nothing). Signatures stay in memory until Done.

use crate::review::{ButtonEvent, Review, Row, View};

/// Keys able to sign, and signing with them.
pub trait Signer {
    /// A short label per key, e.g. `slot 0 ed25519 3fa2…`.
    fn keys(&self) -> Vec<String>;
    /// Sign `message` with key `index`.
    fn sign(&mut self, index: usize, message: &[u8]) -> Result<Vec<u8>, String>;
}

/// The screen the user is on; #6 moves to the next one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Review,
    Sign,
    Keys,
}

/// How the session ended, if it did.
#[derive(Debug, Clone, PartialEq)]
pub enum Finish {
    Continue,
    /// Done: the signatures added, as `(key index, signature)`; empty
    /// when nothing was signed.
    Done(Vec<(usize, Vec<u8>)>),
}

/// What Keys mode offers; not wired to the secure element yet.
const KEY_ACTIONS: [&str; 5] = [
    "Set PIN",
    "Create key",
    "Restore key",
    "Back up key",
    "Export public key",
];

pub struct Session {
    review: Review,
    mode: Mode,
    message: Vec<u8>,
    keys: Vec<String>,
    signatures: Vec<Option<Vec<u8>>>,
    /// Cursor in Sign mode (keys, then Done) and in Keys mode.
    cursor: usize,
    /// A one-line notice for the user, shown until the next press.
    notice: Option<String>,
}

impl Session {
    pub fn new(review: Review, message: Vec<u8>, signer: &dyn Signer) -> Self {
        let keys = signer.keys();
        Self {
            review,
            mode: Mode::Review,
            message,
            signatures: vec![None; keys.len()],
            keys,
            cursor: 0,
            notice: None,
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn handle(&mut self, event: ButtonEvent, signer: &mut dyn Signer) -> Finish {
        self.notice = None;
        if event == ButtonEvent::Mode {
            self.mode = match self.mode {
                Mode::Review => Mode::Sign,
                Mode::Sign => Mode::Keys,
                Mode::Keys => Mode::Review,
            };
            self.cursor = 0;
            return Finish::Continue;
        }
        match self.mode {
            // The review's own approve / reject outcomes are ignored:
            // signing is a choice made in Sign mode.
            Mode::Review => {
                let _ = self.review.handle(event);
            }
            Mode::Sign => return self.sign_mode(event, signer),
            Mode::Keys => match event {
                ButtonEvent::Up | ButtonEvent::UpHeld => self.step(KEY_ACTIONS.len(), false),
                ButtonEvent::Down | ButtonEvent::DownHeld => self.step(KEY_ACTIONS.len(), true),
                ButtonEvent::Confirm => self.notice = Some("not available yet".into()),
                _ => {}
            },
        }
        Finish::Continue
    }

    fn sign_mode(&mut self, event: ButtonEvent, signer: &mut dyn Signer) -> Finish {
        let entries = self.keys.len() + 1;
        match event {
            ButtonEvent::Up | ButtonEvent::UpHeld => self.step(entries, false),
            ButtonEvent::Down | ButtonEvent::DownHeld => self.step(entries, true),
            ButtonEvent::Confirm if self.cursor == self.keys.len() => {
                let done = self
                    .signatures
                    .iter()
                    .enumerate()
                    .filter_map(|(i, s)| s.clone().map(|s| (i, s)))
                    .collect();
                return Finish::Done(done);
            }
            ButtonEvent::Confirm if self.signatures[self.cursor].is_some() => {
                self.signatures[self.cursor] = None;
            }
            ButtonEvent::Confirm if self.review.remaining() > 0 => {
                self.notice = Some(format!("{} left to review", self.review.remaining()));
            }
            ButtonEvent::Confirm => match signer.sign(self.cursor, &self.message) {
                Ok(sig) => self.signatures[self.cursor] = Some(sig),
                Err(e) => self.notice = Some(e),
            },
            _ => {}
        }
        Finish::Continue
    }

    fn step(&mut self, entries: usize, down: bool) {
        self.cursor = if down {
            (self.cursor + 1) % entries
        } else {
            (self.cursor + entries - 1) % entries
        };
    }

    pub fn view(&self) -> View {
        let row = |i: usize, text: String, pending: bool| Row {
            text,
            selected: i == self.cursor,
            pending,
            waived: false,
        };
        let status = |name: &str| match &self.notice {
            Some(n) => format!("{name} {n}"),
            None => name.to_string(),
        };
        match self.mode {
            Mode::Review => self.review.view(),
            Mode::Sign => {
                let left = match (self.keys.len(), self.review.remaining()) {
                    (0, _) => "no key".to_string(),
                    (_, 0) => "ready".to_string(),
                    (_, n) => format!("{n} left"),
                };
                let mut rows: Vec<Row> = self
                    .keys
                    .iter()
                    .enumerate()
                    .map(|(i, k)| match self.signatures[i] {
                        Some(_) => row(i, format!("SIGNED {k}"), false),
                        None => row(i, k.clone(), false),
                    })
                    .collect();
                rows.push(row(self.keys.len(), "Done".into(), false));
                let header = match &self.notice {
                    Some(n) => format!("SIGN {n}"),
                    None => format!("SIGN {left}"),
                };
                View::Overview { header, rows }
            }
            Mode::Keys => View::Overview {
                header: status("KEYS"),
                rows: KEY_ACTIONS
                    .iter()
                    .enumerate()
                    .map(|(i, a)| row(i, a.to_string(), false))
                    .collect(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::json_to_lines;

    struct Fake {
        signed: Vec<(usize, Vec<u8>)>,
    }

    impl Signer for Fake {
        fn keys(&self) -> Vec<String> {
            vec!["slot 0".into(), "slot 1".into()]
        }
        fn sign(&mut self, index: usize, message: &[u8]) -> Result<Vec<u8>, String> {
            self.signed.push((index, message.to_vec()));
            Ok(vec![index as u8; 4])
        }
    }

    fn fake() -> Fake {
        Fake { signed: Vec::new() }
    }

    fn session(json: serde_json::Value, signer: &Fake) -> Session {
        let review = Review::new(json_to_lines(&json), 21, 10).unwrap();
        Session::new(review, b"body".to_vec(), signer)
    }

    fn rows(s: &Session) -> (String, Vec<String>, usize) {
        match s.view() {
            View::Overview { header, rows } => {
                let sel = rows.iter().position(|r| r.selected).unwrap_or(usize::MAX);
                (header, rows.into_iter().map(|r| r.text).collect(), sel)
            }
            v => panic!("expected a list, got {v:?}"),
        }
    }

    fn to_sign(s: &mut Session, signer: &mut Fake) {
        while s.mode() != Mode::Sign {
            s.handle(ButtonEvent::Mode, signer);
        }
    }

    #[test]
    fn six_cycles_review_sign_keys() {
        let mut f = fake();
        let mut s = session(serde_json::json!({"fee": "1"}), &f);
        assert_eq!(s.mode(), Mode::Review);
        s.handle(ButtonEvent::Mode, &mut f);
        assert_eq!(s.mode(), Mode::Sign);
        assert!(rows(&s).0.starts_with("SIGN"));
        s.handle(ButtonEvent::Mode, &mut f);
        assert_eq!(s.mode(), Mode::Keys);
        assert!(rows(&s).0.starts_with("KEYS"));
        s.handle(ButtonEvent::Mode, &mut f);
        assert_eq!(s.mode(), Mode::Review);
    }

    #[test]
    fn five_on_a_key_adds_and_again_removes_its_signature() {
        let mut f = fake();
        let mut s = session(serde_json::json!({"fee": "1"}), &f);
        to_sign(&mut s, &mut f);
        let (_, r, sel) = rows(&s);
        assert!(r[sel].contains("slot 0"));
        s.handle(ButtonEvent::Confirm, &mut f);
        assert!(rows(&s).1[sel].starts_with("SIGNED"), "{:?}", rows(&s).1);
        assert_eq!(
            f.signed,
            vec![(0, b"body".to_vec())],
            "signed the session's message"
        );
        s.handle(ButtonEvent::Confirm, &mut f);
        assert!(!rows(&s).1[sel].starts_with("SIGNED"));
    }

    #[test]
    fn signing_is_refused_while_something_is_left_to_review() {
        let mut f = fake();
        let long = "x".repeat(100);
        let mut s = session(serde_json::json!({"memo": long}), &f);
        to_sign(&mut s, &mut f);
        s.handle(ButtonEvent::Confirm, &mut f);
        assert!(f.signed.is_empty());
        let (header, r, _) = rows(&s);
        assert!(
            r.iter().any(|t| t.contains("left to review")) || header.contains("left"),
            "{header} {r:?}"
        );
    }

    #[test]
    fn done_returns_only_the_signatures_still_added() {
        let mut f = fake();
        let mut s = session(serde_json::json!({"fee": "1"}), &f);
        to_sign(&mut s, &mut f);
        s.handle(ButtonEvent::Confirm, &mut f); // add slot 0
        s.handle(ButtonEvent::Down, &mut f);
        s.handle(ButtonEvent::Confirm, &mut f); // add slot 1
        s.handle(ButtonEvent::Confirm, &mut f); // remove slot 1
        s.handle(ButtonEvent::Down, &mut f); // Done
        assert_eq!(
            s.handle(ButtonEvent::Confirm, &mut f),
            Finish::Done(vec![(0, vec![0; 4])])
        );
    }

    #[test]
    fn done_with_nothing_signed_signs_nothing() {
        let mut f = fake();
        let mut s = session(serde_json::json!({"fee": "1"}), &f);
        to_sign(&mut s, &mut f);
        s.handle(ButtonEvent::Down, &mut f);
        s.handle(ButtonEvent::Down, &mut f);
        assert_eq!(s.handle(ButtonEvent::Confirm, &mut f), Finish::Done(vec![]));
    }

    #[test]
    fn review_keeps_its_controls_inside_the_session() {
        let mut f = fake();
        let long = "x".repeat(100);
        let mut s = session(serde_json::json!({"memo": long}), &f);
        s.handle(ButtonEvent::Confirm, &mut f); // accept the cut memo unread
        let (header, r, _) = rows(&s);
        assert!(header.ends_with(" 1 waived"), "{header} {r:?}");
    }

    #[test]
    fn with_no_key_sign_offers_only_done_and_says_why() {
        struct NoKey;
        impl Signer for NoKey {
            fn keys(&self) -> Vec<String> {
                Vec::new()
            }
            fn sign(&mut self, _: usize, _: &[u8]) -> Result<Vec<u8>, String> {
                unreachable!()
            }
        }
        let review = Review::new(json_to_lines(&serde_json::json!({"fee": "1"})), 21, 10).unwrap();
        let mut k = NoKey;
        let mut s = Session::new(review, vec![], &k);
        s.handle(ButtonEvent::Mode, &mut k);
        let (header, r, _) = rows(&s);
        assert_eq!(
            (header.as_str(), r),
            ("SIGN no key", vec!["Done".to_string()])
        );
        assert_eq!(s.handle(ButtonEvent::Confirm, &mut k), Finish::Done(vec![]));
    }

    #[test]
    fn keys_mode_lists_its_actions() {
        let mut f = fake();
        let mut s = session(serde_json::json!({"fee": "1"}), &f);
        s.handle(ButtonEvent::Mode, &mut f);
        s.handle(ButtonEvent::Mode, &mut f);
        let (_, r, _) = rows(&s);
        assert_eq!(
            r,
            KEY_ACTIONS
                .iter()
                .map(|a| a.to_string())
                .collect::<Vec<_>>()
        );
    }
}
