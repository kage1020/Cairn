//! Helpers shared by the `cairn-lang-formats` integration test binaries.

use std::path::PathBuf;

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::resolve::DroppedIntent;
use cairn_lang_core::{Edition, lower, parse, resolve};
use cairn_lang_formats::bedrock_structure::ParityNote;
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

/// The number of `.crn` files `examples/` ships; bump it with the next example.
pub const SHIPPED_EXAMPLES: usize = 12;

/// Every `.crn` under `examples/`, as `(file name, source)` sorted by name, refusing any set but
/// the shipped one, since a sweep over nothing passes.
///
/// # Panics
///
/// When `examples/` cannot be read or holds other than [`SHIPPED_EXAMPLES`] files.
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
/// When the file cannot be read.
pub fn example_source(name: &str) -> String {
    let path = examples_dir().join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|err| panic!("cannot read {}: {err}", path.display()))
}

/// The version `--target latest` pins on `edition`.
pub fn latest(edition: Edition) -> &'static str {
    &builtin_pack(edition).data_versions.latest
}

/// The block-array IR `cairn compile --edition <edition>` lowers `source` to; a `mc_version` of
/// `None` pins no version, as `cairn info` does.
///
/// # Panics
///
/// When `source` does not parse.
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

/// Every version the pack can build for.
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
/// When the root has no `structure` compound.
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
/// When the root has no `structure.palette.default` compound.
pub fn default_palette(root: &Compound) -> &Compound {
    match structure_compound(root).entries.get("palette") {
        Some(Tag::Compound(p)) => match p.entries.get("default") {
            Some(Tag::Compound(d)) => d,
            other => panic!("palette.default is not a Compound: {other:?}"),
        },
        other => panic!("palette is not a Compound: {other:?}"),
    }
}

/// The `structure.palette.default.block_palette` entries of a `.mcstructure` root.
///
/// # Panics
///
/// When the root has no `block_palette` list.
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

/// Stair properties in the order lowering inserts them.
pub fn stair_props(facing: &str, half: &str, shape: &str) -> IndexMap<String, String> {
    props([("facing", facing), ("half", half), ("shape", shape)])
}

/// Each note's `(id, states, dropped)`: a `#[non_exhaustive]` note cannot be built to compare.
pub fn note_fields(notes: &[ParityNote]) -> Vec<(&str, &str, &DroppedIntent)> {
    notes
        .iter()
        .map(|note| (note.id.as_str(), note.states.as_str(), &note.dropped))
        .collect()
}

/// The intent a stair drops when Bedrock has no form for its `shape=value`.
pub fn dropped_shape(value: &str) -> DroppedIntent {
    DroppedIntent::Shape {
        value: value.to_owned(),
    }
}
