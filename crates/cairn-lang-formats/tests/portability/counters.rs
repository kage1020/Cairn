//! The portability counters over hand-built palettes and block tables.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{
    BlockArray, BlockArrayIr, BlockState, Dims, Palette, PaletteIndex,
};
use cairn_lang_core::resolve::{DegradedEntry, DroppedIntent, UnsupportedReason};
use cairn_lang_formats::bedrock_state::{BedrockStateError, translate_states};
use cairn_lang_formats::portability::{
    InvalidPalette, PortabilityCounts, PortabilityReport, portability_for_bedrock,
    portability_for_java,
};
use cairn_lang_formats::registry::{
    AbsentCatalog, AbsentIndex, AliasCatalog, AliasIndex, BlocksCatalog, BlocksIndex,
    SensitivityCatalog, StateMapIndex, StatesCatalog,
};
use indexmap::IndexMap;

use crate::common::stair_props;

fn no_aliases() -> AliasIndex {
    AliasIndex::empty()
}

/// The Bedrock report for a palette expected to carry no leak.
fn bedrock_report(
    ir: &BlockArrayIr,
    blocks: &BlocksIndex,
    aliases: &AliasIndex,
) -> PortabilityReport {
    bedrock(ir, blocks, aliases)
        .expect("the palette carries no blockstate a pack would have refused")
}

/// The Bedrock report through [`states`], which maps [`table`]'s versions.
fn bedrock(
    ir: &BlockArrayIr,
    blocks: &BlocksIndex,
    aliases: &AliasIndex,
) -> Result<PortabilityReport, InvalidPalette> {
    portability_for_bedrock(ir, blocks, aliases, &AbsentIndex::empty(), Some(&states()))
}

/// Three versions with a rename between the first two; only the third has a stair corner state.
fn table() -> BlocksIndex {
    let stairs_1_0 = r#"{ "upside_down_bit": [false, true], "weirdo_direction": [0, 1, 2, 3] }"#;
    let stairs_1_2 = r#"{ "minecraft:corner": ["none", "outer_left", "inner_right"],
        "upside_down_bit": [false, true], "weirdo_direction": [0, 1, 2, 3] }"#;
    let catalog: BlocksCatalog = serde_json::from_str(&format!(
        r#"{{
        "schema_version": 2,
        "namespace": "minecraft",
        "base": {{
            "mc_version": "1.0",
            "blocks": {{
                "oak_planks": {{}}, "oak_stairs": {stairs_1_0},
                "oak_door": {{ "direction": [0, 1, 2, 3] }}, "frame": {{ "facing_direction": [0, 1] }},
                "stonebrick": {{}}
            }}
        }},
        "diffs": [
            {{ "mc_version": "1.1", "inherits": "1.0",
              "added": {{ "stone_bricks": {{}} }}, "removed": ["stonebrick"] }},
            {{ "mc_version": "1.2", "inherits": "1.1",
              "changed": {{ "oak_stairs": {stairs_1_2} }} }}
        ]
    }}"#
    ))
    .expect("test catalog parses as JSON");
    BlocksIndex::from_catalog(catalog, Edition::Bedrock).expect("test catalog folds")
}

/// Stairs that drop their `shape` but at 1.2, and a door 1.0's palette refuses.
fn states() -> StateMapIndex {
    let stairs = |versions: &str, corner: &str| {
        format!(
            r#"{{
            "mc_versions": {versions}, "ids": ["oak_stairs"],
            "java": {{
                "facing": ["north", "south", "west", "east"], "half": ["top", "bottom"],
                "shape": ["straight", "outer_left", "inner_right"], "waterlogged": ["true", "false"]
            }},
            "defaults": {{ "facing": "north", "half": "bottom", "shape": "straight", "waterlogged": "false" }},
            "states": {{
                {corner}
                "upside_down_bit": {{ "reads": ["half"], "map": {{ "top": true, "bottom": false }} }},
                "weirdo_direction": {{ "reads": ["facing"],
                    "map": {{ "north": 3, "south": 2, "west": 1, "east": 0 }} }}
            }},
            "dropped": {dropped}
        }}"#,
            dropped = if corner.is_empty() {
                r#"["shape"]"#
            } else {
                "[]"
            },
        )
    };
    let corner = r#""minecraft:corner": { "reads": ["shape"],
        "map": { "straight": "none", "outer_left": "outer_left", "inner_right": "inner_right" } },"#;
    let catalog: StatesCatalog = serde_json::from_str(&format!(
        r#"{{
        "schema_version": 1,
        "namespace": "minecraft",
        "refused": {{ "1.0": ["oak_door"] }},
        "families": [
            {},
            {},
            {{
                "mc_versions": ["1.1", "1.2"], "ids": ["oak_door"],
                "java": {{ "facing": ["north", "south", "west", "east"] }},
                "defaults": {{ "facing": "north" }},
                "states": {{ "direction": {{ "reads": ["facing"],
                    "map": {{ "north": 3, "south": 1, "west": 2, "east": 0 }} }} }}
            }}
        ]
    }}"#,
        stairs(r#"["1.0", "1.1"]"#, ""),
        stairs(r#"["1.2"]"#, corner),
    ))
    .expect("test states parse as JSON");
    let sensitivity: SensitivityCatalog = serde_json::from_str(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "dropped_states": {
            "shape": [{ "kind": "missing_state", "reason": "stairs render straight" }]
        }
    }"#,
    )
    .expect("test sensitivity parses as JSON");
    StateMapIndex::from_catalogs(catalog, sensitivity, &table()).expect("test states validate")
}

/// The shape a stair drops at `mc_versions`, as [`states`] words it.
fn dropped_shape(value: &str, mc_versions: &[&str]) -> DroppedIntent {
    DroppedIntent {
        key: "shape".to_owned(),
        value: value.to_owned(),
        mc_versions: mc_versions.iter().map(|v| (*v).to_owned()).collect(),
        reason: "stairs render straight".to_owned(),
    }
}

fn facing(value: &str) -> IndexMap<String, String> {
    [("facing".to_owned(), value.to_owned())]
        .into_iter()
        .collect()
}

fn one_block_array(scope: &str, states: Vec<BlockState>) -> BlockArray {
    let mut palette = Palette::new_with_air();
    for s in states {
        palette.intern(s);
    }
    BlockArray {
        dims: Dims { x: 1, y: 1, z: 1 },
        palette,
        voxels: vec![PaletteIndex::AIR],
        block_entities: Vec::new(),
        entities: Vec::new(),
        source_scope: scope.to_owned(),
        derived: Vec::new(),
    }
}

fn one_state_ir(states: Vec<BlockState>) -> BlockArrayIr {
    let mut structures = IndexMap::new();
    structures.insert(
        "struct::probe".to_owned(),
        one_block_array("struct::probe", states),
    );
    BlockArrayIr {
        structures,
        placements: IndexMap::new(),
        walkways: IndexMap::new(),
        members: IndexMap::new(),
        diagnostics: Vec::new(),
    }
}

#[test]
fn bedrock_counts_bare_stair_corner_and_unmapped_entries() {
    let ir = one_state_ir(vec![
        BlockState::bare("minecraft:oak_planks"),
        BlockState {
            id: "minecraft:oak_stairs".to_owned(),
            properties: stair_props("north", "bottom", "straight"),
        },
        BlockState {
            id: "minecraft:oak_stairs".to_owned(),
            properties: stair_props("south", "top", "outer_left"),
        },
        BlockState {
            id: "minecraft:frame".to_owned(),
            properties: facing("north"),
        },
    ]);
    let counts = bedrock_report(&ir, &table(), &no_aliases()).counts();
    assert_eq!(
        counts,
        PortabilityCounts {
            portable: 2,
            degraded: 1,
            unsupported: 1,
        },
        "unexpected fold: {counts:?}",
    );
}

#[test]
fn a_state_some_version_maps_is_portable_for_the_edition() {
    let door = BlockState {
        id: "minecraft:oak_door".to_owned(),
        properties: facing("north"),
    };
    let map = states();
    let at = |version| translate_states(&map.at(version).expect("mapped"), &door, &[]);
    assert!(
        matches!(at("1.0"), Err(BedrockStateError::Unmapped { .. })),
        "premise: 1.0 has no form for the door",
    );
    assert!(at("1.1").is_ok(), "premise: 1.1 writes it");
    assert_eq!(
        bedrock_report(&one_state_ir(vec![door]), &table(), &no_aliases()).counts(),
        PortabilityCounts {
            portable: 1,
            degraded: 0,
            unsupported: 0,
        },
    );
}

#[test]
fn java_reports_every_non_air_entry_as_portable() {
    let ir = one_state_ir(vec![
        BlockState::bare("minecraft:oak_planks"),
        BlockState {
            id: "minecraft:oak_stairs".to_owned(),
            properties: stair_props("south", "top", "outer_left"),
        },
        BlockState {
            id: "minecraft:oak_door".to_owned(),
            properties: {
                let mut m = IndexMap::new();
                m.insert("facing".to_owned(), "north".to_owned());
                m
            },
        },
    ]);
    let counts = portability_for_java(&ir, &table(), &no_aliases(), &AbsentIndex::empty()).counts();
    assert_eq!(
        counts,
        PortabilityCounts {
            portable: 3,
            degraded: 0,
            unsupported: 0,
        },
    );
}

#[test]
fn air_slot_is_skipped_by_both_editions() {
    let ir = one_state_ir(Vec::new());
    assert_eq!(
        portability_for_java(&ir, &table(), &no_aliases(), &AbsentIndex::empty()).counts(),
        PortabilityCounts::default()
    );
    assert_eq!(
        bedrock_report(&ir, &table(), &no_aliases()).counts(),
        PortabilityCounts::default()
    );
}

#[test]
fn an_id_the_edition_never_declares_is_unsupported_on_both_sides() {
    let ir = one_state_ir(vec![
        BlockState::bare("minecraft:oak_planks"),
        BlockState::bare("minecraft:totally_not_a_block"),
    ]);
    let blocks = table();
    assert_eq!(
        bedrock_report(&ir, &blocks, &no_aliases()).counts(),
        PortabilityCounts {
            portable: 1,
            degraded: 0,
            unsupported: 1,
        },
    );
    assert_eq!(
        portability_for_java(&ir, &blocks, &no_aliases(), &AbsentIndex::empty()).counts(),
        PortabilityCounts {
            portable: 1,
            degraded: 0,
            unsupported: 1,
        },
    );
}

#[test]
fn an_id_only_part_of_the_range_declares_is_not_unsupported() {
    let ir = one_state_ir(vec![
        BlockState::bare("minecraft:stonebrick"),
        BlockState::bare("minecraft:stone_bricks"),
    ]);
    let blocks = table();
    assert_eq!(
        bedrock_report(&ir, &blocks, &no_aliases()).counts(),
        PortabilityCounts {
            portable: 2,
            degraded: 0,
            unsupported: 0,
        },
    );
    assert_eq!(
        portability_for_java(&ir, &blocks, &no_aliases(), &AbsentIndex::empty()).counts(),
        PortabilityCounts {
            portable: 2,
            degraded: 0,
            unsupported: 0,
        },
    );
}

#[test]
fn an_absent_id_is_unsupported_once_whatever_its_states() {
    for (id, properties) in [
        ("minecraft:spruce_door", facing("north")),
        (
            "minecraft:spruce_stairs",
            stair_props("south", "top", "outer_left"),
        ),
    ] {
        let ir = one_state_ir(vec![BlockState {
            id: id.to_owned(),
            properties,
        }]);
        let report = bedrock_report(&ir, &table(), &no_aliases());
        assert_eq!(
            report.counts(),
            PortabilityCounts {
                portable: 0,
                degraded: 0,
                unsupported: 1,
            },
            "{id}",
        );
        assert!(
            matches!(
                only_reason(&report),
                UnsupportedReason::AbsentFromEdition { .. }
            ),
            "{id}: {:?}",
            report.unsupported(),
        );
    }
}

#[test]
fn a_java_pack_with_no_id_tables_refutes_nothing() {
    let ir = one_state_ir(vec![
        BlockState::bare("minecraft:oak_planks"),
        BlockState::bare("minecraft:totally_not_a_block"),
    ]);
    let report = portability_for_java(
        &ir,
        &BlocksIndex::empty(),
        &no_aliases(),
        &AbsentIndex::empty(),
    );
    assert_eq!(
        report.counts(),
        PortabilityCounts {
            portable: 2,
            degraded: 0,
            unsupported: 0,
        },
    );
    assert_eq!(report.unsupported(), []);
}

#[test]
fn a_bedrock_pack_without_a_state_map_writes_no_entry() {
    let ir = one_state_ir(vec![BlockState::bare("minecraft:oak_planks")]);
    let report = portability_for_bedrock(&ir, &table(), &no_aliases(), &AbsentIndex::empty(), None)
        .expect("no leak");
    assert_eq!(
        only_reason(&report),
        &UnsupportedReason::StatesUnmapped {
            states: String::new(),
            detail: "no version that declares the block carries a state map".to_owned(),
        },
    );
}

#[test]
fn a_walkway_strip_is_counted_like_any_other_entry() {
    let mut structures = IndexMap::new();
    structures.insert(
        "site::duo::home1".to_owned(),
        one_block_array(
            "site::duo::home1",
            vec![BlockState::bare("minecraft:oak_planks")],
        ),
    );
    structures.insert(
        "walkway::duo::a.entry__b.entry".to_owned(),
        one_block_array(
            "walkway::duo::a.entry__b.entry",
            vec![BlockState::bare("minecraft:totally_not_a_block")],
        ),
    );
    let ir = BlockArrayIr {
        structures,
        placements: IndexMap::new(),
        walkways: IndexMap::new(),
        members: IndexMap::new(),
        diagnostics: Vec::new(),
    };
    let report = bedrock_report(&ir, &table(), &no_aliases());
    assert_eq!(
        report.counts(),
        PortabilityCounts {
            portable: 1,
            degraded: 0,
            unsupported: 1,
        },
    );
    assert_eq!(
        only_reason(&report),
        &UnsupportedReason::AbsentFromEdition {
            suggestion: None,
            aliases: Vec::new(),
        },
    );
}

#[test]
fn empty_ir_produces_zero_counts() {
    let ir = BlockArrayIr {
        structures: IndexMap::new(),
        placements: IndexMap::new(),
        walkways: IndexMap::new(),
        members: IndexMap::new(),
        diagnostics: Vec::new(),
    };
    assert_eq!(
        portability_for_java(&ir, &table(), &no_aliases(), &AbsentIndex::empty()).counts(),
        PortabilityCounts::default()
    );
    assert_eq!(
        bedrock_report(&ir, &table(), &no_aliases()).counts(),
        PortabilityCounts::default()
    );
}

/// The reason of `report`'s single unsupported entry.
fn only_reason(report: &PortabilityReport) -> &UnsupportedReason {
    assert_eq!(
        report.unsupported().len(),
        1,
        "expected exactly one named entry, got {:?}",
        report.unsupported(),
    );
    &report.unsupported()[0].reason
}

#[test]
fn an_absent_id_is_named_with_the_nearest_id_the_edition_declares() {
    let ir = one_state_ir(vec![BlockState::bare("minecraft:oak_planck")]);
    let report = portability_for_java(&ir, &table(), &no_aliases(), &AbsentIndex::empty());
    assert_eq!(report.counts().unsupported, 1);
    assert_eq!(report.unsupported()[0].id, "minecraft:oak_planck");
    assert_eq!(
        only_reason(&report),
        &UnsupportedReason::AbsentFromEdition {
            suggestion: Some("minecraft:oak_planks".to_owned()),
            aliases: Vec::new(),
        },
    );
}

#[test]
fn a_spelling_only_the_other_edition_has_is_named_under_both() {
    let ir = one_state_ir(vec![BlockState::bare("minecraft:standing_sign")]);
    for report in [
        portability_for_java(&ir, &table(), &no_aliases(), &AbsentIndex::empty()),
        bedrock_report(&ir, &table(), &no_aliases()),
    ] {
        assert_eq!(report.counts().unsupported, 1);
        assert_eq!(report.unsupported()[0].id, "minecraft:standing_sign");
        assert!(
            matches!(
                only_reason(&report),
                UnsupportedReason::AbsentFromEdition { .. }
            ),
            "got {:?}",
            report.unsupported()[0].reason,
        );
    }
}

#[test]
fn a_block_only_the_other_edition_has_is_named_with_the_packs_reason() {
    let catalog: AbsentCatalog = serde_json::from_str(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "entries": [{ "ids": ["oak_planck"], "reason": "it is the other edition's" }]
    }"#,
    )
    .expect("test catalog parses as JSON");
    let absent = AbsentIndex::from_catalog(catalog).expect("test catalog validates");
    let ir = one_state_ir(vec![BlockState::bare("minecraft:oak_planck")]);
    let expected = UnsupportedReason::NoCounterpart {
        detail: "it is the other edition's".to_owned(),
    };
    let java = portability_for_java(&ir, &table(), &no_aliases(), &absent);
    let bedrock = portability_for_bedrock(&ir, &table(), &no_aliases(), &absent, Some(&states()))
        .expect("the palette carries no blockstate a pack would have refused");
    assert_eq!(
        [only_reason(&java), only_reason(&bedrock)],
        [&expected, &expected],
    );
}

/// The alias table for [`table`]: its rename, and a cross-edition spelling.
fn aliases() -> AliasIndex {
    let catalog: AliasCatalog = serde_json::from_str(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "groups": [
            { "spellings": ["stonebrick", "stone_bricks"] },
            { "spellings": ["standing_sign", "oak_sign"] }
        ]
    }"#,
    )
    .expect("test catalog parses as JSON");
    AliasIndex::from_catalog(catalog).expect("test catalog folds")
}

/// `standing_sign` is seven edits from `oak_sign`, out of the typo search's reach.
#[test]
fn a_renamed_id_is_reported_with_the_name_this_edition_uses() {
    let ir = one_state_ir(vec![BlockState::bare("minecraft:standing_sign")]);
    let with_table = portability_for_java(
        &ir,
        &table_with_oak_sign(),
        &aliases(),
        &AbsentIndex::empty(),
    );
    assert_eq!(
        only_reason(&with_table),
        &UnsupportedReason::AbsentFromEdition {
            suggestion: None,
            aliases: vec!["minecraft:oak_sign".to_owned()],
        },
    );
}

#[test]
fn an_alias_any_version_declares_counts_for_the_edition() {
    let ir = one_state_ir(vec![BlockState::bare("minecraft:chiseled_stone_bricks")]);
    let catalog: AliasCatalog = serde_json::from_str(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "groups": [{
            "spellings": ["chiseled_stone_bricks", "stonebrick", "stone_bricks"]
        }]
    }"#,
    )
    .expect("test catalog parses as JSON");
    let index = AliasIndex::from_catalog(catalog).expect("test catalog folds");
    let report = portability_for_java(&ir, &table(), &index, &AbsentIndex::empty());
    assert_eq!(
        only_reason(&report),
        &UnsupportedReason::AbsentFromEdition {
            suggestion: None,
            aliases: vec![
                "minecraft:stonebrick".to_owned(),
                "minecraft:stone_bricks".to_owned(),
            ],
        },
        "both spellings are the edition's, one version each",
    );
}

#[test]
fn an_alias_this_edition_lacks_is_not_offered() {
    let ir = one_state_ir(vec![BlockState::bare("minecraft:standing_sign")]);
    let report = portability_for_java(&ir, &table(), &aliases(), &AbsentIndex::empty());
    assert_eq!(
        only_reason(&report),
        &UnsupportedReason::AbsentFromEdition {
            suggestion: None,
            aliases: Vec::new(),
        },
        "`oak_sign` is no more in this table than `standing_sign` is",
    );
}

/// [`table`] plus `oak_sign`.
fn table_with_oak_sign() -> BlocksIndex {
    let catalog: BlocksCatalog = serde_json::from_str(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "base": {
            "mc_version": "1.0",
            "blocks": ["oak_planks", "oak_stairs", "oak_door", "oak_sign", "stonebrick"]
        },
        "diffs": [
            { "mc_version": "1.1", "inherits": "1.0",
              "added": ["stone_bricks"], "removed": ["stonebrick"] }
        ]
    }"#,
    )
    .expect("test catalog parses as JSON");
    BlocksIndex::from_catalog(catalog, Edition::Bedrock).expect("test catalog folds")
}

#[test]
fn an_id_nothing_resembles_is_named_without_a_suggestion() {
    let ir = one_state_ir(vec![BlockState::bare("minecraft:totally_not_a_block")]);
    let report = bedrock_report(&ir, &table(), &no_aliases());
    assert_eq!(report.unsupported()[0].id, "minecraft:totally_not_a_block");
    assert_eq!(
        only_reason(&report),
        &UnsupportedReason::AbsentFromEdition {
            suggestion: None,
            aliases: Vec::new(),
        },
    );
}

#[test]
fn a_suggestion_is_scored_on_the_path_and_not_on_the_whole_id() {
    // `ok_dr` is three edits from `oak_door`: too many for its path, few enough for the whole id.
    let ir = one_state_ir(vec![BlockState::bare("minecraft:ok_dr")]);
    let report = portability_for_java(&ir, &table(), &no_aliases(), &AbsentIndex::empty());
    assert_eq!(
        only_reason(&report),
        &UnsupportedReason::AbsentFromEdition {
            suggestion: None,
            aliases: Vec::new(),
        },
    );
}

#[test]
fn a_suggestion_can_come_from_any_version_the_edition_declares() {
    // `stone_bricks` is only in the diff and `stonebrick` only in the base.
    for (typo, nearest) in [
        ("minecraft:stone_brickz", "minecraft:stone_bricks"),
        ("minecraft:stonebrik", "minecraft:stonebrick"),
    ] {
        let ir = one_state_ir(vec![BlockState::bare(typo)]);
        let report = bedrock_report(&ir, &table(), &no_aliases());
        assert_eq!(
            only_reason(&report),
            &UnsupportedReason::AbsentFromEdition {
                suggestion: Some(nearest.to_owned()),
                aliases: Vec::new(),
            },
            "for {typo}",
        );
    }
}

#[test]
fn a_suggestion_is_never_drawn_from_the_other_namespace() {
    let ir = one_state_ir(vec![BlockState::bare("mod:oak_planck")]);
    let report = portability_for_java(&ir, &table(), &no_aliases(), &AbsentIndex::empty());
    assert_eq!(
        only_reason(&report),
        &UnsupportedReason::AbsentFromEdition {
            suggestion: None,
            aliases: Vec::new(),
        },
    );
}

#[test]
fn a_block_no_version_maps_the_states_of_is_named_by_the_states_it_carries() {
    let ir = one_state_ir(vec![BlockState {
        id: "minecraft:frame".to_owned(),
        properties: facing("north"),
    }]);
    let report = bedrock_report(&ir, &table(), &no_aliases());
    assert_eq!(report.counts().unsupported, 1);
    assert_eq!(report.unsupported()[0].id, "minecraft:frame");
    assert_eq!(
        only_reason(&report),
        &UnsupportedReason::StatesUnmapped {
            states: "facing=north".to_owned(),
            detail: "no Java block maps onto `minecraft:frame` there, so the canonical vocabulary \
                     names none of its states"
                .to_owned(),
        },
    );
}

/// A note block whose `instrument` gains `trumpet` at 1.2, as the Java release its map reads does.
fn note_block_pack() -> (BlocksIndex, StateMapIndex) {
    let blocks: BlocksCatalog = serde_json::from_str(
        r#"{
        "schema_version": 2,
        "namespace": "minecraft",
        "base": { "mc_version": "1.0", "blocks": { "noteblock": {} } },
        "diffs": [{ "mc_version": "1.2", "inherits": "1.0" }]
    }"#,
    )
    .expect("test catalog parses as JSON");
    let blocks = BlocksIndex::from_catalog(blocks, Edition::Bedrock).expect("test catalog folds");
    let family = |versions: &str, instruments: &str| {
        format!(
            r#"{{
            "mc_versions": {versions}, "ids": ["noteblock=note_block"],
            "java": {{ "instrument": {instruments} }}, "defaults": {{ "instrument": "harp" }},
            "states": {{}}, "dropped": ["instrument"]
        }}"#
        )
    };
    let catalog: StatesCatalog = serde_json::from_str(&format!(
        r#"{{ "schema_version": 1, "namespace": "minecraft", "families": [{}, {}] }}"#,
        family(r#"["1.0"]"#, r#"["harp", "bell"]"#),
        family(r#"["1.2"]"#, r#"["harp", "bell", "trumpet"]"#),
    ))
    .expect("test states parse as JSON");
    let sensitivity: SensitivityCatalog = serde_json::from_str(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "dropped_states": {
            "instrument": [{ "kind": "derived", "reason": "Bedrock reads the block below" }]
        }
    }"#,
    )
    .expect("test sensitivity parses as JSON");
    let states =
        StateMapIndex::from_catalogs(catalog, sensitivity, &blocks).expect("test states validate");
    (blocks, states)
}

fn note_block(instrument: &str) -> BlockState {
    BlockState {
        id: "minecraft:noteblock".to_owned(),
        properties: [("instrument".to_owned(), instrument.to_owned())]
            .into_iter()
            .collect(),
    }
}

#[test]
fn a_value_only_a_later_versions_map_holds_is_unmapped_before_it_and_no_leak() {
    let (blocks, states) = note_block_pack();
    let ir = one_state_ir(vec![note_block("trumpet")]);
    let report = portability_for_bedrock(
        &ir,
        &blocks,
        &no_aliases(),
        &AbsentIndex::empty(),
        Some(&states),
    )
    .expect("a value some version's map holds is no leak");
    assert_eq!(
        report.counts(),
        PortabilityCounts {
            portable: 0,
            degraded: 1,
            unsupported: 0,
        },
    );
    let map = states.at("1.0").expect("1.0 has a map");
    assert_eq!(
        translate_states(&map, &note_block("trumpet"), &[]).expect_err("unmapped at 1.0"),
        BedrockStateError::Unmapped {
            id: "minecraft:noteblock".to_owned(),
            properties: "instrument=trumpet".to_owned(),
            mc_version: "1.0".to_owned(),
            reason: "its state map there is read from a Java release whose \
                     `minecraft:note_block` has no `instrument=trumpet`, which the map of 1.2 \
                     holds"
                .to_owned(),
        },
    );

    let ir = one_state_ir(vec![note_block("kazoo")]);
    let invalid = portability_for_bedrock(
        &ir,
        &blocks,
        &no_aliases(),
        &AbsentIndex::empty(),
        Some(&states),
    )
    .expect_err("a value no version's map holds leaks");
    assert_eq!(
        invalid.leaks(),
        [BedrockStateError::UnknownValue {
            id: "minecraft:noteblock".to_owned(),
            key: "instrument".to_owned(),
            value: "kazoo".to_owned(),
            valid: "harp, bell, trumpet".to_owned(),
        }],
    );
}

#[test]
fn a_state_bedrock_works_out_itself_degrades_an_entry_only_where_no_lowering_derived_it() {
    let (blocks, states) = note_block_pack();
    let report = |derived: Vec<Vec<String>>| {
        let mut ir = one_state_ir(vec![note_block("bell")]);
        ir.structures["struct::probe"].derived = derived;
        portability_for_bedrock(
            &ir,
            &blocks,
            &no_aliases(),
            &AbsentIndex::empty(),
            Some(&states),
        )
        .expect("bell is in every version's map")
    };
    assert_eq!(
        report(vec![Vec::new(), vec!["instrument".to_owned()]]).counts(),
        PortabilityCounts {
            portable: 1,
            degraded: 0,
            unsupported: 0,
        },
    );
    assert_eq!(
        report(Vec::new()).degraded(),
        [DegradedEntry {
            id: "minecraft:noteblock".to_owned(),
            states: "instrument=bell".to_owned(),
            dropped: vec![DroppedIntent {
                key: "instrument".to_owned(),
                value: "bell".to_owned(),
                mc_versions: vec!["1.0".to_owned(), "1.2".to_owned()],
                reason: "the written `instrument=bell` is not kept: Bedrock reads the block below"
                    .to_owned(),
            }],
        }],
    );
}

#[test]
fn a_state_value_outside_the_java_domain_refuses_the_report() {
    let ir = one_state_ir(vec![BlockState {
        id: "minecraft:oak_stairs".to_owned(),
        properties: stair_props("up", "bottom", "straight"),
    }]);
    let invalid =
        bedrock(&ir, &table(), &no_aliases()).expect_err("a leaked state value refuses the report");
    assert_eq!(
        invalid.leaks(),
        [BedrockStateError::UnknownValue {
            id: "minecraft:oak_stairs".to_owned(),
            key: "facing".to_owned(),
            value: "up".to_owned(),
            valid: "north, south, west, east".to_owned(),
        }],
    );
}

#[test]
fn a_property_the_java_block_lacks_refuses_the_report() {
    let ir = one_state_ir(vec![BlockState {
        id: "minecraft:oak_stairs".to_owned(),
        properties: {
            let mut m = stair_props("north", "bottom", "straight");
            m.insert("axis".to_owned(), "x".to_owned());
            m
        },
    }]);
    let invalid =
        bedrock(&ir, &table(), &no_aliases()).expect_err("a leaked state key refuses the report");
    assert_eq!(
        invalid.leaks(),
        [BedrockStateError::UnknownProperty {
            id: "minecraft:oak_stairs".to_owned(),
            key: "axis".to_owned(),
            valid: "facing, half, shape, waterlogged".to_owned(),
        }],
    );
}

#[test]
fn a_leak_refuses_the_whole_palette_and_names_every_one_of_them() {
    let ir = one_state_ir(vec![
        BlockState::bare("minecraft:oak_planks"),
        BlockState {
            id: "minecraft:oak_stairs".to_owned(),
            properties: stair_props("south", "top", "outer_left"),
        },
        BlockState::bare("minecraft:nothing_like_this"),
        BlockState {
            id: "minecraft:oak_stairs".to_owned(),
            properties: stair_props("up", "bottom", "straight"),
        },
        BlockState {
            id: "minecraft:oak_stairs".to_owned(),
            properties: {
                let mut m = stair_props("north", "top", "straight");
                m.insert("axis".to_owned(), "x".to_owned());
                m
            },
        },
    ]);
    let invalid = bedrock(&ir, &table(), &no_aliases())
        .expect_err("one leak refuses the palette however well the rest classifies");
    let leaked: Vec<String> = invalid.leaks().iter().map(ToString::to_string).collect();
    assert_eq!(
        leaked.len(),
        2,
        "every leak is named, not the first: {leaked:?}",
    );
    assert!(
        leaked[0].contains("facing=up") && leaked[1].contains("`axis`"),
        "in palette order, unreworded: {leaked:?}",
    );
    let clean = one_state_ir(vec![
        BlockState::bare("minecraft:oak_planks"),
        BlockState {
            id: "minecraft:oak_stairs".to_owned(),
            properties: stair_props("south", "top", "outer_left"),
        },
        BlockState::bare("minecraft:nothing_like_this"),
    ]);
    assert_eq!(
        bedrock_report(&clean, &table(), &no_aliases()).counts(),
        PortabilityCounts {
            portable: 1,
            degraded: 1,
            unsupported: 1,
        },
    );
}

#[test]
fn a_block_no_version_maps_the_states_of_is_not_a_leak() {
    let ir = one_state_ir(vec![BlockState {
        id: "minecraft:frame".to_owned(),
        properties: facing("north"),
    }]);
    let report = bedrock(&ir, &table(), &no_aliases())
        .expect("an unmapped entry is a portability answer, not a leak");
    assert_eq!(report.counts().unsupported, 1);
}

#[test]
fn a_degraded_entry_is_named_with_the_states_that_degraded_it() {
    let ir = one_state_ir(vec![BlockState {
        id: "minecraft:oak_stairs".to_owned(),
        properties: stair_props("south", "top", "outer_left"),
    }]);
    let report = bedrock_report(&ir, &table(), &no_aliases());
    assert_eq!(report.counts().degraded, 1);
    assert_eq!(
        report.degraded(),
        [DegradedEntry {
            id: "minecraft:oak_stairs".to_owned(),
            states: "facing=south,half=top,shape=outer_left".to_owned(),
            dropped: vec![dropped_shape("outer_left", &["1.0", "1.1"])],
        }],
    );
    assert!(
        report.unsupported().is_empty(),
        "got {:?}",
        report.unsupported(),
    );
}

#[test]
fn two_state_combinations_of_one_id_are_two_named_entries() {
    let ir = one_state_ir(vec![
        BlockState {
            id: "minecraft:oak_stairs".to_owned(),
            properties: stair_props("south", "top", "outer_left"),
        },
        BlockState {
            id: "minecraft:oak_stairs".to_owned(),
            properties: stair_props("south", "top", "inner_right"),
        },
    ]);
    let report = bedrock_report(&ir, &table(), &no_aliases());
    assert_eq!(report.counts().degraded, 2);
    let dropped: Vec<&DroppedIntent> = report
        .degraded()
        .iter()
        .flat_map(|entry| entry.dropped.iter())
        .collect();
    assert_eq!(
        dropped,
        [
            &dropped_shape("outer_left", &["1.0", "1.1"]),
            &dropped_shape("inner_right", &["1.0", "1.1"]),
        ],
    );
    assert_eq!(
        report.degraded().len(),
        report.counts().degraded as usize,
        "the list and the figure count the same entries",
    );
}

#[test]
fn the_named_entries_are_the_counted_ones_in_palette_order() {
    let ir = one_state_ir(vec![
        BlockState::bare("minecraft:no_such_block_at_all"),
        BlockState::bare("minecraft:oak_planks"),
        BlockState {
            id: "minecraft:oak_stairs".to_owned(),
            properties: stair_props("south", "top", "outer_left"),
        },
        BlockState {
            id: "minecraft:frame".to_owned(),
            properties: facing("north"),
        },
    ]);
    let report = bedrock_report(&ir, &table(), &no_aliases());
    assert_eq!(
        report.counts(),
        PortabilityCounts {
            portable: 1,
            degraded: 1,
            unsupported: 2,
        },
    );
    let named: Vec<&str> = report
        .unsupported()
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    assert_eq!(named, ["minecraft:no_such_block_at_all", "minecraft:frame"]);
    assert_eq!(
        report
            .degraded()
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        ["minecraft:oak_stairs"],
    );
    assert_eq!(
        report.unsupported().len(),
        report.counts().unsupported as usize,
        "the list and the figure count the same entries",
    );
    assert_eq!(
        report.degraded().len(),
        report.counts().degraded as usize,
        "the list and the figure count the same entries",
    );
}

#[test]
fn total_returns_sum_of_categories() {
    let counts = PortabilityCounts {
        portable: 3,
        degraded: 2,
        unsupported: 1,
    };
    assert_eq!(counts.total(), 6);
}
