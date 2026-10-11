//! Error and source-position types used throughout `cairn-lang-core`.

use std::num::{IntErrorKind, NonZeroU32};
use std::ops::Range;

use serde::Serialize;
use thiserror::Error;

/// Byte range into the original source text.
pub type Span = Range<usize>;

/// 1-based line/column position used in human-readable diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Position {
    /// 1-based line number.
    pub line: NonZeroU32,
    /// 1-based column number, in Unicode scalar values.
    pub col: NonZeroU32,
}

impl Position {
    /// Line 1, column 1.
    pub const START: Position = Position {
        line: NonZeroU32::MIN,
        col: NonZeroU32::MIN,
    };

    /// Translate a position inside a detached slice onto the source it was cut from at `base`.
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
    /// Leading clause of the message, which downstream substring matchers depend on.
    fn describe(self) -> &'static str {
        match self {
            Self::SizeWidth | Self::SizeHeight | Self::IntLiteral => "invalid integer literal",
            Self::WithinBound => "invalid `within` bound",
        }
    }
}

/// Short rendering of an [`IntErrorKind`] for a diagnostic.
#[must_use]
pub fn describe_int_kind(kind: IntErrorKind) -> &'static str {
    match kind {
        IntErrorKind::Empty => "empty",
        IntErrorKind::InvalidDigit => "invalid digit",
        IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => "value out of range",
        _ => "invalid integer",
    }
}

fn render_nesting_too_deep(limit: usize) -> String {
    format!(
        "nesting is limited to {limit} levels here; \
         flatten the list, unindent the block, or split the expression \
         across intermediate `logic` bindings"
    )
}

/// Name a character the lexer could not place, by codepoint alone when it shows no glyph.
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
    /// A size literal is followed by text that would have continued it, as in `2x2x9` or `2x2y`.
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

/// Errors raised during syntactic analysis.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ParseError {
    /// A lexer error other than an integer-parse failure, which becomes
    /// [`ParseError::InvalidInt`].
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
    #[error("{position}: {}", render_nesting_too_deep(*limit))]
    NestingTooDeep {
        /// First token inside the level that went one too far.
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

    /// The same error with its position rebased onto the enclosing source.
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

    /// The same error with its position rebased onto the enclosing source.
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
