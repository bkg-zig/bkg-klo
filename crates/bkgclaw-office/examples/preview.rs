//! Print the office to the terminal, so the art can be looked at.
//!
//! ```sh
//! cargo run -p bkgclaw-office --example preview -- --half-block
//! ```
//!
//! Tests prove the geometry — that two pixel rows land in one cell, that a
//! figure does not overlap its neighbour. They cannot prove the office *looks*
//! like an office. This does: it renders the real scene and prints it, so the
//! output can be read directly.

use bkgclaw_office::braille::Render;
use bkgclaw_office::room::{Activity, Agent, Office};

/// Write the buffer as text, with real 24-bit colour.
///
/// The glyph alone is not the picture. A braille cell is a *mask*; without the
/// colour it is a full block everywhere the floor is opaque, and the office
/// looks like a wall. So the escape codes are the point, not decoration.
fn dump(buffer: &ratatui::buffer::Buffer, width: u16, height: u16) {
    for y in 0..height {
        let mut line = String::new();
        for x in 0..width {
            let cell = &buffer[(x, y)];
            match cell.fg {
                ratatui::style::Color::Rgb(r, g, b) => {
                    line.push_str(&format!("\u{1b}[38;2;{r};{g};{b}m"));
                    line.push_str(cell.symbol());
                }
                _ => line.push_str(cell.symbol()),
            }
        }
        // Reset before the newline so the shell prompt does not inherit a
        // colour from the last cell of the picture.
        line.push_str("\u{1b}[0m");
        println!("{line}");
    }
}

fn main() {
    let half_block = std::env::args().any(|arg| arg == "--half-block");
    let render_mode = if half_block {
        Render::HalfBlock
    } else {
        Render::Braille
    };

    let width = 56u16;
    let height = 14u16;

    let mut office = Office::with_capacity(
        6,
        width * render_mode.sub_width(),
        height * render_mode.sub_height(),
    );

    let subjects = [
        (
            "nova",
            Activity::Working {
                detail: "read_file".into(),
            },
        ),
        (
            "atlas",
            Activity::Waiting {
                reason: "freigabe".into(),
            },
        ),
        ("echo", Activity::Idle),
        (
            "quill",
            Activity::Working {
                detail: "write_file".into(),
            },
        ),
    ];

    for (index, (name, activity)) in subjects.into_iter().enumerate() {
        // Start the third agent working and then stop it, so the office shows
        // a dozing figure next to two busy ones — the state separation is the
        // point of the picture.
        let since = if index == 2 { 0 } else { 60_000 };
        office.admit(Agent {
            id: format!("s-{name}"),
            label: name.into(),
            activity,
            since_ms: since,
        });
    }
    office.touch_daemon("evolve", "versuch 0007", 60_000);

    let mut buffer =
        ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, width, height));
    let area = ratatui::layout::Rect::new(0, 0, width, height);
    bkgclaw_office::scene::render_with(&office, &mut buffer, area, 60_000, 0, render_mode);

    println!("bkgclaw-office — {render_mode:?}, {width}x{height} Zellen\n");
    dump(&buffer, width, height);

    println!("\nagenten:");
    for agent in office.agents() {
        println!("  {:<8} {}", agent.label, agent.activity.label());
    }
    for daemon in office.daemons() {
        println!("  {:<8} daemon: {}", "(daemon)", daemon.detail);
    }
}
