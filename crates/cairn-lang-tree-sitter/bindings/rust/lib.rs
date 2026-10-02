//! Rust binding for the tree-sitter Cairn grammar.
//!
//! Consumers turn [`LANGUAGE`] into a `tree_sitter::Language` (via
//! `Into`) to drive the Cairn parser. The FFI symbol is emitted by the
//! C parser generated from [`grammar.js`](../../grammar.js).

pub use ffi::LANGUAGE;

/// The one module in the workspace allowed to lift `unsafe_code`.
///
/// The workspace denies the lint, and `cairn-lang-core`'s
/// `unsafe_code_is_confined` test — whose module doc states the policy —
/// fails on an attribute lifting it anywhere outside this module, this
/// file's crate root included. Keep this module to the declaration and the
/// handle below.
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
    // SAFETY: `from_raw` requires a language function generated from a
    // grammar by the Tree-sitter CLI. `tree_sitter_cairn` is that: the CLI
    // generates it from `grammar.js` into `src/parser.c`, and it returns the
    // `TSLanguage` static there, valid for the whole program. It stays sound
    // only while `parser.c` is regenerated with the CLI rather than edited by
    // hand, so that its ABI matches the linked Tree-sitter runtime.
    pub const LANGUAGE: LanguageFn = unsafe { LanguageFn::from_raw(tree_sitter_cairn) };
}

/// Highlight query source, embedded from `queries/highlights.scm`.
pub const HIGHLIGHTS_QUERY: &str = include_str!("../../queries/highlights.scm");

/// Locals query source, embedded from `queries/locals.scm`.
pub const LOCALS_QUERY: &str = include_str!("../../queries/locals.scm");

/// Injections query source, embedded from `queries/injections.scm`.
pub const INJECTIONS_QUERY: &str = include_str!("../../queries/injections.scm");
