//! On-device inspection of public Ed25519 keys, independent of a transaction.
use crate::review::{ButtonEvent, Row, View};
use sha2::{Digest, Sha256};

pub const SLOT_COUNT: usize = 4;
pub type PublicSlots = [Option<[u8; 32]>; SLOT_COUNT];

pub struct KeyMenu {
    slots: Option<PublicSlots>,
    practice: bool,
    selected: usize,
    page: Option<usize>,
    body: usize,
}

impl KeyMenu {
    pub fn new(slots: Option<PublicSlots>, screen_rows: usize) -> Self {
        Self {
            slots,
            practice: false,
            selected: 0,
            page: None,
            body: screen_rows.saturating_sub(1).max(1),
        }
    }

    /// Refreshing drops the old inventory even if the chip read fails.
    pub fn refresh(&mut self, slots: Option<PublicSlots>) {
        self.slots = slots;
        self.page = None;
    }

    /// Include the bench action only when the local panel can run it.
    pub fn enable_practice(&mut self) {
        self.practice = true;
    }

    /// Returns true when the owner opens the separate practice key.
    pub fn handle(&mut self, event: ButtonEvent) -> bool {
        use ButtonEvent::*;
        let Some(slots) = &self.slots else {
            return false;
        };
        if let Some(page) = &mut self.page {
            let last = 3 / self.body;
            match event {
                Down | DownHeld => *page = (*page + 1).min(last),
                Up | UpHeld => *page = page.saturating_sub(1),
                Back | Confirm => self.page = None,
                _ => {}
            }
        } else {
            let entries = SLOT_COUNT + usize::from(self.practice);
            match event {
                Down => self.selected = (self.selected + 1) % entries,
                Up => self.selected = (self.selected + entries - 1) % entries,
                DownHeld => self.selected = (self.selected + 1).min(entries - 1),
                UpHeld => self.selected = self.selected.saturating_sub(1),
                Confirm | Open if self.selected == SLOT_COUNT => return true,
                Confirm | Open if slots[self.selected].is_some() => self.page = Some(0),
                _ => {}
            }
        }
        false
    }

    pub fn view(&self) -> View {
        let Some(slots) = &self.slots else {
            return View::Detail {
                header: "KEYS read failed".into(),
                rows: vec!["Confirm to retry".into()],
            };
        };
        if let (Some(page), Some(public)) = (self.page, slots.get(self.selected).copied().flatten())
        {
            let pages = 4usize.div_ceil(self.body);
            return View::Detail {
                header: format!("Slot {} key {}/{}", self.selected, page + 1, pages),
                rows: public
                    .as_chunks::<8>()
                    .0
                    .iter()
                    .skip(page * self.body)
                    .take(self.body)
                    .map(hex::encode)
                    .collect(),
            };
        }
        let top = (self.selected + 1).saturating_sub(self.body);
        let rows = slots
            .iter()
            .enumerate()
            .map(|(i, public)| {
                let label = public
                    .map(|key| hex::encode(&Sha256::digest(key)[..6]))
                    .unwrap_or_else(|| "empty".into());
                Row {
                    text: format!("{i}: {label}"),
                    selected: i == self.selected,
                    pending: false,
                    waived: false,
                }
            })
            .chain(self.practice.then(|| Row {
                text: "Practice key".into(),
                selected: self.selected == SLOT_COUNT,
                pending: false,
                waived: false,
            }))
            .skip(top)
            .take(self.body)
            .collect();
        View::Overview {
            header: "KEYS slots".into(),
            rows,
        }
    }

    pub fn needs_retry(&self) -> bool {
        self.slots.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn practice_is_a_separate_action_and_failed_inventory_disables_it() {
        let mut menu = KeyMenu::new(Some([None; SLOT_COUNT]), 3);
        menu.enable_practice();
        menu.handle(ButtonEvent::Up);
        let View::Overview { rows, .. } = menu.view() else {
            panic!("expected slots")
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].text, "Practice key");
        assert!(rows[1].selected);
        assert!(menu.handle(ButtonEvent::Confirm));
        menu.refresh(None);
        assert!(!menu.handle(ButtonEvent::Open));
        menu.refresh(Some([None; SLOT_COUNT]));
        menu.handle(ButtonEvent::Down);
        assert!(!menu.handle(ButtonEvent::Confirm));
    }

    #[test]
    fn full_key_is_readable_on_the_smallest_grid_and_cannot_become_stale() {
        let public = std::array::from_fn(|i| i as u8);
        let mut menu = KeyMenu::new(Some([None, None, None, Some(public)]), 4);
        menu.handle(ButtonEvent::Up);
        let View::Overview { rows, .. } = menu.view() else {
            panic!("slot list")
        };
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].text, "3: 630dcd2966c4");
        assert!(rows[2].selected);
        menu.handle(ButtonEvent::Open);
        let View::Detail {
            rows: mut digits, ..
        } = menu.view()
        else {
            panic!("public key")
        };
        menu.handle(ButtonEvent::Down);
        let View::Detail { rows, .. } = menu.view() else {
            panic!("last page")
        };
        digits.extend(rows);
        assert_eq!(digits.join(""), hex::encode(public));
        menu.refresh(None);
        assert!(menu.needs_retry());
        assert!(matches!(menu.view(), View::Detail { header, .. } if header.contains("failed")));
    }
}
