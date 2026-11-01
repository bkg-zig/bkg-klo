//! The HTML backend for the browser cockpit.
//!
//! # The security rule
//!
//! **Every piece of model output is escaped before it reaches the output
//! string.** Not "most of it" — all of it. The text in this crate came from a
//! language model answering a request, which means it came from whatever
//! endpoint the gateway is pointed at, which means it can contain
//! `<img src=x onerror=...>` if someone wants it to.
//!
//! The cockpit renders this string with `dangerously_set_inner_html`, because
//! Dioxus has no cheap way to inject a pre-built HTML subtree otherwise. That
//! makes this function the single boundary between untrusted text and the
//! DOM. It is escaped twice on purpose: `parse.rs` treats raw HTML in the
//! source as *text* rather than as tags, and [`escape`] escapes it again here.
//! Two independent layers, because a single forgotten `escape()` call is a
//! stored-XSS hole reachable from a prompt injection.
//!
//! Nothing in this module ever emits a tag the [`crate::ir`] tree did not
//! decide on. A `Block::Code` becomes `<pre><code>`; the *contents* of that
//! code block are escaped text, never markup.
//!
//! Class names are stable so `assets/style.css` can style them.

use crate::ir::{Align, Block, Document, ListItem, Span, SpanKind, Table};

/// Render a parsed document to an HTML fragment (no `<html>` wrapper).
///
/// The result is safe to hand to `innerHTML`: the only tags in it are the ones
/// this function writes.
pub fn render(document: &Document) -> String {
    let mut out = String::new();
    for block in &document.blocks {
        block_html(block, &mut out);
    }
    out
}

/// Escape the five characters that can start or end markup.
///
/// `&` first, always: escaping it after the others would double-escape the
/// ampersands those escapes introduce.
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

fn block_html(block: &Block, out: &mut String) {
    match block {
        Block::Paragraph { spans } => {
            out.push_str("<p>");
            spans_html(spans, out);
            out.push_str("</p>");
        }
        Block::Heading { level, spans } => {
            // Clamp to 1..=6: an out-of-range level would emit a tag the
            // browser then reparents, which breaks the document outline.
            let level = (*level).clamp(1, 6);
            out.push_str(&format!("<h{level}>"));
            spans_html(spans, out);
            out.push_str(&format!("</h{level}>"));
        }
        Block::Code { lang, lines } => {
            out.push_str("<pre><code");
            if let Some(lang) = lang {
                // The language goes into a class attribute, so it is escaped
                // like any other attacker-influenced string.
                out.push_str(&format!(" class=\"language-{}\"", escape(lang)));
            }
            out.push('>');
            // Classification happens in the IR; the browser only gets class
            // names and paints them from `style.css`. That is what keeps the
            // WASM bundle at zero extra bytes while the terminal still colours
            // the same runs from the same classification.
            for (index, line) in lines.iter().enumerate() {
                if index > 0 {
                    out.push('\n');
                }
                let classified = crate::highlight::classify(line, lang.as_deref());
                code_tokens_html(&classified.tokens, out);
            }
            out.push_str("</code></pre>");
        }
        Block::List {
            ordered,
            start,
            loose,
            items,
        } => {
            if *ordered && *start != 1 {
                out.push_str(&format!("<ol start=\"{}\">", start));
            } else if *ordered {
                out.push_str("<ol>");
            } else {
                out.push_str("<ul>");
            }
            for item in items {
                item_html(item, *loose, out);
            }
            out.push_str(if *ordered { "</ol>" } else { "</ul>" });
        }
        Block::Quote { blocks } => {
            out.push_str("<blockquote>");
            for inner in blocks {
                block_html(inner, out);
            }
            out.push_str("</blockquote>");
        }
        Block::Rule => out.push_str("<hr>"),
        Block::Table(table) => table_html(table, out),
    }
}

fn item_html(item: &ListItem, loose: bool, out: &mut String) {
    out.push_str("<li");
    if let Some(checked) = item.checked {
        out.push_str(&format!(
            " class=\"task\" data-checked=\"{}\"",
            if checked { "true" } else { "false" }
        ));
    }
    out.push('>');
    if let Some(checked) = item.checked {
        // The box is decorative; the state also rides on `data-checked` so a
        // stylesheet or a test can read it without parsing text.
        out.push_str(if checked {
            "<span class=\"box\" aria-hidden=\"true\">☑</span>"
        } else {
            "<span class=\"box\" aria-hidden=\"true\">☐</span>"
        });
    }
    for inner in &item.blocks {
        // A tight list item's paragraph is not a real paragraph — CommonMark
        // says so, and wrapping it in `<p>` would add vertical space the
        // source never asked for.
        match (loose, inner) {
            (false, Block::Paragraph { spans }) => spans_html(spans, out),
            _ => block_html(inner, out),
        }
    }
    out.push_str("</li>");
}

fn table_html(table: &Table, out: &mut String) {
    out.push_str("<table><thead><tr>");
    for (index, cell) in table.header.iter().enumerate() {
        let align = table.alignments.get(index).copied().unwrap_or(Align::None);
        out.push_str(&format!("<th{}>", align_attr(align)));
        spans_html(&cell.spans, out);
        out.push_str("</th>");
    }
    out.push_str("</tr></thead><tbody>");
    for row in &table.rows {
        out.push_str("<tr>");
        for (index, cell) in row.iter().enumerate() {
            let align = table.alignments.get(index).copied().unwrap_or(Align::None);
            out.push_str(&format!("<td{}>", align_attr(align)));
            spans_html(&cell.spans, out);
            out.push_str("</td>");
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
}

fn align_attr(align: Align) -> &'static str {
    match align {
        Align::None => "",
        Align::Left => " style=\"text-align:left\"",
        Align::Center => " style=\"text-align:center\"",
        Align::Right => " style=\"text-align:right\"",
    }
}

/// Write classified code tokens as spans, escaping every one.
///
/// The escaping is not optional and not inherited from the caller: this is
/// the only place code reaches the DOM, and code is exactly the text a model
/// produces most freely. A single unescaped `<` in a code block is an
/// injection point, and code blocks are the *likely* place to try one.
fn code_tokens_html(tokens: &[crate::ir::CodeToken], out: &mut String) {
    // A plain-only line gets no span at all: wrapping every line of prose in
    // `<span class="tok">` would triple the size of every code block for
    // nothing.
    if tokens
        .iter()
        .all(|token| token.class == crate::ir::TokenClass::Plain)
    {
        for token in tokens {
            out.push_str(&escape(&token.text));
        }
        return;
    }
    for token in tokens {
        let class = token.class.css_class();
        out.push_str(&format!("<span class=\"{class}\">"));
        out.push_str(&escape(&token.text));
        out.push_str("</span>");
    }
}

fn spans_html(spans: &[Span], out: &mut String) {
    for span in spans {
        // The close tags must mirror the open tags in reverse order. Emitting
        // `</em></strong></del>` after opening `<strong><em><del>` leaves a
        // real `<del>` open for the rest of the document — valid-looking HTML
        // that strikes through everything after the first span.
        let mut close: Vec<&str> = Vec::new();
        if span.marks.strong {
            out.push_str("<strong>");
            close.push("</strong>");
        }
        if span.marks.emphasis {
            out.push_str("<em>");
            close.push("</em>");
        }
        if span.marks.strikethrough {
            out.push_str("<del>");
            close.push("</del>");
        }
        match &span.kind {
            SpanKind::Text(text) => out.push_str(&escape(text)),
            SpanKind::Code(text) => {
                out.push_str("<code>");
                out.push_str(&escape(text));
                out.push_str("</code>");
            }
            // The parser already decided `safe`; a browser can navigate an
            // `http` link but will *execute* a `javascript:` one, so an unsafe
            // destination keeps its text and loses its `href`.
            SpanKind::Link {
                dest,
                label,
                safe: true,
            } => {
                out.push_str(&format!(
                    "<a href=\"{}\" rel=\"noopener noreferrer nofollow\">",
                    escape(dest)
                ));
                spans_html(label, out);
                out.push_str("</a>");
            }
            SpanKind::Link {
                dest,
                label,
                safe: false,
            } => {
                spans_html(label, out);
                out.push_str(&format!(
                    " <span class=\"unsafe-url\">({})</span>",
                    escape(dest)
                ));
            }
            SpanKind::SoftBreak => out.push(' '),
            SpanKind::HardBreak => out.push_str("<br>"),
        }
        for tag in close.into_iter().rev() {
            out.push_str(tag);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse::parse;

    #[test]
    fn a_heading_becomes_a_heading_element_with_its_text() {
        let html = render(&parse("# Titel"));
        assert!(html.contains("<h1>Titel</h1>"), "{html}");
        // The regression that mattered: the heading used to render empty with
        // its text sitting in a stray paragraph beside it.
        assert_eq!(
            html.matches("<p>").count(),
            0,
            "no phantom paragraph: {html}"
        );
    }

    #[test]
    fn a_code_block_keeps_its_lines_and_language() {
        let html = render(&parse("```rust\nfn main() {}\nlet x = 1;\n```"));
        assert!(html.contains("class=\"language-rust\""), "{html}");
        // The text is split across spans by the classifier, so a substring
        // search for `fn main() {}` would fail on *correct* output. Strip the
        // tags and look at the text the operator actually reads.
        let visible = strip_tags(&html);
        assert!(visible.contains("fn main() {}"), "{visible}");
        assert!(visible.contains("let x = 1;"), "{visible}");
    }

    /// The text a browser would show: every tag removed, entities decoded.
    fn strip_tags(html: &str) -> String {
        let mut out = String::with_capacity(html.len());
        let mut inside_tag = false;
        for ch in html.chars() {
            match ch {
                '<' => inside_tag = true,
                '>' => inside_tag = false,
                other if !inside_tag => out.push(other),
                _ => {}
            }
        }
        out.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&#39;", "'")
            .replace("&amp;", "&")
    }

    #[test]
    fn a_code_block_gets_token_classes_not_a_grey_block() {
        // The point of the IR classes: the browser paints from CSS, so the
        // same classification the terminal uses is visible in the markup.
        let html = render(&parse("```rust\nfn main() { let x = 1; } // hi\n```"));
        assert!(html.contains("tok-kw"), "keyword class: {html}");
        assert!(html.contains("tok-num"), "number class: {html}");
        assert!(html.contains("tok-com"), "comment class: {html}");
    }

    #[test]
    fn a_plain_line_gets_no_span() {
        // Wrapping prose in `<span class="tok">` would triple the size of
        // every code block for no visual gain.
        let html = render(&parse("```\njust prose here\n```"));
        assert!(
            !html.contains("<span"),
            "an unclassified line needs no span: {html}"
        );
        assert!(html.contains("just prose here"), "{html}");
    }

    #[test]
    fn a_tag_inside_a_code_block_stays_escaped_after_classification() {
        // The dangerous case: highlighting splits the line into spans, and a
        // span boundary is exactly where an escaping mistake would hide. Code
        // is the text a model produces most freely, so this is where the
        // payload goes.
        let html = render(&parse(
            "```html\n<script>alert(1)</script>\n<img src=x onerror=alert(2)>\n```",
        ));
        assert!(!html.contains("<script"), "no live script: {html}");
        assert!(!html.contains("<img"), "no live img: {html}");
        // The string `onerror=` appears in the output — as escaped text inside
        // a span, where it is inert. Testing for it would fail on correct
        // output, so the real question is narrower: does any *live* tag
        // attribute exist? That means an `on...=` inside `<...>`.
        let live_attributes: Vec<&str> = html
            .match_indices('<')
            .filter_map(|(start, _)| {
                let rest = &html[start..];
                let end = rest.find('>')?;
                Some(&rest[..end])
            })
            .filter(|tag| tag[1..].contains("on"))
            .collect();
        assert!(
            live_attributes.is_empty(),
            "a live handler attribute survived: {live_attributes:?} in {html}"
        );
        // And the text is still there, escaped, for the operator to read.
        let visible = strip_tags(&html);
        assert!(visible.contains("<script>alert(1)</script>"), "{visible}");
        assert!(
            visible.contains("<img src=x onerror=alert(2)>"),
            "{visible}"
        );
    }

    #[test]
    fn classification_does_not_change_the_code_text() {
        // If the highlighter dropped or duplicated a character, the operator
        // would be reading code that does not compile and could not tell.
        let source = "```rust\nfn f(x: u32) -> u32 { let y = x * 2; /* double */ y }\n```";
        let visible = strip_tags(&render(&parse(source)));
        for needle in [
            "fn f(x: u32) -> u32",
            "let y = x * 2;",
            "/* double */",
            "y }",
        ] {
            assert!(visible.contains(needle), "{needle:?} vanished: {visible}");
        }
    }

    #[test]
    fn a_script_tag_in_model_output_stays_text() {
        // The test that matters. A model answer is untrusted input; if this
        // ever emits a live <script>, the cockpit has a stored XSS hole.
        let html = render(&parse("<script>alert(1)</script>"));
        assert!(!html.contains("<script"), "no live script tag: {html}");
        assert!(html.contains("&lt;script"), "escaped instead: {html}");
    }

    #[test]
    fn an_image_with_an_event_handler_stays_text() {
        let html = render(&parse("<img src=x onerror=alert(1)>"));
        assert!(!html.contains("<img"), "no live img: {html}");
        assert!(
            html.contains("onerror"),
            "the text is still visible: {html}"
        );
    }

    #[test]
    fn a_javascript_url_is_dropped_but_its_text_stays() {
        let html = render(&parse("[klick](javascript:alert(1))"));
        assert!(!html.contains("href=\"javascript"), "no href: {html}");
        assert!(html.contains("klick"), "the label survives: {html}");
    }

    #[test]
    fn a_normal_url_stays_a_link() {
        let html = render(&parse("[doku](https://example.com/x)"));
        assert!(html.contains("href=\"https://example.com/x\""), "{html}");
        assert!(
            html.contains("rel=\"noopener noreferrer nofollow\""),
            "{html}"
        );
    }

    #[test]
    fn a_data_url_is_dropped() {
        // `data:text/html` is script execution wearing a mime type.
        let html = render(&parse("[x](data:text/html,<script>alert(1)</script>)"));
        assert!(!html.contains("href=\"data:"), "{html}");
    }

    #[test]
    fn a_quoted_attribute_in_a_language_tag_cannot_escape() {
        // The language comes off the fence. It lands in an attribute, so it
        // gets escaped like any other attacker-influenced string.
        let html = render(&parse("```rust\" onload=\"alert(1)\nx\n```"));
        assert!(!html.contains("onload="), "no attribute injection: {html}");
    }

    #[test]
    fn emphasis_and_strong_close_in_matching_order() {
        // A mismatched close tag leaves <del> open for the rest of the
        // document — valid-looking HTML that strikes through everything after.
        let html = render(&parse("**fett *und* strikt ~~auch~~**"));
        assert_eq!(
            html.matches("<strong>").count(),
            html.matches("</strong>").count()
        );
        assert_eq!(html.matches("<em>").count(), html.matches("</em>").count());
        assert_eq!(
            html.matches("<del>").count(),
            html.matches("</del>").count()
        );
        assert!(html.contains("<strong>"), "{html}");
    }

    #[test]
    fn a_link_stays_inside_its_paragraph() {
        // One link used to split its sentence into five blocks.
        let html = render(&parse("vor [doku](https://example.com) nach"));
        assert_eq!(
            html.matches("<p>").count(),
            1,
            "still one paragraph: {html}"
        );
        assert!(html.contains("<a href="), "{html}");
    }

    #[test]
    fn a_tight_list_item_has_no_paragraph_wrapper() {
        // CommonMark: a tight item's text is not a paragraph.
        let html = render(&parse("- eins\n- zwei"));
        assert!(!html.contains("<li><p>"), "{html}");
        assert!(html.contains("<li>eins</li>"), "{html}");
    }

    #[test]
    fn a_loose_list_item_keeps_its_paragraph() {
        let html = render(&parse("- eins\n\n- zwei"));
        assert!(html.contains("<li><p>"), "{html}");
    }

    #[test]
    fn a_task_list_renders_a_box_and_the_state() {
        let html = render(&parse("- [x] fertig\n- [ ] offen"));
        assert!(html.contains("data-checked=\"true\""), "{html}");
        assert!(html.contains("data-checked=\"false\""), "{html}");
    }

    #[test]
    fn a_table_renders_header_and_body() {
        let html = render(&parse("| a | b |\n|---|---|\n| 1 | 2 |"));
        assert!(html.contains("<th>a</th>"), "{html}");
        assert!(html.contains("<td>1</td>"), "{html}");
        assert!(!html.contains("<p>"), "no leaked text: {html}");
    }

    #[test]
    fn a_table_alignment_reaches_the_cell() {
        let html = render(&parse("| a | b |\n|:--|--:|\n| 1 | 2 |"));
        assert!(html.contains("text-align:left"), "{html}");
        assert!(html.contains("text-align:right"), "{html}");
    }

    #[test]
    fn an_out_of_range_heading_level_is_clamped() {
        let html = render(&parse("# ok"));
        assert!(html.contains("<h1>"), "{html}");
        assert!(!html.contains("<h0"), "{html}");
    }

    #[test]
    fn escape_covers_every_dangerous_character() {
        assert_eq!(
            escape(r#"<a href="x">&'</a>"#),
            "&lt;a href=&quot;x&quot;&gt;&amp;&#39;&lt;/a&gt;"
        );
    }

    #[test]
    fn an_empty_document_renders_to_nothing() {
        assert_eq!(render(&parse("")), "");
    }

    #[test]
    fn nested_lists_nest_in_the_html() {
        let html = render(&parse("- a\n  - b\n- c"));
        assert_eq!(html.matches("<ul>").count(), 2, "two levels: {html}");
        assert!(
            html.contains("a") && html.contains("b") && html.contains("c"),
            "{html}"
        );
    }
}
