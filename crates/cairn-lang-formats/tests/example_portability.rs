//! `cairn info`'s portability figures against what a build actually emits.
//!
//! The two sides lower the same source differently on purpose: `info`
//! reports across a pack's whole version range and so lowers with no target
//! pinned, while a build pins one and gets that version's spelling of every
//! material. The figures still have to agree, because a portability count
//! that does not describe the build it is about is worse than no count —
//! `unsupported: 0` on a source no version of the edition can load is the
//! shape this file exists to refuse.
//!
//! What is checked per example, per edition:
//!
//! - every entry the range-wide lowering interns is a block the edition
//!   declares somewhere, so nothing shipped reports as unsupported;
//! - the entry count matches the count a pinned build emits, on every
//!   supported version — the two lowerings may disagree about *which*
//!   spelling, never about how many intents there are;
//! - and so does the extent, because the volume counts the members whose
//!   material resolves and "resolves" is asked against the pinned target.
//!   A count comparison alone cannot see an array that shortened.
//!
//! Whether each pinned build's ids exist in that pinned version is the
//! neighbouring question, held by `pack_ids_exist.rs`.

use std::collections::BTreeMap;
use std::path::PathBuf;

use cairn_lang_core::block_array::{BlockArrayIr, BlockState, lower_to_block_array};
use cairn_lang_core::{Edition, lower, parse, resolve};
use cairn_lang_formats::portability::{portability_for_bedrock, portability_for_java};
use cairn_lang_formats::registry::{RegistryPack, builtin_bedrock, builtin_java};

/// Every version the pack can build for, as the `--target` strings a user
/// types.
///
/// The `targetable` rows only. The table also names the releases the pack
/// can order an `@requires` floor against and has no block data for, and
/// walking those here would assert a pinned lowering against a version no
/// `--target` accepts.
fn supported_versions(pack: &RegistryPack) -> Vec<&str> {
    pack.data_versions
        .versions
        .iter()
        .filter(|e| e.targetable)
        .map(|e| e.mc_version.as_str())
        .collect()
}

/// Every `.crn` under `examples/`, as `(file name, source)`.
///
/// Refuses to return a set too small to be the shipped one. Every test here
/// is a loop over this, and a loop over nothing passes — the guard belongs
/// with the iteration source so no future test can forget it.
fn examples() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples");
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
    assert!(
        found.len() >= 5,
        "found only {} examples under {}, which is not the shipped set",
        found.len(),
        dir.display(),
    );
    found
}

/// The block-array IR for one source, lowered the way the named command
/// does: `None` for `cairn info`'s range-wide pass, `Some(version)` for a
/// build pinned to that target.
fn lower_for(
    source: &str,
    edition: Edition,
    pack: &RegistryPack,
    target: Option<&str>,
) -> BlockArrayIr {
    let module = parse(source).expect("shipped example parses");
    let ir = lower(&module);
    let resolution = resolve(&ir, Some(edition));
    lower_to_block_array(&ir, &resolution, Some(&pack.view(target)))
}

/// Every structure's extent, keyed by scope so a comparison cannot pass
/// by two scopes swapping sizes.
///
/// The volume counts the members whose material resolves, and "resolves"
/// is asked against the pinned target — so an id only some versions
/// declare moves `Dims` as well as the palette. The entry count alone
/// cannot see that: a wall that stops painting drops one entry *and*
/// shortens the array, and a comparison of counts is satisfied by the
/// first half.
fn extents(ir: &BlockArrayIr) -> BTreeMap<&str, (u32, u32, u32)> {
    ir.structures
        .iter()
        .map(|(key, ba)| (key.as_str(), (ba.dims.x, ba.dims.y, ba.dims.z)))
        .collect()
}

fn non_air_entries(ir: &BlockArrayIr) -> usize {
    ir.structures
        .values()
        .flat_map(|ba| ba.palette.entries.iter())
        .filter(|entry| entry.id != BlockState::AIR_ID)
        .count()
}

fn editions() -> [(Edition, &'static RegistryPack); 2] {
    [
        (Edition::Java, builtin_java()),
        (Edition::Bedrock, builtin_bedrock()),
    ]
}

#[test]
fn no_shipped_example_reports_an_unsupported_entry() {
    for (name, source) in &examples() {
        for (edition, pack) in editions() {
            let block_ir = lower_for(source, edition, pack, None);
            let counts = match edition {
                Edition::Java => portability_for_java(&block_ir, &pack.blocks),
                Edition::Bedrock => portability_for_bedrock(&block_ir, &pack.blocks),
            }
            .counts();
            assert_eq!(
                counts.unsupported, 0,
                "{name} reports {} unsupported entries on {edition}; every shipped example is \
                 expected to be buildable on both editions. Three things can produce this: a \
                 material mapped onto a block the edition does not have, an intent whose states \
                 the edition cannot express, or an id axis that has become too strict",
                counts.unsupported,
            );
            assert!(
                counts.total() > 0,
                "{name} on {edition} counted nothing — an example with an empty palette would \
                 make every assertion here vacuous",
            );
        }
    }
}

#[test]
fn the_reported_entry_count_matches_what_a_pinned_build_emits() {
    for (name, source) in &examples() {
        for (edition, pack) in editions() {
            let unpinned = lower_for(source, edition, pack, None);
            let reported = match edition {
                Edition::Java => portability_for_java(&unpinned, &pack.blocks),
                Edition::Bedrock => portability_for_bedrock(&unpinned, &pack.blocks),
            }
            .counts()
            .total();
            for version in supported_versions(pack) {
                let built = lower_for(source, edition, pack, Some(version));
                assert!(
                    built.diagnostics.is_empty(),
                    "{name} does not build for {edition} {version}: {:?}",
                    built.diagnostics,
                );
                assert_eq!(
                    reported as usize,
                    non_air_entries(&built),
                    "{name}: `info` reports {reported} palette entries for {edition} but a build \
                     pinned to {version} emits {}",
                    non_air_entries(&built),
                );
                assert_eq!(
                    extents(&unpinned),
                    extents(&built),
                    "{name}: the extent for {edition} moves between the range-wide lowering and \
                     a build pinned to {version}",
                );
            }
        }
    }
}
