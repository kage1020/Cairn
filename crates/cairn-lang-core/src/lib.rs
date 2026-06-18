//! Core of the Cairn language.
//!
//! Hosts the surface-syntax lexer, parser, and AST plus the Intent IR — the
//! first semantic layer above the surface, where struct/site/def bodies are
//! reorganised into typed members with roles. Registry-backed lifting
//! (materials, themes, per-edition blockstate) and the diagnostic-collecting
//! `check` pipeline live in downstream layers.

pub mod ast;
pub mod error;
pub mod intent;
pub mod lex;
pub mod parse;

pub use ast::{DottedRef, Module, Statement};
pub use error::{LexError, ParseError, Position, Span};
pub use intent::{IntentModule, Member, MemberRole, SemanticLevel, lower};
pub use lex::{Token, TokenKind, lex};
pub use parse::parse;

/// The Cairn release version, in date-based versioning (`YYYY.0M[.PATCH]`).
pub const CAIRN_VERSION: &str = "2026.06";
