//! WebAssembly bindings for the Cairn compiler.
//!
//! Intended to let the playground (and other browser-hosted tools) parse, compile, and serialize
//! Cairn sources without a server, sharing exactly the same core implementation as the CLI.
//!
//! The crate has no `wasm-bindgen` dependency and nothing below carries an export attribute, so
//! a page has nothing to call; the crate README lists what is missing.

/// The Cairn release version.
#[must_use]
pub fn cairn_version() -> &'static str {
    cairn_lang_core::CAIRN_VERSION
}
