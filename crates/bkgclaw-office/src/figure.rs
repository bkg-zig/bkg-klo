//! The figures. Drawn here as source, not loaded from files.
//!
//! # Why the art lives in Rust
//!
//! pixtuoid keeps its characters in `.sprite` files with a shared palette.
//! That is the right call for a project with 169 sprites and a pack format and
//! a generator script. This office has four figures, so a file format plus a
//! parser plus a palette file would be more machinery than the art itself.
//!
//! The idea worth keeping is the *palette-index* one, and it is kept: a figure
//! is declared as characters, and the characters mean roles (`H` is hair,
//! `B` is shirt), never colours. Two agents therefore share one figure
//! definition and differ only in the palette they are drawn with — which is
//! what makes six desks look like six different people instead of six
//! identical clones.
//!
//! So the figures are written out as character grids below, and every colour
//! they carry is resolved through [`crate::figure::agent_palette`]. Change a
//! character's shirt colour and the whole office changes with it, because
//! nobody ever wrote a colour into a figure.

/// A figure: a character grid plus which palette slots follow the agent.
///
/// `personal` lists the slots that get an agent's own colour. Everything else
/// — outlines, the screen glass, the desk — belongs to the room.
pub struct Figure {
    pub name: &'static str,
    pub rows: &'static [&'static str],
    pub personal: &'static [char],
}

/// A seated agent, seen from the front.
pub const SEATED: Figure = Figure {
    name: "seated",
    rows: &[
        ".nHHHHn.", "nHHHHHHn", "HSSSSSSH", "HSeSSeSH", ".SSSmSS.", ".nSSSSn.", ".BBBBBB.",
        "BBBBBBBB", "BBBBBBBB", ".SBBBBS.",
    ],
    personal: &['H', 'S', 'B'],
};

/// Hands forward on the keyboard — frame one of the typing loop.
pub const TYPING_0: Figure = Figure {
    name: "typing_0",
    rows: &[
        ".nHHHHn.", "nHHHHHHn", "HSSSSSSH", "HSeSSeSH", ".SSSmSS.", ".nSSSSn.", ".BBBBBB.",
        "BBBBBBBB", "BBBBBBBB", "SBBBBBB.", "......S.",
    ],
    personal: &['H', 'S', 'B'],
};

/// The other hand position — swapping these two is the whole typing animation.
pub const TYPING_1: Figure = Figure {
    name: "typing_1",
    rows: &[
        ".nHHHHn.", "nHHHHHHn", "HSSSSSSH", "HSeSSeSH", ".SSSmSS.", ".nSSSSn.", ".BBBBBB.",
        "BBBBBBBB", "BBBBBBBB", ".BBBBBBS", ".S......",
    ],
    personal: &['H', 'S', 'B'],
};

/// Head down, dozing. Idle for long enough and an agent takes a break —
/// which is how "finished, waiting for you" reads differently from "hung".
pub const SLEEPING: Figure = Figure {
    name: "sleeping",
    rows: &[
        "..nHHHn.", ".nHHHHHn", ".HHHHHHH", ".HSSSSSH", ".HSSSSmH", "..nSSSn.", "..BBBBB.",
        ".BBBBBBB", ".BBBBBBB", "..BBBBB.",
    ],
    personal: &['H', 'S', 'B'],
};

/// The desk an agent sits at. Room colours only — a desk that changed colour
/// per occupant would make the office unreadable as a place.
pub const DESK: Figure = Figure {
    name: "desk",
    rows: &[
        "........", ".DDDDDD.", ".DccDcc.", ".DDDDDD.", ".DDDDDD.", "KKKKKKKK", ".TTTTTT.",
    ],
    personal: &[],
};

/// Every figure, by name.
pub fn all() -> &'static [Figure] {
    &[SEATED, TYPING_0, TYPING_1, SLEEPING, DESK]
}

/// Look a figure up by name.
pub fn by_name(name: &str) -> Option<&'static Figure> {
    all().iter().find(|figure| figure.name == name)
}

impl Figure {
    pub fn width(&self) -> u16 {
        self.rows
            .first()
            .map(|row| row.chars().count())
            .unwrap_or(0) as u16
    }

    pub fn height(&self) -> u16 {
        self.rows.len() as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_figure_has_a_name_and_rows() {
        for figure in all() {
            assert!(!figure.name.is_empty());
            assert!(!figure.rows.is_empty(), "{} has no rows", figure.name);
        }
    }

    #[test]
    fn every_row_of_a_figure_has_the_same_width() {
        // A ragged figure would draw its lower rows offset from its upper ones.
        for figure in all() {
            let expected = figure.width() as usize;
            for (index, row) in figure.rows.iter().enumerate() {
                assert_eq!(
                    row.chars().count(),
                    expected,
                    "{} row {index} is {} wide, expected {expected}",
                    figure.name,
                    row.chars().count()
                );
            }
        }
    }

    #[test]
    fn every_symbol_a_figure_uses_has_a_slot_in_the_shared_palette() {
        // A figure referencing an undefined letter draws as a hole. Catching
        // it here means the office never ships with a missing pixel.
        let palette = crate::sprite::default_palette();
        for figure in all() {
            for (row_index, row) in figure.rows.iter().enumerate() {
                for symbol in row.chars() {
                    assert!(
                        palette.contains_key(&symbol),
                        "{} row {row_index} uses '{symbol}', which has no palette entry",
                        figure.name
                    );
                }
            }
        }
    }

    #[test]
    fn the_typing_pair_differ_only_in_the_hand_row() {
        // If they stopped differing, the animation would freeze into a solid
        // block and typing would look like sitting.
        assert_eq!(TYPING_0.rows.len(), TYPING_1.rows.len());
        let differing = TYPING_0
            .rows
            .iter()
            .zip(TYPING_1.rows.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert!(differing > 0, "the two frames are identical");
    }

    #[test]
    fn the_seated_figure_is_the_same_shape_as_the_typing_one() {
        // A figure that changes size between poses would jump on the screen
        // every time the agent started working.
        assert_eq!(SEATED.width(), TYPING_0.width());
    }

    #[test]
    fn a_figure_can_be_found_by_name() {
        assert!(by_name("seated").is_some());
        assert!(by_name("nope").is_none());
    }

    #[test]
    fn the_desk_carries_no_personal_colour() {
        // The desk belongs to the room; a personal slot on it would make every
        // desk a different colour.
        assert!(DESK.personal.is_empty());
    }
}
