//! The intermediate form: Markdown in, a target-neutral tree out.
//!
//! # Why an IR at all
//!
//! Two clients render the same model output — the terminal and the browser.
//! Each of them knows a different output language: ratatui builds `Line`s of
//! styled cells, a browser parses HTML. If both parsed the Markdown
//! themselves, the two parsers would be two places to get structure wrong, and
//! a heading that indents in the terminal would be a paragraph in the browser.
//!
//! So: parse once into this tree, then walk it twice. The parsing decisions —
//! what a list is, where emphasis ends, which fence closes — happen in exactly
//! one place, and both renderers inherit them.
//!
//! What this type deliberately does *not* know: colours, padding, font sizes,
//! anything about a screen or a viewport. Those belong to the backend. An IR
//! that carried styles would be a renderer wearing a costume.

/// A styled run of text inside a block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub kind: SpanKind,
    pub marks: Marks,
}

impl Span {
    pub fn text(content: impl Into<String>) -> Self {
        Span {
            kind: SpanKind::Text(content.into()),
            marks: Marks::default(),
        }
    }

    pub fn with_marks(mut self, marks: Marks) -> Self {
        self.marks = marks;
        self
    }
}

/// What kind of thing a span is, independent of how it is drawn.
///
/// A link lives *here*, inside a span, rather than as a sibling block. That
/// placement is not cosmetic: `[a](b)` in the middle of a sentence must not
/// split the sentence into two paragraphs. Modelled as a block, one link
/// shattered its paragraph into three — which is the exact failure the IR was
/// supposed to make impossible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SpanKind {
    /// Ordinary prose.
    Text(String),
    /// `inline code` — a monospace run.
    Code(String),
    /// A line break the source asked for.
    SoftBreak,
    /// A hard break (two trailing spaces, or a backslash).
    HardBreak,
    /// A link or image. `label` is the visible text; for an image it is the
    /// alt text. `safe` is false when the destination uses a scheme a browser
    /// would execute — the parser decides this once, so no backend has to.
    Link {
        dest: String,
        label: Vec<Span>,
        safe: bool,
    },
}

/// The marks a span carries. A bitmask of small booleans rather than nested
/// enums, because a span can be both emphasised and inline code, and every
/// renderer wants to ask "is it bold?" independently of "is it code?".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Marks {
    pub emphasis: bool,
    pub strong: bool,
    pub strikethrough: bool,
    /// Heading level, meaningful inside a heading only.
    pub heading: u8,
}

impl Marks {
    /// True when nothing distinguishes this span from plain prose.
    pub fn is_plain(&self) -> bool {
        *self == Marks::default()
    }
}

/// A top-level block. Markdown's block grammar, flattened to what a chat
/// transcript actually contains — no footnotes, no definition lists, no
/// frontmatter. Those do not appear in model output, and an unhandled case
/// should degrade to text, not to a silent hole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// `# heading` through `###### heading`.
    Heading { level: u8, spans: Vec<Span> },
    /// An ordinary paragraph.
    Paragraph { spans: Vec<Span> },
    /// A fenced or indented code block. `lang` is the info string on the fence,
    /// normalised to a lower-case language token — a highlighter picks by that
    /// token, so normalising here keeps two renderers from guessing
    /// differently.
    Code {
        lang: Option<String>,
        lines: Vec<String>,
    },
    /// An ordered or unordered list. Nested items carry their own blocks.
    List {
        ordered: bool,
        start: u64,
        /// True when items are separated by blank lines. CommonMark calls this
        /// "loose", and the distinction decides whether an item's text is
        /// wrapped in `<p>` or not — a difference both backends must agree on.
        loose: bool,
        items: Vec<ListItem>,
    },
    /// `> quote`. Nested blocks, because a quote can contain a code block.
    Quote { blocks: Vec<Block> },
    /// `---` or `***`.
    Rule,
    /// A GFM table. Header and body use the same cell type on purpose: an
    /// asymmetric one forced the parser to flatten cells and lose everything
    /// that was not a paragraph.
    Table(Table),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListItem {
    /// The task list checkbox state, for `- [x]` items.
    pub checked: Option<bool>,
    pub blocks: Vec<Block>,
}

/// One table cell: its inline content, plus the alignment its column asks for.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Cell {
    pub spans: Vec<Span>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub header: Vec<Cell>,
    pub rows: Vec<Vec<Cell>>,
    pub alignments: Vec<Align>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Align {
    #[default]
    None,
    Left,
    Center,
    Right,
}

/// A parsed document.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Document {
    pub blocks: Vec<Block>,
}

impl Document {
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Total number of blocks, one level deep. Used by tests and by renderers
    /// that want a cheap "is this worth rendering at all" check.
    pub fn len(&self) -> usize {
        self.blocks.len()
    }
}

/// The kinds of token a code block is built from.
///
/// # Why this enum and not a highlighter
///
/// Highlighting is the expensive part — a real engine's syntax assets weigh
/// 368 KB, which is nothing in a native binary and visible in a WASM download.
/// So the *classification* lives here, in the IR, where it costs nothing, and
/// each backend decides what to do with it:
///
/// ```text
///   TUI  →  TokenClass::Keyword  →  ratatui::style::Style  (truecolor)
///   web →  TokenClass::Keyword  →  class="tok-kw"           (0 KB, just CSS)
/// ```
///
/// The browser gets colour without the engine: the class name names the same
/// category the terminal colours, and `style.css` paints it. If web
/// highlighting is ever wanted with a real engine, it plugs in here and
/// nothing above this line moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TokenClass {
    /// Plain code — the overwhelming majority of a program.
    #[default]
    Plain,
    /// `fn`, `let`, `if`, `return`, …
    Keyword,
    /// `u8`, `String`, `Option<T>`, …
    Type,
    /// A name the program defines.
    Function,
    /// A number literal.
    Number,
    /// `"text"`, `'c'`.
    String,
    /// `// …`, `/* … */`.
    Comment,
    /// Operators and punctuation.
    Punctuation,
    /// `#[derive(Debug)]`, attributes, macros.
    Attribute,
}

/// A run of code with its classification.
///
/// Plain `String` lines would be simpler, and simpler is what the first
/// version did — which is why code blocks rendered as one undifferentiated
/// grey block. The class travels with the text, so both renderers colour the
/// same run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodeToken {
    pub class: TokenClass,
    pub text: String,
}

impl TokenClass {
    /// The CSS class name for this token kind.
    ///
    /// On the *class*, not the token: the browser asks "what class is this?",
    /// and the answer does not depend on the text. Stable names, so a
    /// stylesheet can target them and a second client can reuse them.
    pub fn css_class(self) -> &'static str {
        match self {
            TokenClass::Plain => "tok",
            TokenClass::Keyword => "tok-kw",
            TokenClass::Type => "tok-ty",
            TokenClass::Function => "tok-fn",
            TokenClass::Number => "tok-num",
            TokenClass::String => "tok-str",
            TokenClass::Comment => "tok-com",
            TokenClass::Punctuation => "tok-pun",
            TokenClass::Attribute => "tok-attr",
        }
    }
}

impl CodeToken {
    pub fn new(class: TokenClass, text: impl Into<String>) -> Self {
        CodeToken {
            class,
            text: text.into(),
        }
    }
}

/// A classified code line.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CodeLine {
    pub tokens: Vec<CodeToken>,
}

impl CodeLine {
    /// The line as plain text — the highlighter has to reproduce it exactly,
    /// because dropping or duplicating a character turns the operator's code
    /// into different code.
    pub fn text(&self) -> String {
        self.tokens
            .iter()
            .map(|token| token.text.as_str())
            .collect()
    }

    /// A line with no classification, for a block the highlighter skipped.
    pub fn plain(text: &str) -> Self {
        CodeLine {
            tokens: vec![CodeToken::new(TokenClass::Plain, text)],
        }
    }
}
