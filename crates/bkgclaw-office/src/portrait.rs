//! One agent, drawn: the desk, the figure in front of it, the marker above.
//!
//! # Why a second composer when `scene` exists
//!
//! `scene::paint` composes the whole office *floor* and writes into a
//! ratatui buffer — it cannot compile for wasm32, so the browser cockpit
//! cannot call it. But the rules of the compose are the office's identity:
//! desk first, or the desk covers the agent from the waist down; glow on the
//! screen row, or a working monitor looks dark; the marker above the head,
//! or the one state that needs a human is invisible.
//!
//! So the rules live here, target-neutral, painting into a plain
//! [`Pixels`] canvas. The floor scene calls them per desk; the browser strip
//! calls [`portrait`] once. One implementation, two canvases — the picture
//! cannot drift between the terminal and the web.

use crate::figure;
use crate::pixels::{Pixels, Rgb};
use crate::room::{Office, Pose};

/// The glow a working monitor throws onto its desk.
pub const GLOW: Rgb = Rgb::new(120, 200, 255);

/// Draw one agent — desk, figure, marker — with the desk's top-left at
/// `(x, y)`.
///
/// `now_ms` and `beat` decide the pose exactly as the floor does, so the
/// same agent state produces the same figure here and in the TUI strip.
pub fn draw_agent(
    pixels: &mut Pixels,
    office: &Office,
    index: usize,
    x: u16,
    y: u16,
    now_ms: u64,
    beat: u64,
) {
    if office.agents().get(index).is_none() {
        return;
    }
    let pose = office.pose_for(index, now_ms, beat);
    if pose == Pose::Absent {
        return;
    }
    let working = matches!(pose, Pose::Typing { .. });

    // Draw order is fixed and is the whole point of this function: desk,
    // glow, figure, marker. The figure sits *five* rows down — far enough
    // that the monitor at desk row 2 stays visible above the agent's head
    // (the first compose put the head one row below the desk corner and hid
    // the screen behind the hair, so the glow brightened pixels nobody could
    // see). Five also lands the typing hands on rows 14-15, and a braille
    // cell carries the colour of its last sub-row — so the frame the hands
    // alternate in stays visible in the terminal, not only in the pixels.
    let palette = crate::sprite::default_palette();
    pixels.draw_figure(&figure::DESK, x, y, &palette, None);
    if working {
        draw_screen_glow(pixels, x, y);
    }
    if let Some(agent_figure) = figure::by_name(pose.figure_name()) {
        let tint = |symbol: char| office.colour_for(index, symbol);
        pixels.draw_figure(
            agent_figure,
            x + 1,
            y + FIGURE_DROP,
            &palette,
            Some((&tint, true)),
        );
    }
    draw_marker(pixels, pose, x, y);
}

/// How far the figure sits below the desk's top edge, in pixels.
///
/// Five, for two reasons that both have to hold:
///
/// 1. **The monitor stays visible.** The screen is desk row 2; the figure's
///    first rows are hair. Below three rows of drop the head covers the
///    screen and the working glow lights pixels nobody can see.
/// 2. **The typing animation survives braille.** A braille cell is one
///    colour — the colour of its *last* sub-row. The hands alternate in the
///    figure's last two rows; at drop 5 those land on canvas rows 14-15,
///    and row 15 is a cell's last sub-row (15 mod 4 == 3), so the frame
///    change reaches the cell colour instead of dissolving into it.
const FIGURE_DROP: u16 = 5;

/// A one-agent strip on a transparent canvas — the shape a web client shows.
///
/// The canvas is exactly as large as the drawing needs: the desk is eight
/// pixels wide, the figure sits one pixel right of it, and a `?` marker may
/// reach two pixels past the figure. Everything outside stays transparent,
/// so the strip composites onto any background.
pub fn portrait(office: &Office, index: usize, now_ms: u64, beat: u64) -> Pixels {
    let figure_height = figure::all()
        .iter()
        .map(|agent_figure| agent_figure.rows.len())
        .max()
        .unwrap_or(10) as u16;
    let mut pixels = Pixels::new(MARKER_OFFSET + 3, FIGURE_DROP + figure_height);
    draw_agent(&mut pixels, office, index, 0, 0, now_ms, beat);
    pixels
}

/// Light the monitor of a working agent.
///
/// The desk's screen row, brightened rather than replaced: the same object
/// getting lighter reads better than a colour swap.
fn draw_screen_glow(pixels: &mut Pixels, x: u16, y: u16) {
    for dx in 2..8u16 {
        for dy in 1..3u16 {
            if let Some(colour) = pixels.get(x.saturating_add(dx), y.saturating_add(dy)) {
                pixels.set(
                    x.saturating_add(dx),
                    y.saturating_add(dy),
                    Some(colour.blend(GLOW, 0.55)),
                );
            }
        }
    }
}

/// The marker a state puts next to the figure.
///
/// A waiting agent gets a `?`; a dozing one gets a `z`. Both are drawn in
/// pixels rather than as characters so they sit inside the picture instead
/// of floating over it.
fn draw_marker(pixels: &mut Pixels, pose: Pose, x: u16, y: u16) {
    let (glyph, colour) = match pose {
        Pose::Waiting => ('?', Rgb::new(255, 196, 64)),
        Pose::Dozing => ('z', Rgb::new(180, 190, 200)),
        _ => return,
    };
    // A 3-wide glyph for the question mark, a 2-wide z.
    let shape: &[(u16, u16)] = if glyph == '?' {
        &[(0, 0), (1, 0), (2, 0), (2, 1), (1, 2), (1, 4)]
    } else {
        &[(0, 0), (1, 1), (0, 2), (1, 3)]
    };
    let origin_x = x + MARKER_OFFSET;
    let origin_y = y.saturating_sub(1);
    for (dx, dy) in shape {
        pixels.set(origin_x + dx, origin_y + dy, Some(colour));
    }
}

/// Where a marker sits relative to its desk: just right of the figure —
/// the same offset the floor scene used, so the terminal and the web put
/// the question mark in the same place.
const MARKER_OFFSET: u16 = 10;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::room::{Activity, Agent};

    fn office_with(activity: Activity) -> Office {
        let mut office = Office::with_capacity(1, 32, 32);
        office.admit(Agent {
            id: "s-test".into(),
            label: "test".into(),
            activity,
            since_ms: 0,
        });
        office
    }

    fn painted(pixels: &Pixels) -> usize {
        (0..pixels.height())
            .flat_map(|y| (0..pixels.width()).map(move |x| (x, y)))
            .filter(|(x, y)| pixels.get(*x, *y).is_some())
            .count()
    }

    #[test]
    fn a_working_agent_paints_desk_and_figure() {
        let office = office_with(Activity::Working {
            detail: String::new(),
        });
        let pixels = portrait(&office, 0, 0, 0);
        assert!(
            painted(&pixels) > 30,
            "desk plus figure are a real picture: {} pixels",
            painted(&pixels)
        );
    }

    #[test]
    fn a_working_monitor_glows_and_an_idle_one_does_not() {
        // The screen row is the desk's `cc` pixels. Working brightens them;
        // idle leaves them as the palette drew them.
        let working = portrait(
            &office_with(Activity::Working {
                detail: String::new(),
            }),
            0,
            0,
            0,
        );
        let idle = portrait(&office_with(Activity::Idle), 0, 0, 0);
        let screen = |pixels: &Pixels| pixels.get(3, 2).expect("screen pixel");
        assert_ne!(
            screen(&working),
            screen(&idle),
            "the glow changed the screen"
        );
    }

    #[test]
    fn a_waiting_agent_gets_a_question_mark() {
        let office = office_with(Activity::Waiting {
            reason: "freigabe".into(),
        });
        let pixels = portrait(&office, 0, 0, 0);
        // The marker's first pixel, above the desk line.
        assert_eq!(pixels.get(MARKER_OFFSET, 0), Some(Rgb::new(255, 196, 64)));
    }

    #[test]
    fn an_index_without_an_agent_paints_nothing() {
        // A panel asking for a seat nobody sits in must get an empty canvas,
        // not a panic — the session list can shrink between render passes.
        let office = office_with(Activity::Idle);
        let pixels = portrait(&office, 7, 0, 0);
        assert_eq!(painted(&pixels), 0);
    }

    #[test]
    fn the_canvas_holds_the_whole_drawing() {
        // If the figure outgrows the canvas, the bottom rows would silently
        // clip — an agent losing their legs to a rounding mistake.
        let office = office_with(Activity::Working {
            detail: String::new(),
        });
        let pixels = portrait(&office, 0, 0, 0);
        let last_row = pixels.height() - 1;
        assert!(
            (0..pixels.width()).any(|x| pixels.get(x, last_row).is_some()),
            "the bottom row has pixels"
        );
    }
}
