//! The tree-sitter Cairn grammar, as a [`LANGUAGE`] that converts into a `tree_sitter::Language`.

pub use ffi::LANGUAGE;

/// The one module in the workspace allowed to lift `unsafe_code`; keep it to these two items.
mod ffi {
    #![expect(
        unsafe_code,
        reason = "the generated C parser is only reachable through FFI"
    )]

    use tree_sitter_language::LanguageFn;

    unsafe extern "C" {
        // Defined by `src/parser.c`, which `build.rs` compiles.
        fn tree_sitter_cairn() -> *const ();
    }

    /// The [`tree_sitter_language::LanguageFn`] handle for the Cairn grammar.
    // SAFETY: the Tree-sitter CLI generates `tree_sitter_cairn`; `parser.c` is never hand-edited.
    pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_cairn) };
}

/// Highlight query source, embedded from `queries/highlights.scm`.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");

/// Locals query source, embedded from `queries/locals.scm`.
pub const LOCALS_QUERY: &str = include_str!("../../queries/locals.scm");

/// Injections query source, embedded from `queries/injections.scm`.
pub const INJECTIONS_QUERY: &str = include_str!("../../queries/injections.scm");
