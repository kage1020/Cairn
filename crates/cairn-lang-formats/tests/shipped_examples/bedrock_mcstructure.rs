//! Examples compiled through the Bedrock `.mcstructure` backend. The roof
//! stair family, the only block kind lowering interns with blockstate
//! properties, maps to Bedrock `states`: `cottage` compiles without a parity
//! note and `themed-tower` with exactly the shape-drop note its eave corners
//! incur.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_formats::bedrock_structure::build_mcstructure_tag;
use cairn_lang_formats::data_version::resolve_bedrock_target;

use crate::common::{block_palette, latest, lower_example};

/// Lower an example the way `cairn compile --edition bedrock` does.
fn lower_bedrock(example: &str) -> BlockArrayIr {
    lower_example(example, Edition::Bedrock, Some(latest(Edition::Bedrock)))
}

#[test]
fn cottage_compiles_to_mcstructure_without_degradation() {
    // cottage's gable roof stairs are all `shape=straight`
    // (`roof::gable_stair_state`), which is Bedrock's default — so every
    // stair palette entry maps losslessly and no W_INTENT_DEGRADED note is
    // raised.
    let out = lower_bedrock("cottage.crn");
    let target = resolve_bedrock_target("latest").expect("known target");
    let mut total_notes = 0;
    for (scope, ba) in &out.structures {
        let (_root, notes) =
            build_mcstructure_tag(ba, &target).unwrap_or_else(|e| panic!("build `{scope}`: {e}"));
        total_notes += notes.len();
    }
    assert_eq!(
        total_notes, 0,
        "cottage's straight gable stairs should map to Bedrock losslessly"
    );
}

#[test]
fn cottage_roof_stairs_carry_bedrock_states() {
    // The roof stair palette entry must reach the Bedrock backend with its
    // Java properties and come out with a non-empty typed `states` compound —
    // proof the mapping fired rather than the old stateless empty compound.
    use cairn_lang_nbt::tag::Tag;

    let out = lower_bedrock("cottage.crn");
    let target = resolve_bedrock_target("latest").expect("known target");
    let (_, ba) = out
        .structures
        .iter()
        .find(|(_, ba)| ba.palette.entries.iter().any(|e| e.id.ends_with("_stairs")))
        .expect("cottage has a stair in its palette");
    let stair_id = &ba
        .palette
        .entries
        .iter()
        .find(|e| e.id.ends_with("_stairs"))
        .expect("stair present")
        .id;
    let (root, _notes) = build_mcstructure_tag(ba, &target).expect("build");

    // The `block_palette` entry named `stair_id`, then its states. Found by
    // `name` rather than by the Java palette index: translation can merge
    // Java entries into one `block_palette` entry, so the two lists need not
    // line up.
    let palette = block_palette(&root);
    let stair = palette
        .iter()
        .find_map(|entry| match entry {
            Tag::Compound(c) => match c.entries.get("name") {
                Some(Tag::String(name)) if name == stair_id => Some(c),
                Some(Tag::String(_)) => None,
                other => panic!("name: {other:?}"),
            },
            other => panic!("palette entry: {other:?}"),
        })
        .unwrap_or_else(|| panic!("no block_palette entry is named {stair_id}: {palette:?}"));
    let states = match stair.entries.get("states") {
        Some(Tag::Compound(s)) => s,
        other => panic!("states: {other:?}"),
    };
    assert!(
        states.entries.contains_key("weirdo_direction"),
        "stair states carry weirdo_direction: {states:?}"
    );
    assert!(
        states.entries.contains_key("upside_down_bit"),
        "stair states carry upside_down_bit: {states:?}"
    );
}

#[test]
fn themed_tower_compiles_to_mcstructure_with_shape_degradation() {
    // themed-tower's eave stair binds `shape=outer_left`, which Bedrock has
    // no state for. The build succeeds; palette dedup means one warning per
    // distinct dropped `(id, shape)`, and today's source has exactly one
    // such palette entry (`dark_oak_stairs` with `outer_left`).
    let out = lower_bedrock("themed-tower.crn");
    let target = resolve_bedrock_target("latest").expect("known target");
    let mut total_notes = 0;
    for (scope, ba) in &out.structures {
        let (_root, notes) =
            build_mcstructure_tag(ba, &target).unwrap_or_else(|e| panic!("build `{scope}`: {e}"));
        for note in &notes {
            assert!(note.message.contains("shape"), "note: {}", note.message);
        }
        total_notes += notes.len();
    }
    assert_eq!(
        total_notes, 1,
        "themed-tower has exactly one non-straight stair palette entry today"
    );
}
