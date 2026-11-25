//! `Display` on a Linux framebuffer.

use crate::frame::{Frame, Rgb, TextFont};
use crate::layout::{wrap_lines, wrap_message};
use signer_core::display::DisplayLine;
use signer_core::review::{View, MIN_COLS, MIN_ROWS};
use signer_hal::HalError;
use std::fs::{File, OpenOptions};
use std::os::unix::fs::{FileExt, FileTypeExt};
use std::path::{Path, PathBuf};

const SIZE: usize = 240;

/// Find the framebuffer device whose driver name is `name` by scanning
/// `<sysfs>/fb*/name`. Linux numbers framebuffers in registration order,
/// so the index is not fixed.
pub fn find_framebuffer(sysfs: &Path, name: &str) -> Option<PathBuf> {
    let mut entries: Vec<_> = std::fs::read_dir(sysfs).ok()?.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    entries.into_iter().find_map(|e| {
        let dev = e.file_name().into_string().ok()?;
        let found = std::fs::read_to_string(e.path().join("name")).ok()?;
        (dev.starts_with("fb") && found.trim() == name).then(|| PathBuf::from("/dev").join(dev))
    })
}

/// Check, from its sysfs directory, that a framebuffer is the 240×240
/// RGB565 panel this display draws for.
pub fn check_geometry(fb_sysfs: &Path) -> Result<(), HalError> {
    let expected = [
        ("bits_per_pixel", "16".to_string()),
        ("virtual_size", format!("{SIZE},{SIZE}")),
        ("stride", (SIZE * 2).to_string()),
    ];
    for (attr, want) in expected {
        let path = fb_sysfs.join(attr);
        let got = std::fs::read_to_string(&path)
            .map_err(|e| HalError::Display(format!("{}: {e}", path.display())))?;
        if got.trim() != want {
            return Err(HalError::Display(format!(
                "{}: expected {want}, found {}",
                path.display(),
                got.trim()
            )));
        }
    }
    Ok(())
}

/// The bonnet's display, driven through its framebuffer device.
pub struct FbDisplay {
    device: PathBuf,
    file: File,
    frame: Frame,
    font: TextFont,
    /// Groups are drawn in bold.
    bold: TextFont,
    fg: Rgb,
    bg: Rgb,
    accent: Rgb,
    /// Rows that still need attention before Confirm counts.
    pending: Rgb,
    /// Background of the review cursor row.
    selected: Rgb,
}

impl FbDisplay {
    /// Open an existing framebuffer `device` (never creating a file) and
    /// check its geometry under `sysfs`. `px` is the font size; 18 gives
    /// 22 columns and 11 rows.
    pub fn open(sysfs: &Path, device: &Path, px: f32) -> Result<Self, HalError> {
        if !(8.0..=60.0).contains(&px) {
            return Err(HalError::Display(format!(
                "font size {px} is outside 8..=60 pixels"
            )));
        }
        let err = |e: std::io::Error| HalError::Display(format!("{}: {e}", device.display()));
        let file = OpenOptions::new().write(true).open(device).map_err(err)?;
        if !file.metadata().map_err(err)?.file_type().is_char_device() {
            return Err(HalError::Display(format!(
                "{} is not a framebuffer device",
                device.display()
            )));
        }
        let name = device
            .file_name()
            .ok_or_else(|| HalError::Display(format!("{}: no device name", device.display())))?;
        check_geometry(&sysfs.join(name))?;
        let display = Self::with_file(device.to_path_buf(), file, px);
        let (cols, rows) = (display.cols(), display.rows());
        if cols < MIN_COLS || rows < MIN_ROWS {
            return Err(HalError::Display(format!(
                "font size {px} gives a {cols}x{rows} grid; need at least {MIN_COLS}x{MIN_ROWS}"
            )));
        }
        Ok(display)
    }

    fn with_file(device: PathBuf, file: File, px: f32) -> Self {
        Self {
            device,
            file,
            frame: Frame::new(SIZE, SIZE),
            font: TextFont::mono(px),
            bold: TextFont::mono_bold(px),
            fg: Rgb(240, 240, 240),
            bg: Rgb(0, 0, 0),
            accent: Rgb(250, 190, 40),
            pending: Rgb(255, 110, 100),
            selected: Rgb(235, 235, 235),
        }
    }

    fn cols(&self) -> usize {
        SIZE / self.font.advance
    }

    /// Rows whose glyphs, descenders included, all land on the panel.
    fn rows(&self) -> usize {
        let ink = self.font.ink_bottom.max(self.bold.ink_bottom);
        match SIZE.checked_sub(ink) {
            Some(room) => room / self.font.line_height + 1,
            None => 0,
        }
    }

    fn text(&mut self, text: &str, col: usize, row: usize, colour: Rgb) {
        let (x, y) = (col * self.font.advance, row * self.font.line_height);
        self.frame
            .draw_text(&self.font, text, x, y, colour, self.bg);
    }

    fn flush(&self) -> Result<(), HalError> {
        self.file
            .write_all_at(&self.frame.to_le_bytes(), 0)
            .map_err(|e| HalError::Display(format!("{}: {e}", self.device.display())))
    }
}

impl signer_hal::Display for FbDisplay {
    fn clear(&mut self) -> Result<(), HalError> {
        self.frame.fill(self.bg);
        self.flush()
    }

    fn show_message(&mut self, text: &str) -> Result<(), HalError> {
        self.frame.fill(self.bg);
        let (cols, max_rows) = (self.cols(), self.rows());
        let mut rows = wrap_message(text, cols);
        if rows.len() > max_rows {
            // Never drop text silently: the last row ends with `…`.
            let rest = rows[max_rows - 1..].join(" ");
            rows.truncate(max_rows - 1);
            let mut last: String = rest.chars().take(cols - 1).collect();
            last.push('…');
            rows.push(last);
        }
        let top = max_rows.saturating_sub(rows.len()) / 2;
        for (i, row) in rows.iter().enumerate() {
            let col = (cols - row.chars().count()) / 2;
            self.text(row, col, top + i, self.fg);
        }
        self.flush()
    }

    /// `scroll_offset` counts display lines, as the flow does; the first
    /// shown line starts at the top and wrapped values flow below it.
    fn show_lines(&mut self, lines: &[DisplayLine], scroll_offset: usize) -> Result<(), HalError> {
        self.frame.fill(self.bg);
        let header = format!("{}/{} 5=OK 6=NO", scroll_offset + 1, lines.len().max(1));
        self.text(&header, 0, 0, self.accent);
        let rows = wrap_lines(lines.get(scroll_offset..).unwrap_or(&[]), self.cols());
        for (i, row) in rows.iter().take(self.rows() - 1).enumerate() {
            self.text(row, 0, i + 1, self.fg);
        }
        self.flush()
    }

    fn text_grid(&self) -> (usize, usize) {
        (self.cols(), self.rows())
    }

    fn show_review(&mut self, view: &View) -> Result<(), HalError> {
        self.frame.fill(self.bg);
        match view {
            View::Overview { header, rows } => {
                self.text(header, 0, 0, self.accent);
                let h = self.font.line_height;
                for (i, row) in rows.iter().enumerate() {
                    let y = (i + 1) * h;
                    let bg = if row.selected { self.selected } else { self.bg };
                    if row.selected {
                        self.frame.fill_rect(0, y, SIZE, h, bg);
                    }
                    let font = if row.expandable {
                        &self.bold
                    } else {
                        &self.font
                    };
                    // On the light cursor bar, text is black, or dark red
                    // while it still has to be seen.
                    let colour = match (row.selected, row.pending) {
                        (true, true) => Rgb(170, 0, 0),
                        (true, false) => Rgb(0, 0, 0),
                        (false, true) => self.pending,
                        (false, false) => self.fg,
                    };
                    self.frame.draw_text(font, &row.text, 0, y, colour, bg);
                }
            }
            View::Detail { header, rows } => {
                self.text(header, 0, 0, self.accent);
                for (i, row) in rows.iter().enumerate() {
                    self.text(row, 0, i + 1, self.fg);
                }
            }
        }
        self.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn finds_framebuffer_by_driver_name() {
        let dir = tempfile::tempdir().unwrap();
        for (fb, name) in [("fb0", "vc4drmfb"), ("fb1", "fb_st7789v")] {
            fs::create_dir(dir.path().join(fb)).unwrap();
            fs::write(dir.path().join(fb).join("name"), format!("{name}\n")).unwrap();
        }
        assert_eq!(
            find_framebuffer(dir.path(), "fb_st7789v"),
            Some(PathBuf::from("/dev/fb1"))
        );
        assert_eq!(find_framebuffer(dir.path(), "missing"), None);
    }

    fn fake_fb(geometry: [(&str, &str); 3]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (file, value) in geometry {
            fs::write(dir.path().join(file), format!("{value}\n")).unwrap();
        }
        dir
    }

    #[test]
    fn accepts_the_panel_geometry() {
        let fb = fake_fb([
            ("bits_per_pixel", "16"),
            ("virtual_size", "240,240"),
            ("stride", "480"),
        ]);
        assert!(check_geometry(fb.path()).is_ok());
    }

    #[test]
    fn rejects_other_geometries() {
        for bad in [
            [
                ("bits_per_pixel", "32"),
                ("virtual_size", "240,240"),
                ("stride", "960"),
            ],
            [
                ("bits_per_pixel", "16"),
                ("virtual_size", "1920,1080"),
                ("stride", "3840"),
            ],
            [
                ("bits_per_pixel", "16"),
                ("virtual_size", "240,240"),
                ("stride", "512"),
            ],
        ] {
            let fb = fake_fb(bad);
            assert!(check_geometry(fb.path()).is_err(), "{bad:?}");
        }
        assert!(check_geometry(Path::new("/nonexistent/fb9")).is_err());
    }

    #[test]
    fn review_cursor_row_is_highlighted() {
        use signer_core::review::Row;
        use signer_hal::Display;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frame");
        let file = File::create(&path).unwrap();
        let mut d = FbDisplay::with_file(path.clone(), file, 18.0);
        let row = |text: &str, selected| Row {
            text: text.into(),
            selected,
            expandable: false,
            pending: false,
        };
        let view = View::Overview {
            header: "h".into(),
            rows: vec![row("a", false), row("b", true)],
        };
        d.show_review(&view).unwrap();
        let bytes = fs::read(&path).unwrap();
        let at = |x: usize, y: usize| {
            u16::from_le_bytes([bytes[2 * (y * SIZE + x)], bytes[2 * (y * SIZE + x) + 1]])
        };
        let h = d.font.line_height;
        let selected = d.selected.to_565();
        assert_eq!(
            at(SIZE - 1, 2 * h + 1),
            selected,
            "row 2 (cursor) is highlighted"
        );
        assert_eq!(at(SIZE - 1, h + 1), d.bg.to_565(), "row 1 is not");
    }

    #[test]
    fn a_message_too_long_for_the_screen_ends_with_an_ellipsis() {
        use signer_hal::Display;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frame");
        let mut d = FbDisplay::with_file(path.clone(), File::create(&path).unwrap(), 18.0);
        let long = "word ".repeat(200);
        d.show_message(&long).unwrap();
        // The bottom text row is inked up to its last cell, where `…` sits.
        let bytes = fs::read(&path).unwrap();
        let h = d.font.line_height;
        let last_row = d.rows() - 1;
        let cell_x = (d.cols() - 1) * d.font.advance;
        let inked = (last_row * h..(last_row + 1) * h).any(|y| {
            (cell_x..cell_x + d.font.advance).any(|x| {
                let i = 2 * (y * SIZE + x);
                u16::from_le_bytes([bytes[i], bytes[i + 1]]) != d.bg.to_565()
            })
        });
        assert!(inked);
    }

    #[test]
    fn pending_rows_are_drawn_in_their_own_colour() {
        use signer_core::review::Row;
        use signer_hal::Display;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frame");
        let mut d = FbDisplay::with_file(path.clone(), File::create(&path).unwrap(), 18.0);
        let row = |text: &str, pending| Row {
            text: text.into(),
            selected: false,
            expandable: false,
            pending,
        };
        let view = View::Overview {
            header: "h".into(),
            rows: vec![row("done", false), row("todo", true)],
        };
        d.show_review(&view).unwrap();
        let bytes = fs::read(&path).unwrap();
        let h = d.font.line_height;
        let band = |r: usize| -> Vec<u16> {
            (r * h..(r + 1) * h)
                .flat_map(|y| (0..SIZE).map(move |x| (x, y)))
                .map(|(x, y)| {
                    u16::from_le_bytes([bytes[2 * (y * SIZE + x)], bytes[2 * (y * SIZE + x) + 1]])
                })
                .collect()
        };
        assert!(
            band(1).contains(&d.fg.to_565()),
            "a done row is drawn in the text colour"
        );
        assert!(
            band(2).contains(&d.pending.to_565()),
            "a pending row is drawn in the pending colour"
        );
        assert!(!band(2).contains(&d.fg.to_565()));
    }

    #[test]
    fn the_last_row_is_never_clipped_at_any_font_size() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("frame");
        // Every whole size, plus the one a review found clipping.
        for px in (8..=60).map(|p| p as f32).chain([10.2]) {
            let d = FbDisplay::with_file(path.clone(), File::create(&path).unwrap(), px);
            let last_top = (d.rows() - 1) * d.font.line_height;
            for font in [&d.font, &d.bold] {
                assert!(
                    last_top + font.ink_bottom <= SIZE,
                    "size {px}: last row inks down to {}",
                    last_top + font.ink_bottom
                );
            }
        }
    }

    #[test]
    fn refuses_a_path_that_is_not_a_device() {
        let dir = tempfile::tempdir().unwrap();
        let typo = dir.path().join("fb1");
        assert!(FbDisplay::open(dir.path(), &typo, 18.0).is_err());
        assert!(!typo.exists(), "open must not create the file");
        assert!(
            FbDisplay::open(dir.path(), &typo, 0.0).is_err(),
            "no divide by zero"
        );
        fs::write(&typo, b"").unwrap();
        assert!(FbDisplay::open(dir.path(), &typo, 18.0).is_err());
    }
}
