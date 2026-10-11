//! Helpers shared by the `cairn-lang-formats` integration test binaries.
//!
//! Every binary here reads the same `examples/` tree and walks the same
//! built-in registry packs, so the plumbing for both lives once.

use std::path::PathBuf;

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::{Edition, lower, parse, resolve};
use cairn_lang_formats::registry::{RegistryPack, builtin_pack};
use cairn_lang_nbt::tag::{Compound, Tag};
use indexmap::IndexMap;

/// The repository's `examples/` directory.
pub fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

/// The number of `.crn` files `examples/` ships. Bump it with the next
/// example; every sweep below refuses a smaller set, and a loop over a
/// filtered-down set passes silently.
pub const SHIPPED_EXAMPLES: usize = 12;

/// Every `.crn` under `examples/`, as `(file name, source)`, sorted by name.
///
/// Refuses to return a set other than the shipped one. Every sweep is a
/// loop over this, and a loop over nothing passes — the guard lives with
/// the iteration source so no test can forget it.
///
/// # Panics
///
/// Panics if `examples/` cannot be read or holds other than
/// [`SHIPPED_EXAMPLES`] files.
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
    assert_eq!(
        found.len(),
        SHIPPED_EXAMPLES,
        "found {} examples under {}, not the {SHIPPED_EXAMPLES} shipped; bump SHIPPED_EXAMPLES \
         when adding one",
        found.len(),
        dir.display(),
    );
    found
}

/// The source of the shipped example `name` (`"cottage.crn"`).
///
/// # Panics
///
/// Panics if the file cannot be read.
pub fn example_source(name: &str) -> String {
    let path = examples_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

/// The version `--target latest` pins on `edition`.
pub fn latest(edition: Edition) -> &'static str {
    &builtin_pack(edition).data_versions.latest
}

/// The block-array IR `cairn compile --edition <edition>` lowers `source`
/// to: resolved for that edition, through that edition's built-in pack,
/// with the id table of `mc_version`. `None` pins no version, which is how
/// `cairn info` lowers.
///
/// # Panics
///
/// Panics if `source` does not parse.
pub fn lower_source(source: &str, edition: Edition, mc_version: Option<&str>) -> BlockArrayIr {
    let module = parse(source).unwrap_or_else(|err| panic!("source does not parse: {err}"));
    let ir = lower(&module);
    let resolution = resolve(&ir, Some(edition));
    lower_to_block_array(
        &ir,
        &resolution,
        Some(&builtin_pack(edition).view(mc_version)),
    )
}

/// [`lower_source`] over the shipped example `name`.
pub fn lower_example(name: &str, edition: Edition, mc_version: Option<&str>) -> BlockArrayIr {
    lower_source(&example_source(name), edition, mc_version)
}

/// Every version the pack can build for, as the `--target` strings a user
/// types.
pub fn supported_versions(pack: &RegistryPack) -> Vec<&str> {
    pack.targetable()
        .into_iter()
        .map(|e| e.mc_version.as_str())
        .collect()
}

/// The `structure` compound of a `.mcstructure` root.
///
/// # Panics
///
/// Panics if the root has no `structure` compound.
pub fn structure_compound(root: &Compound) -> &Compound {
    match root.entries.get("structure") {
        Some(Tag::Compound(c)) => c,
        other => panic!("structure is not a Compound: {other:?}"),
    }
}

/// The `structure.palette.default` compound of a `.mcstructure` root.
///
/// # Panics
///
/// Panics if the root has no `structure.palette.default` compound.
pub fn default_palette(root: &Compound) -> &Compound {
    match structure_compound(root).entries.get("palette") {
        Some(Tag::Compound(p)) => match p.entries.get("default") {
            Some(Tag::Compound(d)) => d,
            other => panic!("palette.default is not a Compound: {other:?}"),
        },
        other => panic!("palette is not a Compound: {other:?}"),
    }
}

/// The `structure.palette.default.block_palette` entries of a
/// `.mcstructure` root.
///
/// # Panics
///
/// Panics if the root has no `block_palette` list.
pub fn block_palette(root: &Compound) -> &[Tag] {
    match default_palette(root).entries.get("block_palette") {
        Some(Tag::List(l)) => &l.items,
        other => panic!("block_palette is not a List: {other:?}"),
    }
}

/// A blockstate property map from `(key, value)` pairs, in the order given.
pub fn props<const N: usize>(pairs: [(&str, &str); N]) -> IndexMap<String, String> {
    pairs
        .into_iter()
        .map(|(k, v)| (k.to_owned(), v.to_owned()))
        .collect()
}

/// Stair properties in the `facing` / `half` / `shape` insertion order the
/// lowering uses, so `translate_states` sees the same key stream as at
/// runtime.
pub fn stair_props(facing: &str, half: &str, shape: &str) -> IndexMap<String, String> {
    props([("facing", facing), ("half", half), ("shape", shape)])
}
