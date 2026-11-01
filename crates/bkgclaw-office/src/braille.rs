//! Braille cells: eight pixels per character instead of two.
//!
//! # The upgrade, and what it costs
//!
//! `pixels.rs` packs two pixel rows into one character with `▀` (U+2580). That
//! gives a W×H pixel buffer in W×(H/2) cells. Braille gives **2×4** — eight
//! sub-pixels per character — so the same cell count carries four times the
//! vertical resolution:
//!
//! ```text
//!     ▀  half block   ▀   1 pixel wide  × 2 tall
//!     ⠿  braille      ⠿  2 pixels wide × 4 tall
//! ```
//!
//! A 100×50 terminal goes from a 100×100 pixel canvas to 200×200. For an
//! office full of 8×10 figures that is the difference between figures that
//! look like sprites and figures that look like blobs.
//!
//! # What it costs, stated honestly
//!
//! Braille is a *single colour per cell*. Half-blocks get two colours because
//! they use foreground *and* background; a braille cell has one foreground
//! colour and the background behind it. So a braille office draws its
//! figures in flat colour per cell, with the palette resolved per cell rather
//! than per pixel.
//!
//! That is a real trade, and for this office the right one: a figure is 8×10
//! pixels, which is 4 cells wide by 3 tall — four colours instead of sixteen.
//! The shading that makes half-blocks look good at desk level is exactly the
//! detail that a 10-row figure cannot afford anyway.
//!
//! Both renderers are kept. [`crate::pixels::Render`] picks between them, so
//! the office can be drawn either way — which is also how the tests check
//! that the two agree on geometry.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;

use crate::pixels::{Pixels, Rgb};

/// The base codepoint of the Braille Patterns block (U+2800).
///
/// Every braille cell is `0x2800 + bits`, where `bits` selects which of the
/// eight dot positions are raised. This is a published Unicode property, not
/// anyone's invention — which is why it can be reimplemented rather than
/// imported.
const BRAILLE_BASE: u32 = 0x2800;

/// The dot bit for one sub-pixel of a cell.
///
/// Braille columns are numbered left-to-right, rows top-to-bottom, and the bit
/// values are *not* in visual order — that is the one thing worth getting
/// right, because a wrong table produces glyphs that look almost right.
///
/// ```text
///      col 0   col 1
/// row 0   0x01   0x08
/// row 1   0x02   0x10
/// row 2   0x04   0x20
/// row 3   0x40   0x80
/// ```
const fn dot_bit(sub_x: u16, sub_y: u16) -> u8 {
    match (sub_x, sub_y) {
        (0, 0) => 0x01,
        (1, 0) => 0x08,
        (0, 1) => 0x02,
        (1, 1) => 0x10,
        (0, 2) => 0x04,
        (1, 2) => 0x20,
        (0, 3) => 0x40,
        (1, 3) => 0x80,
        _ => 0,
    }
}

/// One cell's worth of braille: which dots are up, and in what colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BrailleCell {
    dots: u8,
    colour: Option<Rgb>,
}

impl BrailleCell {
    /// Raise a dot.
    pub fn set(&mut self, sub_x: u16, sub_y: u16, colour: Rgb) {
        self.dots |= dot_bit(sub_x, sub_y);
        self.colour = Some(colour);
    }

    /// True when no dot is raised, so the cell should be left as a space.
    pub fn is_empty(&self) -> bool {
        self.dots == 0
    }

    /// The character this cell renders as.
    pub fn glyph(&self) -> char {
        char::from_u32(BRAILLE_BASE + self.dots as u32).unwrap_or(' ')
    }

    /// The colour the dots are drawn in.
    pub fn colour(&self) -> Option<Rgb> {
        self.colour
    }

    fn as_ratatui(self) -> Color {
        match self.colour {
            Some(rgb) => Color::Rgb(rgb.r, rgb.g, rgb.b),
            None => Color::Reset,
        }
    }
}

/// How a pixel buffer is packed into characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Render {
    /// Two pixel rows per character via `▀`, with two colours per cell.
    HalfBlock,
    /// Two columns × four rows per character via braille, one colour per cell.
    Braille,
}

impl Render {
    /// Pixel columns one character covers.
    pub fn sub_width(self) -> u16 {
        match self {
            Render::HalfBlock => 1,
            Render::Braille => 2,
        }
    }

    /// Pixel rows one character covers.
    pub fn sub_height(self) -> u16 {
        match self {
            Render::HalfBlock => 2,
            Render::Braille => 4,
        }
    }
}

/// Pack a pixel buffer into braille cells and write them into `buffer`.
///
/// The mapping: character `(cx, cy)` covers the 2×4 pixel block whose top-left
/// corner is `(cx*2, cy*4)`. Four times the pixel rows of a half-block, twice
/// the columns.
pub fn flush_braille(pixels: &Pixels, buffer: &mut Buffer, area: Rect) {
    for cy in 0..area.height {
        for cx in 0..area.width {
            let origin_x = cx * 2;
            let origin_y = cy * 4;
            let mut cell = BrailleCell::default();

            for sub_y in 0..4u16 {
                for sub_x in 0..2u16 {
                    if let Some(colour) = pixels.get(origin_x + sub_x, origin_y + sub_y) {
                        cell.set(sub_x, sub_y, colour);
                    }
                }
            }
            if cell.is_empty() {
                // Leave the cell as whatever the panel behind it drew. An empty
                // braille character is a blank glyph on an untouched cell, and
                // writing it would erase text the office is sharing a row with.
                continue;
            }

            let cell_x = area.x + cx;
            let cell_y = area.y + cy;
            if cell_x >= buffer.area.right() || cell_y >= buffer.area.bottom() {
                continue;
            }
            let target = &mut buffer[(cell_x, cell_y)];
            // `set_symbol` takes a `&str`, and the borrow has to outlive the
            // call — a `&str` into a local buffer would point at a dropped
            // stack frame by the time ratatui reads it.
            let mut encoded = [0u8; 4];
            let glyph = cell.glyph().encode_utf8(&mut encoded);
            target.set_symbol(glyph);
            target.set_fg(cell.as_ratatui());
        }
    }
}

/// Flush with whichever renderer the caller chose.
pub fn flush(pixels: &Pixels, buffer: &mut Buffer, area: Rect, render: Render) {
    match render {
        Render::HalfBlock => pixels.flush_into(buffer, area),
        Render::Braille => flush_braille(pixels, buffer, area),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn buffer_of(width: u16, height: u16) -> Buffer {
        Buffer::empty(Rect::new(0, 0, width, height))
    }

    #[test]
    fn one_dot_is_the_expected_glyph() {
        // U+2801 is the braille cell with only the top-left dot raised. If the
        // bit table is wrong this comes out as some other dot.
        let mut cell = BrailleCell::default();
        cell.set(0, 0, Rgb::new(255, 255, 255));
        assert_eq!(cell.glyph(), '\u{2801}');
    }

    #[test]
    fn the_dot_table_matches_the_unicode_block() {
        // Every one of the eight positions, against its published codepoint.
        let expected = [
            ((0u16, 0u16), '\u{2801}'),
            ((1, 0), '\u{2808}'),
            ((0, 1), '\u{2802}'),
            ((1, 1), '\u{2810}'),
            ((0, 2), '\u{2804}'),
            ((1, 2), '\u{2820}'),
            ((0, 3), '\u{2840}'),
            ((1, 3), '\u{2880}'),
        ];
        for ((x, y), glyph) in expected {
            let mut cell = BrailleCell::default();
            cell.set(x, y, Rgb::new(1, 2, 3));
            assert_eq!(cell.glyph(), glyph, "dot at ({x},{y})");
        }
    }

    #[test]
    fn all_eight_dots_give_the_full_block() {
        let mut cell = BrailleCell::default();
        for y in 0..4 {
            for x in 0..2 {
                cell.set(x, y, Rgb::new(9, 9, 9));
            }
        }
        assert_eq!(cell.glyph(), '\u{28FF}');
        assert!(!cell.is_empty());
    }

    #[test]
    fn an_empty_cell_renders_nothing_at_all() {
        let cell = BrailleCell::default();
        assert!(cell.is_empty());
        // It must never write a blank glyph over a cell that might hold text.
        let mut buffer = buffer_of(1, 1);
        buffer[(0, 0)].set_symbol("X");
        let pixels = Pixels::new(2, 4);
        flush_braille(&pixels, &mut buffer, Rect::new(0, 0, 1, 1));
        assert_eq!(buffer[(0, 0)].symbol(), "X", "the cell was left alone");
    }

    #[test]
    fn braille_carries_four_times_the_pixel_rows() {
        assert_eq!(Render::Braille.sub_height(), 4);
        assert_eq!(Render::HalfBlock.sub_height(), 2);
        assert_eq!(Render::Braille.sub_width(), 2);
        assert_eq!(Render::HalfBlock.sub_width(), 1);
    }

    #[test]
    fn one_cell_holds_four_pixel_rows_of_content() {
        let mut pixels = Pixels::new(2, 4);
        for y in 0..4 {
            pixels.set(0, y, Some(Rgb::new(7, 7, 7)));
        }
        let mut buffer = buffer_of(1, 1);
        flush_braille(&pixels, &mut buffer, Rect::new(0, 0, 1, 1));
        // Four rows of the left column → the four left dots: 0x01|0x02|0x04|0x40.
        assert_eq!(buffer[(0, 0)].symbol(), "\u{2847}");
    }

    #[test]
    fn both_renderers_fill_the_same_area_without_overrunning() {
        let mut pixels = Pixels::new(20, 20);
        for y in 0..20 {
            for x in 0..20 {
                pixels.set(x, y, Some(Rgb::new((x * 8) as u8, (y * 8) as u8, 128)));
            }
        }
        for render in [Render::HalfBlock, Render::Braille] {
            let mut buffer = buffer_of(6, 4);
            flush(&pixels, &mut buffer, Rect::new(0, 0, 6, 4), render);
            for y in 0..4 {
                for x in 0..6 {
                    assert_ne!(
                        buffer[(x, y)].symbol(),
                        " ",
                        "{render:?} left a hole at {x},{y}"
                    );
                }
            }
        }
    }

    #[test]
    fn a_zero_sized_area_is_harmless() {
        let pixels = Pixels::new(0, 0);
        let mut buffer = buffer_of(1, 1);
        flush(&pixels, &mut buffer, Rect::new(0, 0, 0, 0), Render::Braille);
    }
}
