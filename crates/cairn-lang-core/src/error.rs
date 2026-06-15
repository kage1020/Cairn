//! Error and source-position types used throughout `cairn-lang-core`.

use std::ops::Range;

use thiserror::Error;

/// Byte range into the original source text.
pub type Span = Range<usize>;

/// 1-based line/column position used in human-readable diagnostics.
///
/// `col` counts Unicode scalar values (`char`), not bytes, so the value matches
/// what editors and `gcc`/`clang`-style tools display for the same source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    /// 1-based line number.
    pub line: u32,
    /// 1-based column number, counted in Unicode scalar values from the start
    /// of the line.
    pub col: u32,
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.line, self.col)
    }
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
    /// An integer literal could not be parsed.
    #[error("{position}: invalid integer literal `{lexeme}`")]
    InvalidInt {
        /// Where the literal starts.
        position: Position,
        /// The raw lexeme.
        lexeme: String,
    },
}

/// Errors raised during syntactic analysis.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ParseError {
    /// A lexer error surfaced from token production.
    #[error(transparent)]
    Lex(#[from] LexError),
    /// A syntactic mismatch with a human-readable message.
    #[error("{position}: {message}")]
    Syntax {
        /// Where the mismatch occurred.
        position: Position,
        /// What was expected vs found.
        message: String,
    },
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
            Self::Syntax { position, .. } => *position,
        }
    }

    /// Human-readable message without the leading `line:col` prefix.
    #[must_use]
    pub fn user_message(&self) -> String {
        match self {
            Self::Lex(err) => err.user_message(),
            Self::Syntax { message, .. } => message.clone(),
        }
    }
}
