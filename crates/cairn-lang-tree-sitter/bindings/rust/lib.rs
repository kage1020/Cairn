//! Rust binding for the tree-sitter Cairn grammar.
//!
//! Consumers use [`LANGUAGE`] as the [`tree_sitter::Language`] handle for
//! the Cairn parser. The FFI symbol is emitted by the C parser generated
//! from [`grammar.js`](../../grammar.js).

pub use ffi::LANGUAGE;

/// The one place in the workspace that lifts `unsafe_code`.
///
/// The workspace denies it everywhere else, and a test in `cairn-lang-core`
/// (`unsafe_code_is_confined`) fails if any other file names the lint in an
/// attribute. Keep this module to the declaration and the handle below.
mod ffi {
    #![expect(
        unsafe_code,
        reason = "the generated C parser is only reachable through FFI"
    )]

    use tree_sitter_language::LanguageFn;

    unsafe extern "C" {
        // Defined by `src/parser.c`, which `build.rs` compiles and links
        // together with `src/scanner.c` into this crate.
        fn tree_sitter_cairn() -> *const ();
    }

    /// The [`tree_sitter_language::LanguageFn`] handle for the Cairn grammar.
    // SAFETY: `tree_sitter_cairn` takes no arguments and returns a pointer to
    // the `TSLanguage` static in `parser.c`, which is the contract
    // `LanguageFn::from_raw` asks for.
    pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_cairn) };
}

/// Highlight query source, embedded from `queries/highlights.scm`.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");

/// Locals query source, embedded from `queries/locals.scm`.
pub const LOCALS_QUERY: &str = include_str!("../../queries/locals.scm");

/// Injections query source, embedded from `queries/injections.scm`.
pub const INJECTIONS_QUERY: &str = include_str!("../../queries/injections.scm");
