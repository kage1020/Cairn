//! Core of the Cairn language.
//!
//! Hosts the surface-syntax lexer, parser, and AST used by every other Cairn
//! crate. M1 ships only up to and including the AST; semantic analysis and IR
//! lifting land in later milestones.

pub mod ast;
pub mod error;
pub mod lex;
pub mod parse;

pub use ast::Module;
pub use error::{LexError, ParseError, Position, Span};
pub use lex::{Token, TokenKind, lex};
pub use parse::parse;

/// The Cairn release version, in date-based versioning (`YYYY.0M[.PATCH]`).
pub const CAIRN_VERSION: &str = "2026.06";
