//! The office's picture layer: pixels, sprites, and the bridge to ratatui.
//!
//! # The idea this whole crate rests on
//!
//! A terminal cell is one character, and a character has one foreground colour.
//! That sounds like a hard limit on drawing. It is not, because of one glyph:
//!
//! ```text
//! ▀   U+2580, UPPER HALF BLOCK
//! ```
//!
//! In that single character the terminal gives you **two** independently
//! coloured pixels stacked vertically — the top half takes the foreground
//! colour, the bottom half the background. So a W×H pixel buffer becomes
//! W×(H/2) cells, and the office renders at twice the vertical resolution of a
//! normal text interface while still being a plain ratatui widget.
//!
//! # Why this instead of a ratatui widget
//!
//! Because the office is not a widget — it is a *buffer* that gets flushed.
//! That distinction is what lets it sit next to a transcript, a sessions list
//! and an input box in one layout: it occupies a `Rect` and writes cells, the
//! same contract every other widget honours. An ASCII-art widget built from
//! `Line`s could never carry real colour per pixel; this can, and it costs 35
//! lines of code.
//!
//! The alternative — Sixel or Kitty graphics — draws *actual* pixels with real
//! resolution, and pixtuoid implements it. It also requires terminal support
//! that most terminals do not have, and it cannot be scrolled, selected from or
//! composited with the rest of a TUI. Half-blocks degrade gracefully; that is
//! the deciding factor.

use std::collections::HashMap;

// ratatui exists only behind the `terminal` feature: the browser cockpit
// compiles this crate for wasm32, and crossterm does not exist there.
#[cfg(feature = "terminal")]
use ratatui::buffer::Buffer;
#[cfg(feature = "terminal")]
use ratatui::layout::Rect;

use crate::figure::Figure;

/// The half-block glyph. Two pixel rows live inside one cell.
#[cfg(feature = "terminal")]
pub const HALF_BLOCK: &str = "\u{2580}";

/// One pixel: 24-bit colour, or transparent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Rgb { r, g, b }
    }

    /// Blend towards `other` by `t` (0.0 = self, 1.0 = other).
    ///
    /// Used for the "lit screen" glow: a desk that is being worked on gets its
    /// monitor lightened rather than swapped for a different colour, which
    /// reads as the same object getting brighter.
    pub fn blend(self, other: Rgb, t: f32) -> Rgb {
        let t = t.clamp(0.0, 1.0);
        Rgb {
            r: (self.r as f32 + (other.r as f32 - self.r as f32) * t) as u8,
            g: (self.g as f32 + (other.g as f32 - self.g as f32) * t) as u8,
            b: (self.b as f32 + (other.b as f32 - self.b as f32) * t) as u8,
        }
    }

    /// Lift every channel toward white. The cheap version of a glow.
    pub fn brighten(self, amount: f32) -> Rgb {
        self.blend(Rgb::new(255, 255, 255), amount)
    }
}

/// A fixed-size pixel canvas.
///
/// Fixed, not growing: the office knows how large it wants to be before it
/// draws, so the buffer is allocated once and reused every frame. An office
/// that reallocated per frame would produce a fresh `Vec` per repaint at 30
/// frames a second.
#[derive(Debug, Clone)]
pub struct Pixels {
    width: u16,
    height: u16,
    /// One entry per pixel, row-major. `None` is transparent — the cell behind
    /// it shows through, which is what makes the office sit on the terminal's
    /// own background instead of painting an opaque box.
    cells: Vec<Option<Rgb>>,
}

impl Pixels {
    pub fn new(width: u16, height: u16) -> Self {
        Pixels {
            width,
            height,
            cells: vec![None; width as usize * height as usize],
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    pub fn get(&self, x: u16, y: u16) -> Option<Rgb> {
        if x >= self.width || y >= self.height {
            return None;
        }
        self.cells[y as usize * self.width as usize + x as usize]
    }

    /// Set a pixel. Out-of-bounds writes are dropped, not panics — a sprite
    /// clipped at the edge of the floor is a normal event, not a bug.
    pub fn set(&mut self, x: u16, y: u16, colour: Option<Rgb>) {
        if x >= self.width || y >= self.height {
            return;
        }
        let index = y as usize * self.width as usize + x as usize;
        self.cells[index] = colour;
    }

    pub fn fill(&mut self, colour: Option<Rgb>) {
        self.cells.fill(colour);
    }

    pub fn clear(&mut self) {
        self.cells.fill(None);
    }

    /// Draw a figure with its top-left corner at `(x, y)`.
    ///
    /// `tint` is consulted only for the figure's personal slots, so the desk
    /// and the agent's shirt come out of one call with different rules.
    /// A symbol with no palette entry is skipped rather than treated as an
    /// error: a typo in one figure should cost one pixel, not the frame.
    pub fn draw_figure(
        &mut self,
        figure: &Figure,
        x: u16,
        y: u16,
        palette: &HashMap<char, Option<Rgb>>,
        personal: Option<(&dyn Fn(char) -> Rgb, bool)>,
    ) {
        for (row_index, row) in figure.rows.iter().enumerate() {
            for (column, symbol) in row.chars().enumerate() {
                let Some(Some(base)) = palette.get(&symbol).copied() else {
                    continue;
                };
                let colour = match personal {
                    // The `bool` says whether this figure has personal slots at
                    // all: the desk carries none, so asking for a tint for it
                    // would borrow another agent's shirt colour.
                    Some((tint, true)) if figure.personal.contains(&symbol) => tint(symbol),
                    _ => base,
                };
                self.set(
                    x.saturating_add(column as u16),
                    y.saturating_add(row_index as u16),
                    Some(colour),
                );
            }
        }
    }

    /// Write this canvas into a ratatui buffer, two pixel rows per cell.
    ///
    /// # The mapping
    ///
    /// Cell `(x, y)` covers pixels `y*2` (foreground, top) and `y*2 + 1`
    /// (background, bottom). That mapping is the whole trick — read it once
    /// and the rest is bookkeeping:
    ///
    /// ```text
    /// pixel row 0 ─┐
    ///              ├─ cell row 0 : fg = row 0, bg = row 1
    /// pixel row 1 ─┘
    /// pixel row 2 ─┐
    ///              ├─ cell row 1 : fg = row 2, bg = row 3
    /// pixel row 3 ─┘
    /// ```
    ///
    /// An odd final row has no partner; it renders as a cell whose background
    /// is transparent, which leaves the terminal's own colour showing.
    #[cfg(feature = "terminal")]
    pub fn flush_into(&self, buffer: &mut Buffer, area: Rect) {
        for cy in 0..area.height {
            for cx in 0..area.width {
                let cell_x = area.x + cx;
                let cell_y = area.y + cy;
                if cell_x >= buffer.area.right() || cell_y >= buffer.area.bottom() {
                    continue;
                }
                let top = self.get(cx, cy * 2);
                let bottom = self.get(cx, cy * 2 + 1);

                let (fg, bg) = match (top, bottom) {
                    (None, None) => continue,
                    (Some(upper), None) => (upper, upper),
                    (None, Some(lower)) => (lower, lower),
                    (Some(upper), Some(lower)) => (upper, lower),
                };
                let cell = &mut buffer[(cell_x, cell_y)];
                cell.set_symbol(HALF_BLOCK);
                cell.set_fg(ratatui::style::Color::Rgb(fg.r, fg.g, fg.b));
                cell.set_bg(ratatui::style::Color::Rgb(bg.r, bg.g, bg.b));
            }
        }
    }
}

/// The cell height a pixel canvas needs to fill a given number of text rows.
///
/// `flush_into` packs two pixel rows per cell, so a `height`-row area holds
/// `2*height` pixel rows.
#[cfg(feature = "terminal")]
pub fn cells_for_pixels(pixel_rows: u16) -> u16 {
    pixel_rows.div_ceil(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_outside_the_buffer_are_dropped_not_fatal() {
        // A sprite clipped at the floor edge is normal. A panic there would
        // kill the TUI on a resize.
        let mut pixels = Pixels::new(2, 2);
        pixels.set(99, 99, Some(Rgb::new(255, 255, 255)));
        pixels.set(u16::MAX, 0, Some(Rgb::new(1, 1, 1)));
    }

    #[test]
    fn blend_stays_inside_the_byte_range() {
        let base = Rgb::new(0, 0, 0);
        let white = Rgb::new(255, 255, 255);
        // Out-of-range factors must clamp, not wrap. A wrapped u8 is a wildly
        // wrong colour and would look like a rendering bug, not a math one.
        assert_eq!(base.blend(white, -5.0), base);
        assert_eq!(base.blend(white, 5.0), white);
        let mid = base.blend(white, 0.5);
        assert!((120..=135).contains(&mid.r), "{mid:?}");
    }

    /// Everything that writes into a ratatui `Buffer` — the terminal half of
    /// this crate, compiled only when the terminal renderer exists.
    #[cfg(feature = "terminal")]
    mod flush_into {
        use super::*;
        use ratatui::style::Color;

        fn buffer_of(width: u16, height: u16) -> Buffer {
            Buffer::empty(Rect::new(0, 0, width, height))
        }

        #[test]
        fn two_pixel_rows_land_in_one_cell() {
            let mut pixels = Pixels::new(1, 2);
            pixels.set(0, 0, Some(Rgb::new(255, 0, 0)));
            pixels.set(0, 1, Some(Rgb::new(0, 0, 255)));

            let mut buffer = buffer_of(1, 1);
            pixels.flush_into(&mut buffer, Rect::new(0, 0, 1, 1));

            let cell = &buffer[(0, 0)];
            assert_eq!(cell.symbol(), HALF_BLOCK);
            assert_eq!(cell.fg, Color::Rgb(255, 0, 0), "top pixel is foreground");
            assert_eq!(cell.bg, Color::Rgb(0, 0, 255), "bottom pixel is background");
        }

        #[test]
        fn an_empty_pixel_leaves_the_cell_alone() {
            // The common case: most of the office is empty. Touching those cells
            // would paint the office's background over the panel behind it.
            let pixels = Pixels::new(4, 4);
            let mut buffer = buffer_of(2, 2);
            pixels.flush_into(&mut buffer, Rect::new(0, 0, 2, 2));
            for cy in 0..2 {
                for cx in 0..2 {
                    assert_eq!(buffer[(cx, cy)].symbol(), " ", "cell {cx},{cy} untouched");
                }
            }
        }

        #[test]
        fn a_single_pixel_paints_its_whole_cell() {
            // One pixel, no partner: the cell takes that colour twice, so the dot
            // is still visible instead of collapsing into a space.
            let mut pixels = Pixels::new(1, 1);
            pixels.set(0, 0, Some(Rgb::new(1, 2, 3)));
            let mut buffer = buffer_of(1, 1);
            pixels.flush_into(&mut buffer, Rect::new(0, 0, 1, 1));
            assert_eq!(buffer[(0, 0)].fg, Color::Rgb(1, 2, 3));
        }

        #[test]
        fn flush_never_writes_past_the_area() {
            // A fully opaque canvas, so every cell inside the area must draw.
            let mut pixels = Pixels::new(8, 8);
            for y in 0..8 {
                for x in 0..8 {
                    pixels.set(x, y, Some(Rgb::new(9, 9, 9)));
                }
            }
            // A 2×1 area starting at (1,1) inside a 4×4 buffer. The flush maps cell
            // (cx,cy) to pixel (cx,cy*2) — relative to the area, not the buffer —
            // so this covers buffer cells (1,1) and (2,1) only.
            let mut buffer = buffer_of(4, 4);
            pixels.flush_into(&mut buffer, Rect::new(1, 1, 2, 1));
            assert_eq!(buffer[(0, 0)].symbol(), " ", "outside, left");
            assert_eq!(buffer[(3, 1)].symbol(), " ", "outside, right");
            assert_eq!(buffer[(1, 0)].symbol(), " ", "outside, above");
            assert_eq!(buffer[(1, 1)].symbol(), HALF_BLOCK, "inside drew");
            assert_eq!(buffer[(2, 1)].symbol(), HALF_BLOCK, "inside drew");
        }

        #[test]
        fn an_odd_pixel_row_still_renders() {
            // 3 pixel rows is 2 cells with one orphan row. It must not panic and
            // must not lose the orphan.
            let mut pixels = Pixels::new(1, 3);
            pixels.set(0, 0, Some(Rgb::new(10, 10, 10)));
            pixels.set(0, 2, Some(Rgb::new(20, 20, 20)));
            let mut buffer = buffer_of(1, 2);
            pixels.flush_into(&mut buffer, Rect::new(0, 0, 1, 2));
            assert_eq!(buffer[(0, 0)].fg, Color::Rgb(10, 10, 10));
            assert_eq!(buffer[(0, 1)].fg, Color::Rgb(20, 20, 20));
        }

        #[test]
        fn cells_for_pixels_rounds_up() {
            assert_eq!(cells_for_pixels(4), 2);
            assert_eq!(cells_for_pixels(5), 3);
            assert_eq!(cells_for_pixels(0), 0);
        }
    }
}
