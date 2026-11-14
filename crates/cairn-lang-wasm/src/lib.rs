//! This crate's Rust API is in the
//! [Internal tier](https://cairn.kage1020.com/spec/compatibility/#internal) and carries no
//! promise: any release may change it.
//!
//! WebAssembly bindings for the Cairn compiler, not yet exported to a page: the crate README
//! lists what is missing.

/// The Cairn release version.
#[must_use]
pub fn cairn_version() -> &'static str {
    cairn_lang_core::CAIRN_VERSION
}
