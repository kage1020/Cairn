//! WebAssembly bindings for the Cairn compiler.
//!
//! Intended to let the playground (and other browser-hosted tools) parse, compile, and serialize
//! Cairn sources without a server, sharing exactly the same core implementation as the CLI.
//!
//! Not yet: the crate has no `wasm-bindgen` dependency and nothing below carries an export
//! attribute, so there is no callable surface for a page. See the crate README for what has to
//! land first.

/// The Cairn release version.
///
/// A plain Rust function today — nothing exposes it to a JavaScript caller yet.
#[must_use]
pub fn cairn_version() -> &'static str {
    cairn_lang_core::CAIRN_VERSION
}
