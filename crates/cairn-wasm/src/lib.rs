//! WebAssembly bindings for the Cairn compiler.
//!
//! Lets the playground (and other browser-hosted tools) parse, compile, and serialize Cairn
//! sources without a server, sharing exactly the same core implementation as the CLI.

/// The Cairn release version, re-exported for JS callers.
#[must_use]
pub fn cairn_version() -> &'static str {
    cairn_core::CAIRN_VERSION
}
