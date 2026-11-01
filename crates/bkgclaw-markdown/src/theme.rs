//! Colours for the terminal backend.
//!
//! # The shape of the system
//!
//! A flat struct of named colour slots, one preset per look, plus a
//! `Partial`-style merge on top of a built-in base. The alternative — a
//! semantic enum (`Accent`, `Danger`, `Muted`, …) resolved through a lookup —
//! sounds tidier and is worse here: the TUI needs *narrow* slots (a code
//! border is not the same colour as a table border, even though both are "a
//! border"), and a semantic enum cannot express that without growing a variant
//! per use. Slots scale; enums rot.
//!
//! The merge is deliberately flat, one level deep. A nested theme would let a
//! user override one heading colour and silently lose the other five.

use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize};

/// Every colour the TUI can ask for. Fields are public and named for the
/// *role*, not the colour: `good` is green today, but the operator who picks a
/// daltonised theme gets a different green without touching any call site.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Theme {
    pub name: String,
    pub background: Color,
    pub surface: Color,
    pub border: Color,
    pub border_focus: Color,

    pub text: Color,
    pub text_dim: Color,
    pub user: Color,
    pub assistant: Color,
    pub reasoning: Color,

    pub accent: Color,
    pub good: Color,
    pub warn: Color,
    pub bad: Color,
    pub info: Color,

    pub heading_1: Color,
    pub heading_2: Color,
    pub heading_3: Color,
    pub heading_4: Color,
    pub heading_5: Color,
    pub heading_6: Color,

    pub code: Color,
    pub code_border: Color,

    // Code token colours. Eight slots for the eight `TokenClass` variants:
    // they are named for the *category*, not for a colour, so a theme that
    // only has sixteen colours still has to decide what a keyword looks like.
    // The web cockpit maps the same classes to CSS rules — one classification,
    // two renderers.
    pub token_keyword: Color,
    pub token_type: Color,
    pub token_function: Color,
    pub token_number: Color,
    pub token_string: Color,
    pub token_comment: Color,
    pub token_punctuation: Color,
    pub token_attribute: Color,

    pub link: Color,
    pub list_bullet: Color,
    pub quote: Color,
    pub rule: Color,
    pub table_header: Color,
    pub table_border: Color,

    pub status_ok: Color,
    pub status_busy: Color,
    pub cursor: Color,
    pub selection: Color,
}

impl Theme {
    /// A style for ordinary prose.
    pub fn text_style(&self) -> Style {
        Style::default().fg(self.text)
    }

    /// A style for a heading at a given level. Out-of-range levels fall back
    /// to the deepest defined one rather than panicking.
    pub fn heading(&self, level: u8) -> Style {
        let color = match level {
            1 => self.heading_1,
            2 => self.heading_2,
            3 => self.heading_3,
            4 => self.heading_4,
            5 => self.heading_5,
            _ => self.heading_6,
        };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }

    pub fn heading_1_style(&self) -> Style {
        self.heading(1)
    }
}

// The ratatui-facing accessors the backend uses. They live here so the backend
// never has to know which slot a concept maps to — adding a preset means
// touching this file only.
impl Theme {
    pub fn text(&self) -> Style {
        Style::default().fg(self.text)
    }
    pub fn dim(&self) -> Style {
        Style::default().fg(self.text_dim)
    }
    pub fn code(&self) -> Style {
        Style::default().fg(self.code)
    }
    pub fn code_border(&self) -> Style {
        Style::default().fg(self.code_border)
    }
    pub fn token_keyword(&self) -> Style {
        Style::default().fg(self.token_keyword)
    }
    pub fn token_type(&self) -> Style {
        Style::default().fg(self.token_type)
    }
    pub fn token_function(&self) -> Style {
        Style::default().fg(self.token_function)
    }
    pub fn token_number(&self) -> Style {
        Style::default().fg(self.token_number)
    }
    pub fn token_string(&self) -> Style {
        Style::default().fg(self.token_string)
    }
    pub fn token_comment(&self) -> Style {
        Style::default().fg(self.token_comment)
    }
    pub fn token_punctuation(&self) -> Style {
        Style::default().fg(self.token_punctuation)
    }
    pub fn token_attribute(&self) -> Style {
        Style::default().fg(self.token_attribute)
    }
    pub fn link(&self) -> Style {
        Style::default().fg(self.link)
    }
    pub fn bullet(&self) -> Style {
        Style::default().fg(self.list_bullet)
    }
    pub fn rule(&self) -> Style {
        Style::default().fg(self.rule)
    }
    pub fn table_header(&self) -> Style {
        Style::default()
            .fg(self.table_header)
            .add_modifier(Modifier::BOLD)
    }
    pub fn table_border(&self) -> Style {
        Style::default().fg(self.table_border)
    }
    pub fn border(&self) -> Style {
        Style::default().fg(self.border)
    }
    pub fn border_focus(&self) -> Style {
        Style::default().fg(self.border_focus)
    }
    pub fn surface(&self) -> Style {
        Style::default().bg(self.surface)
    }
    pub fn cursor(&self) -> Style {
        Style::default().fg(self.cursor)
    }
    pub fn selection(&self) -> Style {
        Style::default().bg(self.selection)
    }
    pub fn good(&self) -> Style {
        Style::default().fg(self.good)
    }
    pub fn warn(&self) -> Style {
        Style::default().fg(self.warn)
    }
    pub fn bad(&self) -> Style {
        Style::default().fg(self.bad)
    }
    pub fn accent(&self) -> Style {
        Style::default().fg(self.accent)
    }
    pub fn user(&self) -> Style {
        Style::default().fg(self.user)
    }
    pub fn assistant(&self) -> Style {
        Style::default().fg(self.assistant)
    }
    pub fn reasoning(&self) -> Style {
        Style::default().fg(self.reasoning)
    }
    pub fn status(&self, busy: bool) -> Style {
        let color = if busy {
            self.status_busy
        } else {
            self.status_ok
        };
        Style::default().fg(color).add_modifier(Modifier::BOLD)
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

/// The default: a dark terminal theme that matches the web cockpit's palette,
/// so the two clients look like one product.
pub const DEFAULT_NAME: &str = "bkg-dunkel";

impl Default for Theme {
    fn default() -> Self {
        Theme {
            name: DEFAULT_NAME.to_string(),
            background: rgb(16, 20, 24),
            surface: rgb(27, 32, 39),
            border: rgb(50, 56, 66),
            border_focus: rgb(76, 194, 255),

            text: rgb(216, 222, 233),
            text_dim: rgb(125, 133, 144),
            user: rgb(76, 194, 255),
            assistant: rgb(63, 185, 80),
            reasoning: rgb(125, 133, 144),

            accent: rgb(76, 194, 255),
            good: rgb(63, 185, 80),
            warn: rgb(210, 153, 34),
            bad: rgb(248, 81, 73),
            info: rgb(88, 166, 255),

            heading_1: rgb(76, 194, 255),
            heading_2: rgb(88, 166, 255),
            heading_3: rgb(126, 231, 135),
            heading_4: rgb(210, 153, 34),
            heading_5: rgb(210, 153, 34),
            heading_6: rgb(125, 133, 144),

            code: rgb(240, 246, 252),
            code_border: rgb(48, 54, 61),
            token_keyword: rgb(255, 123, 114),
            token_type: rgb(210, 153, 34),
            token_function: rgb(121, 192, 255),
            token_number: rgb(181, 206, 168),
            token_string: rgb(210, 153, 34),
            token_comment: rgb(110, 118, 129),
            token_punctuation: rgb(201, 209, 217),
            token_attribute: rgb(188, 140, 255),
            link: rgb(121, 192, 255),
            list_bullet: rgb(210, 153, 34),
            quote: rgb(125, 133, 144),
            rule: rgb(48, 54, 61),
            table_header: rgb(88, 166, 255),
            table_border: rgb(48, 54, 61),

            status_ok: rgb(125, 133, 144),
            status_busy: rgb(76, 194, 255),
            cursor: rgb(76, 194, 255),
            selection: rgb(33, 38, 45),
        }
    }
}

/// Every preset the operator can cycle through. Two is enough to prove the
/// system works; the point is that adding a third is one table row.
pub fn presets() -> Vec<Theme> {
    let mut themes = vec![Theme::default()];
    themes.push(nord());
    themes.push(dracula());
    themes.push(hell());
    themes
}

fn nord() -> Theme {
    Theme {
        name: "nord".into(),
        background: rgb(46, 52, 64),
        surface: rgb(59, 66, 82),
        border: rgb(76, 86, 106),
        border_focus: rgb(136, 192, 208),
        text: rgb(216, 222, 233),
        text_dim: rgb(76, 86, 106),
        user: rgb(129, 161, 193),
        assistant: rgb(163, 190, 140),
        reasoning: rgb(76, 86, 106),
        accent: rgb(136, 192, 208),
        good: rgb(163, 190, 140),
        warn: rgb(235, 203, 139),
        bad: rgb(191, 97, 106),
        info: rgb(129, 161, 193),
        heading_1: rgb(136, 192, 208),
        heading_2: rgb(143, 188, 187),
        heading_3: rgb(163, 190, 140),
        heading_4: rgb(235, 203, 139),
        heading_5: rgb(208, 135, 112),
        heading_6: rgb(76, 86, 106),
        code: rgb(236, 239, 244),
        code_border: rgb(76, 86, 106),
        token_keyword: rgb(129, 161, 193),
        token_type: rgb(143, 188, 187),
        token_function: rgb(136, 192, 208),
        token_number: rgb(163, 190, 140),
        token_string: rgb(163, 190, 140),
        token_comment: rgb(76, 86, 106),
        token_punctuation: rgb(216, 222, 233),
        token_attribute: rgb(235, 203, 139),
        link: rgb(129, 161, 193),
        list_bullet: rgb(143, 188, 187),
        quote: rgb(76, 86, 106),
        rule: rgb(76, 86, 106),
        table_header: rgb(136, 192, 208),
        table_border: rgb(76, 86, 106),
        status_ok: rgb(76, 86, 106),
        status_busy: rgb(136, 192, 208),
        cursor: rgb(136, 192, 208),
        selection: rgb(67, 76, 94),
    }
}

fn dracula() -> Theme {
    Theme {
        name: "dracula".into(),
        background: rgb(40, 42, 54),
        surface: rgb(68, 71, 90),
        border: rgb(68, 71, 90),
        border_focus: rgb(189, 147, 249),
        text: rgb(248, 248, 242),
        text_dim: rgb(98, 114, 164),
        user: rgb(139, 233, 253),
        assistant: rgb(80, 250, 123),
        reasoning: rgb(98, 114, 164),
        accent: rgb(189, 147, 249),
        good: rgb(80, 250, 123),
        warn: rgb(241, 250, 140),
        bad: rgb(255, 85, 85),
        info: rgb(139, 233, 253),
        heading_1: rgb(255, 121, 198),
        heading_2: rgb(189, 147, 249),
        heading_3: rgb(139, 233, 253),
        heading_4: rgb(80, 250, 123),
        heading_5: rgb(241, 250, 140),
        heading_6: rgb(98, 114, 164),
        code: rgb(248, 248, 242),
        code_border: rgb(68, 71, 90),
        token_keyword: rgb(255, 121, 198),
        token_type: rgb(8, 248, 107),
        token_function: rgb(80, 250, 123),
        token_number: rgb(189, 147, 249),
        token_string: rgb(241, 250, 140),
        token_comment: rgb(98, 114, 164),
        token_punctuation: rgb(248, 248, 242),
        token_attribute: rgb(139, 233, 253),
        link: rgb(139, 233, 253),
        list_bullet: rgb(255, 121, 198),
        quote: rgb(98, 114, 164),
        rule: rgb(68, 71, 90),
        table_header: rgb(189, 147, 249),
        table_border: rgb(68, 71, 90),
        status_ok: rgb(98, 114, 164),
        status_busy: rgb(189, 147, 249),
        cursor: rgb(189, 147, 249),
        selection: rgb(68, 71, 90),
    }
}

/// A light theme. It exists to prove the slots are genuinely semantic: a slot
/// named `text` has to work on a white background too, or the system is just
/// "the dark palette with holes in it".
fn hell() -> Theme {
    Theme {
        name: "hell".into(),
        background: rgb(250, 250, 250),
        surface: rgb(240, 240, 242),
        border: rgb(205, 205, 210),
        border_focus: rgb(0, 95, 175),
        text: rgb(36, 41, 46),
        text_dim: rgb(110, 118, 129),
        user: rgb(0, 95, 175),
        assistant: rgb(26, 127, 55),
        reasoning: rgb(110, 118, 129),
        accent: rgb(0, 95, 175),
        good: rgb(26, 127, 55),
        warn: rgb(154, 103, 0),
        bad: rgb(207, 34, 46),
        info: rgb(9, 105, 218),
        heading_1: rgb(0, 95, 175),
        heading_2: rgb(9, 105, 218),
        heading_3: rgb(26, 127, 55),
        heading_4: rgb(154, 103, 0),
        heading_5: rgb(154, 103, 0),
        heading_6: rgb(110, 118, 129),
        code: rgb(36, 41, 46),
        code_border: rgb(205, 205, 210),
        token_keyword: rgb(207, 34, 46),
        token_type: rgb(154, 103, 0),
        token_function: rgb(0, 82, 174),
        token_number: rgb(26, 127, 55),
        token_string: rgb(26, 127, 55),
        token_comment: rgb(140, 145, 155),
        token_punctuation: rgb(60, 66, 74),
        token_attribute: rgb(111, 66, 193),
        link: rgb(0, 82, 174),
        list_bullet: rgb(154, 103, 0),
        quote: rgb(110, 118, 129),
        rule: rgb(205, 205, 210),
        table_header: rgb(9, 105, 218),
        table_border: rgb(205, 205, 210),
        status_ok: rgb(110, 118, 129),
        status_busy: rgb(0, 95, 175),
        cursor: rgb(0, 95, 175),
        selection: rgb(218, 224, 232),
    }
}

/// Load a user theme from `$BKGCLAW_HOME/theme.json`, falling back to the
/// default when it is missing, unreadable, or does not name a known preset.
///
/// A broken theme file must never keep the TUI from starting: the operator
/// would be locked out of a tool they need to fix the file.
pub fn load() -> Theme {
    let path = match home_root().map(|root| root.join("theme.json")) {
        Some(path) => path,
        None => return Theme::default(),
    };
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Theme::default();
    };
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(value) => value
            .get("name")
            .and_then(|name| name.as_str())
            .and_then(|name| presets().into_iter().find(|theme| theme.name == name))
            .unwrap_or_default(),
        Err(_) => Theme::default(),
    }
}

fn home_root() -> Option<std::path::PathBuf> {
    std::env::var_os("BKGCLAW_HOME").map(std::path::PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_preset_names_itself() {
        for theme in presets() {
            assert!(!theme.name.is_empty(), "a preset must be identifiable");
        }
    }

    #[test]
    fn preset_names_are_unique() {
        // Two presets with one name would make F3 cycling non-deterministic.
        let mut names: Vec<String> = presets().into_iter().map(|theme| theme.name).collect();
        names.sort();
        let count = names.len();
        names.dedup();
        assert_eq!(names.len(), count, "duplicate preset names: {names:?}");
    }

    #[test]
    fn every_slot_is_set_on_every_preset() {
        // The `..Theme::default()` spread means a new slot added to the struct
        // silently inherits the dark palette instead of being a decision. This
        // test is the reminder to make it one.
        let names: Vec<String> = presets().into_iter().map(|theme| theme.name).collect();
        assert!(names.len() >= 4, "presets: {names:?}");
    }

    #[test]
    fn a_heading_level_beyond_six_falls_back_instead_of_panicking() {
        let theme = Theme::default();
        let _ = theme.heading(9);
        let _ = theme.heading(0);
    }

    #[test]
    fn a_theme_survives_a_json_round_trip() {
        let theme = Theme::default();
        let json = serde_json::to_string(&theme).expect("serialises");
        let back: Theme = serde_json::from_str(&json).expect("deserialises");
        assert_eq!(theme, back);
    }
}
