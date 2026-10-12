//! The portability counters over hand-built palettes and block tables.

use cairn_lang_core::block_array::{
    BlockArray, BlockArrayIr, BlockState, Dims, Palette, PaletteIndex,
};
use cairn_lang_core::resolve::{DegradedEntry, DroppedIntent, UnsupportedReason};
use cairn_lang_formats::bedrock_state::{BedrockStateError, translate_states};
use cairn_lang_formats::portability::{
    PortabilityCounts, PortabilityReport, portability_for_bedrock, portability_for_java,
};
use cairn_lang_formats::registry::{AliasCatalog, AliasIndex, BlocksCatalog, BlocksIndex};
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
    portability_for_bedrock(ir, blocks, aliases)
        .expect("the palette carries no blockstate a pack would have refused")
}

/// Two versions with a rename between them.
fn table() -> BlocksIndex {
    let catalog: BlocksCatalog = serde_json::from_str(
        r#"{
        "schema_version": 1,
        "namespace": "minecraft",
        "base": {
            "mc_version": "1.0",
            "blocks": ["oak_planks", "oak_stairs", "oak_door", "stonebrick"]
        },
        "diffs": [
            { "mc_version": "1.1", "inherits": "1.0",
              "added": ["stone_bricks"], "removed": ["stonebrick"] }
        ]
    }"#,
    )
    .expect("test catalog parses as JSON");
    BlocksIndex::from_catalog(catalog).expect("test catalog folds")
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
        diagnostics: Vec::new(),
    }
}

#[test]
fn bedrock_counts_bare_stair_corner_and_stateful_non_stair() {
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
            id: "minecraft:oak_door".to_owned(),
            properties: {
                let mut m = IndexMap::new();
                m.insert("facing".to_owned(), "north".to_owned());
                m
            },
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
    let counts = portability_for_java(&ir, &table(), &no_aliases()).counts();
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
        portability_for_java(&ir, &table(), &no_aliases()).counts(),
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
        portability_for_java(&ir, &blocks, &no_aliases()).counts(),
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
        portability_for_java(&ir, &blocks, &no_aliases()).counts(),
        PortabilityCounts {
            portable: 2,
            degraded: 0,
            unsupported: 0,
        },
    );
}

#[test]
fn an_absent_id_with_unsupported_states_is_counted_once() {
    let mut properties = IndexMap::new();
    properties.insert("facing".to_owned(), "north".to_owned());
    let door = BlockState {
        id: "minecraft:spruce_door".to_owned(),
        properties,
    };
    assert!(
        translate_states(&door).is_err(),
        "premise: the states alone would already be unsupported",
    );
    let ir = one_state_ir(vec![door]);
    let counts = bedrock_report(&ir, &table(), &no_aliases()).counts();
    assert_eq!(
        counts,
        PortabilityCounts {
            portable: 0,
            degraded: 0,
            unsupported: 1,
        },
    );
    assert_eq!(counts.total(), 1, "one palette entry, one count");
}

#[test]
fn an_absent_id_whose_states_would_degrade_is_still_unsupported() {
    let stair = BlockState {
        id: "minecraft:spruce_stairs".to_owned(),
        properties: stair_props("south", "top", "outer_left"),
    };
    let translated = translate_states(&stair).expect("premise: the states translate");
    assert!(
        !translated.degraded.is_empty(),
        "premise: the states translate with loss, so `degraded` is the competing answer",
    );
    let ir = one_state_ir(vec![stair]);
    assert_eq!(
        bedrock_report(&ir, &table(), &no_aliases()).counts(),
        PortabilityCounts {
            portable: 0,
            degraded: 0,
            unsupported: 1,
        },
    );
}

#[test]
fn a_pack_with_no_id_tables_refutes_nothing() {
    let ir = one_state_ir(vec![
        BlockState::bare("minecraft:oak_planks"),
        BlockState::bare("minecraft:totally_not_a_block"),
    ]);
    let none = BlocksIndex::empty();
    assert_eq!(
        portability_for_java(&ir, &none, &no_aliases()).counts(),
        PortabilityCounts {
            portable: 2,
            degraded: 0,
            unsupported: 0,
        },
    );
    assert_eq!(
        bedrock_report(&ir, &none, &no_aliases()).counts(),
        PortabilityCounts {
            portable: 2,
            degraded: 0,
            unsupported: 0,
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
        diagnostics: Vec::new(),
    };
    assert_eq!(
        portability_for_java(&ir, &table(), &no_aliases()).counts(),
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
    let report = portability_for_java(&ir, &table(), &no_aliases());
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
        portability_for_java(&ir, &table(), &no_aliases()),
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
    let with_table = portability_for_java(&ir, &table_with_oak_sign(), &aliases());
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
    let report = portability_for_java(&ir, &table(), &index);
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
    let report = portability_for_java(&ir, &table(), &aliases());
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
    BlocksIndex::from_catalog(catalog).expect("test catalog folds")
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
    let report = portability_for_java(&ir, &table(), &no_aliases());
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
    let report = portability_for_java(&ir, &table(), &no_aliases());
    assert_eq!(
        only_reason(&report),
        &UnsupportedReason::AbsentFromEdition {
            suggestion: None,
            aliases: Vec::new(),
        },
    );
}

#[test]
fn a_block_the_backend_maps_no_states_for_is_named_by_the_states_it_carries() {
    let mut properties = IndexMap::new();
    properties.insert("facing".to_owned(), "north".to_owned());
    let ir = one_state_ir(vec![BlockState {
        id: "minecraft:oak_door".to_owned(),
        properties,
    }]);
    let report = bedrock_report(&ir, &table(), &no_aliases());
    assert_eq!(report.counts().unsupported, 1);
    assert_eq!(report.unsupported()[0].id, "minecraft:oak_door");
    assert_eq!(
        only_reason(&report),
        &UnsupportedReason::StatesUnmapped {
            states: "facing=north".to_owned(),
            mapped: "the stair family".to_owned(),
        },
    );
}

#[test]
fn a_state_value_outside_the_java_domain_refuses_the_report() {
    let ir = one_state_ir(vec![BlockState {
        id: "minecraft:oak_stairs".to_owned(),
        properties: stair_props("up", "bottom", "straight"),
    }]);
    let invalid = portability_for_bedrock(&ir, &table(), &no_aliases())
        .expect_err("a leaked state value refuses the report");
    assert_eq!(
        invalid.leaks(),
        [BedrockStateError::UnknownStairState {
            id: "minecraft:oak_stairs".to_owned(),
            key: "facing",
            value: "up".to_owned(),
            valid: "east, west, south, north",
        }],
    );
}

#[test]
fn a_state_key_the_backend_does_not_read_refuses_the_report() {
    let ir = one_state_ir(vec![BlockState {
        id: "minecraft:oak_stairs".to_owned(),
        properties: {
            let mut m = stair_props("north", "bottom", "straight");
            m.insert("waterlogged".to_owned(), "true".to_owned());
            m
        },
    }]);
    let invalid = portability_for_bedrock(&ir, &table(), &no_aliases())
        .expect_err("a leaked state key refuses the report");
    assert_eq!(
        invalid.leaks(),
        [BedrockStateError::UnknownStairKey {
            id: "minecraft:oak_stairs".to_owned(),
            key: "waterlogged".to_owned(),
            handled: "facing, half, shape",
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
                m.insert("waterlogged".to_owned(), "true".to_owned());
                m
            },
        },
    ]);
    let invalid = portability_for_bedrock(&ir, &table(), &no_aliases())
        .expect_err("one leak refuses the palette however well the rest classifies");
    let leaked: Vec<String> = invalid.leaks().iter().map(ToString::to_string).collect();
    assert_eq!(
        leaked.len(),
        2,
        "every leak is named, not the first: {leaked:?}",
    );
    assert!(
        leaked[0].contains("facing=up") && leaked[1].contains("waterlogged"),
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
fn a_block_the_backend_maps_no_states_for_is_not_a_leak() {
    let mut properties = IndexMap::new();
    properties.insert("facing".to_owned(), "north".to_owned());
    let ir = one_state_ir(vec![BlockState {
        id: "minecraft:oak_door".to_owned(),
        properties,
    }]);
    let report = portability_for_bedrock(&ir, &table(), &no_aliases())
        .expect("an unmapped family is a portability answer, not a leak");
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
            dropped: vec![DroppedIntent::Shape {
                value: "outer_left".to_owned(),
            }],
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
            &DroppedIntent::Shape {
                value: "outer_left".to_owned(),
            },
            &DroppedIntent::Shape {
                value: "inner_right".to_owned(),
            },
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
            id: "minecraft:oak_door".to_owned(),
            properties: {
                let mut m = IndexMap::new();
                m.insert("facing".to_owned(), "north".to_owned());
                m
            },
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
    assert_eq!(
        named,
        ["minecraft:no_such_block_at_all", "minecraft:oak_door"]
    );
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
fn a_pack_that_cannot_refute_an_id_names_nothing() {
    let ir = one_state_ir(vec![BlockState::bare("minecraft:totally_not_a_block")]);
    let none = BlocksIndex::empty();
    for report in [
        portability_for_java(&ir, &none, &no_aliases()),
        bedrock_report(&ir, &none, &no_aliases()),
    ] {
        assert_eq!(report.counts().unsupported, 0);
        assert!(
            report.unsupported().is_empty(),
            "got {:?}",
            report.unsupported()
        );
    }
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
