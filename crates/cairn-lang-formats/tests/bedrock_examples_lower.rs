//! End-to-end coverage for compiling the example builds through the Bedrock
//! `.mcstructure` backend.
//!
//! Lives in `cairn-lang-formats` because the pipeline needs both the built-in
//! Bedrock registry pack (`builtin_bedrock`, to resolve the abstract material
//! tokens) and the Bedrock structure backend (`build_mcstructure_tag`), which
//! `cairn-lang-core` cannot reach without a dependency cycle.
//!
//! Contract pinned: the roof stair family — the only block kind the lowering
//! interns with blockstate properties — maps to Bedrock `states` rather than
//! failing loud, so `cottage` compiles cleanly and `themed-tower` compiles
//! with exactly the shape-drop degradation its eave corners incur.

use std::path::PathBuf;

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::{lower, parse, resolve};
use cairn_lang_formats::bedrock_structure::build_mcstructure_tag;
use cairn_lang_formats::data_version::resolve_bedrock_target;
use cairn_lang_formats::registry::builtin_bedrock;

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
}

/// Lower an example through the Bedrock materials pack, the same way
/// `cairn compile --edition bedrock` does.
fn lower_bedrock(example: &str) -> BlockArrayIr {
    let source =
        std::fs::read_to_string(examples_dir().join(example)).expect("example .crn readable");
    let module = parse(&source).expect("parse example");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    let pack = builtin_bedrock();
    lower_to_block_array(&ir, &resolution, Some(&pack.materials))
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
    let stair_index = ba
        .palette
        .entries
        .iter()
        .position(|e| e.id.ends_with("_stairs"))
        .expect("stair present");
    let (root, _notes) = build_mcstructure_tag(ba, &target).expect("build");

    // Walk root.structure.palette.default.block_palette[stair_index].states.
    let structure = match root.entries.get("structure") {
        Some(Tag::Compound(c)) => c,
        other => panic!("structure: {other:?}"),
    };
    let block_palette = match structure.entries.get("palette") {
        Some(Tag::Compound(p)) => match p.entries.get("default") {
            Some(Tag::Compound(d)) => match d.entries.get("block_palette") {
                Some(Tag::List(l)) => &l.items,
                other => panic!("block_palette: {other:?}"),
            },
            other => panic!("default: {other:?}"),
        },
        other => panic!("palette: {other:?}"),
    };
    let states = match &block_palette[stair_index] {
        Tag::Compound(c) => match c.entries.get("states") {
            Some(Tag::Compound(s)) => s,
            other => panic!("states: {other:?}"),
        },
        other => panic!("palette entry: {other:?}"),
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
