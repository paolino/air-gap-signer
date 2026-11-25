//! An in-memory RGB565 frame with anti-aliased monospace text.

use fontdue::{Font, FontSettings};

static DEJAVU_SANS_MONO: &[u8] = include_bytes!("../assets/DejaVuSansMono.ttf");
static DEJAVU_SANS_MONO_BOLD: &[u8] = include_bytes!("../assets/DejaVuSansMono-Bold.ttf");

/// An 8-bit-per-channel colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// Pack into RGB565.
    pub fn to_565(self) -> u16 {
        let Rgb(r, g, b) = self;
        ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3)
    }
}

/// Mix `fg` over `bg` by glyph `coverage` (0 = background, 255 = foreground).
pub fn blend(fg: Rgb, bg: Rgb, coverage: u8) -> u16 {
    let a = coverage as u16;
    let mix = |f: u8, b: u8| ((f as u16 * a + b as u16 * (255 - a) + 127) / 255) as u8;
    Rgb(mix(fg.0, bg.0), mix(fg.1, bg.1), mix(fg.2, bg.2)).to_565()
}

/// DejaVu Sans Mono at a fixed pixel size. Monospace keeps wrapping a
/// matter of counting characters.
pub struct TextFont {
    font: Font,
    px: f32,
    /// Baseline distance from the top of the line, in pixels.
    ascent: i32,
    /// Horizontal advance of every character, in pixels.
    pub advance: usize,
    /// Distance between baselines, in pixels.
    pub line_height: usize,
    /// How far below the top of its line any drawable glyph can ink, in
    /// pixels (exclusive): descenders and underscores can reach below
    /// `line_height`.
    pub ink_bottom: usize,
}

impl TextFont {
    pub fn mono(px: f32) -> Self {
        Self::load(DEJAVU_SANS_MONO, px)
    }

    /// The bold weight; same advance as `mono`, so columns line up.
    pub fn mono_bold(px: f32) -> Self {
        Self::load(DEJAVU_SANS_MONO_BOLD, px)
    }

    fn load(bytes: &'static [u8], px: f32) -> Self {
        let font = Font::from_bytes(bytes, FontSettings::default()).expect("bundled font parses");
        let line = font
            .horizontal_line_metrics(px)
            .expect("font has horizontal metrics");
        let advance = font.metrics('M', px).advance_width.round() as usize;
        let ascent = line.ascent.ceil() as i32;
        let ink_bottom = (' '..='~')
            .chain(['…', '›'])
            .map(|c| ascent - font.metrics(c, px).ymin)
            .max()
            .unwrap_or(ascent)
            .max(0) as usize;
        Self {
            font,
            px,
            ascent,
            ink_bottom,
            advance,
            line_height: line.new_line_size.ceil() as usize,
        }
    }
}

pub struct Frame {
    pub width: usize,
    pub height: usize,
    pixels: Vec<u16>,
}

impl Frame {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![0; width * height],
        }
    }

    pub fn pixel(&self, x: usize, y: usize) -> u16 {
        self.pixels[y * self.width + x]
    }

    pub fn fill(&mut self, colour: Rgb) {
        self.pixels.fill(colour.to_565());
    }

    /// Fill a rectangle, clipped to the frame.
    pub fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, colour: Rgb) {
        let c = colour.to_565();
        for py in y..(y + h).min(self.height) {
            for px in x..(x + w).min(self.width) {
                self.pixels[py * self.width + px] = c;
            }
        }
    }

    /// Draw `text` in one line whose top-left corner is pixel (`x`, `y`),
    /// blending onto `bg`. Pixels off the frame are clipped.
    pub fn draw_text(&mut self, font: &TextFont, text: &str, x: usize, y: usize, fg: Rgb, bg: Rgb) {
        let baseline = y as i32 + font.ascent;
        for (i, ch) in text.chars().enumerate() {
            let (m, coverage) = font.font.rasterize(ch, font.px);
            let left = (x + i * font.advance) as i32 + m.xmin;
            let top = baseline - m.height as i32 - m.ymin;
            for (j, &c) in coverage.iter().enumerate() {
                if c == 0 {
                    continue;
                }
                let px = left + (j % m.width) as i32;
                let py = top + (j / m.width) as i32;
                if px >= 0 && py >= 0 && (px as usize) < self.width && (py as usize) < self.height {
                    self.pixels[py as usize * self.width + px as usize] = blend(fg, bg, c);
                }
            }
        }
    }

    /// The frame as little-endian RGB565 bytes, row by row, the layout a
    /// 16-bit Linux framebuffer expects.
    pub fn to_le_bytes(&self) -> Vec<u8> {
        self.pixels.iter().flat_map(|p| p.to_le_bytes()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHITE: Rgb = Rgb(255, 255, 255);
    const BLACK: Rgb = Rgb(0, 0, 0);

    #[test]
    fn rgb565_primaries() {
        assert_eq!(Rgb(255, 0, 0).to_565(), 0xF800);
        assert_eq!(Rgb(0, 255, 0).to_565(), 0x07E0);
        assert_eq!(Rgb(0, 0, 255).to_565(), 0x001F);
        assert_eq!(WHITE.to_565(), 0xFFFF);
    }

    #[test]
    fn blend_endpoints_and_midpoint() {
        assert_eq!(blend(WHITE, BLACK, 255), 0xFFFF);
        assert_eq!(blend(WHITE, BLACK, 0), 0x0000);
        let mid = blend(WHITE, BLACK, 128);
        assert!(mid != 0 && mid != 0xFFFF);
    }

    #[test]
    fn bytes_are_little_endian_row_major() {
        let mut f = Frame::new(2, 2);
        f.pixels.copy_from_slice(&[0x0102, 0x0304, 0x0506, 0x0708]);
        assert_eq!(
            f.to_le_bytes(),
            vec![0x02, 0x01, 0x04, 0x03, 0x06, 0x05, 0x08, 0x07]
        );
    }

    #[test]
    fn ellipsis_has_a_visible_glyph() {
        let font = TextFont::mono(18.0);
        let mut f = Frame::new(20, 30);
        f.draw_text(&font, "…", 0, 0, WHITE, BLACK);
        assert!(f.pixels.iter().any(|&p| p != 0));
    }

    #[test]
    fn fill_rect_is_clipped() {
        let mut f = Frame::new(4, 4);
        f.fill_rect(2, 2, 10, 10, WHITE);
        assert_eq!(f.pixel(1, 1), 0);
        assert_eq!(f.pixel(2, 2), 0xFFFF);
        assert_eq!(f.pixel(3, 3), 0xFFFF);
    }

    #[test]
    fn eighteen_pixel_font_fits_twenty_columns_and_ten_rows() {
        let font = TextFont::mono(18.0);
        assert!(240 / font.advance >= 20, "advance {}", font.advance);
        assert!(240 / font.line_height >= 10, "line {}", font.line_height);
    }

    #[test]
    fn glyph_stays_inside_its_cell() {
        let font = TextFont::mono(18.0);
        let mut f = Frame::new(60, 40);
        f.draw_text(&font, " W", 0, 0, WHITE, BLACK);
        let mut inked = 0;
        for y in 0..f.height {
            for x in 0..f.width {
                if f.pixel(x, y) != 0 {
                    inked += 1;
                    assert!(x >= font.advance && x < 2 * font.advance, "x={x}");
                    assert!(y < font.line_height, "y={y}");
                }
            }
        }
        assert!(inked > 0);
    }

    #[test]
    fn space_draws_nothing() {
        let font = TextFont::mono(18.0);
        let mut f = Frame::new(40, 40);
        f.draw_text(&font, "  ", 0, 0, WHITE, BLACK);
        assert!(f.pixels.iter().all(|&p| p == 0));
    }

    #[test]
    fn text_off_the_frame_is_clipped_not_wrapped() {
        let font = TextFont::mono(18.0);
        let mut f = Frame::new(20, 20);
        f.draw_text(&font, "WWWW", 10, 10, WHITE, BLACK);
        // Ink only lands right of and below the start; nothing wraps
        // around to the left edge or the top.
        for y in 0..20 {
            for x in 0..20 {
                if x < 10 || y < 10 {
                    assert_eq!(f.pixel(x, y), 0, "({x},{y})");
                }
            }
        }
        assert!(f.pixels.iter().any(|&p| p != 0));
    }

    #[test]
    fn bold_has_the_same_advance_and_more_ink() {
        let (regular, bold) = (TextFont::mono(18.0), TextFont::mono_bold(18.0));
        assert_eq!(regular.advance, bold.advance);
        assert_eq!(regular.line_height, bold.line_height);
        let ink = |font: &TextFont| {
            let mut f = Frame::new(240, 30);
            f.draw_text(font, "outputs (2)", 0, 0, WHITE, BLACK);
            f.pixels.iter().map(|&p| (p >> 11) as u32).sum::<u32>()
        };
        assert!(
            ink(&bold) > ink(&regular) * 11 / 10,
            "bold must be visibly heavier"
        );
    }

    #[test]
    fn ink_bottom_covers_every_drawable_glyph() {
        for font in [TextFont::mono(18.0), TextFont::mono_bold(18.0)] {
            let mut f = Frame::new(30, 60);
            for ch in (' '..='~').chain(['…', '›']) {
                f.fill(BLACK);
                f.draw_text(&font, &ch.to_string(), 0, 0, WHITE, BLACK);
                for y in font.ink_bottom..60 {
                    for x in 0..30 {
                        assert_eq!(
                            f.pixel(x, y),
                            0,
                            "{ch:?} inks at y={y}, below {}",
                            font.ink_bottom
                        );
                    }
                }
            }
        }
    }
}
