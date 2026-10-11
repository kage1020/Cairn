//! WebAssembly bindings for the Cairn compiler, not yet exported to a page: the crate README
//! lists what is missing.

/// The Cairn release version.
#[must_use]
pub fn cairn_version() -> &'static str {
    cairn_lang_core::CAIRN_VERSION
}
