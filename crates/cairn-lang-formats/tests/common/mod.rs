//! Helpers shared by the `cairn-lang-formats` integration test binaries.
//!
//! Every binary here reads the same `examples/` tree and walks the same
//! built-in registry packs, so the plumbing for both lives once.

// Each test binary uses its own subset of these.
#![allow(dead_code)]

use std::path::PathBuf;

use cairn_lang_formats::registry::RegistryPack;

/// The repository's `examples/` directory.
pub fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

/// Every `.crn` under `examples/`, as `(file name, source)`, sorted by name.
pub fn examples() -> Vec<(String, String)> {
    let dir = examples_dir();
    let entries = std::fs::read_dir(&dir)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", dir.display()));
    let mut found: Vec<(String, String)> = entries
        .map(|entry| {
            entry
                .unwrap_or_else(|err| panic!("cannot read an entry: {err}"))
                .path()
        })
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("crn"))
        .map(|path| {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()));
            (
                path.file_name().expect("named").to_string_lossy().into(),
                source,
            )
        })
        .collect();
    found.sort();
    found
}

/// Every version the pack can build for, as the `--target` strings a user
/// types. The `targetable` rows only — an ordering row exists so the pack
/// can order an `@requires` floor against it, has no block table, and so
/// declares nothing a test could walk.
pub fn supported_versions(pack: &RegistryPack) -> Vec<&str> {
    pack.data_versions
        .versions
        .iter()
        .filter(|e| e.targetable)
        .map(|e| e.mc_version.as_str())
        .collect()
}
