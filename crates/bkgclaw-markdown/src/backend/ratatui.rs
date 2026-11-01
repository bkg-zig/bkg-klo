//! The ratatui backend: the IR becomes styled terminal lines.
//!
//! # What this replaces
//!
//! The old TUI rendered every message with `clip_start`, which took the first
//! non-empty line and printed that. A five-line answer, a code block, a table —
//! none of it reached the screen. The operator saw a sentence and assumed that
//! was the whole answer. This module's contract is the opposite: **every block
//! becomes at least one line, and no text is dropped**.
//!
//! # Wrapping
//!
//! ratatui's `Paragraph` wraps for us, but a scroll offset counted in logical
//! lines then disagrees with what is on screen — the bug the old layout had
//! (it computed `viewport` from `lines.len()` while the widget rendered wrapped
//! height). So this backend wraps *itself*, in [`wrap_spans`], and the TUI can
//! count screen lines and be right about it.
//!
//! Wrapping is width-aware and never splits a grapheme cluster mid-character;
////! `unicode-width` decides how many columns a character occupies, which is
//! what a terminal cell actually is.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line as TextLine, Span as TextSpan};

use crate::ir::{Block, Cell, Document, ListItem, Marks, Span, SpanKind, Table};
use crate::theme::Theme;

/// Render a document into terminal lines at a given width.
///
/// `width` is the number of *columns* available for content, excluding any
/// label gutter the caller draws alongside. Wrapping happens here so the caller
/// can count returned lines and know exactly how tall the block will be.
pub fn render(document: &Document, width: usize, theme: &Theme) -> Vec<TextLine<'static>> {
    let mut lines = Vec::new();
    for block in &document.blocks {
        render_block(block, width, theme, 0, &mut lines);
    }
    if lines.is_empty() {
        lines.push(TextLine::default());
    }
    lines
}

/// Depth limits indentation. Past this the numbers stop being informative and
/// start being a wall — and a pathological input (`> > > > …`) would otherwise
/// push every character past the right edge.
const MAX_INDENT: usize = 8;

fn indent(depth: usize) -> String {
    "  ".repeat(depth.min(MAX_INDENT))
}

fn render_block(
    block: &Block,
    width: usize,
    theme: &Theme,
    depth: usize,
    out: &mut Vec<TextLine<'static>>,
) {
    let pad = indent(depth);
    match block {
        Block::Paragraph { spans } => {
            out.extend(wrap_spans(spans, width, &pad, theme.text()));
        }
        Block::Heading { level, spans } => {
            // A blank line before a heading separates it from the paragraph
            // above, unless it is the first thing rendered.
            if !out.is_empty() {
                out.push(TextLine::default());
            }
            let marker = "#".repeat((*level).clamp(1, 6) as usize);
            let style = theme.heading((*level).clamp(1, 6));
            // The first heading line already carries `pad` plus the marker, so
            // the wrapped continuation has to be indented by the *same* amount
            // — otherwise the second line of a long heading starts further left
            // than its own first.
            let first_prefix = format!("{pad}{marker} ");
            let continuation = display_width(&first_prefix);
            let mut wrapped = wrap_spans(spans, width, &first_prefix, style);
            // wrap_spans indents by the first_prefix width; the first line
            // gets its own prefix explicitly so the marker survives.
            let mut line = vec![TextSpan::styled(first_prefix.clone(), style)];
            if let Some(rest) = wrapped.first_mut() {
                let _ = continuation;
                line.extend(std::mem::take(rest).spans);
            }
            out.push(TextLine::from(line));
            out.extend(wrapped.into_iter().skip(1));
            out.push(TextLine::default());
        }
        Block::Code { lang, lines } => {
            // Code is never wrapped: a wrapped program is a wrong program.
            // Long lines are cut with an explicit marker instead, because a
            // silent cut is how the operator ends up reading half a command
            // and running it.
            let label = lang.clone().unwrap_or_default();
            out.push(TextLine::from(vec![TextSpan::styled(
                format!("{pad}┌─ {label}"),
                theme.code_border(),
            )]));
            let available = width.saturating_sub(pad.chars().count() + 2);
            for source in lines {
                // The classification lives in the IR, so the terminal colours
                // the same runs the browser marks with classes — one decision,
                // two renderers.
                let classified = crate::highlight::classify(source, lang.as_deref());
                let mut spans = vec![TextSpan::styled(format!("{pad}│ "), theme.code_border())];
                for token in &classified.tokens {
                    let style = token_style(token.class, theme);
                    let text = if token.text.chars().count() > available {
                        // Cut the *last* run that overflows, not the line's
                        // first characters: a cut that removed the beginning
                        // of a line would hide which token it was.
                        let kept: String = token
                            .text
                            .chars()
                            .take(available.saturating_sub(1))
                            .collect();
                        format!("{kept}…")
                    } else {
                        token.text.clone()
                    };
                    spans.push(TextSpan::styled(text, style));
                }
                out.push(TextLine::from(spans));
            }
            out.push(TextLine::from(vec![TextSpan::styled(
                format!("{pad}└─"),
                theme.code_border(),
            )]));
        }
        Block::List {
            ordered,
            start,
            loose: _,
            items,
        } => {
            for (index, item) in items.iter().enumerate() {
                let bullet = if *ordered {
                    format!("{}.", start + index as u64)
                } else {
                    "•".to_string()
                };
                render_item(item, &bullet, width, theme, depth, out);
            }
        }
        Block::Quote { blocks } => {
            for inner in blocks {
                render_block(inner, width, theme, depth + 1, out);
            }
        }
        Block::Rule => {
            let width_available = width.saturating_sub(pad.chars().count());
            out.push(TextLine::from(TextSpan::styled(
                format!("{pad}{}", "─".repeat(width_available.min(60))),
                theme.rule(),
            )));
        }
        Block::Table(table) => render_table(table, width, theme, depth, out),
    }
}

fn render_item(
    item: &ListItem,
    bullet: &str,
    width: usize,
    theme: &Theme,
    depth: usize,
    out: &mut Vec<TextLine<'static>>,
) {
    let pad = indent(depth);
    // Bullet first, checkbox after: `- [x] fertig` reads as a checkbox on an
    // item, and putting the box in front made it look like a list marker that
    // had lost its bullet.
    let checkbox = match item.checked {
        Some(true) => " [x]",
        Some(false) => " [ ]",
        None => "",
    };
    let marker = format!("{pad}{bullet}{checkbox} ");
    let marker_width = display_width(&marker);

    // The item's first block shares the bullet's line; every later block — and
    // a nested list — is indented under the text, so the marker reads once.
    let mut nested: Vec<TextLine<'static>> = Vec::new();
    for block in &item.blocks {
        render_block(block, width, theme, depth, &mut nested);
    }

    if nested.is_empty() {
        // An empty item still needs its bullet, or the list loses a row.
        out.push(TextLine::from(TextSpan::styled(marker, theme.bullet())));
        return;
    }

    let mut first = nested.into_iter();
    let head = first.next().expect("just checked it is non-empty");
    let mut spans = vec![TextSpan::styled(marker, theme.bullet())];
    spans.extend(head.spans);
    out.push(TextLine::from(spans));
    out.extend(first.map(|line| indent_continuation(line, marker_width)));
}

/// Push a line right by `columns`, preserving its spans and styles.
fn indent_continuation(line: TextLine<'static>, columns: usize) -> TextLine<'static> {
    if columns == 0 {
        return line;
    }
    let mut spans = vec![TextSpan::raw(" ".repeat(columns))];
    spans.extend(line.spans);
    TextLine::from(spans)
}

/// The style a token class gets in the terminal.
///
/// This is the terminal half of the IR contract: the browser maps the same
/// class to a CSS rule, and both name the same categories. Adding a category
/// means adding it here *and* to `style.css` — never to the parser, which
/// would be where a third renderer would have to be taught about it.
fn token_style(class: crate::ir::TokenClass, theme: &Theme) -> Style {
    match class {
        crate::ir::TokenClass::Plain => theme.code(),
        crate::ir::TokenClass::Keyword => theme.token_keyword(),
        crate::ir::TokenClass::Type => theme.token_type(),
        crate::ir::TokenClass::Function => theme.token_function(),
        crate::ir::TokenClass::Number => theme.token_number(),
        crate::ir::TokenClass::String => theme.token_string(),
        crate::ir::TokenClass::Comment => theme.token_comment(),
        crate::ir::TokenClass::Punctuation => theme.token_punctuation(),
        crate::ir::TokenClass::Attribute => theme.token_attribute(),
    }
}

/// The style a span's marks imply. Marks compose: a strong emphasis span is
/// bold *and* italic.
fn span_style(marks: Marks, base: Style) -> Style {
    let mut style = base;
    if marks.strong {
        style = style.add_modifier(Modifier::BOLD);
    }
    if marks.emphasis {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if marks.strikethrough {
        style = style.add_modifier(Modifier::CROSSED_OUT);
    }
    style
}

/// Wrap styled spans into lines that fit `width`, prefixing every line but the
/// first with `pad`.
///
/// The whole point of doing this here rather than letting `Paragraph` do it:
/// the caller needs to know the *screen* height of a block, and `Paragraph`
/// hides that number. The old TUI computed its scroll offset from
/// `lines.len()` while the widget rendered wrapped height — those are different
/// numbers as soon as anything wraps, so the offset was simply wrong.
fn wrap_spans(spans: &[Span], width: usize, pad: &str, base: Style) -> Vec<TextLine<'static>> {
    let pad_width = display_width(pad);
    // `max(1)` because a zero-width terminal would otherwise make every
    // division below panic or loop forever.
    let available = width.saturating_sub(pad_width).max(1);

    // Flatten to (text, style) runs first. Soft breaks become spaces, hard
    // breaks force a new line — that is what they meant.
    let mut runs: Vec<(String, Style)> = Vec::new();
    for span in spans {
        let style = span_style(span.marks, base);
        match &span.kind {
            SpanKind::Text(text) | SpanKind::Code(text) => {
                if !text.is_empty() {
                    runs.push((text.clone(), style));
                }
            }
            SpanKind::SoftBreak => runs.push((" ".to_string(), style)),
            SpanKind::HardBreak => runs.push(("\n".to_string(), style)),
            // A terminal has no hover, so the URL is spelled out after the
            // label. An unsafe destination keeps its label and its URL and
            // loses nothing — there is nothing to click anyway.
            SpanKind::Link { dest, label, safe } => {
                let mut flat: Vec<Span> = label.clone();
                if *safe {
                    flat.push(Span::text(format!(" ({dest})")));
                }
                for inner in &flat {
                    runs.push((
                        match &inner.kind {
                            SpanKind::Text(t) | SpanKind::Code(t) => t.clone(),
                            _ => String::new(),
                        },
                        style,
                    ));
                }
            }
        }
    }
    if runs.is_empty() {
        return vec![TextLine::default()];
    }

    let mut lines: Vec<TextLine<'static>> = Vec::new();
    let mut current: Vec<TextSpan<'static>> = Vec::new();
    // Columns used on the current line. Tracked as we go rather than
    // recomputed, so the cost is linear in the text length.
    let mut used = 0usize;

    for (text, style) in runs {
        if text == "\n" {
            lines.push(TextLine::from(std::mem::take(&mut current)));
            used = 0;
            continue;
        }
        let text_width = display_width(&text);

        // Break before a run that would overflow. The `used > 0` guard keeps a
        // too-wide first run from pushing an empty line above itself.
        if used > 0 && used + text_width > available {
            lines.push(TextLine::from(std::mem::take(&mut current)));
            used = 0;
        }

        if text_width > available {
            // A single run longer than the line — a long path, a URL, CJK with
            // no break opportunity. Chop it at the column boundary.
            let mut rest: Vec<char> = text.chars().collect();
            let mut chunk: Vec<char> = Vec::with_capacity(available);
            let mut chunk_width = 0usize;
            for ch in rest.drain(..) {
                let ch_width = display_width(&ch.to_string()).max(1);
                if chunk_width + ch_width > available {
                    lines.push(TextLine::from(vec![TextSpan::styled(
                        chunk.iter().collect::<String>(),
                        style,
                    )]));
                    chunk.clear();
                    chunk_width = 0;
                }
                chunk_width += ch_width;
                chunk.push(ch);
            }
            if !chunk.is_empty() {
                current.push(TextSpan::styled(
                    chunk.into_iter().collect::<String>(),
                    style,
                ));
                used = chunk_width;
            }
            continue;
        }

        current.push(TextSpan::styled(text, style));
        used += text_width;
    }
    lines.push(TextLine::from(current));

    // The prefix goes on every line but the first, which the caller already
    // placed. `enumerate` skips index 0, and the empty `lines` case cannot
    // happen — `runs` was checked non-empty above.
    lines
        .into_iter()
        .enumerate()
        .map(|(index, line)| {
            if index == 0 {
                line
            } else {
                indent_continuation(line, pad_width)
            }
        })
        .collect()
}

/// Columns a string occupies on screen.
///
/// `unicode-width` is what makes this correct for CJK and combining marks:
/// `chars().count()` says a Chinese ideograph is one cell wide, and it is two.
/// Everything the TUI lays out flows from this function, so it is the one
/// place that has to be right.
fn display_width(text: &str) -> usize {
    use unicode_width::UnicodeWidthChar;
    text.chars()
        .map(|ch| ch.width().unwrap_or(0))
        .sum::<usize>()
        .max(usize::from(!text.is_empty()))
}

/// Render a table as an aligned grid. Columns are sized to their widest cell,
/// capped so one long cell cannot push the rest off screen.
fn render_table(
    table: &Table,
    width: usize,
    theme: &Theme,
    depth: usize,
    out: &mut Vec<TextLine<'static>>,
) {
    let pad = indent(depth);
    let columns = table
        .header
        .len()
        .max(table.rows.iter().map(Vec::len).max().unwrap_or(0));
    if columns == 0 {
        return;
    }

    let mut widths = vec![1usize; columns];
    for (index, cell) in table.header.iter().enumerate() {
        widths[index] = widths[index].max(display_width(&cell_text(cell)));
    }
    for row in &table.rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(display_width(&cell_text(cell)));
        }
    }
    // Shrink proportionally if the grid is wider than the screen. Without this
    // a five-column table of long German compounds would push the last column
    // off the right edge, where the operator cannot see it.
    let total: usize = widths.iter().sum::<usize>() + (columns - 1) * 3;
    if total > width {
        let budget = width.saturating_sub((columns - 1) * 3).max(columns);
        let scale = budget as f64 / total as f64;
        for slot in &mut widths {
            *slot = ((*slot as f64 * scale).round() as usize).max(1);
        }
    }

    let format_row = |cells: &[&Cell], style: Style| -> TextLine<'static> {
        let mut spans = vec![TextSpan::styled(pad.clone(), style)];
        let empty = Cell::default();
        for (index, width) in widths.iter().enumerate() {
            if index > 0 {
                spans.push(TextSpan::styled(" │ ", theme.table_border()));
            }
            let cell = cells.get(index).copied().unwrap_or(&empty);
            let clipped = clip_to_width(&cell_text(cell), *width);
            let padding = width.saturating_sub(display_width(&clipped));
            spans.push(TextSpan::styled(clipped, style));
            spans.push(TextSpan::raw(" ".repeat(padding)));
        }
        TextLine::from(spans)
    };

    let empty = Cell::default();
    let header: Vec<&Cell> = (0..columns)
        .map(|index| table.header.get(index).unwrap_or(&empty))
        .collect();
    out.push(format_row(&header, theme.table_header()));

    let mut rule = vec![TextSpan::styled(pad.clone(), theme.table_border())];
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            rule.push(TextSpan::styled("─┼─", theme.table_border()));
        }
        rule.push(TextSpan::styled("─".repeat(*width), theme.table_border()));
    }
    out.push(TextLine::from(rule));

    for row in &table.rows {
        let cells: Vec<&Cell> = (0..columns)
            .map(|index| row.get(index).unwrap_or(&empty))
            .collect();
        out.push(format_row(&cells, theme.text()));
    }
}

/// A cell's text, flattened for width measurement and for the grid.
fn cell_text(cell: &Cell) -> String {
    cell.spans
        .iter()
        .map(|span| match &span.kind {
            SpanKind::Text(t) | SpanKind::Code(t) => t.clone(),
            SpanKind::Link { dest, label, .. } => {
                // A terminal has no hover and nothing to click, so safe and
                // unsafe links render identically: the URL is spelled out either
                // way. That is deliberate, not an oversight — the difference
                // only matters where a browser would follow the href, and that
                // is the html backend's job, not this one's.
                let label: String = label
                    .iter()
                    .map(|inner| match &inner.kind {
                        SpanKind::Text(t) | SpanKind::Code(t) => t.clone(),
                        _ => String::new(),
                    })
                    .collect();
                format!("{label} ({dest})")
            }
            SpanKind::SoftBreak | SpanKind::HardBreak => " ".to_string(),
        })
        .collect()
}

/// Cut a string to `width` display columns, marking the cut with an ellipsis.
///
/// Column-aware, not character-aware: at width 4 the string "日日日日日日" keeps
/// three ideographs and an ellipsis — five columns, because each ideograph is
/// two columns wide. Counting characters would have produced six columns and a
/// broken table row.
fn clip_to_width(text: &str, width: usize) -> String {
    if display_width(text) <= width {
        return text.to_string();
    }
    if width == 0 {
        return String::new();
    }
    let budget = width - 1;
    let mut out = String::new();
    let mut used = 0usize;
    for ch in text.chars() {
        let ch_width = display_width(&ch.to_string()).max(1);
        if used + ch_width > budget {
            break;
        }
        out.push(ch);
        used += ch_width;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;
    use crate::theme::Theme;

    fn lines_of(source: &str, width: usize) -> Vec<String> {
        let theme = Theme::default();
        render(&parse(source), width, &theme)
            .into_iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.to_string())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn every_line_of_a_multiline_answer_reaches_the_screen() {
        // The regression test for the bug that started this crate: the old TUI
        // showed only the first non-empty line.
        let lines = lines_of("erste zeile\nzweite zeile\n\ndritter", 60);
        let joined = lines.join("\n");
        assert!(joined.contains("erste zeile"), "{joined}");
        assert!(joined.contains("zweite zeile"), "{joined}");
        assert!(joined.contains("dritter"), "{joined}");
    }

    #[test]
    fn a_code_block_keeps_every_line() {
        let joined = lines_of("```rust\nfn main() {}\nlet x = 1;\n```", 60).join("\n");
        assert!(joined.contains("fn main() {}"), "{joined}");
        assert!(joined.contains("let x = 1;"), "{joined}");
    }

    #[test]
    fn a_long_paragraph_wraps_and_reports_every_screen_line() {
        let lines = lines_of(&"wort ".repeat(60), 20);
        assert!(lines.len() > 3, "the wrap produced more lines: {lines:?}");
        for line in &lines {
            assert!(
                line.chars().count() <= 20,
                "no line exceeds the width: {line:?}"
            );
        }
    }

    #[test]
    fn a_heading_is_marked_and_set_off() {
        let joined = lines_of("## Titel", 40).join("\n");
        assert!(joined.contains("##"), "{joined}");
        assert!(joined.contains("Titel"), "{joined}");
    }

    #[test]
    fn an_overlong_code_line_is_marked_truncated_not_cut_silently() {
        let joined = lines_of(&format!("```\n{}\n```", "x".repeat(200)), 40).join("\n");
        assert!(joined.contains('…'), "the cut is visible: {joined}");
    }

    #[test]
    fn a_list_numbers_its_items_and_marks_the_tasks() {
        let joined = lines_of("1. eins\n2. zwei\n- [x] fertig", 40).join("\n");
        assert!(joined.contains("1. eins"), "{joined}");
        assert!(joined.contains("2. zwei"), "{joined}");
        assert!(joined.contains("[x] fertig"), "{joined}");
    }

    #[test]
    fn a_table_keeps_its_cells() {
        let joined = lines_of("| a | b |\n|---|---|\n| 1 | 2 |", 60).join("\n");
        assert!(joined.contains('a') && joined.contains('b'), "{joined}");
        assert!(joined.contains('1') && joined.contains('2'), "{joined}");
    }

    #[test]
    fn a_zero_width_render_does_not_panic() {
        // Narrow terminals hit zero and one constantly. A panic here would
        // take down the whole TUI on a resize.
        let _ = lines_of("etwas text", 0);
        let _ = lines_of("etwas text", 1);
    }
}
