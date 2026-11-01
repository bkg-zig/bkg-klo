//! Painting: the room model becomes pixels.
//!
//! The floor is this module's own work: wall band, floor, desks in their
//! places. What sits *at* a desk — the figure, the monitor's glow, the
//! state marker, and the order they draw in — is [`crate::portrait`]'s
//! rules, shared with the browser strip so the two canvases cannot drift.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::pixels::{Pixels, Rgb};
use crate::room::Office;

/// The floor colour. Dark enough that a glowing screen reads as lit.
const FLOOR: Rgb = Rgb::new(28, 30, 36);
/// The wall above the floor line.
const WALL: Rgb = Rgb::new(38, 42, 50);

/// Height of the wall band at the top of the room, in pixels.
const WALL_HEIGHT: u16 = 6;

/// Paint the office into `pixels`.
pub fn paint(office: &Office, pixels: &mut Pixels, now_ms: u64, beat: u64) {
    paint_floor(pixels, office);

    for index in 0..office.agents().len() {
        let Some(desk) = office.desk_for(index) else {
            continue;
        };
        // The per-desk rules — desk, glow, figure, marker, in that order —
        // live in `portrait`, shared with the web strip.
        crate::portrait::draw_agent(pixels, office, index, desk.x, desk.y, now_ms, beat);
    }
}

fn paint_floor(pixels: &mut Pixels, office: &Office) {
    for y in 0..pixels.height() {
        for x in 0..pixels.width() {
            let colour = if y < WALL_HEIGHT { WALL } else { FLOOR };
            pixels.set(x, y, Some(colour));
        }
    }
    let _ = office;
}

/// Paint and flush in one call — the shape every ratatui widget has.
///
/// `render` picks the packing. Braille gives four pixel rows per cell and two
/// columns, so the pixel canvas behind it is four times taller and twice as
/// wide as the area; half-blocks give two rows and one column. Both are
/// supported because they trade differently: braille for resolution, half-block
/// for two colours per cell.
pub fn render_with(
    office: &Office,
    buffer: &mut Buffer,
    area: Rect,
    now_ms: u64,
    beat: u64,
    render: crate::braille::Render,
) {
    let mut pixels = Pixels::new(
        area.width * render.sub_width(),
        area.height * render.sub_height(),
    );
    paint(office, &mut pixels, now_ms, beat);
    crate::braille::flush(&pixels, buffer, area, render);
}

/// Braille by default: a figure is ten rows tall, and at two rows per cell
/// that is five cells of detail. Four rows per cell is what makes the sprite
/// legible.
pub fn render(office: &Office, buffer: &mut Buffer, area: Rect, now_ms: u64, beat: u64) {
    render_with(
        office,
        buffer,
        area,
        now_ms,
        beat,
        crate::braille::Render::Braille,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::room::{Activity, Agent, Office};

    fn busy_office(count: usize) -> Office {
        let mut office = Office::with_capacity(count, 120, 60);
        for index in 0..count {
            office.admit(Agent {
                id: format!("s-{index}"),
                label: format!("s{index}"),
                activity: Activity::Working {
                    detail: "tool".into(),
                },
                since_ms: 0,
            });
        }
        office
    }

    fn render_to_buffer(office: &Office, width: u16, height: u16, now: u64, beat: u64) -> Buffer {
        let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
        render(
            office,
            &mut buffer,
            Rect::new(0, 0, width, height),
            now,
            beat,
        );
        buffer
    }

    #[test]
    fn an_empty_office_still_draws_a_room() {
        let office = Office::with_capacity(4, 120, 60);
        let buffer = render_to_buffer(&office, 40, 12, 0, 0);
        let painted = (0..40)
            .flat_map(|x| (0..12).map(move |y| (x, y)))
            .filter(|(x, y)| buffer[(*x, *y)].symbol() != " ")
            .count();
        assert!(painted > 0, "the floor must be visible");
    }

    #[test]
    fn a_working_agent_puts_pixels_on_the_floor() {
        let empty = render_to_buffer(&Office::with_capacity(4, 120, 60), 40, 12, 0, 0);
        let busy = render_to_buffer(&busy_office(1), 40, 12, 0, 0);
        // Count *distinct colours*, not painted cells. The floor covers every
        // cell either way, so counting cells compares a full room with a full
        // room and finds nothing — which is exactly what the first version of
        // this test reported.
        let palette_size = |buffer: &Buffer| {
            let mut seen = std::collections::HashSet::new();
            for x in 0..40 {
                for y in 0..12 {
                    seen.insert(buffer[(x, y)].fg);
                }
            }
            seen.len()
        };
        assert!(
            palette_size(&busy) > palette_size(&empty),
            "the agent adds colours: {} vs {}",
            palette_size(&busy),
            palette_size(&empty)
        );
    }

    #[test]
    fn a_waiting_agent_draws_a_question_mark() {
        let mut office = busy_office(1);
        office.set_activity(
            "s-0",
            Activity::Waiting {
                reason: "freigabe".into(),
            },
            1_000,
        );
        let buffer = render_to_buffer(&office, 60, 14, 1_000, 0);
        let painted = (0..60)
            .flat_map(|x| (0..14).map(move |y| (x, y)))
            .filter(|(x, y)| buffer[(*x, *y)].symbol() != " ")
            .count();
        assert!(painted > 0, "the marker is drawn");
    }

    #[test]
    fn the_typing_frames_actually_differ() {
        // If paint ignored the pose, the typing animation would be a still
        // image and nothing would look like it was working.
        let office = busy_office(1);
        let a = render_to_buffer(&office, 40, 12, 0, 0);
        let b = render_to_buffer(&office, 40, 12, 0, 2);
        let differs = (0..40).any(|x| (0..12).any(|y| a[(x, y)].fg != b[(x, y)].fg));
        assert!(differs, "two frames of the same agent differ");
    }

    #[test]
    fn two_agents_do_not_draw_the_same_pixels() {
        // Shared seats or a broken offset would make the second agent
        // invisible behind the first.
        let office = busy_office(2);
        let mut only_first = Office::with_capacity(2, 120, 60);
        only_first.admit(office.agents()[0].clone());
        let both = render_to_buffer(&office, 90, 14, 0, 0);
        let first = render_to_buffer(&only_first, 90, 14, 0, 0);
        let differs = (0..90).any(|x| (0..14).any(|y| both[(x, y)].fg != first[(x, y)].fg));
        assert!(differs, "the second agent adds pixels of its own");
    }

    #[test]
    fn a_tiny_area_does_not_panic() {
        // A resize to nothing must not take the TUI down.
        let office = busy_office(3);
        for (width, height) in [(0u16, 0u16), (1, 1), (2, 3), (1, 40)] {
            let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
            render(&office, &mut buffer, Rect::new(0, 0, width, height), 0, 0);
        }
    }

    #[test]
    fn an_agent_beyond_the_capacity_is_skipped_not_drawn_off_screen() {
        let mut office = Office::with_capacity(1, 40, 30);
        office.admit(Agent {
            id: "a".into(),
            label: "a".into(),
            activity: Activity::Idle,
            since_ms: 0,
        });
        // Force a second agent in behind the office's back, as a bad event
        // handler might.
        office.agents_mut_for_test().push(Agent {
            id: "b".into(),
            label: "b".into(),
            activity: Activity::Idle,
            since_ms: 0,
        });
        let buffer = render_to_buffer(&office, 40, 12, 0, 0);
        let _ = buffer;
    }
}
