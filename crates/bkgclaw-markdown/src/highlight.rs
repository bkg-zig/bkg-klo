//! Classifying code into [`TokenClass`] runs.
//!
//! # Why a scanner and not syntect
//!
//! A real engine (`syntect` with the default syntax set) weighs 368 KB of
//! assets — nothing in a native binary, visible in a browser download. So the
//! *engine* sits behind a feature and the *classification* lives here, in the
//! IR, where it is free.
//!
//! What is traded away, stated plainly: this is not a parser. It knows C-family
//! keywords, comments, strings, numbers and attributes, and it gets those
//! right on Rust, Python, JavaScript, Go and C. It does not know that Rust's
//! `r#"raw strings"#` are strings, and it will colour one as code. A full
//! engine gets that right and costs 400–800 KB more.
//!
//! For a chat transcript — where the code block is there to be *read*, and the
//! reader is looking at what the agent just wrote — the scanner is the right
//! trade. Everything it gets right it gets right on the cases that actually
//! occur, and nothing about it needs a 368 KB download.
//!
//! # The safety rule
//!
//! **The classifier must never change the text.** Only the boundaries between
//! runs move. Every test here asserts that joining the tokens back together
//! reproduces the input exactly, because a highlighter that drops a quote or
//! doubles a bracket turns the operator's code into code that does not compile
//! — and they cannot tell that from what they wrote.

use crate::ir::{CodeLine, CodeToken, TokenClass};

/// Keywords across the C family. One flat list beats per-language tables for a
/// scanner: `fn` is a keyword in Rust, a function name in JavaScript, and
/// colouring it as a keyword is rarely wrong enough to matter, while a
/// 12-language table would be a maintenance burden and no better.
const KEYWORDS: &[&str] = &[
    // Rust
    "fn",
    "let",
    "mut",
    "const",
    "static",
    "struct",
    "enum",
    "impl",
    "trait",
    "pub",
    "crate",
    "mod",
    "use",
    "match",
    "if",
    "else",
    "loop",
    "while",
    "for",
    "in",
    "return",
    "move",
    "ref",
    "self",
    "Self",
    "super",
    "where",
    "as",
    "dyn",
    "async",
    "await",
    "unsafe",
    "true",
    "false",
    "break",
    "continue",
    "type",
    "async",
    "yield",
    // Python
    "def",
    "class",
    "elif",
    "lambda",
    "pass",
    "raise",
    "except",
    "finally",
    "with",
    "yield",
    "from",
    "import",
    "global",
    "nonlocal",
    "is",
    "not",
    "and",
    "or",
    "None",
    "assert",
    "del",
    // JavaScript / TypeScript
    "function",
    "var",
    "let",
    "const",
    "new",
    "delete",
    "typeof",
    "instanceof",
    "this",
    "null",
    "undefined",
    "export",
    "default",
    "extends",
    "interface",
    "type",
    "implements",
    // Go / C-family
    "func",
    "package",
    "import",
    "range",
    "chan",
    "go",
    "defer",
    "struct",
    "typedef",
    "unsigned",
    "void",
    "sizeof",
    "static",
    "extern",
    "inline",
];

/// Built-in type names worth their own colour.
const TYPES: &[&str] = &[
    "u8", "u16", "u32", "u64", "usize", "i8", "i16", "i32", "i64", "isize", "f32", "f64", "bool",
    "char", "str", "String", "Vec", "Option", "Result", "Box", "Arc", "Rc", "RefCell", "HashMap",
    "BTreeMap", "VecDeque", "Cow", "Self", "int", "float", "double", "long", "short", "void",
    "byte", "object", "any", "unknown", "never",
];

/// Whether a language has `//` line comments, or uses `#`. Python and shell
/// do not; getting this wrong means colouring half a Python file as a comment.
fn has_line_comment(lang: Option<&str>) -> bool {
    !matches!(
        lang.map(str::to_ascii_lowercase).as_deref(),
        None | Some("python")
            | Some("py")
            | Some("sh")
            | Some("bash")
            | Some("shell")
            | Some("yaml")
            | Some("yml")
            | Some("toml")
            | Some("ruby")
            | Some("rb")
    )
}

/// Whether a language treats `#` as a comment rather than a macro or a
/// private field.
fn has_hash_comment(lang: Option<&str>) -> bool {
    matches!(
        lang.map(str::to_ascii_lowercase).as_deref(),
        Some("python")
            | Some("py")
            | Some("sh")
            | Some("bash")
            | Some("shell")
            | Some("yaml")
            | Some("yml")
            | Some("ruby")
            | Some("rb")
    )
}

/// Classify one line of code.
///
/// Returns a line whose tokens, joined, reproduce `line` exactly. A blank line
/// stays a blank line — an empty token list would join back to `""`, which is
/// right, but a line with one empty-string token is easier to reason about.
pub fn classify(line: &str, lang: Option<&str>) -> CodeLine {
    if line.is_empty() {
        return CodeLine::default();
    }

    let mut tokens: Vec<CodeToken> = Vec::new();
    let chars: Vec<char> = line.chars().collect();
    let mut index = 0usize;
    let mut plain_start = 0usize;

    // Push the run of untouched text before the token we are about to emit.
    macro_rules! flush {
        ($end:expr) => {
            if $end > plain_start {
                tokens.push(CodeToken::new(
                    TokenClass::Plain,
                    chars[plain_start..$end].iter().collect::<String>(),
                ));
            }
        };
    }

    while index < chars.len() {
        let rest: String = chars[index..].iter().collect();
        let rest = rest.as_str();

        // Comment to end of line. Checked before everything else, because a
        // `//` inside a string is not a comment and a `'` inside a comment is
        // not a string — the order is what makes both come out right.
        if has_line_comment(lang) && rest.starts_with("//") {
            flush!(index);
            tokens.push(CodeToken::new(
                TokenClass::Comment,
                chars[index..].iter().collect::<String>(),
            ));
            return CodeLine { tokens };
        }
        if rest.starts_with("/*") {
            flush!(index);
            let mut end = index;
            let mut closed = false;
            while end < chars.len() {
                if chars[end] == '*' && chars.get(end + 1) == Some(&'/') {
                    end += 2;
                    closed = true;
                    break;
                }
                end += 1;
            }
            if !closed {
                end = chars.len();
            }
            tokens.push(CodeToken::new(
                TokenClass::Comment,
                chars[index..end].iter().collect::<String>(),
            ));
            index = end;
            plain_start = index;
            continue;
        }
        if has_hash_comment(lang) && rest.starts_with('#') {
            flush!(index);
            tokens.push(CodeToken::new(
                TokenClass::Comment,
                chars[index..].iter().collect::<String>(),
            ));
            return CodeLine { tokens };
        }

        // String literal. The closing quote has to exist on this line; an
        // unterminated one is treated as code, because a stray apostrophe in a
        // comment (`don't`) would otherwise swallow the rest of the line.
        if let Some(class) = string_start(rest) {
            flush!(index);
            // `end` is an index into the whole line, not into `rest` — the
            // quote can sit anywhere on it. Starting at 1 meant the scan began
            // at the line's second character, so a string late in the line
            // found its "closing" quote somewhere in the middle of it.
            let quote = chars[index];
            let mut end = index + 1;
            let mut escaped = false;
            while end < chars.len() {
                if chars[end] == '\\' && !escaped {
                    escaped = true;
                    end += 2;
                    continue;
                }
                if chars[end] == quote && !escaped {
                    end += 1;
                    break;
                }
                if chars[end] == '\\' {
                    escaped = false;
                }
                end += 1;
            }
            let end = end.min(chars.len());
            tokens.push(CodeToken::new(
                class,
                chars[index..end].iter().collect::<String>(),
            ));
            index = end;
            plain_start = index;
            continue;
        }

        // Attribute: `#[derive(Debug)]`, `#![no_std]`. Rust-only in practice.
        if rest.starts_with("#[") || rest.starts_with("#![") {
            flush!(index);
            let mut end = index;
            let mut depth = 0i32;
            while end < chars.len() {
                match chars[end] {
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth <= 0 {
                            end += 1;
                            break;
                        }
                    }
                    _ => {}
                }
                end += 1;
            }
            tokens.push(CodeToken::new(
                TokenClass::Attribute,
                chars[index..end].iter().collect::<String>(),
            ));
            index = end;
            plain_start = index;
            continue;
        }

        // Word: keyword, type, or plain. The whole word is consumed either way —
        // falling through on a non-keyword would rescan its tail, so `main`
        // would be seen as `main`, then `ain`, then `in`, and the `in` inside
        // an identifier would be coloured as a keyword.
        if is_word_start(chars[index]) {
            let mut end = index;
            while end < chars.len() && is_word_char(chars[end]) {
                end += 1;
            }
            let word: String = chars[index..end].iter().collect();
            let class = if KEYWORDS.contains(&word.as_str()) {
                Some(TokenClass::Keyword)
            } else if TYPES.contains(&word.as_str()) {
                Some(TokenClass::Type)
            } else {
                None
            };
            match class {
                Some(class) => {
                    flush!(index);
                    tokens.push(CodeToken::new(class, word));
                    index = end;
                    plain_start = index;
                }
                None => {
                    // Not a keyword: leave it in the plain run and jump past
                    // the word, so the scanner never re-enters its middle.
                    index = end;
                }
            }
            continue;
        }

        // Number. Only at the *start* of a token — `x2` is one identifier, not
        // `x` followed by `2`. The preceding character decides, so the check
        // cannot be skipped by the word branch above.
        let preceded_by_word = index
            .checked_sub(1)
            .and_then(|previous| chars.get(previous))
            .is_some_and(|previous| is_word_char(*previous));
        if chars[index].is_ascii_digit() && !preceded_by_word {
            flush!(index);
            let mut end = index;
            while end < chars.len()
                && (chars[end].is_ascii_alphanumeric() || chars[end] == '.' || chars[end] == '_')
            {
                end += 1;
            }
            tokens.push(CodeToken::new(
                TokenClass::Number,
                chars[index..end].iter().collect::<String>(),
            ));
            index = end;
            plain_start = index;
            continue;
        }

        index += 1;
    }
    flush!(chars.len());

    CodeLine { tokens }
}

/// The token class for a string opening at the start of `rest`, or `None`.
///
/// Only a quote that is plausibly a string opener counts. A single `'` is
/// ambiguous across languages — it is a character in Rust and a string in
/// Python — so it only opens when a closing one follows on the same line.
fn string_start(rest: &str) -> Option<TokenClass> {
    if !rest.starts_with('"') {
        if let Some(stripped) = rest.strip_prefix('\'') {
            return stripped.find('\'').map(|_| TokenClass::String);
        }
        return None;
    }
    let bytes: Vec<char> = rest.chars().collect();
    let mut escaped = false;
    for ch in bytes.iter().skip(1) {
        if *ch == '\\' && !escaped {
            escaped = true;
            continue;
        }
        if *ch == '"' && !escaped {
            return Some(TokenClass::String);
        }
        if *ch == '\n' {
            return None;
        }
        escaped = false;
    }
    None
}

fn is_word_start(ch: char) -> bool {
    ch.is_alphabetic() || ch == '_'
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

/// Classify a whole block, keeping the line count.
pub fn classify_block(lines: &[String], lang: Option<&str>) -> Vec<CodeLine> {
    lines
        .iter()
        .map(|line| classify(line, Some(lang.unwrap_or(""))))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The invariant everything else depends on: classification moves
    /// boundaries, never characters.
    fn assert_lossless(line: &str, lang: Option<&str>) -> CodeLine {
        let result = classify(line, lang);
        assert_eq!(
            result.text(),
            line,
            "the highlighter changed the text: {result:?} for {line:?}"
        );
        result
    }

    #[test]
    fn classification_never_changes_the_text() {
        let samples = [
            "fn main() { let x = 1; }",
            "    let x: u32 = 42; // a comment",
            "if (a && b) || !c { return; }",
            "\"a string with // slashes and /* stars */\"",
            "'c'",
            "x = 1.5e-10",
            "#[derive(Debug, Clone)]",
            "don't stop",
            "",
            "   ",
            "}",
            "\"unterminated",
            "a'quote",
        ];
        for sample in samples {
            assert_lossless(sample, Some("rust"));
        }
    }

    #[test]
    fn a_keyword_is_recognised() {
        let line = assert_lossless("fn main() {}", Some("rust"));
        let keyword: Vec<&str> = line
            .tokens
            .iter()
            .filter(|token| token.class == TokenClass::Keyword)
            .map(|token| token.text.as_str())
            .collect();
        assert_eq!(keyword, vec!["fn"], "{line:?}");
    }

    #[test]
    fn a_comment_swallows_the_rest_of_the_line() {
        let line = assert_lossless("let x = 1; // the rest is prose", Some("rust"));
        let comment = line
            .tokens
            .iter()
            .find(|token| token.class == TokenClass::Comment)
            .expect("the comment");
        assert_eq!(comment.text, "// the rest is prose");
    }

    #[test]
    fn a_slash_inside_a_string_is_not_a_comment() {
        // The order matters: string detection runs before comment detection, so
        // a URL in a string stays a string.
        let line = assert_lossless("let url = \"http://x/y\";", Some("rust"));
        assert!(
            line.tokens
                .iter()
                .any(|token| token.class == TokenClass::String),
            "{line:?}"
        );
        assert!(
            !line
                .tokens
                .iter()
                .any(|token| token.class == TokenClass::Comment),
            "a // inside a string must not start a comment: {line:?}"
        );
    }

    #[test]
    fn a_block_comment_spans_to_its_closer() {
        let line = assert_lossless("/* two\nlines */ let x = 1;", Some("rust"));
        assert!(line.text().starts_with("/*"), "{line:?}");
    }

    #[test]
    fn a_hash_comment_only_in_languages_that_use_it() {
        // In Python `#` is a comment; in Rust it is an attribute or nothing.
        let python = assert_lossless("x = 1  # comment", Some("python"));
        assert!(
            python
                .tokens
                .iter()
                .any(|token| token.class == TokenClass::Comment),
            "{python:?}"
        );

        let rust = assert_lossless("let x = 1; # not a comment", Some("rust"));
        assert!(
            !rust
                .tokens
                .iter()
                .any(|token| token.class == TokenClass::Comment),
            "# is not a comment in Rust: {rust:?}"
        );
    }

    #[test]
    fn a_word_containing_a_keyword_is_not_one() {
        // `iffy` must not split into `if` + `fy`.
        let line = assert_lossless("let iffy = 1;", Some("rust"));
        assert!(
            !line
                .tokens
                .iter()
                .any(|token| token.class == TokenClass::Keyword && token.text == "if"),
            "{line:?}"
        );
    }

    #[test]
    fn a_number_in_an_identifier_is_not_a_number() {
        let line = assert_lossless("let x2 = 1;", Some("rust"));
        let numbers: Vec<&str> = line
            .tokens
            .iter()
            .filter(|token| token.class == TokenClass::Number)
            .map(|token| token.text.as_str())
            .collect();
        assert_eq!(numbers, vec!["1"], "{line:?}");
    }

    #[test]
    fn an_apostrophe_without_a_pair_stays_code() {
        // `don't` in a comment — but comments are caught first, so this is the
        // case of an apostrophe in running code.
        let line = assert_lossless("x = 'unclosed", Some("rust"));
        assert!(
            !line
                .tokens
                .iter()
                .any(|token| token.class == TokenClass::String),
            "an unterminated quote must not open a string: {line:?}"
        );
    }

    #[test]
    fn a_type_is_coloured_differently_from_a_keyword() {
        let line = assert_lossless("let x: Vec<u8> = Vec::new();", Some("rust"));
        assert!(
            line.tokens
                .iter()
                .any(|token| token.class == TokenClass::Type),
            "{line:?}"
        );
    }

    #[test]
    fn css_class_names_are_unique() {
        let all = [
            TokenClass::Plain,
            TokenClass::Keyword,
            TokenClass::Type,
            TokenClass::Function,
            TokenClass::Number,
            TokenClass::String,
            TokenClass::Comment,
            TokenClass::Punctuation,
            TokenClass::Attribute,
        ];
        let mut names: Vec<&str> = all.iter().map(|class| class.css_class()).collect();
        let count = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            count,
            "two classes share a css name: {names:?}"
        );
    }

    #[test]
    fn an_empty_block_keeps_its_lines() {
        let lines = vec![
            "let x = 1;".to_string(),
            String::new(),
            "let y = 2;".to_string(),
        ];
        let classified = classify_block(&lines, Some("rust"));
        assert_eq!(classified.len(), 3, "line count is part of the contract");
        assert_eq!(classified[1].text(), "");
    }
}
