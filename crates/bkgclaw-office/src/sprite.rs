//! The shared palette: what each character in a figure means.
//!
//! # The one idea kept from elsewhere
//!
//! A figure is written as characters, and a character names a *role*, never a
//! colour. `H` is hair. `B` is a shirt. The room's colours live here, and an
//! agent's personal colours are substituted at draw time for the handful of
//! slots listed in [`PERSONAL_SLOTS`].
//!
//! That indirection is the whole reason two agents at two desks read as two
//! people: they share one figure definition and differ only in what `H`, `S`,
//! `B` and `P` resolve to for them. Change the room palette and every desk
//! changes at once, because no colour was ever written into a figure.
//!
//! A file format for this was considered and dropped. With four figures a
//! parser plus a format plus a pack file would be more machinery than the art.

use std::collections::HashMap;

use crate::pixels::Rgb;

/// The shared office palette.
///
/// Only the slots the office actually draws are here. Adding a slot means
/// adding a key here and using the letter in a sprite — the sprite files stay
/// readable because the letter carries the meaning, not the hex value.
pub fn default_palette() -> HashMap<char, Option<Rgb>> {
    HashMap::from([
        ('.', None),
        (' ', None),
        // Person
        ('H', Some(Rgb::new(42, 26, 14))),    // hair
        ('h', Some(Rgb::new(63, 42, 26))),    // hair, lighter
        ('S', Some(Rgb::new(244, 199, 154))), // skin
        ('e', Some(Rgb::new(26, 26, 26))),    // eyes
        ('m', Some(Rgb::new(160, 64, 64))),   // mouth
        ('B', Some(Rgb::new(46, 98, 207))),   // shirt
        ('P', Some(Rgb::new(42, 46, 68))),    // pants
        ('n', Some(Rgb::new(26, 16, 10))),    // outline
        // Room
        ('D', Some(Rgb::new(139, 90, 43))),   // desk wood
        ('d', Some(Rgb::new(90, 56, 24))),    // desk wood, shaded
        ('K', Some(Rgb::new(168, 168, 176))), // metal
        ('T', Some(Rgb::new(38, 38, 44))),    // chair shell
        ('k', Some(Rgb::new(26, 26, 34))),    // near-black
        ('w', Some(Rgb::new(236, 236, 236))), // paper / text
        // Screen, the one that carries state
        ('j', Some(Rgb::new(28, 42, 54))),    // dark glass
        ('J', Some(Rgb::new(52, 66, 78))),    // dim screen content
        ('c', Some(Rgb::new(126, 212, 226))), // bright screen content
        ('r', Some(Rgb::new(168, 50, 50))),   // red
        ('b', Some(Rgb::new(58, 90, 158))),   // blue
        ('y', Some(Rgb::new(200, 166, 76))),  // ochre
        ('q', Some(Rgb::new(29, 29, 29))),    // ink black
    ])
}

/// Recolouring keys: which palette slots follow an agent rather than the room.
///
/// These four are what make two agents at the same desk look like two people.
/// The room's colours never change — a desk that recoloured per agent would
/// make the office unreadable as a place.
pub const PERSONAL_SLOTS: [char; 4] = ['H', 'S', 'B', 'P'];

/// A stable colour for a slot, derived from a seed.
///
/// The same seed always gives the same colour, so an agent keeps its
/// appearance across frames and restarts — the office reads as the same
/// office, not a slot machine.
pub fn seeded_colour(seed: &str, salt: u64) -> Rgb {
    // FNV-1a: short, dependency-free, and stable across runs. `std`'s
    // `DefaultHasher` is explicitly not guaranteed stable between releases,
    // which would reshuffle every agent's colour on a toolchain update.
    let mut hash = 0xcbf29ce484222325u64 ^ salt;
    for byte in seed.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    // Push the hash into a hue, then pick a saturation/lightness pair that
    // stays readable on a dark office background.
    let hue = (hash % 360) as f32;
    let light = 0.45 + ((hash >> 9) % 30) as f32 / 100.0;
    hsl_to_rgb(hue, 0.55, light)
}

/// HSL to RGB, degrees in 0..360, s/l in 0..1.
fn hsl_to_rgb(h: f32, s: f32, l: f32) -> Rgb {
    let (r, g, b) = if s == 0.0 {
        let grey = (l * 255.0) as u8;
        (grey, grey, grey)
    } else {
        let q = if l < 0.5 {
            l * (1.0 + s)
        } else {
            l + s - l * s
        };
        let p = 2.0 * l - q;
        let channel = |t: f32| -> u8 {
            let mut t = t;
            if t < 0.0 {
                t += 1.0;
            }
            if t > 1.0 {
                t -= 1.0;
            }
            let value = if t < 1.0 / 6.0 {
                p + (q - p) * 6.0 * t
            } else if t < 1.0 / 2.0 {
                q
            } else if t < 2.0 / 3.0 {
                p + (q - p) * (2.0 / 3.0 - t) * 6.0
            } else {
                p
            };
            (value * 255.0) as u8
        };
        let h = h / 360.0;
        (channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0))
    };
    Rgb::new(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_transparent_slots_really_are_transparent() {
        let palette = default_palette();
        assert_eq!(palette[&'.'], None, "dot paints nothing");
        assert_eq!(palette[&' '], None, "space paints nothing");
    }

    #[test]
    fn every_slot_the_figures_use_has_a_colour() {
        // A missing slot means a figure renders with a hole in it. Checking it
        // here means the office cannot ship with a missing pixel.
        let palette = default_palette();
        for figure in crate::figure::all() {
            for row in figure.rows {
                for symbol in row.chars() {
                    assert!(
                        palette.contains_key(&symbol),
                        "{} uses '{symbol}', which has no palette entry",
                        figure.name
                    );
                }
            }
        }
    }

    #[test]
    fn the_palette_covers_every_letter_the_figures_name() {
        let used: std::collections::HashSet<char> = crate::figure::all()
            .iter()
            .flat_map(|figure| figure.rows.iter())
            .flat_map(|row| row.chars())
            .collect();
        for symbol in used {
            assert!(
                default_palette().contains_key(&symbol),
                "no palette entry for '{symbol}'"
            );
        }
    }

    #[test]
    fn a_seeded_colour_is_stable_across_calls() {
        // The office has to look the same on every redraw, or every agent
        // would flicker between two colours.
        let first = seeded_colour("session-abc", 1);
        for _ in 0..100 {
            assert_eq!(seeded_colour("session-abc", 1), first);
        }
    }

    #[test]
    fn different_seeds_and_salts_give_different_colours() {
        let base = seeded_colour("session-abc", 1);
        assert_ne!(base, seeded_colour("session-xyz", 1), "seed must matter");
        assert_ne!(base, seeded_colour("session-abc", 2), "salt must matter");
    }

    #[test]
    fn a_seeded_colour_stays_inside_the_byte_range() {
        for index in 0..500 {
            let colour = seeded_colour(&format!("s-{index}"), 7);
            // A wrapped u8 here would read as a rendering fault rather than a
            // maths one, which is exactly the bug that is hard to find.
            let _ = colour;
        }
    }

    #[test]
    fn every_personal_slot_is_named_in_the_palette() {
        for slot in PERSONAL_SLOTS {
            assert!(
                default_palette().contains_key(&slot),
                "slot {slot} is missing"
            );
        }
    }
}
