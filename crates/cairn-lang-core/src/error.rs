//! Error and source-position types used throughout `cairn-lang-core`.

use std::num::{IntErrorKind, NonZeroU32};
use std::ops::Range;

use thiserror::Error;

/// Byte range into the original source text.
pub type Span = Range<usize>;

/// 1-based line/column position used in human-readable diagnostics.
///
/// Both components are `NonZeroU32` because every source location is at line
/// 1, column 1 or later; the type rules out an accidental `(0, 0)` sentinel
/// that downstream code would have to special-case. `col` counts Unicode
/// scalar values (`char`), not bytes, so the value matches what editors and
/// `gcc`/`clang`-style tools display for the same source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Used as the fallback anchor when an error refers to an empty source —
    /// there is no last token to point at. This means an empty-source EOF
    /// error and a genuine error at `1:1` are indistinguishable by position
    /// alone; a future diagnostics layer (LSP) that needs to tell them apart
    /// should carry the distinction in the error variant rather than in the
    /// `Position`.
    pub const START: Position = Position {
        line: NonZeroU32::MIN,
        col: NonZeroU32::MIN,
    };
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
}

/// Grammatical position in which an integer literal was being parsed when it
/// failed. Surface-level diagnostics use this to phrase a message, and LSP
/// quick-fix logic uses it to dispatch on the offending site without resorting
/// to substring matches on the rendered text.
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
    /// Phrasing used as the leading clause of a human-readable diagnostic.
    /// Kept in sync with the legacy messages so substring matchers in
    /// downstream tooling continue to work.
    fn describe(self) -> &'static str {
        match self {
            Self::SizeWidth | Self::SizeHeight | Self::IntLiteral => "invalid integer literal",
            Self::WithinBound => "invalid `within` bound",
        }
    }
}

/// One-word rendering of [`IntErrorKind`] suitable for embedding in a
/// diagnostic. `IntErrorKind` is `#[non_exhaustive]`, so the catch-all arm
/// guards against future variants without misleading the reader.
///
/// `IntErrorKind::Zero` is only produced by `NonZero*::from_str` and never
/// by the `u32` / `i64` parses this crate currently performs. It is not
/// listed explicitly and falls through to `"invalid integer"`; callers
/// parsing `NonZero*` types should phrase Zero themselves.
#[must_use]
pub fn describe_int_kind(kind: IntErrorKind) -> &'static str {
    match kind {
        IntErrorKind::Empty => "empty",
        IntErrorKind::InvalidDigit => "invalid digit",
        IntErrorKind::PosOverflow | IntErrorKind::NegOverflow => "value out of range",
        _ => "invalid integer",
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
    #[error("{position}: unexpected character `{ch}`")]
    UnexpectedChar {
        /// Where the character is.
        position: Position,
        /// The offending character.
        ch: char,
    },
    /// An integer literal could not be parsed. Carries the failure `kind` so
    /// downstream tooling (e.g. LSP quick-fix) can dispatch without parsing
    /// the rendered message.
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
    /// A lexer error surfaced from token production. Lexer integer-parse
    /// failures are *not* wrapped here; they bubble up directly as
    /// [`ParseError::InvalidInt`] so callers have a single match arm to
    /// handle for any integer-parse failure regardless of layer.
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
            | Self::UnmatchedDedent { position }
            | Self::UnterminatedString { position }
            | Self::UnexpectedChar { position, .. }
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
}

impl ParseError {
    /// Source position the error refers to.
    #[must_use]
    pub fn position(&self) -> Position {
        match self {
            Self::Lex(err) => err.position(),
            Self::Syntax { position, .. } | Self::InvalidInt { position, .. } => *position,
        }
    }

    /// Human-readable message without the leading `line:col` prefix.
    #[must_use]
    pub fn user_message(&self) -> String {
        match self {
            Self::Lex(err) => err.user_message(),
            Self::Syntax { message, .. } => message.clone(),
            Self::InvalidInt {
                context,
                lexeme,
                kind,
                ..
            } => render_invalid_int(*context, lexeme, *kind),
        }
    }
}
