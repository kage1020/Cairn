//! The Bedrock figures for a Java block the built-in state map writes as a Bedrock block of its
//! own, per `spec/materials-themes` "Mappings across version and edition", and for a raw block
//! naming it, which "Raw blocks" leaves unread by that mapping.

use cairn_lang_core::Edition;
use cairn_lang_core::resolve::UnsupportedReason;
use cairn_lang_formats::portability::{PortabilityCounts, portability_for_bedrock};
use cairn_lang_formats::registry::builtin_pack;

use crate::common::lower_source;

/// Bedrock keeps a pot's plant in its block entity, so no block it writes says which plant.
#[test]
fn a_java_block_bedrock_writes_apart_is_portable_and_one_it_cannot_tell_apart_is_not() {
    let ir = lower_source(
        "theme t:\n  slot floor -> @oak_sign\n  slot wall -> @potted_poppy\nstruct s size=3x3\n  \
         floor mat_slot=floor\n  walls mat_slot=wall height=1\n",
        Edition::Bedrock,
        None,
    );
    let pack = builtin_pack(Edition::Bedrock);
    let report = portability_for_bedrock(
        &ir,
        pack.blocks(),
        pack.aliases(),
        pack.absent(),
        pack.states(),
    )
    .expect("the palette carries no blockstate a pack would have refused");
    assert_eq!(
        report.counts(),
        PortabilityCounts {
            portable: 1,
            degraded: 0,
            unsupported: 1,
        },
    );
    let unsupported: Vec<(&str, &UnsupportedReason)> = report
        .unsupported()
        .iter()
        .map(|entry| (entry.id.as_str(), &entry.reason))
        .collect();
    assert_eq!(
        unsupported,
        [(
            "minecraft:potted_poppy",
            &UnsupportedReason::AbsentFromEdition {
                suggestion: None,
                aliases: Vec::new(),
            }
        )],
    );
}

/// A raw block names the id it writes, so only a version declaring `oak_sign` could build it.
#[test]
fn a_raw_block_bedrock_only_writes_as_a_token_is_unsupported() {
    let ir = lower_source(
        "theme t:\n  slot floor -> @oak_sign\nstruct s size=3x3\n  floor mat_slot=floor\n\
         struct r size=3x3\n  raw_block mat=minecraft:oak_sign at=1,0,1\n",
        Edition::Bedrock,
        None,
    );
    let pack = builtin_pack(Edition::Bedrock);
    let report = portability_for_bedrock(
        &ir,
        pack.blocks(),
        pack.aliases(),
        pack.absent(),
        pack.states(),
    )
    .expect("the palette carries no blockstate a pack would have refused");
    assert_eq!(
        report.counts(),
        PortabilityCounts {
            portable: 1,
            degraded: 0,
            unsupported: 1,
        },
    );
    let unsupported: Vec<(&str, &UnsupportedReason)> = report
        .unsupported()
        .iter()
        .map(|entry| (entry.id.as_str(), &entry.reason))
        .collect();
    assert_eq!(
        unsupported,
        [(
            "minecraft:oak_sign",
            &UnsupportedReason::AbsentFromEdition {
                suggestion: Some("minecraft:oak_log".to_owned()),
                aliases: vec!["minecraft:standing_sign".to_owned()],
            }
        )],
    );
}
