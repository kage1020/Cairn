//! Shipped examples through the Bedrock `.mcstructure` backend; roof stairs and doors carry states.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_formats::bedrock_structure::{ParityNote, build_mcstructure_tag};
use cairn_lang_formats::data_version::resolve_bedrock_target;

use crate::common::{block_palette, dropped_shape, latest, lower_example, note_fields, states_for};

/// An example lowered as `cairn compile --edition bedrock` lowers it.
fn lower_bedrock(example: &str) -> BlockArrayIr {
    lower_example(example, Edition::Bedrock, Some(latest(Edition::Bedrock)))
}

/// Each structure of `example` built at `mc_version`, with the notes it raised.
fn built_at(example: &str, mc_version: &str) -> Vec<(String, Vec<ParityNote>)> {
    let out = lower_example(example, Edition::Bedrock, Some(mc_version));
    let target = resolve_bedrock_target(mc_version).expect("known target");
    out.structures
        .iter()
        .map(|(scope, ba)| {
            let (_root, notes) = build_mcstructure_tag(ba, &target, &states_for(&target))
                .unwrap_or_else(|e| panic!("build `{scope}` at {mc_version}: {e}"));
            (scope.clone(), notes)
        })
        .collect()
}

#[test]
fn cottage_compiles_to_mcstructure_without_degradation() {
    for version in ["1.21.60", latest(Edition::Bedrock)] {
        let built = built_at("cottage.crn", version);
        let notes: Vec<&ParityNote> = built.iter().flat_map(|(_, notes)| notes).collect();
        assert_eq!(
            notes,
            [] as [&ParityNote; 0],
            "cottage's straight gable stairs map to Bedrock {version} losslessly"
        );
    }
}

#[test]
fn cottage_roof_stairs_carry_bedrock_states() {
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
    let (root, _notes) = build_mcstructure_tag(ba, &target, &states_for(&target)).expect("build");

    // By `name`, since translation can merge Java entries into one `block_palette` entry.
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

/// The states of each `block_palette` entry named `name`, each as `key=value`.
fn bedrock_states_of(root: &cairn_lang_nbt::tag::Compound, name: &str) -> Vec<Vec<String>> {
    use cairn_lang_nbt::tag::Tag;

    block_palette(root)
        .iter()
        .filter_map(|entry| match entry {
            Tag::Compound(c) => match (c.entries.get("name"), c.entries.get("states")) {
                (Some(Tag::String(n)), Some(Tag::Compound(states))) if n == name => Some(
                    states
                        .entries
                        .iter()
                        .map(|(key, value)| match value {
                            Tag::Byte(v) => format!("{key}={v}"),
                            Tag::Int(v) => format!("{key}={v}"),
                            Tag::String(v) => format!("{key}={v}"),
                            other => panic!("{key}: {other:?}"),
                        })
                        .collect(),
                ),
                _ => None,
            },
            other => panic!("palette entry: {other:?}"),
        })
        .collect()
}

#[test]
fn cottage_door_carries_the_bedrock_door_states_of_each_target() {
    for (version, facing) in [
        ("1.21.40", "direction=3"),
        (
            latest(Edition::Bedrock),
            "minecraft:cardinal_direction=east",
        ),
    ] {
        let out = lower_example("cottage.crn", Edition::Bedrock, Some(version));
        let target = resolve_bedrock_target(version).expect("known target");
        let ba = &out.structures["struct::cottage"];
        let (root, notes) =
            build_mcstructure_tag(ba, &target, &states_for(&target)).expect("build");
        assert_eq!(notes, [] as [ParityNote; 0], "{version}");
        let mut states = bedrock_states_of(&root, "minecraft:wooden_door");
        for entry in &mut states {
            entry.sort();
        }
        let mut expected: Vec<Vec<String>> = [0, 1]
            .map(|upper| {
                let mut entry = [
                    facing.to_owned(),
                    "door_hinge_bit=0".to_owned(),
                    "open_bit=0".to_owned(),
                    format!("upper_block_bit={upper}"),
                ];
                entry.sort();
                entry.to_vec()
            })
            .to_vec();
        expected.sort();
        states.sort();
        assert_eq!(states, expected, "{version}");
    }
}

#[test]
fn themed_tower_drops_its_corner_stair_only_where_bedrock_has_no_corner_state() {
    let built = built_at("themed-tower.crn", "1.21.60");
    let degraded: Vec<_> = built
        .iter()
        .filter(|(_, notes)| !notes.is_empty())
        .map(|(scope, notes)| (scope.as_str(), note_fields(notes)))
        .collect();
    let corner = dropped_shape("outer_left", &["1.21.60"]);
    assert_eq!(
        degraded,
        [(
            "struct::keep",
            vec![(
                "minecraft:dark_oak_stairs",
                "facing=south,half=top,shape=outer_left",
                &corner,
            )],
        )],
        "themed-tower's one non-straight stair palette entry",
    );

    let latest = built_at("themed-tower.crn", latest(Edition::Bedrock));
    let notes: Vec<&ParityNote> = latest.iter().flat_map(|(_, notes)| notes).collect();
    assert_eq!(
        notes,
        [] as [&ParityNote; 0],
        "the latest version keeps the corner"
    );
}
