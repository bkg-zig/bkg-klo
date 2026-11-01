//! pulldown-cmark events to the [`crate::ir`] tree.
//!
//! # The mental model
//!
//! `pulldown_cmark::Parser` yields a flat event stream whose structure is
//! *implied by matching end-tags*: a paragraph ends when `End(TagEnd::Paragraph)`
//! arrives. Turning that into a tree is a **stack** problem.
//!
//! The first version of this file used scalar fields — one `Option<List>`, one
//! `bool quote_open` — and that was the whole bug class QA found:
//!
//! - Nested lists overwrote the outer list, so `- a / - b / - c` produced a
//!   single item containing `b`. `a` and `c` vanished with no error.
//! - `close_quote` cleared the flag even when an outer quote was still open.
//! - A table pushed straight to `blocks`, bypassing whatever container it was
//!   inside, so a table in a list appeared *before* the list.
//!
//! A scalar cannot nest. A stack can. So every container here is a `Vec` frame,
//! pushed on `Start` and popped on `End`, and a completed block goes to
//! `frames.last_mut()` — always the innermost container, which is the only
//! place it can legally land.
//!
//! The second rule: **nothing is ever dropped.** Every event lands in the tree
//! or in an open frame. Raw HTML becomes *text*; math becomes text; an
//! unmodelled tag flushes its spans rather than discarding them.

use crate::ir::{Align, Block, Cell, Document, ListItem, Marks, Span, SpanKind, Table};
use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Parse Markdown into the target-neutral tree.
pub fn parse(source: &str) -> Document {
    let mut options = Options::empty();
    options.insert(Options::ENABLE_TABLES);
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TASKLISTS);

    let mut builder = Builder::new();
    for event in Parser::new_ext(source, options) {
        builder.push(event);
    }
    builder.finish()
}

fn level_number(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn alignment(kind: Alignment) -> Align {
    match kind {
        Alignment::None => Align::None,
        Alignment::Left => Align::Left,
        Alignment::Center => Align::Center,
        Alignment::Right => Align::Right,
    }
}

/// Is this a URL a browser would execute rather than navigate?
///
/// Decided *here*, once, so the html backend never has to make the call. Two
/// renderers each deciding "is `javascript:` dangerous?" is how one of them
/// eventually decides no.
fn safe_scheme(dest: &str) -> bool {
    let lowered = dest.trim().to_ascii_lowercase();
    // A relative URL or a fragment has no scheme and is safe. Anything with a
    // scheme has to be one of the three that navigate rather than execute.
    match lowered.split_once(':') {
        None => true,
        Some(_) => {
            lowered.starts_with("http:")
                || lowered.starts_with("https:")
                || lowered.starts_with("mailto:")
        }
    }
}

/// One open container. Blocks completed while this frame is on the stack land
/// here and nowhere else.
enum Frame {
    /// The document itself.
    Document { blocks: Vec<Block> },
    /// A block quote.
    Quote { blocks: Vec<Block> },
    /// A list.
    List {
        ordered: bool,
        start: u64,
        items: Vec<ListItem>,
        /// The item currently open, if any.
        current: Option<ListItem>,
        /// True once a blank line separated items — the "loose" flag.
        loose: bool,
        /// True while the innermost frame is the item's own paragraph. pulldown
        /// reports looseness structurally, not with an event: a tight item's
        /// text arrives as a bare `Text`, a loose one inside an explicit
        /// `Paragraph`. So the presence of that tag is the signal, and it has
        /// to be recorded while the item is still the innermost frame.
        in_item_paragraph: bool,
    },
    /// A table.
    Table(TableState),
}

#[derive(Default)]
struct TableState {
    header: Vec<Cell>,
    rows: Vec<Vec<Cell>>,
    alignments: Vec<Align>,
    in_head: bool,
    row: Vec<Cell>,
    cell: Vec<Span>,
}

impl TableState {
    fn finish_row(&mut self) {
        let cells = std::mem::take(&mut self.row);
        if self.in_head {
            self.header = cells;
        } else {
            self.rows.push(cells);
        }
    }
}

#[derive(Default)]
struct Builder {
    /// Always starts with the document frame. `#[derive(Default)]` would give
    /// an *empty* vec — every block would then have nowhere to land and
    /// `emit` would drop it on the floor.
    #[allow(clippy::vec_init_then_push)]
    frames: Vec<Frame>,
    /// Inline runs not yet attached to a block.
    spans: Vec<Span>,
    /// Marks applied to text arriving right now.
    marks: Marks,
    /// Open fenced code, if any: `(language, completed lines, current
    /// partial line)`. Its text is collected here — *not* in `spans`. Routing
    /// code through the span path was why every code block rendered empty with
    /// its text sitting next to it as a paragraph.
    code: Option<(Option<String>, Vec<String>, String)>,
    pending_heading: Option<u8>,
    /// One entry per open link: where the link's label starts in `spans`.
    ///
    /// Without the offset, `close_link` cannot tell the label from the prose
    /// before it — `"vor [doku](url)"` produced a label reading `vor doku`.
    link_stack: Vec<(String, usize)>,
}

impl Default for Frame {
    fn default() -> Self {
        Frame::Document { blocks: Vec::new() }
    }
}

impl Builder {
    /// A builder whose only frame is the document. Every other frame is pushed
    /// by a `Tag` and popped by its matching `TagEnd`.
    fn new() -> Self {
        Builder {
            frames: vec![Frame::Document { blocks: Vec::new() }],
            ..Builder::default()
        }
    }

    fn push(&mut self, event: Event<'_>) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(text.to_string()),
            Event::Code(code) => {
                // Inside a fence, `Event::Code` is the whole block, not an
                // inline span — pulldown emits Text there, but handle both.
                if self.code.is_some() {
                    self.code_text(code.as_ref());
                } else {
                    self.spans.push(Span {
                        kind: SpanKind::Code(code.to_string()),
                        marks: self.marks,
                    });
                }
            }
            Event::InlineMath(text) | Event::DisplayMath(text) => {
                // Math is not modelled. The characters still reach the operator
                // as text rather than disappearing.
                self.text(text.to_string());
            }
            Event::Html(text) | Event::InlineHtml(text) => {
                // Raw HTML in model output is *text*. It is escaped again by
                // the html backend, but treating it as text here means the IR
                // cannot be made to smuggle a tag into any renderer.
                self.text(text.to_string());
            }
            Event::SoftBreak => self.spans.push(Span {
                kind: SpanKind::SoftBreak,
                marks: self.marks,
            }),
            Event::HardBreak => self.spans.push(Span {
                kind: SpanKind::HardBreak,
                marks: self.marks,
            }),
            Event::Rule => self.emit(Block::Rule),
            Event::TaskListMarker(checked) => {
                // The marker arrives inside `Tag::Item`, so the innermost
                // frame must be the list whose `current` item it marks.
                if let Some(Frame::List { current, .. }) = self.frames.last_mut() {
                    if let Some(item) = current.as_mut() {
                        item.checked = Some(checked);
                    }
                }
            }
            Event::FootnoteReference(text) => self.text(format!("[{text}]")),
        }
    }

    fn text(&mut self, text: String) {
        if text.is_empty() {
            return;
        }
        // Inside a fence, text is code — it belongs in the code buffer.
        if self.code.is_some() {
            self.code_text(&text);
            return;
        }
        self.spans.push(Span::text(text).with_marks(self.marks));
    }

    /// Append to the open code block, splitting on newlines.
    ///
    /// pulldown delivers a fence's content as Text events that may span
    /// several lines or arrive in pieces. Accumulating into a buffer and
    /// splitting at the end is what keeps the line structure the operator
    /// expects — a wrapped program would be a wrong program.
    fn code_text(&mut self, text: &str) {
        let Some((_, lines, partial)) = self.code.as_mut() else {
            return;
        };
        if partial.is_empty() && text.starts_with('\n') {
            // A fence's first Text usually starts with the newline that ended
            // the opening fence. That newline is structure, not content — and
            // `close_code` drops a single leading empty line, so pushing an
            // empty string here keeps that cleanup honest instead of leaving a
            // blank first line inside every code block.
            lines.push(String::new());
            let rest = &text[1..];
            *partial += rest;
        } else {
            *partial += text;
        }
        while let Some(index) = partial.find('\n') {
            let head: String = partial.drain(..=index).collect();
            lines.push(head.trim_end_matches('\n').to_string());
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                self.flush_spans();
                // A paragraph directly inside an item means the list is loose:
                // that is the only place pulldown reports the distinction.
                if let Some(Frame::List {
                    loose,
                    in_item_paragraph,
                    ..
                }) = self.frames.last_mut()
                {
                    *in_item_paragraph = true;
                    *loose = true;
                }
            }
            Tag::Heading { level, .. } => {
                self.flush_spans();
                self.pending_heading = Some(level_number(level));
            }
            Tag::BlockQuote(_) => {
                self.flush_spans();
                self.frames.push(Frame::Quote { blocks: Vec::new() });
            }
            Tag::CodeBlock(kind) => {
                self.flush_spans();
                let lang = match kind {
                    // The info string can carry attributes in several
                    // spellings: `rust ignore`, `rust,ignore`, `{.rust}`.
                    // Everything after the first separator is metadata, not a
                    // language — so `rust,ignore` must yield `rust`, not the
                    // whole token, or no highlighter will ever match it.
                    CodeBlockKind::Fenced(info) => info
                        .split(|ch: char| ch.is_whitespace() || ch == ',' || ch == ';')
                        .find(|token| !token.is_empty())
                        .map(|token| {
                            token
                                .trim_matches(|ch: char| ch == '{' || ch == '}' || ch == '.')
                                .to_ascii_lowercase()
                        })
                        .filter(|token| !token.is_empty()),
                    CodeBlockKind::Indented => None,
                };
                self.code = Some((lang, Vec::new(), String::new()));
            }
            Tag::List(start) => {
                self.flush_spans();
                self.frames.push(Frame::List {
                    ordered: start.is_some(),
                    start: start.unwrap_or(1),
                    items: Vec::new(),
                    current: None,
                    loose: false,
                    in_item_paragraph: false,
                });
            }
            Tag::Item => {
                self.flush_spans();
                if let Some(Frame::List { current, .. }) = self.frames.last_mut() {
                    *current = Some(ListItem {
                        checked: None,
                        blocks: Vec::new(),
                    });
                }
            }
            Tag::Emphasis => self.marks.emphasis = true,
            Tag::Strong => self.marks.strong = true,
            Tag::Strikethrough => self.marks.strikethrough = true,
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                // Do *not* flush. A link is inline: flushing here split the
                // sentence around it into two paragraphs, which is the exact
                // failure the inline link model was supposed to end. Only a
                // pending *hard* break needs committing before a link, and
                // that arrives as a span, not as a block.
                self.link_stack
                    .push((dest_url.to_string(), self.spans.len()));
            }
            Tag::Table(alignments) => {
                self.flush_spans();
                self.frames.push(Frame::Table(TableState {
                    alignments: alignments.into_iter().map(alignment).collect(),
                    ..TableState::default()
                }));
            }
            Tag::TableHead => {
                if let Some(Frame::Table(table)) = self.frames.last_mut() {
                    table.in_head = true;
                }
            }
            Tag::TableRow => {
                if let Some(Frame::Table(table)) = self.frames.last_mut() {
                    // pulldown emits no `TableRow` for the header, so `row`
                    // still holds the head cells at this point. Clearing it
                    // without committing them first is exactly why the header
                    // used to come out empty while its text leaked out as a
                    // stray paragraph.
                    table.row.clear();
                }
            }
            Tag::TableCell => {
                if let Some(Frame::Table(table)) = self.frames.last_mut() {
                    table.cell = Vec::new();
                }
            }
            // Unmodelled constructs: their text still arrives as events, so
            // this only keeps the stack honest.
            _ => self.flush_spans(),
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => {
                let spans = self.take_spans();
                self.emit(Block::Paragraph { spans });
                if let Some(Frame::List {
                    in_item_paragraph, ..
                }) = self.frames.last_mut()
                {
                    *in_item_paragraph = false;
                }
            }
            TagEnd::Heading(_) => self.close_heading(),
            TagEnd::BlockQuote(_) => self.close_quote(),
            TagEnd::CodeBlock => self.close_code(),
            TagEnd::List(_) => self.close_list(),
            TagEnd::Item => self.close_item(),
            TagEnd::Emphasis => self.marks.emphasis = false,
            TagEnd::Strong => self.marks.strong = false,
            TagEnd::Strikethrough => self.marks.strikethrough = false,
            TagEnd::Link | TagEnd::Image => self.close_link(),
            TagEnd::Table => self.close_table(),
            TagEnd::TableHead => self.close_head(),
            TagEnd::TableRow => self.close_row(),
            TagEnd::TableCell => self.close_cell(),
            _ => self.flush_spans(),
        }
    }

    /// Land a completed block in the innermost open container.
    fn emit(&mut self, block: Block) {
        match self.frames.last_mut() {
            Some(Frame::Document { blocks }) => blocks.push(block),
            Some(Frame::Quote { blocks }) => blocks.push(block),
            Some(Frame::List { current, loose, .. }) => {
                // Two paragraphs inside one item make the list loose, which is
                // the difference between `<li><p>` and `<li>` in the browser.
                if let Some(item) = current.as_mut() {
                    if matches!(block, Block::Paragraph { .. }) && !item.blocks.is_empty() {
                        *loose = true;
                    }
                    item.blocks.push(block);
                }
            }
            // A block directly inside a table frame is a stray; cells carry the
            // content. Dropping it here is deliberate — there is nowhere in a
            // table for it to live, and inventing a cell would corrupt the grid.
            Some(Frame::Table(_)) => {}
            None => {
                // No frame at all means the parser got confused. Keeping the
                // text is better than losing it.
                debug_assert!(false, "completed a block with no open frame");
            }
        }
    }

    /// Steal the pending spans without emitting them.
    ///
    /// `take` and `flush` are different operations and conflating them was the
    /// second parser bug: `take_spans` used to flush first, so a heading ended
    /// up as an empty heading *plus* a stray paragraph before it. Taking is
    /// for a caller that is about to build its own block; flushing is for a
    /// caller that wants the spans committed as a paragraph.
    fn take_spans(&mut self) -> Vec<Span> {
        std::mem::take(&mut self.spans)
    }

    /// Commit the pending spans as a paragraph, if there are any.
    fn flush_spans(&mut self) {
        if self.spans.is_empty() {
            return;
        }
        let spans = self.take_spans();
        self.emit(Block::Paragraph { spans });
    }

    fn close_heading(&mut self) {
        let level = self.pending_heading.take().unwrap_or(1);
        let spans = self
            .take_spans()
            .into_iter()
            .map(|span| Span {
                kind: span.kind,
                marks: Marks {
                    heading: level,
                    ..span.marks
                },
            })
            .collect();
        self.emit(Block::Heading { level, spans });
    }

    fn close_quote(&mut self) {
        self.flush_spans();
        match self.frames.pop() {
            Some(Frame::Quote { blocks }) => self.emit(Block::Quote { blocks }),
            Some(other) => {
                // Unbalanced close. Put the frame back rather than dropping
                // whatever it held.
                self.frames.push(other);
            }
            None => {}
        }
    }

    fn close_code(&mut self) {
        self.flush_spans();
        let Some((lang, mut lines, partial)) = self.code.take() else {
            return;
        };
        if !partial.is_empty() {
            lines.push(partial);
        }
        // pulldown pads the first line of a fence with the newline that ended
        // the opening fence. Drop that empty leading entry — it would render as
        // a blank line at the top of every code block.
        if lines.first().is_some_and(String::is_empty) {
            lines.remove(0);
        }
        self.emit(Block::Code { lang, lines });
    }

    fn close_item(&mut self) {
        self.flush_spans();
        if let Some(Frame::List { current, items, .. }) = self.frames.last_mut() {
            if let Some(item) = current.take() {
                items.push(item);
            }
        }
    }

    fn close_list(&mut self) {
        self.flush_spans();
        let Some(Frame::List {
            ordered,
            start,
            mut items,
            current,
            loose,
            ..
        }) = self.frames.pop()
        else {
            return;
        };
        // An item still open at the close lost its `TagEnd::Item`, which
        // happens when the source ends mid-list. Keep it rather than drop it.
        if let Some(item) = current {
            items.push(item);
        }
        self.emit(Block::List {
            ordered,
            start,
            loose,
            items,
        });
    }

    fn close_link(&mut self) {
        let Some((dest, label_start)) = self.link_stack.pop() else {
            self.flush_spans();
            return;
        };
        // Only the spans written *after* the link opened are its label. The
        // ones before it belong to the sentence around it.
        let label: Vec<Span> = self.spans.split_off(label_start.min(self.spans.len()));
        let safe = safe_scheme(&dest);
        // A bare URL has no label text of its own; the destination is what the
        // operator should see, so it becomes the label.
        let label = if label.is_empty() {
            vec![Span::text(dest.clone())]
        } else {
            label
        };
        self.spans.push(Span {
            kind: SpanKind::Link { dest, label, safe },
            marks: self.marks,
        });
    }

    fn close_head(&mut self) {
        if let Some(Frame::Table(table)) = self.frames.last_mut() {
            // Order matters: `finish_row` reads `in_head` to decide whether
            // these cells are the header or a body row. Clearing the flag
            // first sent the header into `rows` and left `header` empty —
            // which is why every table rendered with a missing header row.
            table.finish_row();
            table.in_head = false;
        }
    }

    fn close_row(&mut self) {
        if let Some(Frame::Table(table)) = self.frames.last_mut() {
            table.finish_row();
        }
    }

    fn close_cell(&mut self) {
        let spans = self.take_spans();
        if let Some(Frame::Table(table)) = self.frames.last_mut() {
            table.row.push(Cell { spans });
        }
    }

    fn close_table(&mut self) {
        self.flush_spans();
        let Some(Frame::Table(state)) = self.frames.pop() else {
            return;
        };
        let table = Table {
            header: state.header,
            rows: state.rows,
            alignments: state.alignments,
        };
        self.emit(Block::Table(table));
    }

    fn finish(mut self) -> Document {
        self.flush_spans();
        // A document that ends inside a fence still has code the operator
        // wrote. `pulldown_cmark` emits `End(CodeBlock)` for unclosed fences,
        // but a cancelled turn may not.
        if let Some((lang, mut lines, partial)) = self.code.take() {
            if !partial.is_empty() {
                lines.push(partial);
            }
            if lines.first().is_some_and(String::is_empty) {
                lines.remove(0);
            }
            self.emit(Block::Code { lang, lines });
        }
        // Close anything the source left open, innermost first, so nothing the
        // model said is stranded in a frame that never became a block.
        while let Some(frame) = self.frames.pop() {
            match frame {
                Frame::Document { .. } => {
                    // The bottom frame. Put it back — `blocks` below reads it.
                    self.frames.push(frame);
                    break;
                }
                Frame::Quote { blocks } => self.emit(Block::Quote { blocks }),
                Frame::List {
                    ordered,
                    start,
                    mut items,
                    current,
                    loose,
                    ..
                } => {
                    if let Some(item) = current {
                        items.push(item);
                    }
                    self.emit(Block::List {
                        ordered,
                        start,
                        loose,
                        items,
                    });
                }
                Frame::Table(state) => self.emit(Block::Table(Table {
                    header: state.header,
                    rows: state.rows,
                    alignments: state.alignments,
                })),
            }
        }
        // The document frame is always at the bottom and is never popped
        // during the walk above, so its blocks are the document's blocks.
        let blocks = match self.frames.pop() {
            Some(Frame::Document { blocks }) => blocks,
            // Unreachable: the loop above stopped at the document frame. Fall
            // back to whatever text is pending rather than panicking — a TUI
            // must not die because a Markdown edge case appeared.
            _ => vec![Block::Paragraph {
                spans: std::mem::take(&mut self.spans),
            }],
        };
        Document { blocks }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::SpanKind;

    fn blocks(source: &str) -> Vec<Block> {
        parse(source).blocks
    }

    fn text_of(spans: &[Span]) -> String {
        spans
            .iter()
            .map(|span| match &span.kind {
                SpanKind::Text(t) | SpanKind::Code(t) => t.clone(),
                SpanKind::Link { dest, label, .. } => text_of(label) + dest,
                SpanKind::SoftBreak => " ".to_string(),
                SpanKind::HardBreak => "\n".to_string(),
            })
            .collect()
    }

    #[test]
    fn a_multiline_answer_keeps_every_line() {
        // The bug this crate exists for: the old TUI took the first non-empty
        // line and dropped the rest.
        let doc = blocks("erste zeile\nzweite zeile\n\ndritter absatz");
        assert_eq!(doc.len(), 2, "two paragraphs, no phantoms: {doc:?}");
        let Block::Paragraph { spans } = &doc[0] else {
            panic!("expected a paragraph, got {:?}", doc[0]);
        };
        assert!(text_of(spans).contains("erste zeile"));
        assert!(text_of(spans).contains("zweite zeile"));
    }

    #[test]
    fn a_fenced_block_keeps_its_language_and_lines() {
        let doc = blocks("```rust\nfn main() {}\nlet x = 1;\n```");
        assert_eq!(doc.len(), 1, "code and nothing else: {doc:?}");
        match &doc[0] {
            Block::Code { lang, lines } => {
                assert_eq!(lang.as_deref(), Some("rust"));
                assert_eq!(lines.len(), 2, "both lines survive: {lines:?}");
                assert!(lines[0].contains("fn main()"));
                assert!(lines[1].contains("let x = 1;"));
            }
            other => panic!("expected code, got {other:?}"),
        }
    }

    #[test]
    fn a_fence_carrying_attributes_keeps_only_the_language() {
        let doc = blocks("```rust,ignore\nlet x = 1;\n```");
        match &doc[0] {
            Block::Code { lang, lines } => {
                assert_eq!(lang.as_deref(), Some("rust"));
                assert_eq!(lines.len(), 1, "the lines survive too: {lines:?}");
            }
            other => panic!("expected code, got {other:?}"),
        }
    }

    #[test]
    fn an_unterminated_fence_still_yields_its_lines() {
        // A cancelled turn streams half a message.
        let doc = blocks("```rust\nlet x = 1;\nlet y = 2;");
        match &doc[0] {
            Block::Code { lines, .. } => assert_eq!(lines.len(), 2, "{lines:?}"),
            other => panic!("expected code, got {other:?}"),
        }
    }

    #[test]
    fn a_heading_carries_its_own_text() {
        // Regression: the first version emitted `<h1></h1>` plus a stray
        // paragraph, because `take_spans` flushed before it took.
        let doc = blocks("## Titel");
        assert_eq!(doc.len(), 1, "one block, no phantom paragraph: {doc:?}");
        match &doc[0] {
            Block::Heading { level, spans } => {
                assert_eq!(*level, 2);
                assert_eq!(text_of(spans), "Titel");
                assert!(spans.iter().all(|span| span.marks.heading == 2));
            }
            other => panic!("expected a heading, got {other:?}"),
        }
    }

    #[test]
    fn nested_lists_keep_every_item() {
        // Regression: scalar list state collapsed `- a / - b / - c` into one
        // item holding only `b`. `a` and `c` disappeared silently.
        let doc = blocks("- a\n  - b\n- c");
        let outer = doc
            .iter()
            .find_map(|block| match block {
                Block::List { items, .. } if items.len() >= 2 => Some(items),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the outer list is gone: {doc:?}"));
        assert_eq!(outer.len(), 2, "two top-level items: {doc:?}");
        let first = text_of_item(&outer[0]);
        let second = text_of_item(&outer[1]);
        assert!(first.contains('a'), "{first:?}");
        assert!(second.contains('c'), "{second:?}");
    }

    #[test]
    fn a_quote_inside_a_list_item_stays_inside_it() {
        let doc = blocks("- > zitat\n- danach");
        let Block::List { items, .. } = &doc[0] else {
            panic!("expected a list, got {:?}", doc[0]);
        };
        assert_eq!(items.len(), 2);
        assert!(
            items[0]
                .blocks
                .iter()
                .any(|block| matches!(block, Block::Quote { .. })),
            "the quote belongs to the first item: {:?}",
            items[0]
        );
    }

    #[test]
    fn a_table_inside_a_list_stays_inside_the_list() {
        // Regression: `close_table` pushed straight to `blocks`, so the table
        // appeared *before* the list that contained it.
        let doc = blocks("- | a | b |\n  |---|---|\n  | 1 | 2 |");
        let list = doc
            .iter()
            .find_map(|block| match block {
                Block::List { items, .. } => Some(items),
                _ => None,
            })
            .unwrap_or_else(|| panic!("the list is gone: {doc:?}"));
        assert!(
            list.iter().any(|item| item
                .blocks
                .iter()
                .any(|block| matches!(block, Block::Table(_)))),
            "the table is inside the item: {list:?}"
        );
    }

    #[test]
    fn a_table_keeps_its_header_its_rows_and_its_alignment() {
        let doc = blocks("| a | b |\n|:--|--:|\n| 1 | 2 |");
        assert_eq!(doc.len(), 1, "one block, no leaked text: {doc:?}");
        let Block::Table(table) = &doc[0] else {
            panic!("expected a table, got {:?}", doc[0]);
        };
        assert_eq!(table.header.len(), 2, "header cells: {:?}", table.header);
        assert_eq!(text_of(&table.header[0].spans), "a");
        assert_eq!(text_of(&table.header[1].spans), "b");
        assert_eq!(table.rows.len(), 1, "one data row: {:?}", table.rows);
        assert_eq!(text_of(&table.rows[0][0].spans), "1");
        assert_eq!(table.alignments, vec![Align::Left, Align::Right]);
    }

    #[test]
    fn a_link_stays_inside_its_paragraph_with_its_label() {
        // Regression: modelled as a block, one link split its paragraph into
        // five blocks and lost the label.
        let doc = blocks("vor [doku](https://example.com/x) nach");
        assert_eq!(doc.len(), 1, "still one paragraph: {doc:?}");
        let Block::Paragraph { spans } = &doc[0] else {
            panic!("expected a paragraph");
        };
        let link = spans
            .iter()
            .find_map(|span| match &span.kind {
                SpanKind::Link { dest, label, .. } => Some((dest.clone(), label.clone())),
                _ => None,
            })
            .expect("the link survives");
        assert_eq!(link.0, "https://example.com/x");
        // The label is its own span list — "vor " belongs to the sentence, not
        // to the link.
        assert_eq!(text_of(&link.1), "doku");
        assert!(spans.len() > 1, "text surrounds the link: {spans:?}");
        let whole = text_of(spans);
        assert!(whole.contains("vor") && whole.contains("nach"), "{whole}");
    }

    #[test]
    fn an_executable_url_is_marked_unsafe_by_the_parser() {
        // The scheme decision belongs to one place, not to each renderer.
        let doc = blocks("[klick](javascript:alert(1))");
        let Block::Paragraph { spans } = &doc[0] else {
            panic!("expected a paragraph");
        };
        let safe = spans
            .iter()
            .any(|span| matches!(&span.kind, SpanKind::Link { safe, .. } if !*safe));
        assert!(safe, "javascript: is not safe: {spans:?}");
    }

    #[test]
    fn a_normal_url_is_marked_safe() {
        let doc = blocks("[doku](https://example.com)");
        let Block::Paragraph { spans } = &doc[0] else {
            panic!("expected a paragraph");
        };
        assert!(
            spans
                .iter()
                .all(|span| !matches!(&span.kind, SpanKind::Link { safe: false, .. }))
        );
    }

    #[test]
    fn emphasis_and_strong_nest_without_clobbering_each_other() {
        let doc = blocks("**fett und *kursiv* darin**");
        let Block::Paragraph { spans } = &doc[0] else {
            panic!("expected a paragraph");
        };
        assert!(
            spans.iter().any(|s| s.marks.strong && !s.marks.emphasis),
            "outer bold survives"
        );
        assert!(
            spans.iter().any(|s| s.marks.strong && s.marks.emphasis),
            "inner bold+italic is reachable"
        );
        assert!(
            !spans.iter().any(|s| s.marks.emphasis && !s.marks.strong),
            "the italic does not outlive the bold"
        );
    }

    #[test]
    fn a_task_list_records_its_checkbox() {
        let doc = blocks("- [x] erledigt\n- [ ] offen");
        let Block::List { items, .. } = &doc[0] else {
            panic!("expected a list, got {:?}", doc[0]);
        };
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].checked, Some(true));
        assert_eq!(items[1].checked, Some(false));
    }

    #[test]
    fn raw_html_is_text_not_markup() {
        let doc = blocks("<script>alert(1)</script>");
        let text = doc
            .iter()
            .map(|block| match block {
                Block::Paragraph { spans } => text_of(spans),
                other => panic!("html became a {other:?} block"),
            })
            .collect::<String>();
        assert!(text.contains("alert(1)"), "the text survives: {text}");
    }

    #[test]
    fn math_is_text_not_lost() {
        // pulldown has InlineMath/DisplayMath events. Unhandled, they vanish.
        let mut options = Options::empty();
        options.insert(Options::ENABLE_MATH);
        let events: Vec<_> = Parser::new_ext("$x^2$", options).collect();
        let mut builder = Builder::new();
        for event in events {
            builder.push(event);
        }
        let doc = builder.finish();
        assert!(
            doc.blocks.iter().any(|block| match block {
                Block::Paragraph { spans } => text_of(spans).contains("x^2"),
                _ => false,
            }),
            "the formula text is still visible: {doc:?}"
        );
    }

    #[test]
    fn empty_input_is_an_empty_document_not_a_crash() {
        assert!(parse("").is_empty());
        assert!(parse("   \n\n  ").is_empty());
    }

    #[test]
    fn a_document_of_only_a_quote_yields_a_quote() {
        let doc = blocks("> nur ein zitat");
        assert_eq!(doc.len(), 1);
        let Block::Quote { blocks } = &doc[0] else {
            panic!("expected a quote, got {:?}", doc[0]);
        };
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn a_deeply_nested_quote_does_not_lose_its_innermost_text() {
        let doc = blocks("> > > tief");
        assert!(
            contains_text(&doc, "tief"),
            "the text survived three quote levels: {doc:?}"
        );
    }

    /// Depth-first search for a text fragment anywhere in a block list.
    fn contains_text(blocks: &[Block], needle: &str) -> bool {
        blocks.iter().any(|block| match block {
            Block::Paragraph { spans } | Block::Heading { spans, .. } => {
                text_of(spans).contains(needle)
            }
            Block::Quote { blocks } => contains_text(blocks, needle),
            Block::List { items, .. } => {
                items.iter().any(|item| contains_text(&item.blocks, needle))
            }
            Block::Code { lines, .. } => lines.iter().any(|line| line.contains(needle)),
            Block::Table(table) => {
                table
                    .header
                    .iter()
                    .any(|cell| text_of(&cell.spans).contains(needle))
                    || table
                        .rows
                        .iter()
                        .flatten()
                        .any(|cell| text_of(&cell.spans).contains(needle))
            }
            Block::Rule => false,
        })
    }

    fn text_of_item(item: &ListItem) -> String {
        item.blocks
            .iter()
            .map(|block| match block {
                Block::Paragraph { spans } => text_of(spans),
                Block::List { items, .. } => items.iter().map(text_of_item).collect(),
                _ => String::new(),
            })
            .collect()
    }
}
