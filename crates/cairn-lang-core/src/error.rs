//! Error and source-position types used throughout `cairn-lang-core`.

use std::num::{IntErrorKind, NonZeroU32};
use std::ops::Range;

use serde::Serialize;
use thiserror::Error;

/// Byte range into the original source text.
pub type Span = Range<usize>;

/// 1-based line/column position used in human-readable diagnostics.
///
/// `NonZeroU32` rules out a `(0, 0)` sentinel. `col` counts Unicode scalar values, not bytes, as
/// editors and `gcc`-style tools do. Serializes as two plain numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Position {
    /// 1-based line number.
    pub line: NonZeroU32,
    /// 1-based column number, counted in Unicode scalar values from the start
    /// of the line.
    pub col: NonZeroU32,
}

impl Position {
    /// First byte of any source: line 1, column 1.
    ///
    /// Also the anchor for an error in an empty source, which has no last token to point at, so
    /// telling that apart from an error at `1:1` takes the error variant, not the position.
    pub const START: Position = Position {
        line: NonZeroU32::MIN,
        col: NonZeroU32::MIN,
    };

    /// Translate a position measured inside a detached slice onto the source
    /// that slice was cut from.
    ///
    /// `self` counts from the start of the slice; `base` is where the slice begins in the file.
    /// Only the slice's first line is offset by a column; later lines shift by line only.
    ///
    /// `@intended_targets` re-lexes its value as a detached string, so every diagnostic from that
    /// sub-parse has to come back through here.
    #[must_use]
    pub(crate) fn rebased(self, base: Position) -> Self {
        let line = base.line.saturating_add(self.line.get() - 1);
        let col = if self.line == NonZeroU32::MIN {
            base.col.saturating_add(self.col.get() - 1)
        } else {
            self.col
        };
        Position { line, col }
    }
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// Where in the grammar an integer literal that failed to parse sat.
///
/// Phrases the message, and lets a consumer dispatch on the site without matching the text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum IntContext {
    /// Width component of a `WxH` size literal (e.g. the `9` in `9x7`).
    SizeWidth,
    /// Height component of a `WxH` size literal (e.g. the `7` in `9x7`).
    SizeHeight,
    /// Free-standing integer literal in argument or value position.
    IntLiteral,
    /// Bound of `assert always(... within N)`.
    WithinBound,
}

impl IntContext {
    /// Leading clause of the rendered message. Downstream substring matchers depend on it.
    fn describe(self) -> &'static str {
        match self {
            Self::SizeWidth | Self::SizeHeight | Self::IntLiteral => "invalid integer literal",
            Self::WithinBound => "invalid `within` bound",
        }
    }
}

/// Short rendering of an [`IntErrorKind`] for a diagnostic.
///
/// `IntErrorKind::Zero` falls through to `"invalid integer"`. Only a `NonZero*` parse produces
/// it, and a caller doing one phrases it itself.
#[must_use]
pub fn describe_int_kind(kind: IntErrorKind) -> &'static str {
    match kind {
        IntErrorKind::Empty => "empty",
        IntErrorKind::InvalidDigit => "invalid digit",
        IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => "value out of range",
        _ => "invalid integer",
    }
}

/// Names the limit, because the author has to know how far to unwind.
fn render_nesting_too_deep(limit: usize) -> String {
    format!(
        "nesting is limited to {limit} levels here; \
         flatten the list, unindent the block, or split the expression \
         across intermediate `logic` bindings"
    )
}

/// Name a character the lexer could not place.
///
/// A character with no visible glyph is named by codepoint alone, because quoting U+FEFF shows
/// nothing and quoting U+00A0 shows what looks like an ordinary space. Any other character, ASCII
/// or not (a full-width `＝`, a stray `あ`), keeps its glyph, plus the codepoint for characters
/// that look alike.
fn render_unexpected_char(ch: char) -> String {
    if ch.is_control() || ch.is_whitespace() || ch == '\u{feff}' {
        format!("unexpected character U+{:04X}", ch as u32)
    } else {
        format!("unexpected character `{ch}` (U+{:04X})", ch as u32)
    }
}

fn render_invalid_int(context: IntContext, lexeme: &str, kind: IntErrorKind) -> String {
    format!(
        "{} `{}`: {}",
        context.describe(),
        lexeme,
        describe_int_kind(kind),
    )
}

/// Errors raised during lexical analysis.
#[derive(Debug, Error, PartialEq, Eq)]
#[non_exhaustive]
pub enum LexError {
    /// A literal tab character was used for indentation.
    #[error("{position}: tab character is not allowed for indentation; use 2 spaces per level")]
    TabIndent {
        /// Where the tab was encountered.
        position: Position,
    },
    /// Indentation width is not a multiple of 2 spaces.
    #[error("{position}: indentation must be a multiple of 2 spaces (got {got})")]
    OddIndent {
        /// Where the bad indentation begins.
        position: Position,
        /// Number of leading spaces observed.
        got: u32,
    },
    /// Indentation opens more than one level in a single step.
    ///
    /// Separate from [`LexError::OddIndent`], whose message would mislead: a 4-space jump already
    /// is a multiple of 2.
    #[error(
        "{position}: indentation opens more than one level at once \
         (got {got} spaces, expected {expected} spaces)"
    )]
    IndentJump {
        /// Where the over-deep indentation begins.
        position: Position,
        /// Number of leading spaces observed.
        got: u32,
        /// The one width that opens exactly one level here.
        expected: u32,
    },
    /// Indentation pops to a level that was never entered.
    #[error("{position}: dedent does not match any enclosing indentation level")]
    UnmatchedDedent {
        /// Where the bad dedent begins.
        position: Position,
    },
    /// A string literal lacks a closing quote.
    #[error("{position}: unterminated string literal")]
    UnterminatedString {
        /// Where the string starts.
        position: Position,
    },
    /// An unrecognised character was encountered.
    #[error("{position}: {}", render_unexpected_char(*ch))]
    UnexpectedChar {
        /// Where the character is.
        position: Position,
        /// The offending character.
        ch: char,
    },
    /// A size literal is followed by text that would have continued it.
    ///
    /// `2x2x9` holds a third extent and `2x2y` runs into a word. Rejected at the literal, because
    /// as a `Size` plus an identifier either would surface away from it: as a bare positional
    /// value the author never wrote, or as a parse failure at the end of a declaration header.
    #[error(
        "{position}: size literal `{literal}` is followed by `{found}`; \
         a size is two extents, as in `9x7`"
    )]
    TrailingSizeSegment {
        /// Where the literal starts.
        position: Position,
        /// The literal as written, up to the offending character.
        literal: String,
        /// The character that would have continued it.
        found: char,
    },
    /// An integer literal could not be parsed. Carries `kind` so a consumer can dispatch without
    /// parsing the message.
    #[error("{position}: {}", render_invalid_int(*context, lexeme, *kind))]
    InvalidInt {
        /// Where the literal starts.
        position: Position,
        /// Grammatical site of the failed parse.
        context: IntContext,
        /// The raw lexeme.
        lexeme: String,
        /// Why the parse failed.
        kind: IntErrorKind,
    },
}

/// Errors raised during syntactic analysis.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ParseError {
    /// A lexer error other than an integer-parse failure, which becomes
    /// [`ParseError::InvalidInt`] so one arm handles integer failures from either layer.
    #[error(transparent)]
    Lex(LexError),
    /// A syntactic mismatch with a human-readable message.
    #[error("{position}: {message}")]
    Syntax {
        /// Where the mismatch occurred.
        position: Position,
        /// What was expected vs found.
        message: String,
    },
    /// Value or expression nesting ran past the parser's depth limit.
    ///
    /// Raised before recursion can exhaust the native stack: an overflow aborts the process and
    /// would take every embedder down with it, the language server included.
    #[error("{position}: {}", render_nesting_too_deep(*limit))]
    NestingTooDeep {
        /// First token inside the level that went one too far, not the bracket or keyword that
        /// opened it.
        position: Position,
        /// The limit that was exceeded.
        limit: usize,
    },
    /// An integer literal could not be parsed.
    #[error("{position}: {}", render_invalid_int(*context, lexeme, *kind))]
    InvalidInt {
        /// Where the literal starts.
        position: Position,
        /// Grammatical site of the failed parse.
        context: IntContext,
        /// The raw lexeme.
        lexeme: String,
        /// Why the parse failed.
        kind: IntErrorKind,
    },
}

impl From<LexError> for ParseError {
    fn from(err: LexError) -> Self {
        match err {
            LexError::InvalidInt {
                position,
                context,
                lexeme,
                kind,
            } => Self::InvalidInt {
                position,
                context,
                lexeme,
                kind,
            },
            other => Self::Lex(other),
        }
    }
}

impl LexError {
    /// Source position the error refers to.
    #[must_use]
    pub fn position(&self) -> Position {
        match self {
            Self::TabIndent { position }
            | Self::OddIndent { position, .. }
            | Self::IndentJump { position, .. }
            | Self::UnmatchedDedent { position }
            | Self::UnterminatedString { position }
            | Self::UnexpectedChar { position, .. }
            | Self::TrailingSizeSegment { position, .. }
            | Self::InvalidInt { position, .. } => *position,
        }
    }

    /// Human-readable message without the leading `line:col` prefix.
    #[must_use]
    pub fn user_message(&self) -> String {
        let full = self.to_string();
        full.split_once(": ")
            .map_or(full.clone(), |(_, rest)| rest.to_owned())
    }

    /// The same error with its position translated onto the enclosing
    /// source. See [`Position::rebased`].
    #[must_use]
    pub(crate) fn rebased(mut self, base: Position) -> Self {
        let position = match &mut self {
            Self::TabIndent { position }
            | Self::OddIndent { position, .. }
            | Self::IndentJump { position, .. }
            | Self::UnmatchedDedent { position }
            | Self::UnterminatedString { position }
            | Self::UnexpectedChar { position, .. }
            | Self::TrailingSizeSegment { position, .. }
            | Self::InvalidInt { position, .. } => position,
        };
        *position = position.rebased(base);
        self
    }
}

impl ParseError {
    /// Source position the error refers to.
    #[must_use]
    pub fn position(&self) -> Position {
        match self {
            Self::Lex(err) => err.position(),
            Self::Syntax { position, .. }
            | Self::InvalidInt { position, .. }
            | Self::NestingTooDeep { position, .. } => *position,
        }
    }

    /// Human-readable message without the leading `line:col` prefix.
    #[must_use]
    pub fn user_message(&self) -> String {
        match self {
            Self::Lex(err) => err.user_message(),
            Self::Syntax { message, .. } => message.clone(),
            Self::NestingTooDeep { limit, .. } => render_nesting_too_deep(*limit),
            Self::InvalidInt {
                context,
                lexeme,
                kind,
                ..
            } => render_invalid_int(*context, lexeme, *kind),
        }
    }

    /// The same error with its position translated onto the enclosing
    /// source. See [`Position::rebased`].
    #[must_use]
    pub(crate) fn rebased(self, base: Position) -> Self {
        match self {
            Self::Lex(err) => Self::Lex(err.rebased(base)),
            Self::Syntax { position, message } => Self::Syntax {
                position: position.rebased(base),
                message,
            },
            Self::NestingTooDeep { position, limit } => Self::NestingTooDeep {
                position: position.rebased(base),
                limit,
            },
            Self::InvalidInt {
                position,
                context,
                lexeme,
                kind,
            } => Self::InvalidInt {
                position: position.rebased(base),
                context,
                lexeme,
                kind,
            },
        }
    }
}
