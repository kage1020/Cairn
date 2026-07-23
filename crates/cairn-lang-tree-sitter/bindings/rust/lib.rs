//! Rust binding for the tree-sitter Cairn grammar.
//!
//! Consumers use [`LANGUAGE`] as the [`tree_sitter::Language`] handle for
//! the Cairn parser. The FFI symbol is emitted by the C parser generated
//! from [`grammar.js`](../../grammar.js).

use tree_sitter_language::LanguageFn;

unsafe extern "C" {
    fn tree_sitter_cairn() -> *const ();
}

/// The [`tree_sitter_language::LanguageFn`] handle for the Cairn grammar.
pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_cairn) };

/// Highlight query source, embedded from `queries/highlights.scm`.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");

/// Locals query source, embedded from `queries/locals.scm`.
pub const LOCALS_QUERY: &str = include_str!("../../queries/locals.scm");

/// Injections query source, embedded from `queries/injections.scm`.
pub const INJECTIONS_QUERY: &str = include_str!("../../queries/injections.scm");
