//! Core of the Cairn language.
//!
//! Hosts the surface-syntax lexer, parser, and AST used by every other Cairn
//! crate. The responsibility of this crate ends at producing an AST that
//! preserves the surface form; semantic analysis and IR lifting live in
//! downstream layers.

pub mod ast;
pub mod error;
pub mod lex;
pub mod parse;

pub use ast::{DottedRef, Module, Statement};
pub use error::{LexError, ParseError, Position, Span};
pub use lex::{Token, TokenKind, lex};
pub use parse::parse;

/// The Cairn release version, in date-based versioning (`YYYY.0M[.PATCH]`).
pub const CAIRN_VERSION: &str = "2026.06";
