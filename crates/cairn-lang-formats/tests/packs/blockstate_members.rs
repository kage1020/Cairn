//! What the built-in packs make of the states a `beam`, a `trapdoor` and a `note_block` write
//! (`spec/blockstate` "Derive by default, promote on override").

use cairn_lang_core::resolve::DroppedIntent;
use cairn_lang_core::{DiagnosticCode, Edition};
use cairn_lang_formats::bedrock_structure::build_mcstructure_tag;
use cairn_lang_formats::data_version::resolve_bedrock_target;

use crate::common::{lower_source, note_fields, states_for, written_palette};

/// A room whose theme binds `frame` and `hatch`, holding `members`.
fn room(members: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n  slot frame -> @spruce_log\n  \
         slot hatch -> @spruce_trapdoor\n  slot stone -> @cobblestone\n\n\
         struct s size=7x5\n  walls mat_slot=wall height=4\n{members}"
    )
}

/// Each finding's code and primary text.
fn findings(source: &str, edition: Edition, version: &str) -> Vec<(DiagnosticCode, String)> {
    lower_source(source, edition, Some(version))
        .diagnostics
        .into_iter()
        .map(|diagnostic| (diagnostic.code, diagnostic.primary))
        .collect()
}

#[test]
fn a_bedrock_target_writes_each_members_states_in_its_own_and_notes_the_note_it_drops() {
    let source = room(
        "  beam kind=pillar mat_slot=frame at=front.top\n  beam kind=pillar at=left.top\n  \
         trapdoor at=inside.right offset=2 y=2 mat_slot=hatch open=true\n  \
         trapdoor at=3,1,2 half=top facing=east\n  \
         note_block at=2,1,2 instrument=bit note=12\n  note_block at=1,1,1\n",
    );
    let version = "1.21.60";
    let out = lower_source(&source, Edition::Bedrock, Some(version));
    assert_eq!(out.diagnostics, []);
    let target = resolve_bedrock_target(version).expect("known target");
    let (root, notes) =
        build_mcstructure_tag(&out.structures["struct::s"], &target, &states_for(&target))
            .expect("build");
    let dropped = |key: &str, value: &str, reason: &str| DroppedIntent {
        key: key.to_owned(),
        value: value.to_owned(),
        mc_versions: vec![version.to_owned()],
        reason: reason.to_owned(),
    };
    assert_eq!(
        note_fields(&notes),
        [
            (
                "minecraft:noteblock",
                "instrument=bit,note=12",
                &dropped(
                    "note",
                    "12",
                    "Bedrock keeps a note block's note in its block entity, which Cairn does not \
                     write"
                ),
            ),
            (
                "minecraft:noteblock",
                "instrument=bit,note=12",
                &dropped(
                    "instrument",
                    "bit",
                    "the written `instrument=bit` is not kept: Bedrock works a note block's \
                     instrument out from the block below it"
                ),
            ),
        ],
    );
    assert_eq!(
        written_palette(&root),
        [
            "minecraft:air[]",
            "minecraft:cobblestone[]",
            "minecraft:noteblock[]",
            "minecraft:oak_log[pillar_axis=z]",
            "minecraft:spruce_log[pillar_axis=x]",
            "minecraft:spruce_trapdoor[direction=1,open_bit=1,upside_down_bit=0]",
            "minecraft:trapdoor[direction=0,open_bit=0,upside_down_bit=1]",
        ],
    );
}

#[test]
fn an_instrument_is_checked_against_the_pinned_versions_note_block() {
    let source = room("  note_block at=2,1,2 instrument=trumpet\n");
    assert_eq!(findings(&source, Edition::Java, "26.3"), []);
    assert_eq!(
        findings(&source, Edition::Java, "1.21.4"),
        [(
            DiagnosticCode::StateDomain,
            "`minecraft:note_block` takes no `instrument=trumpet` at java 1.21.4; `instrument` \
             takes harp, basedrum, snare, hat, bass, flute, bell, guitar, chime, xylophone, \
             iron_xylophone, cow_bell, didgeridoo, bit, banjo, pling, zombie, skeleton, creeper, \
             dragon, wither_skeleton, piglin, custom_head"
                .to_owned(),
        )],
    );
}

#[test]
fn a_beam_takes_a_block_whose_axis_lies_each_way_at_either_edition() {
    let log = room("  beam kind=pillar mat_slot=frame at=front.top\n");
    let stone = room("  beam kind=pillar mat_slot=stone at=front.top\n");
    for (edition, version) in [(Edition::Java, "1.21.4"), (Edition::Bedrock, "1.21.60")] {
        assert_eq!(findings(&log, edition, version), [], "{version}");
        assert_eq!(
            findings(&stone, edition, version),
            [(
                DiagnosticCode::IncompatibleMaterial,
                "beam derives `axis` from the wall it lies along and `minecraft:cobblestone` is \
                 not a pillar, so those states have nowhere to go"
                    .to_owned(),
            )],
            "{version}",
        );
    }
}
