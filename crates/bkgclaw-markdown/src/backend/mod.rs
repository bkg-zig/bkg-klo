//! Renderers. Each one walks the [`crate::ir`] tree and emits its own language.
//!
//! Neither backend re-parses Markdown: the tree already decided what a heading
//! is. A backend that inspected the source text would be a second parser with
//! a second opinion, which is the thing this crate exists to avoid.
//!
//! `html` is always available — it is pure string building, and the browser
//! cockpit is its only consumer. `ratatui` sits behind the `terminal` feature
//! because ratatui pulls in crossterm, which does not compile for wasm32.

pub mod html;
#[cfg(feature = "terminal")]
pub mod ratatui;
