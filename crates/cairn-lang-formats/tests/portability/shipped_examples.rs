//! `cairn info`'s portability figures against what a build emits, per
//! shipped example and edition. `info` lowers with no target pinned and a
//! build pins one, so the two may disagree about *which* spelling an entry
//! takes, but every range-wide entry must be a block the edition declares,
//! and the entry count and each structure's extent must match a pinned
//! build on every supported version.

use std::collections::BTreeMap;

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockArrayIr, BlockState};
use cairn_lang_formats::portability::{
    PortabilityReport, portability_for_bedrock, portability_for_java,
};
use cairn_lang_formats::registry::{RegistryPack, builtin_pack};

use crate::common::{examples, lower_source, supported_versions};

/// Every structure's extent, keyed by scope so a comparison cannot pass
/// by two scopes swapping sizes.
///
/// The volume counts the members whose material resolves against the
/// pinned target, so an id only some versions declare moves `Dims` as well
/// as the palette, which an entry count alone cannot see.
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
        (Edition::Java, builtin_pack(Edition::Java)),
        (Edition::Bedrock, builtin_pack(Edition::Bedrock)),
    ]
}

/// The edition's report, refusing the whole test if the palette carries a
/// blockstate the registry pack was expected to reject.
///
/// A leak in a shipped example would be the strongest case of the bug the
/// refusal exists for, so it fails here rather than being unwrapped
/// silently.
fn report_for(
    edition: Edition,
    pack: &RegistryPack,
    ir: &BlockArrayIr,
    name: &str,
) -> PortabilityReport {
    match edition {
        Edition::Java => portability_for_java(ir, &pack.blocks, &pack.aliases),
        Edition::Bedrock => portability_for_bedrock(ir, &pack.blocks, &pack.aliases)
            .unwrap_or_else(|invalid| {
                panic!(
                    "{name} leaked a blockstate the pack was expected to refuse on \
                     {edition}: {:?}",
                    invalid.leaks(),
                )
            }),
    }
}

#[test]
fn no_shipped_example_reports_an_unsupported_entry() {
    for (name, source) in &examples() {
        for (edition, pack) in editions() {
            let block_ir = lower_source(source, edition, None);
            let counts = report_for(edition, pack, &block_ir, name).counts();
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
            let unpinned = lower_source(source, edition, None);
            let reported = report_for(edition, pack, &unpinned, name).counts().total();
            for version in supported_versions(pack) {
                let built = lower_source(source, edition, Some(version));
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
