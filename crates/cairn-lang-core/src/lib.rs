//! Core of the Cairn language.
//!
//! Hosts the surface-syntax lexer, parser, and AST; the Intent IR, the first semantic layer above
//! the surface, where struct/site/def bodies are reorganised into typed members with roles; the
//! diagnostic-collecting `check` pipeline, which turns structural surprises in a lowered module
//! into position-anchored [`check::Diagnostic`] values; and the passes that resolve intent into a
//! block-array IR. Everything past that IR (the NBT codec, the format backends, redstone
//! synthesis, the LSP) lives in sibling crates.

pub mod ast;
pub mod block_array;
pub mod calver;
pub mod check;
pub(crate) mod diagnostic;
pub mod edition;
pub mod error;
pub mod ids;
pub mod intent;
pub mod lex;
pub mod lines;
pub mod lock;
pub mod parse;
pub(crate) mod prose;
pub mod resolve;
pub mod suggest;
pub(crate) mod version;

pub use ast::{DottedRef, Module, Statement};
pub use block_array::{
    BlockArray, BlockArrayIr, BlockEntity, BlockState, Dims, Entity, Palette, PaletteIndex,
    lower_to_block_array,
};
pub use check::{Diagnostic, DiagnosticCode, Severity, check};
pub use edition::{Edition, UnknownEdition};
pub use error::{LexError, ParseError, Position, Span};
pub use ids::{
    EndpointSegmentRole, IdError, KeyConstructError, KeyParseError, KeySegmentRole, PlaceId,
    PortId, SiteName, WalkwayEndpoint, WalkwayScopeKey, artifact_stem,
};
pub use intent::{
    CircuitRegion, CircuitRegionDefect, IntentModule, Member, MemberRole, RejectedCircuitRegion,
    ScopeKind, SemanticLevel, circuit_lines, circuit_regions, lower,
};
pub use lex::{Token, TokenKind, lex};
pub use parse::{MAX_EXPR_DEPTH, MAX_NESTING_DEPTH, diagnose_parse_failure, parse};
pub use resolve::{Resolution, ThemeBinding, VersionAxes, compute_axes, resolve};

/// The Cairn release version, in date-based versioning (`YYYY.M[.PATCH]`).
///
/// Read from this crate's package version, which is `[workspace.package]`'s, the number the
/// release tooling bumps. Everything that names the compiler that produced an artefact
/// (`cairn --version`, a lockfile's `cairn_version`) reads it from here.
pub const CAIRN_VERSION: &str = env!("CARGO_PKG_VERSION");
