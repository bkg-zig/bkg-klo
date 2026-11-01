//! `bkgclaw-markdown` — Markdown in, two renderers out.
//!
//! # The one-paragraph version
//!
//! A model answers in Markdown. The terminal wants ratatui cells, the browser
//! wants HTML, and neither of those languages knows Markdown. This crate parses
//! once into a target-neutral tree ([`ir`]) and hands it to a backend. The
//! terminal uses [`backend::ratatui`]; the browser uses [`backend::html`].
//!
//! # Why not two parsers
//!
//! Because two parsers means two places where structure can be decided, and
//! they will disagree. The day one of them forgets that `- [ ]` is a task item,
//! the two clients stop showing the same conversation — and the operator has
//! no way to tell which one lied. Parsing happens here, once, and every
//! renderer inherits the same decisions.
//!
//! # The rule that keeps the browser safe
//!
//! Model output is untrusted input. The html backend escapes every text node
//! it emits and never passes model text through as markup. `parse.rs` already
//! treats raw HTML as *text* rather than as tags; the backend escapes again on
//! the way out. Two independent escapes, because one mistake is an XSS hole
//! and the payload comes from whatever endpoint the gateway is pointed at.
//!
//! ```
//! use bkgclaw_markdown::{parse, backend::html};
//!
//! let document = parse("# Titel\n\n```rust\nfn main() {}\n```");
//! let html = html::render(&document);
//! assert!(html.contains("<h1>Titel</h1>"));
//! // Code is split into classed spans, so the check is on the *text* — a
//! // substring that straddles a tag boundary would fail on correct output.
//! assert!(html.contains(">fn</span>"), "{html}");
//! assert!(html.contains("main() {}"));
//! ```

#![forbid(unsafe_code)]

pub mod backend;
pub mod highlight;
pub mod ir;
pub mod parse;
#[cfg(feature = "terminal")]
pub mod theme;

pub use ir::{
    Align, Block, CodeLine, CodeToken, Document, ListItem, Marks, Span, SpanKind, Table, TokenClass,
};
pub use parse::parse;
#[cfg(feature = "terminal")]
pub use theme::Theme;
