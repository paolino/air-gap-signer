//! The transaction review a user goes through before approving.
//!
//! The transaction is browsed as a tree. The header is the branch from
//! the root to the level shown (`outputs›_0`, or `REVIEW` at the root); below it are that level's
//! entries, and under each group its own entries, indented one level.
//! Moving right on a group goes into it; on a value, or a group whose
//! name is cut, it shows the entry in full. Moving left goes back up.
//!
//! A value too long for its row is cut in the middle with `…`. Confirm is
//! accepted only once every entry has been on screen and every cut entry
//! has been shown whole or read to its end: nothing the user approves can
//! stay hidden in an unvisited branch or behind an ellipsis.

use crate::display::DisplayLine;

/// A user input during review.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ButtonEvent {
    Confirm,
    Reject,
    Up,
    Down,
    /// Up or down repeated while the joystick is held: like `Up` and
    /// `Down`, but the cursor stops at the first and last row instead of
    /// wrapping around.
    UpHeld,
    DownHeld,
    /// Enter the selected group, or show the selected value in full
    /// (joystick right).
    Open,
    /// Go back up one level (joystick left).
    Back,
    /// Switch to the next mode (#6).
    Mode,
}

/// What the review ended with, if anything.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    Continue,
    Confirmed,
    Rejected,
}

/// One entry of the level shown.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub text: String,
    pub selected: bool,
    /// Something here, or under it, still has to be shown before Confirm
    /// counts; drawn in its own colour.
    pub pending: bool,
    /// Accepted without being read (here or in a branch above); drawn
    /// apart from what was actually seen.
    pub waived: bool,
}

/// What the display must draw now: a header row, then the body.
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    /// The entries of one level of the tree.
    Overview { header: String, rows: Vec<Row> },
    /// One value in full, a page at a time.
    Detail { header: String, rows: Vec<String> },
}

/// Why a review cannot start.
#[derive(Debug, Clone, PartialEq)]
pub enum ReviewError {
    /// The display's text grid cannot hold a review.
    GridTooSmall { cols: usize, rows: usize },
    /// There is nothing to review: approving it would approve unseen bytes.
    Empty,
}

impl std::fmt::Display for ReviewError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GridTooSmall { cols, rows } => write!(
                f,
                "a {cols}x{rows} text grid is too small to review on (need {MIN_COLS}x{MIN_ROWS})"
            ),
            Self::Empty => write!(f, "the transaction has no fields to review"),
        }
    }
}

impl std::error::Error for ReviewError {}

/// Smallest grid a review is shown on: enough for a key and a value tail,
/// and a header plus three rows.
pub const MIN_COLS: usize = 16;
pub const MIN_ROWS: usize = 4;

/// Separates the segments of the branch in the header.
pub const GROUP_MARK: char = '›';

/// The branch label of the root.
const ROOT: &str = "REVIEW";

/// Columns a nested entry is indented by.
const INDENT: usize = 2;

/// Shortest value tail worth showing after a key; below this the whole
/// `key: value` is cut instead.
const MIN_VALUE: usize = 8;

struct Node {
    key: Option<String>,
    value: String,
    parent: usize,
    children: Vec<usize>,
}

/// An accepted branch or value, and its entries' `(node, unseen,
/// unopened)` state before it was accepted.
type Waiver = (usize, Vec<(usize, bool, bool)>);

pub(crate) struct UnreadPage {
    node: usize,
    page: usize,
}

pub struct Review {
    /// Node 0 is the root.
    nodes: Vec<Node>,
    cols: usize,
    rows: usize,
    /// The group whose tree is shown.
    at: usize,
    /// Index of the selected row in `tree()`.
    cursor: usize,
    /// Entries accepted without being read.
    waived: Vec<bool>,
    /// Each accepted branch, with what its entries' state was before, so
    /// accepting can be undone.
    waivers: Vec<Waiver>,
    /// The screens left behind by going right, as `(at, cursor)`.
    back: Vec<(usize, usize, usize)>,
    /// First row of `tree()` on screen.
    top: usize,
    /// `Some(page)` while the selected entry is shown in full.
    detail: Option<usize>,
    /// Entries never on screen.
    unseen: Vec<bool>,
    /// Entries that would be cut with `…` (values, or group names) and
    /// have neither been shown whole nor read to their end.
    unopened: Vec<bool>,
}

impl Review {
    /// `cols` × `rows` is the text grid of the display; the first row is
    /// the header.
    pub fn new(lines: Vec<DisplayLine>, cols: usize, rows: usize) -> Result<Self, ReviewError> {
        if cols < MIN_COLS || rows < MIN_ROWS {
            return Err(ReviewError::GridTooSmall { cols, rows });
        }
        let nodes = build_tree(lines);
        if nodes[0].children.is_empty() {
            return Err(ReviewError::Empty);
        }
        // Until shown whole, anything longer than the narrowest (nested)
        // row it can appear in counts as cut.
        let unopened = nodes
            .iter()
            .map(|n| full_text(n).chars().count() > cols - INDENT)
            .collect();
        let mut unseen = vec![true; nodes.len()];
        unseen[0] = false;
        let nodes_len = nodes.len();
        let mut review = Self {
            nodes,
            cols,
            rows,
            at: 0,
            cursor: 0,
            waived: vec![false; nodes_len],
            waivers: Vec::new(),
            back: Vec::new(),
            top: 0,
            detail: None,
            unseen,
            unopened,
        };
        review.cursor = review.first_stop();
        review.mark_shown();
        Ok(review)
    }

    /// Text rows on the screen, the header included.
    pub fn screen_rows(&self) -> usize {
        self.rows
    }

    /// Entries never shown, plus entries still cut with `…`: all must be
    /// shown before Confirm counts.
    pub fn remaining(&self) -> usize {
        self.unseen
            .iter()
            .zip(&self.unopened)
            .filter(|(u, c)| **u || **c)
            .count()
    }

    pub(crate) fn first_unread(&self) -> Option<UnreadPage> {
        let node = self
            .unseen
            .iter()
            .zip(&self.unopened)
            .position(|(u, c)| *u || *c)?;
        Some(UnreadPage { node, page: 0 })
    }

    pub(crate) fn unread_view(&self, entry: &UnreadPage) -> View {
        let pages = self.node_pages(entry.node);
        let status = format!("{}/{} #5 next", entry.page + 1, pages.len());
        View::Detail {
            header: self.header(&self.branch(entry.node), &status),
            rows: pages[entry.page].clone(),
        }
    }

    pub(crate) fn advance_unread(&mut self, mut entry: UnreadPage) -> Option<UnreadPage> {
        entry.page += 1;
        if entry.page < self.node_pages(entry.node).len() {
            return Some(entry);
        }
        self.unseen[entry.node] = false;
        self.unopened[entry.node] = false;
        self.first_unread()
    }

    pub fn handle(&mut self, event: ButtonEvent) -> Outcome {
        let entries = self.tree().len();
        match (self.detail, event) {
            (_, ButtonEvent::Reject) => return Outcome::Rejected,
            (Some(page), ButtonEvent::Up | ButtonEvent::UpHeld) => {
                self.show_page(page.saturating_sub(1))
            }
            (Some(page), ButtonEvent::Down | ButtonEvent::DownHeld) => {
                self.show_page((page + 1).min(self.pages().len() - 1))
            }
            (Some(_), ButtonEvent::Back) => self.detail = None,
            (Some(_), ButtonEvent::Open) if self.expandable(self.cursor) => {
                self.detail = None;
                self.enter(self.selected());
            }
            (Some(_), _) => {}
            (None, ButtonEvent::Up) => self.cursor = self.step(entries - 1),
            (None, ButtonEvent::Down) => self.cursor = self.step(1),
            (None, ButtonEvent::UpHeld) => self.cursor = self.step_to_end(false),
            (None, ButtonEvent::DownHeld) => self.cursor = self.step_to_end(true),
            (None, ButtonEvent::Open) => {
                let node = self.selected();
                // A value, or any entry whose name is cut, opens in full
                // first; a nested group goes in; a heading, whose entries
                // are already listed under it, does nothing.
                if !self.is_group(node) || self.unopened[node] {
                    self.show_page(0);
                } else if self.expandable(self.cursor) {
                    self.enter(node);
                }
            }
            (None, ButtonEvent::Back) => {
                if let Some((at, cursor, top)) = self.back.pop() {
                    self.at = at;
                    self.cursor = cursor;
                    self.top = top;
                }
            }
            // #5 on something accepted undoes it; on a branch or value
            // that still hides something it accepts it unread; anywhere
            // else it approves, once nothing is left.
            (None, ButtonEvent::Confirm) if self.accepted(self.selected()) => {
                self.unwaive(self.selected())
            }
            (None, ButtonEvent::Confirm) if self.pending(self.selected()) => {
                self.waive(self.selected())
            }
            (None, ButtonEvent::Confirm) if self.remaining() == 0 => return Outcome::Confirmed,
            (None, _) => {}
        }
        if self.detail.is_none() {
            self.mark_shown();
        }
        Outcome::Continue
    }

    pub fn view(&self) -> View {
        match self.detail {
            None => {
                let status = match (self.remaining(), self.waivers.len()) {
                    (0, 0) => "done".to_string(),
                    (0, w) => format!("{w} waived"),
                    (n, _) => format!("{n} left"),
                };
                let rows = self
                    .window()
                    .map(|(i, (node, depth))| Row {
                        text: self.entry_text(node, depth).0,
                        selected: i == self.cursor,
                        pending: self.pending(node),
                        waived: self.waived[node],
                    })
                    .collect();
                View::Overview {
                    header: self.header(&self.branch(self.at), &status),
                    rows,
                }
            }
            Some(page) => {
                let pages = self.pages();
                let status = format!("{}/{}", page + 1, pages.len());
                View::Detail {
                    header: self.header(&self.branch(self.selected()), &status),
                    rows: pages[page].clone(),
                }
            }
        }
    }

    fn is_group(&self, node: usize) -> bool {
        !self.nodes[node].children.is_empty()
    }

    /// Whether the row at `index` of `tree()` is a nested group, which
    /// right goes into.
    fn expandable(&self, index: usize) -> bool {
        let (node, depth) = self.tree()[index];
        depth == 1 && self.is_group(node)
    }

    /// Whether `node`, or anything under it, still has to be shown.
    fn pending(&self, node: usize) -> bool {
        self.unopened[node]
            || self.nodes[node]
                .children
                .iter()
                .any(|&c| self.unseen[c] || self.pending(c))
    }

    /// Whether `node` is the root of an accepted branch.
    fn accepted(&self, node: usize) -> bool {
        self.waivers.iter().any(|(root, _)| *root == node)
    }

    /// Accept `node` and everything under it as if read, remembering their
    /// state so it can be undone.
    fn waive(&mut self, node: usize) {
        let mut before = Vec::new();
        let mut stack = vec![node];
        while let Some(n) = stack.pop() {
            before.push((n, self.unseen[n], self.unopened[n]));
            self.unseen[n] = false;
            self.unopened[n] = false;
            self.waived[n] = true;
            stack.extend(self.nodes[n].children.iter().copied());
        }
        self.waivers.push((node, before));
    }

    /// Undo accepting `node`: its entries are as they were before.
    fn unwaive(&mut self, node: usize) {
        if let Some(i) = self.waivers.iter().position(|(root, _)| *root == node) {
            let (_, before) = self.waivers.remove(i);
            for (n, unseen, unopened) in before {
                self.unseen[n] = unseen;
                self.unopened[n] = unopened;
                self.waived[n] = false;
            }
        }
    }

    fn enter(&mut self, group: usize) {
        self.back.push((self.at, self.cursor, self.top));
        self.top = 0;
        self.at = group;
        self.cursor = self.first_stop();
    }

    /// Whether the cursor stops on the row at `index` of `tree()`: every
    /// row but a heading, unless the heading's name is cut and unread.
    fn stops(&self, index: usize) -> bool {
        let (node, depth) = self.tree()[index];
        !(depth == 0 && self.is_group(node)) || self.unopened[node]
    }

    fn first_stop(&self) -> usize {
        (0..self.tree().len()).find(|&i| self.stops(i)).unwrap_or(0)
    }

    /// The next row the cursor stops on, going down or up, staying put at
    /// the last or first such row instead of wrapping.
    fn step_to_end(&self, down: bool) -> usize {
        let stops = |i: &usize| self.stops(*i);
        let next = if down {
            (self.cursor + 1..self.tree().len()).find(stops)
        } else {
            (0..self.cursor).rev().find(stops)
        };
        next.unwrap_or(self.cursor)
    }

    /// The next row the cursor stops on, moving by `delta` (1 down,
    /// `len - 1` up), wrapping around.
    fn step(&self, delta: usize) -> usize {
        let len = self.tree().len();
        let mut i = self.cursor;
        for _ in 0..len {
            i = (i + delta) % len;
            if self.stops(i) {
                return i;
            }
        }
        self.cursor
    }

    /// The rows of the level shown: each entry of `at`, and under each
    /// group its own entries, one level deep, as `(node, depth)`.
    fn tree(&self) -> Vec<(usize, usize)> {
        let mut rows = Vec::new();
        for &c in &self.nodes[self.at].children {
            rows.push((c, 0));
            for &g in &self.nodes[c].children {
                rows.push((g, 1));
            }
        }
        rows
    }

    /// The rows on screen, with their index in `tree()`.
    fn window(&self) -> impl Iterator<Item = (usize, (usize, usize))> {
        self.tree()
            .into_iter()
            .enumerate()
            .skip(self.top())
            .take(self.visible())
    }

    fn selected(&self) -> usize {
        self.tree()[self.cursor].0
    }

    fn visible(&self) -> usize {
        self.rows - 1
    }

    /// First row of the window, which keeps the cursor on screen.
    fn top(&self) -> usize {
        self.top
    }

    /// Scroll only when the cursor would leave the window; scrolling up,
    /// keep the headings just above the cursor in view.
    fn follow(&mut self) {
        if self.cursor < self.top {
            self.top = self.cursor;
            while self.top > 0
                && !self.stops(self.top - 1)
                && self.cursor - (self.top - 1) < self.visible()
            {
                self.top -= 1;
            }
        } else if self.cursor >= self.top + self.visible() {
            self.top = self.cursor + 1 - self.visible();
        }
    }

    /// Every entry on screen is seen; one shown whole needs no opening.
    fn mark_shown(&mut self) {
        self.follow();
        let shown: Vec<(usize, bool)> = self
            .window()
            .map(|(_, (node, depth))| (node, self.entry_text(node, depth).1))
            .collect();
        for (node, cut) in shown {
            self.unseen[node] = false;
            if !cut {
                self.unopened[node] = false;
            }
        }
    }

    /// Show `page` of the selected entry; reaching its last page counts
    /// as having read it.
    fn show_page(&mut self, page: usize) {
        self.detail = Some(page);
        if page + 1 == self.pages().len() {
            let selected = self.selected();
            self.unopened[selected] = false;
        }
    }

    /// The selected entry in full, split into screen pages: a value as
    /// its key wrapped on its own rows, then the value wrapped and
    /// indented under it; a group as its full label.
    fn pages(&self) -> Vec<Vec<String>> {
        self.node_pages(self.selected())
    }

    fn node_pages(&self, id: usize) -> Vec<Vec<String>> {
        let node = &self.nodes[id];
        let mut rows = Vec::new();
        if !node.children.is_empty() {
            rows = wrap(&full_text(node), self.cols);
        } else {
            if let Some(k) = &node.key {
                rows.extend(wrap(&format!("{k}:"), self.cols));
            }
            rows.extend(
                wrap(&node.value, self.cols - 1)
                    .into_iter()
                    .map(|r| format!(" {r}")),
            );
        }
        rows.chunks(self.visible())
            .map(<[String]>::to_vec)
            .collect()
    }

    /// An entry as one row, indented by its depth, and whether it had to
    /// be cut. A group shows just its key. A value is `key: value`, cut in
    /// the value's middle so the key stays readable.
    fn entry_text(&self, node: usize, depth: usize) -> (String, bool) {
        let n = &self.nodes[node];
        let pad = " ".repeat(INDENT * depth);
        let width = self.cols - pad.len();
        let full = full_text(n);
        if full.chars().count() <= width {
            return (format!("{pad}{full}"), false);
        }
        let cut = match &n.key {
            Some(k)
                if !self.is_group(node)
                    && !n.value.is_empty()
                    && k.chars().count() + 2 + MIN_VALUE <= width =>
            {
                let room = width - k.chars().count() - 2;
                format!("{k}: {}", ellipsize(&n.value, room))
            }
            _ => ellipsize(&full, width),
        };
        (format!("{pad}{cut}"), true)
    }

    /// The keys from the root to `node`, e.g. `outputs›_0`; the mode's
    /// name, `REVIEW`, at the root.
    fn branch(&self, node: usize) -> String {
        let mut keys = Vec::new();
        let mut n = node;
        while n != 0 {
            keys.push(self.nodes[n].key.as_deref().unwrap_or("?"));
            n = self.nodes[n].parent;
        }
        if keys.is_empty() {
            keys.push(ROOT);
        }
        keys.reverse();
        keys.join(&GROUP_MARK.to_string())
    }

    /// The branch on the left, cut from its start when too long so the
    /// deepest segments stay visible, and `status` on the right.
    fn header(&self, branch: &str, status: &str) -> String {
        let room = self.cols.saturating_sub(status.chars().count() + 1);
        if room == 0 {
            return ellipsize(status, self.cols);
        }
        let chars: Vec<char> = branch.chars().collect();
        let left: String = if chars.len() <= room {
            branch.to_string()
        } else {
            let tail = &chars[chars.len() - (room - 1)..];
            std::iter::once('…').chain(tail.iter().copied()).collect()
        };
        let gap = self.cols - left.chars().count() - status.chars().count();
        format!("{left}{}{status}", " ".repeat(gap))
    }
}

/// Rebuild the tree that `json_to_lines` flattened, from the indents.
fn build_tree(lines: Vec<DisplayLine>) -> Vec<Node> {
    let mut nodes = vec![Node {
        key: None,
        value: String::new(),
        parent: 0,
        children: Vec::new(),
    }];
    // (indent, node) of the open ancestors; the root sits below any indent.
    let mut stack: Vec<(Option<usize>, usize)> = vec![(None, 0)];
    for line in lines {
        while stack.len() > 1 && stack.last().is_some_and(|(i, _)| *i >= Some(line.indent)) {
            stack.pop();
        }
        let parent = stack.last().map_or(0, |&(_, n)| n);
        let id = nodes.len();
        // `json_to_lines` keys list items `[i]` and gives the list a
        // `[n items]` value; show the items as `_i`.
        let in_list = parent != 0 && !nodes[parent].value.is_empty();
        let key = match line.key {
            Some(k) if in_list && k.starts_with('[') && k.ends_with(']') => {
                Some(format!("_{}", &k[1..k.len() - 1]))
            }
            k => k,
        };
        nodes.push(Node {
            key: key.as_deref().map(printable),
            value: printable(&line.value),
            parent,
            children: Vec::new(),
        });
        nodes[parent].children.push(id);
        stack.push((Some(line.indent), id));
    }
    nodes
}

/// An entry's whole text: the key for a group; `key: value`, `key:` or
/// the bare value for a value.
fn full_text(node: &Node) -> String {
    if !node.children.is_empty() {
        return node.key.clone().unwrap_or_default();
    }
    match &node.key {
        Some(k) if node.value.is_empty() => format!("{k}:"),
        Some(k) => format!("{k}: {}", node.value),
        None => node.value.clone(),
    }
}

/// Spell out every character outside printable ASCII as `\u{hex}`, and a
/// backslash as `\\`, so that text the font cannot draw, control
/// characters, look-alikes and literal escapes can never show as each
/// other.
fn printable(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\\' => "\\\\".to_string(),
            ' '..='~' => c.to_string(),
            _ => format!("\\u{{{:x}}}", c as u32),
        })
        .collect()
}

/// Shorten `text` to `width` characters by replacing its middle with `…`.
pub fn ellipsize(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return text.to_string();
    }
    let keep = width.saturating_sub(1);
    let head = keep.div_ceil(2);
    let tail = keep - head;
    let mut out: String = chars[..head].iter().collect();
    out.push('…');
    out.extend(&chars[chars.len() - tail..]);
    out
}

/// Split `text` into rows of at most `width` characters, losing nothing.
pub fn wrap(text: &str, width: usize) -> Vec<String> {
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
    use crate::display::json_to_lines;

    const ADDR: &str = "addr_test1qz2fxv2umyhttkxyxp8x0dlpdt3k6cwng5pxj3jhsydzer3jcu5d8ps7zex2k2xt3uqxgjqnnj83ws8lhrn648jjxtwq2ytjqp";

    fn tx() -> Vec<DisplayLine> {
        json_to_lines(&serde_json::json!({
            "fee": "171485",
            "outputs": [
                {"address": ADDR, "lovelace": "5000000"},
                {"address": "addr1vrgp", "lovelace": "94828515"}
            ]
        }))
    }

    fn review(lines: Vec<DisplayLine>) -> Review {
        Review::new(lines, 22, 11).expect("grid is large enough")
    }

    fn level(r: &Review) -> (String, Vec<String>, usize) {
        match r.view() {
            View::Overview { header, rows } => {
                let selected = rows.iter().position(|r| r.selected).unwrap();
                (header, rows.into_iter().map(|r| r.text).collect(), selected)
            }
            v => panic!("expected a level, got {v:?}"),
        }
    }

    fn detail(r: &Review) -> (String, Vec<String>) {
        match r.view() {
            View::Detail { header, rows } => (header, rows),
            v => panic!("expected detail, got {v:?}"),
        }
    }

    /// Move the cursor to the row whose text starts with `prefix`.
    fn select(r: &mut Review, prefix: &str) {
        for _ in 0..50 {
            let (_, rows, sel) = level(r);
            if rows[sel].starts_with(prefix) {
                return;
            }
            r.handle(ButtonEvent::Down);
        }
        panic!("no row starting with {prefix:?}");
    }

    #[test]
    fn ellipsis_keeps_both_ends() {
        assert_eq!(ellipsize("abcdefghij", 7), "abc…hij");
        assert_eq!(ellipsize("short", 7), "short");
    }

    #[test]
    fn wrap_loses_nothing() {
        let rows = wrap(ADDR, 21);
        assert!(rows.iter().all(|r| r.chars().count() <= 21));
        assert_eq!(rows.concat(), ADDR);
    }

    #[test]
    fn each_group_lists_its_entries_under_it() {
        let r = review(tx());
        let (header, rows, sel) = level(&r);
        assert!(header.starts_with("REVIEW "), "{header}");
        assert_eq!(rows, vec!["fee: 171485", "outputs", "  _0", "  _1"]);
        assert_eq!(sel, 0);
    }

    #[test]
    fn rows_still_needing_attention_are_marked_pending_until_done() {
        let pending = |r: &Review| match r.view() {
            View::Overview { rows, .. } => rows
                .into_iter()
                .map(|r| (r.text, r.pending))
                .collect::<Vec<_>>(),
            v => panic!("expected a level, got {v:?}"),
        };
        let mut r = review(tx());
        assert_eq!(
            pending(&r),
            vec![
                ("fee: 171485".to_string(), false),
                ("outputs".to_string(), true),
                ("  _0".to_string(), true),
                ("  _1".to_string(), true),
            ]
        );
        select(&mut r, "  _1");
        r.handle(ButtonEvent::Open);
        r.handle(ButtonEvent::Back);
        let after = pending(&r);
        assert_eq!(
            after[3],
            ("  _1".to_string(), false),
            "[1] done once its fields were shown"
        );
        assert_eq!(
            after[2],
            ("  _0".to_string(), true),
            "[0] still hides a cut address"
        );
        assert!(
            after[1].1,
            "the heading stays pending while anything under it is"
        );
    }

    #[test]
    fn the_cursor_never_stops_on_a_heading() {
        let mut r = review(tx());
        let mut visited = Vec::new();
        for _ in 0..4 {
            let (_, rows, sel) = level(&r);
            visited.push(rows[sel].clone());
            r.handle(ButtonEvent::Down);
        }
        assert_eq!(visited, vec!["fee: 171485", "  _0", "  _1", "fee: 171485"]);
        let mut r = review(tx());
        r.handle(ButtonEvent::Up);
        let (_, rows, sel) = level(&r);
        assert_eq!(
            rows[sel], "  _1",
            "up from the first field wraps past the heading"
        );
    }

    #[test]
    fn right_on_a_nested_group_goes_into_it() {
        let mut r = review(tx());
        select(&mut r, "  _0");
        r.handle(ButtonEvent::Open);
        let (header, rows, sel) = level(&r);
        assert!(header.starts_with("outputs›_0 "), "{header}");
        assert!(rows[0].starts_with("address: addr_"), "{}", rows[0]);
        assert!(rows[0].contains('…'));
        assert_eq!(rows[1], "lovelace: 5000000");
        assert_eq!(sel, 0);
    }

    #[test]
    fn left_goes_back_to_the_screen_it_came_from() {
        let mut r = review(tx());
        select(&mut r, "  _1");
        r.handle(ButtonEvent::Open);
        assert!(level(&r).0.starts_with("outputs›_1 "));
        r.handle(ButtonEvent::Back);
        let (header, rows, sel) = level(&r);
        assert!(header.starts_with("REVIEW "), "{header}");
        assert_eq!(rows[sel], "  _1");
    }

    #[test]
    fn a_value_opens_with_its_key_above_and_the_value_indented() {
        let mut r = review(tx());
        select(&mut r, "  _0");
        r.handle(ButtonEvent::Open);
        select(&mut r, "address: addr_");
        r.handle(ButtonEvent::Open);
        let (header, rows) = detail(&r);
        assert!(header.contains("address"), "{header}");
        assert_eq!(rows[0], "address:");
        let value: String = rows[1..].iter().map(|r| r.trim_start()).collect();
        assert_eq!(value, ADDR);
    }

    #[test]
    fn confirm_waits_until_everything_was_shown_and_every_cut_value_read() {
        let mut r = review(tx());
        assert_eq!(
            r.remaining(),
            4,
            "four fields inside the outputs, including one long address"
        );
        assert_eq!(r.handle(ButtonEvent::Confirm), Outcome::Continue);
        assert!(level(&r).0.ends_with(" left"));
        select(&mut r, "  _0");
        r.handle(ButtonEvent::Open);
        select(&mut r, "address: addr_");
        r.handle(ButtonEvent::Open);
        r.handle(ButtonEvent::Back);
        r.handle(ButtonEvent::Back);
        assert_eq!(r.remaining(), 2, "the fields of [1]");
        assert_eq!(r.handle(ButtonEvent::Confirm), Outcome::Continue);
        select(&mut r, "  _1");
        r.handle(ButtonEvent::Open);
        assert_eq!(r.remaining(), 0);
        assert!(level(&r).0.ends_with("done"));
        assert_eq!(r.handle(ButtonEvent::Confirm), Outcome::Confirmed);
    }

    #[test]
    fn entries_never_scrolled_into_view_block_confirm() {
        let lines: Vec<_> = (0..30)
            .map(|i| DisplayLine {
                indent: 1,
                key: Some("k".into()),
                value: i.to_string(),
            })
            .collect();
        let mut r = review(lines);
        assert_eq!(r.remaining(), 20);
        for _ in 0..29 {
            r.handle(ButtonEvent::Down);
        }
        assert_eq!(r.handle(ButtonEvent::Confirm), Outcome::Confirmed);
    }

    #[test]
    fn a_multipage_value_must_be_read_to_its_last_page() {
        let value = "x".repeat(500);
        let line = DisplayLine {
            indent: 1,
            key: Some("data".into()),
            value: value.clone(),
        };
        let mut r = review(vec![line]);
        r.handle(ButtonEvent::Open);
        r.handle(ButtonEvent::Back);
        assert_eq!(r.remaining(), 1, "opening without paging does not count");
        r.handle(ButtonEvent::Open);
        let mut read = String::new();
        let mut last = String::new();
        for _ in 0..10 {
            let (header, rows) = detail(&r);
            if header != last {
                read.extend(
                    rows.iter()
                        .skip_while(|r| !r.starts_with(' '))
                        .map(|r| r.trim_start()),
                );
                last = header;
            }
            r.handle(ButtonEvent::Down);
        }
        assert_eq!(read, value);
        r.handle(ButtonEvent::Back);
        assert_eq!(r.handle(ButtonEvent::Confirm), Outcome::Confirmed);
    }

    #[test]
    fn the_cursor_moves_through_nested_rows_and_wraps_around() {
        let mut r = review(tx());
        r.handle(ButtonEvent::Up);
        assert_eq!(level(&r).2, 3, "onto the last nested row");
        r.handle(ButtonEvent::Down);
        assert_eq!(level(&r).2, 0);
        r.handle(ButtonEvent::Down);
        assert_eq!(level(&r).2, 2, "past the heading");
    }

    #[test]
    fn back_at_the_root_and_confirm_in_a_value_do_nothing() {
        let mut r = review(tx());
        assert_eq!(r.handle(ButtonEvent::Back), Outcome::Continue);
        assert!(level(&r).0.starts_with("REVIEW "));
        r.handle(ButtonEvent::Open);
        assert_eq!(r.handle(ButtonEvent::Confirm), Outcome::Continue);
        assert!(matches!(r.view(), View::Detail { .. }));
    }

    #[test]
    fn reject_ends_the_review_from_anywhere() {
        let mut r = review(tx());
        select(&mut r, "  _0");
        r.handle(ButtonEvent::Open);
        assert_eq!(r.handle(ButtonEvent::Reject), Outcome::Rejected);
    }

    #[test]
    fn a_deep_branch_keeps_its_deepest_segments() {
        let lines = json_to_lines(&serde_json::json!(
            {"alpha": {"beta": {"gamma": {"delta": {"epsilon": {"v": "1"}}}}}}
        ));
        let mut r = review(lines);
        select(&mut r, "  beta");
        r.handle(ButtonEvent::Open);
        select(&mut r, "  delta");
        r.handle(ButtonEvent::Open);
        let (header, rows, _) = level(&r);
        assert_eq!(header.chars().count(), 22);
        assert!(header.starts_with('…'), "{header}");
        assert!(header.contains("gamma›delta"), "{header}");
        assert_eq!(rows, vec!["epsilon", "  v: 1"]);
    }

    #[test]
    fn a_grid_too_small_to_review_on_is_refused() {
        assert_eq!(
            Review::new(tx(), 15, 11).err(),
            Some(ReviewError::GridTooSmall { cols: 15, rows: 11 })
        );
        assert!(Review::new(tx(), 22, 3).is_err());
        assert!(Review::new(tx(), 16, 4).is_ok());
    }

    #[test]
    fn an_empty_transaction_cannot_be_reviewed() {
        assert_eq!(Review::new(vec![], 22, 11).err(), Some(ReviewError::Empty));
        let empty = json_to_lines(&serde_json::json!({}));
        assert_eq!(Review::new(empty, 22, 11).err(), Some(ReviewError::Empty));
    }

    #[test]
    fn nested_groups_show_just_their_key() {
        let lines = json_to_lines(&serde_json::json!({"g": {"a": [1, 2], "b": {"x": 1}}}));
        let r = review(lines);
        let (_, rows, sel) = level(&r);
        assert_eq!(rows, vec!["g", "  a", "  b"]);
        assert_eq!(
            sel, 1,
            "the cursor starts on the first entry, not the heading"
        );
    }

    #[test]
    fn characters_outside_printable_ascii_are_spelled_out() {
        let lines = json_to_lines(&serde_json::json!({"name": "東西", "tab": "a\tb"}));
        let r = review(lines);
        assert_eq!(
            level(&r).1,
            vec![r"name: \u{6771}\u{897f}", r"tab: a\u{9}b"]
        );
    }

    #[test]
    fn a_long_key_is_shown_in_full_in_the_value_page() {
        let key = "k".repeat(40);
        let mut r = review(vec![DisplayLine {
            indent: 1,
            key: Some(key.clone()),
            value: "v".into(),
        }]);
        r.handle(ButtonEvent::Open);
        let (_, rows) = detail(&r);
        let shown: String = rows
            .iter()
            .take_while(|r| !r.starts_with(' '))
            .cloned()
            .collect();
        assert_eq!(shown, format!("{key}:"));
    }

    #[test]
    fn a_group_with_a_cut_name_shows_its_full_name_before_its_entries() {
        let policy = "f0ff48bbb7bbe9d59a40f1ce90e9e9d0ff5002ec48f232b49ca0fb9a";
        let lines = json_to_lines(&serde_json::json!({"assets": {policy: {"tok": "1"}}}));
        let mut r = review(lines);
        let (_, rows, _) = level(&r);
        assert_eq!(rows[0], "assets");
        assert!(
            rows[1].starts_with("  ") && rows[1].contains('…'),
            "{}",
            rows[1]
        );
        assert_eq!(r.remaining(), 2, "the cut name, and tok inside");
        r.handle(ButtonEvent::Open); // first: the full name
        let (_, page) = detail(&r);
        assert_eq!(page.concat(), policy);
        assert_eq!(r.remaining(), 1);
        r.handle(ButtonEvent::Open); // then: its entries
        let (header, rows, _) = level(&r);
        assert!(header.contains('…'), "{header}");
        assert_eq!(rows, vec!["tok: 1"]);
        assert_eq!(r.remaining(), 0);
    }

    #[test]
    fn list_items_are_numbered_with_an_underscore() {
        let lines = json_to_lines(&serde_json::json!({"xs": ["a", "b"]}));
        let r = review(lines);
        assert_eq!(level(&r).1, vec!["xs", "  _0: a", "  _1: b"]);
    }

    #[test]
    fn a_heading_whose_name_is_cut_still_takes_the_cursor() {
        let long = "h".repeat(30);
        let lines = json_to_lines(&serde_json::json!({ long.clone(): {"a": "1"} }));
        let r = review(lines);
        let (_, rows, sel) = level(&r);
        assert!(rows[sel].contains('…'), "{}", rows[sel]);
    }

    #[test]
    fn a_literal_backslash_cannot_pass_for_an_escape() {
        let tab = review(json_to_lines(&serde_json::json!({"m": "a\tb"})));
        let text = review(json_to_lines(&serde_json::json!({"m": "a\\u{9}b"})));
        assert_eq!(level(&tab).1, vec![r"m: a\u{9}b"]);
        assert_eq!(level(&text).1, vec![r"m: a\\u{9}b"]);
    }

    #[test]
    fn a_blank_transaction_shows_something_to_read() {
        let r = review(json_to_lines(&serde_json::json!(" ")));
        assert_eq!(level(&r).1, vec![r#"" ""#]);
    }

    fn rows_of(r: &Review) -> Vec<Row> {
        match r.view() {
            View::Overview { rows, .. } => rows,
            v => panic!("expected a level, got {v:?}"),
        }
    }

    #[test]
    fn holding_stops_at_the_ends_and_a_fresh_press_wraps() {
        let mut r = review(tx());
        for _ in 0..10 {
            r.handle(ButtonEvent::DownHeld);
        }
        let (_, rows, sel) = level(&r);
        assert_eq!(rows[sel], "  _1", "held: stops on the last row");
        r.handle(ButtonEvent::Down);
        let (_, rows, sel) = level(&r);
        assert_eq!(rows[sel], "fee: 171485", "a fresh press wraps");
        r.handle(ButtonEvent::UpHeld);
        let (_, rows, sel) = level(&r);
        assert_eq!(rows[sel], "fee: 171485", "held: stops on the first row");
        r.handle(ButtonEvent::Up);
        let (_, rows, sel) = level(&r);
        assert_eq!(rows[sel], "  _1", "a fresh press wraps");
    }

    #[test]
    fn held_up_and_down_turn_pages_like_presses() {
        let value = "x".repeat(500);
        let mut r = review(vec![DisplayLine {
            indent: 1,
            key: Some("d".into()),
            value,
        }]);
        r.handle(ButtonEvent::Open);
        let first = detail(&r).0;
        r.handle(ButtonEvent::DownHeld);
        assert_ne!(detail(&r).0, first);
        r.handle(ButtonEvent::UpHeld);
        assert_eq!(detail(&r).0, first);
    }

    #[test]
    fn moving_up_from_the_bottom_moves_the_cursor_not_the_list() {
        let lines: Vec<_> = (0..30)
            .map(|i| DisplayLine {
                indent: 1,
                key: Some("k".into()),
                value: i.to_string(),
            })
            .collect();
        let mut r = review(lines);
        r.handle(ButtonEvent::Up); // wraps to the last row
        let (_, before, sel) = level(&r);
        assert_eq!(before[sel], "k: 29");
        r.handle(ButtonEvent::Up);
        r.handle(ButtonEvent::Up);
        let (_, after, sel) = level(&r);
        assert_eq!(
            after, before,
            "the window stays while the cursor is inside it"
        );
        assert_eq!(after[sel], "k: 27");
    }

    #[test]
    fn scrolling_up_into_a_group_shows_its_heading() {
        let lines = json_to_lines(&serde_json::json!({
            "a": "1", "b": "2", "c": "3", "d": "4", "e": "5", "f": "6", "g": "7", "h": "8",
            "list": ["x", "y"],
            "z": "9"
        }));
        let mut r = review(lines);
        r.handle(ButtonEvent::Up); // wraps to the bottom: z
        for _ in 0..2 {
            r.handle(ButtonEvent::Up); // _1, then _0 of list
        }
        let (_, rows, sel) = level(&r);
        assert_eq!(rows[sel], "  _0: x");
        assert_eq!(rows[sel - 1], "list", "the heading above stays in view");
    }

    #[test]
    fn five_on_a_branch_accepts_it_at_once() {
        let mut r = review(tx());
        let before = r.remaining();
        select(&mut r, "  _1");
        assert_eq!(r.handle(ButtonEvent::Confirm), Outcome::Continue);
        assert!(matches!(r.view(), View::Overview { .. }), "no question");
        assert_eq!(
            r.remaining(),
            before - 2,
            "the two fields of _1 no longer block"
        );
        let row = rows_of(&r)
            .into_iter()
            .find(|row| row.text == "  _1")
            .unwrap();
        assert!(row.waived && !row.pending, "{row:?}");
    }

    #[test]
    fn five_again_undoes_it() {
        let mut r = review(tx());
        let before = r.remaining();
        select(&mut r, "  _1");
        r.handle(ButtonEvent::Confirm);
        r.handle(ButtonEvent::Confirm);
        assert_eq!(r.remaining(), before);
        let row = rows_of(&r)
            .into_iter()
            .find(|row| row.text == "  _1")
            .unwrap();
        assert!(!row.waived && row.pending, "{row:?}");
    }

    #[test]
    fn approval_after_accepting_says_how_many_branches_were_accepted() {
        let mut r = review(tx());
        for branch in ["  _0", "  _1"] {
            select(&mut r, branch);
            r.handle(ButtonEvent::Confirm);
        }
        assert_eq!(r.remaining(), 0);
        let (header, _, _) = level(&r);
        assert!(header.ends_with(" 2 waived"), "{header}");
        select(&mut r, "fee");
        assert_eq!(r.handle(ButtonEvent::Confirm), Outcome::Confirmed);
    }

    #[test]
    fn five_on_a_cut_value_accepts_it_and_again_undoes_it() {
        let mut r = review(tx());
        select(&mut r, "  _1");
        r.handle(ButtonEvent::Open); // into output _1: its address is cut
        r.handle(ButtonEvent::Back);
        select(&mut r, "  _0");
        r.handle(ButtonEvent::Open); // into output _0
        let before = r.remaining();
        select(&mut r, "address: addr_");
        r.handle(ButtonEvent::Confirm);
        assert_eq!(
            r.remaining(),
            before - 1,
            "the cut address no longer blocks"
        );
        let row = rows_of(&r)
            .into_iter()
            .find(|row| row.text.starts_with("address"))
            .unwrap();
        assert!(row.waived && !row.pending, "{row:?}");
        r.handle(ButtonEvent::Confirm);
        assert_eq!(r.remaining(), before);
    }

    #[test]
    fn five_on_a_value_already_shown_accepts_nothing() {
        let mut r = review(tx());
        select(&mut r, "fee");
        let before = r.remaining();
        assert_eq!(r.handle(ButtonEvent::Confirm), Outcome::Continue);
        assert_eq!(r.remaining(), before);
        assert!(!rows_of(&r)[0].waived);
    }

    #[test]
    fn a_branch_already_read_is_not_accepted() {
        let mut r = review(tx());
        select(&mut r, "  _1");
        r.handle(ButtonEvent::Open);
        r.handle(ButtonEvent::Back);
        let before = r.remaining();
        r.handle(ButtonEvent::Confirm);
        assert_eq!(r.remaining(), before);
        let row = rows_of(&r)
            .into_iter()
            .find(|row| row.text == "  _1")
            .unwrap();
        assert!(!row.waived);
    }

    #[test]
    fn six_still_rejects() {
        let mut r = review(tx());
        select(&mut r, "  _1");
        r.handle(ButtonEvent::Confirm);
        assert_eq!(r.handle(ButtonEvent::Reject), Outcome::Rejected);
    }
}
