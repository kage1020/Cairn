//! WebAssembly bindings that run the Cairn compiler in a browser, with the built-in registry packs
//! embedded: `cairn compile` and `cairn info` over a source held in memory.
//!
//! Each export answers a plain object made from a [`report`] value, whose findings take the shape
//! `spec/lint` "Machine-readable payload" gives; a name that is not an edition throws an `Error`.

use cairn_lang_core::Edition;
use cairn_lang_formats::build::gate::requested_editions;
use js_sys::{Array, JSON, Object, Reflect, Uint8Array};
use serde::Serialize;
use wasm_bindgen::prelude::*;

pub mod report;

/// The Cairn release version.
#[must_use]
pub fn cairn_version() -> &'static str {
    cairn_lang_core::CAIRN_VERSION
}

/// The Cairn release version, owned, since JavaScript cannot borrow a Rust string.
#[wasm_bindgen(js_name = cairn_version)]
#[must_use]
pub fn cairn_version_js() -> String {
    cairn_version().to_owned()
}

/// `cairn compile` over `source` as [`report::Compiled`], with `artifacts` as `{ name, bytes }`
/// objects whose `bytes` is a `Uint8Array`.
///
/// # Errors
///
/// An `edition` that is not `java` or `bedrock`.
#[wasm_bindgen]
pub fn compile(source: &str, edition: &str, target: &str) -> Result<JsValue, JsValue> {
    let compiled = report::compile(source, parse_edition(edition)?, target);
    let value = to_js(&compiled)?;
    let artifacts = Array::new();
    for artifact in &compiled.artifacts {
        let entry = Object::new();
        Reflect::set(&entry, &"name".into(), &artifact.name.as_str().into())?;
        Reflect::set(
            &entry,
            &"bytes".into(),
            &Uint8Array::from(artifact.bytes.as_slice()),
        )?;
        artifacts.push(&entry);
    }
    Reflect::set(&value, &"artifacts".into(), &artifacts)?;
    Ok(value)
}

/// `cairn info --editions <editions>` over `source`, as [`report::Informed`].
///
/// # Errors
///
/// An empty `editions` list, or one holding an empty entry or a name that is not an edition.
#[wasm_bindgen]
#[expect(
    clippy::needless_pass_by_value,
    reason = "wasm-bindgen takes a JavaScript array only as an owned `Vec`"
)]
pub fn info(source: &str, editions: Vec<String>) -> Result<JsValue, JsValue> {
    if editions.is_empty() {
        return Err(JsError::new(
            "`editions` names no edition. Valid: java, bedrock. Fix: pass at least one edition name",
        )
        .into());
    }
    let editions = requested_editions(&editions).map_err(|err| JsError::new(&err))?;
    to_js(&report::info(source, &editions))
}

/// The versions `edition` can build for, as [`report::Targets`].
///
/// # Errors
///
/// An `edition` that is not `java` or `bedrock`.
#[wasm_bindgen]
pub fn targets(edition: &str) -> Result<JsValue, JsValue> {
    to_js(&report::targets(parse_edition(edition)?))
}

fn parse_edition(edition: &str) -> Result<Edition, JsError> {
    edition.parse().map_err(JsError::from)
}

fn to_js(value: &impl Serialize) -> Result<JsValue, JsValue> {
    let json = serde_json::to_string(value).map_err(JsError::from)?;
    JSON::parse(&json)
}
