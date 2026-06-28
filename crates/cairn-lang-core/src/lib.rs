//! Core of the Cairn language.
//!
//! Hosts the surface-syntax lexer, parser, and AST, the Intent IR — the
//! first semantic layer above the surface, where struct/site/def bodies are
//! reorganised into typed members with roles — and the diagnostic-collecting
//! `check` pipeline that turns structural surprises in a lowered module into
//! position-anchored [`check::Diagnostic`] values. Registry-backed lifting
//! (materials, themes, per-edition blockstate) lives in downstream layers.

pub mod ast;
pub mod block_array;
pub mod check;
pub mod error;
pub mod ids;
pub mod intent;
pub mod lex;
pub mod lock;
pub mod parse;
pub mod resolve;
pub mod suggest;

pub use ast::{DottedRef, Module, Statement};
pub use block_array::{
    BlockArray, BlockArrayIr, BlockEntity, BlockState, Dims, Entity, Palette, PaletteIndex,
    lower_to_block_array,
};
pub use check::{Diagnostic, DiagnosticCode, Severity, check};
pub use error::{LexError, ParseError, Position, Span};
pub use ids::{
    IdError, KeyParseError, PlaceId, PortId, SiteName, WalkwayEndpoint, WalkwayScopeKey,
};
pub use intent::{IntentModule, Member, MemberRole, SemanticLevel, lower};
pub use lex::{Token, TokenKind, lex};
pub use parse::parse;
pub use resolve::{Resolution, ThemeBinding, VersionAxes, compute_axes, resolve};

/// The Cairn release version, in date-based versioning (`YYYY.0M[.PATCH]`).
pub const CAIRN_VERSION: &str = "2026.09";
