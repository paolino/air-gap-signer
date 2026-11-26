//! A signing session: the review of one transaction plus the modes the
//! user moves between with #6 — Review, Sign, Keys.
//!
//! In Sign mode #5 on a key adds that key's signature at once and #5
//! again removes it: the toggle is the undo, there is no confirmation and
//! no separate reject (a session finished with no signature signs
//! nothing). Signatures stay in memory until Done, and only while the
//! review stays complete: undoing an accepted branch drops them.

use crate::key_menu::{KeyMenu, PublicSlots};
use crate::review::{ButtonEvent, Review, Row, View};
use crate::spec::SignAlgorithm;

/// A key the device holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    pub slot: u8,
    pub algorithm: SignAlgorithm,
    /// A short label, e.g. `slot 0 ed25519 3fa2…`.
    pub label: String,
}

/// Keys able to sign, and signing with them.
pub trait Signer {
    fn keys(&self) -> Vec<Key>;
    /// Inventory is independent of the keys permitted to sign this request.
    fn public_slots(&mut self) -> Result<PublicSlots, String> {
        Err("public inventory unavailable".into())
    }
    /// Sign `message` with the key in `slot`.
    fn sign(&mut self, slot: u8, message: &[u8]) -> Result<Vec<u8>, String>;
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
    /// Done: the signatures added, as `(slot, signature)`; empty when
    /// nothing was signed.
    Done(Vec<(u8, Vec<u8>)>),
}

pub struct Session {
    key_menu: KeyMenu,
    review: Review,
    mode: Mode,
    message: Vec<u8>,
    /// The keys the request allows, in the order shown.
    keys: Vec<Key>,
    signatures: Vec<Option<Vec<u8>>>,
    /// Cursor in Sign mode (keys, then Done) and in Keys mode.
    cursor: usize,
    /// First row shown in Sign and Keys mode.
    top: usize,
    /// A one-line notice for the user, shown until the next press.
    notice: Option<String>,
}

impl Session {
    /// A session signing `message`, offering only the keys whose slot and
    /// algorithm appear in `allowed` (the request's constraints).
    pub fn new(
        review: Review,
        message: Vec<u8>,
        allowed: &[(u8, SignAlgorithm)],
        signer: &dyn Signer,
    ) -> Self {
        let keys: Vec<Key> = signer
            .keys()
            .into_iter()
            .filter(|k| allowed.contains(&(k.slot, k.algorithm)))
            .collect();
        Self {
            key_menu: KeyMenu::new(None, review.screen_rows()),
            review,
            mode: Mode::Review,
            message,
            signatures: vec![None; keys.len()],
            keys,
            cursor: 0,
            top: 0,
            notice: None,
        }
    }

    pub fn key_menu_mut(&mut self) -> &mut KeyMenu {
        &mut self.key_menu
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
            if self.mode == Mode::Keys {
                self.key_menu.refresh(signer.public_slots().ok());
            }
            self.cursor = 0;
            self.top = 0;
            return Finish::Continue;
        }
        match self.mode {
            // The review's own approve / reject outcomes are ignored:
            // signing is a choice made in Sign mode.
            Mode::Review => {
                let _ = self.review.handle(event);
                if self.review.remaining() > 0 {
                    self.signatures.iter_mut().for_each(|s| *s = None);
                }
            }
            Mode::Sign => return self.sign_mode(event, signer),
            Mode::Keys => {
                if self.key_menu.needs_retry() && event == ButtonEvent::Confirm {
                    self.key_menu.refresh(signer.public_slots().ok());
                } else {
                    self.key_menu.handle(event);
                }
            }
        }
        Finish::Continue
    }

    fn sign_mode(&mut self, event: ButtonEvent, signer: &mut dyn Signer) -> Finish {
        let entries = self.keys.len() + 1;
        match event {
            ButtonEvent::Up | ButtonEvent::UpHeld => self.step(entries, false),
            ButtonEvent::Down | ButtonEvent::DownHeld => self.step(entries, true),
            ButtonEvent::Confirm if self.cursor == self.keys.len() => {
                if self.review.remaining() > 0 {
                    return Finish::Done(Vec::new());
                }
                let done = self
                    .keys
                    .iter()
                    .zip(&self.signatures)
                    .filter_map(|(k, s)| s.clone().map(|s| (k.slot, s)))
                    .collect();
                return Finish::Done(done);
            }
            ButtonEvent::Confirm if self.signatures[self.cursor].is_some() => {
                self.signatures[self.cursor] = None;
            }
            ButtonEvent::Confirm if self.review.remaining() > 0 => {
                self.notice = Some(format!("{} left to review", self.review.remaining()));
            }
            ButtonEvent::Confirm => match signer.sign(self.keys[self.cursor].slot, &self.message) {
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
        // Keep the cursor on screen: the list scrolls only when needed.
        let body = self.body_rows();
        if self.cursor < self.top {
            self.top = self.cursor;
        } else if self.cursor >= self.top + body {
            self.top = self.cursor + 1 - body;
        }
    }

    /// Rows below the header.
    fn body_rows(&self) -> usize {
        self.review.screen_rows().saturating_sub(1).max(1)
    }

    pub fn view(&self) -> View {
        let row = |i: usize, text: String| Row {
            text,
            selected: i == self.cursor,
            pending: false,
            waived: false,
        };
        let window = |rows: Vec<Row>| -> Vec<Row> {
            rows.into_iter()
                .skip(self.top)
                .take(self.body_rows())
                .collect()
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
                        Some(_) => row(i, format!("SIGNED {}", k.label)),
                        None => row(i, k.label.clone()),
                    })
                    .collect();
                rows.push(row(self.keys.len(), "Done".into()));
                let header = match &self.notice {
                    Some(n) => format!("SIGN {n}"),
                    None => format!("SIGN {left}"),
                };
                View::Overview {
                    header,
                    rows: window(rows),
                }
            }
            Mode::Keys => self.key_menu.view(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::json_to_lines;

    const ED: SignAlgorithm = SignAlgorithm::Ed25519;

    struct Fake {
        keys: Vec<Key>,
        signed: Vec<(u8, Vec<u8>)>,
    }

    impl Signer for Fake {
        fn public_slots(&mut self) -> Result<PublicSlots, String> {
            Ok([None; 4])
        }
        fn keys(&self) -> Vec<Key> {
            self.keys.clone()
        }
        fn sign(&mut self, slot: u8, message: &[u8]) -> Result<Vec<u8>, String> {
            self.signed.push((slot, message.to_vec()));
            Ok(vec![slot; 4])
        }
    }

    fn key(slot: u8, algorithm: SignAlgorithm) -> Key {
        Key {
            slot,
            algorithm,
            label: format!("slot {slot}"),
        }
    }

    fn fake() -> Fake {
        Fake {
            keys: vec![key(0, ED), key(1, ED)],
            signed: Vec::new(),
        }
    }

    const BOTH: [(u8, SignAlgorithm); 2] = [(0, ED), (1, ED)];

    fn session(json: serde_json::Value, signer: &Fake) -> Session {
        let review = Review::new(json_to_lines(&json), 21, 10).unwrap();
        Session::new(review, b"body".to_vec(), &BOTH, signer)
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
        assert!(rows(&s).0.contains("left"), "{:?}", rows(&s));
    }

    #[test]
    fn undoing_an_accepted_branch_drops_the_signatures() {
        let mut f = fake();
        let long = "x".repeat(100);
        let mut s = session(serde_json::json!({"memo": long}), &f);
        s.handle(ButtonEvent::Confirm, &mut f); // accept the cut memo unread
        to_sign(&mut s, &mut f);
        s.handle(ButtonEvent::Confirm, &mut f); // sign with slot 0
        assert_eq!(f.signed.len(), 1);
        while s.mode() != Mode::Review {
            s.handle(ButtonEvent::Mode, &mut f);
        }
        s.handle(ButtonEvent::Confirm, &mut f); // undo the acceptance
        to_sign(&mut s, &mut f);
        assert!(
            !rows(&s).1.iter().any(|r| r.starts_with("SIGNED")),
            "{:?}",
            rows(&s).1
        );
        s.handle(ButtonEvent::Down, &mut f);
        s.handle(ButtonEvent::Down, &mut f); // Done
        assert_eq!(s.handle(ButtonEvent::Confirm, &mut f), Finish::Done(vec![]));
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
    fn only_keys_the_request_allows_are_offered_and_signing_goes_by_slot() {
        let mut f = Fake {
            keys: vec![
                key(0, SignAlgorithm::Secp256k1Ecdsa),
                key(1, ED),
                key(2, ED),
            ],
            signed: Vec::new(),
        };
        let review = Review::new(json_to_lines(&serde_json::json!({"fee": "1"})), 21, 10).unwrap();
        let mut s = Session::new(review, b"body".to_vec(), &[(0, ED), (2, ED)], &f);
        to_sign(&mut s, &mut f);
        assert_eq!(
            rows(&s).1,
            vec!["slot 2", "Done"],
            "slot 0 has the wrong algorithm, slot 1 is not asked for"
        );
        s.handle(ButtonEvent::Confirm, &mut f);
        assert_eq!(f.signed, vec![(2, b"body".to_vec())]);
    }

    #[test]
    fn the_sign_list_scrolls_to_keep_the_cursor_on_screen() {
        let mut f = Fake {
            keys: (0..12).map(|i| key(i, ED)).collect(),
            signed: Vec::new(),
        };
        let allowed: Vec<_> = (0..12).map(|i| (i, ED)).collect();
        let review = Review::new(json_to_lines(&serde_json::json!({"fee": "1"})), 21, 6).unwrap();
        let mut s = Session::new(review, b"body".to_vec(), &allowed, &f);
        to_sign(&mut s, &mut f);
        for _ in 0..12 {
            s.handle(ButtonEvent::Down, &mut f);
            let (_, r, sel) = rows(&s);
            assert!(r.len() <= 5, "fits under the header: {r:?}");
            assert!(sel < r.len(), "cursor on screen: {r:?}");
        }
        let (_, r, sel) = rows(&s);
        assert_eq!(r[sel], "Done");
        s.handle(ButtonEvent::Confirm, &mut f);
        assert!(f.signed.is_empty(), "#5 acted on the visible Done");
    }

    #[test]
    fn with_no_key_sign_offers_only_done_and_says_why() {
        let mut f = Fake {
            keys: Vec::new(),
            signed: Vec::new(),
        };
        let mut s = session(serde_json::json!({"fee": "1"}), &f);
        s.handle(ButtonEvent::Mode, &mut f);
        let (header, r, _) = rows(&s);
        assert_eq!(
            (header.as_str(), r),
            ("SIGN no key", vec!["Done".to_string()])
        );
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
    fn keys_mode_lists_device_slots() {
        let mut f = fake();
        let mut s = session(serde_json::json!({"fee": "1"}), &f);
        s.handle(ButtonEvent::Mode, &mut f);
        s.handle(ButtonEvent::Mode, &mut f);
        assert_eq!(rows(&s).1, ["0: empty", "1: empty", "2: empty", "3: empty"]);
    }
}
