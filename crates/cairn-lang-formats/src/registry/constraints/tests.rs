use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{Constraint, Face, Support};
use indexmap::IndexMap;

use super::*;
use crate::registry::{builtin_pack, load_builtin_constraints, parse_constraints};

/// A catalog of `tables`, comma-separated JSON members, checked against the built-in packs.
fn catalog(tables: &str) -> Result<ConstraintIndex, ConstraintsError> {
    let text = format!(
        r#"{{"schema_version": 1, "name": "t", "description": "d", "namespace": "minecraft"{}{tables}}}"#,
        if tables.is_empty() { "" } else { ", " },
    );
    parse_constraints(
        &text,
        builtin_pack(Edition::Java),
        builtin_pack(Edition::Bedrock),
    )
}

fn props(pairs: &[(&str, &str)]) -> IndexMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

#[test]
fn the_builtin_catalog_loads_and_hashes_its_text() {
    let builtin = load_builtin_constraints().expect("the built-in catalog loads");
    assert_eq!(
        builtin.hash(),
        &HashHex::from_bytes(include_bytes!("../../../registry-data/constraints.json"))
    );
    insta::assert_snapshot!(builtin.hash().to_string());
}

#[test]
fn an_empty_catalog_loads_and_constrains_nothing() {
    let empty = catalog("").expect("a catalog of no table loads");
    assert_eq!(empty.constraint("minecraft:sand", &props(&[])), None);
}

#[test]
fn a_button_hangs_by_its_face_and_facing() {
    let index = catalog(
        r#""attachment": [{"ids": ["oak_button"], "hangs_by": "face",
            "defaults": {"face": "wall", "facing": "north"}}]"#,
    )
    .expect("loads");
    let hangs = |pairs: &[(&str, &str)]| index.constraint("minecraft:oak_button", &props(pairs));
    assert_eq!(
        hangs(&[("face", "floor")]),
        Some(Constraint::Hangs { host: Face::Down })
    );
    assert_eq!(
        hangs(&[("face", "ceiling")]),
        Some(Constraint::Hangs { host: Face::Up })
    );
    assert_eq!(
        hangs(&[("face", "wall"), ("facing", "east")]),
        Some(Constraint::Hangs { host: Face::West }),
    );
    assert_eq!(
        hangs(&[]),
        Some(Constraint::Hangs { host: Face::South }),
        "the default state"
    );
    assert_eq!(
        index.constraint("minecraft:stone_button", &props(&[])),
        None
    );
}

#[test]
fn a_wall_torch_hangs_on_the_block_behind_the_way_it_faces() {
    let index = catalog(
        r#""attachment": [{"ids": ["wall_torch"], "hangs_by": "facing",
            "defaults": {"facing": "north"}}]"#,
    )
    .expect("loads");
    assert_eq!(
        index.constraint("minecraft:wall_torch", &props(&[("facing", "west")])),
        Some(Constraint::Hangs { host: Face::East }),
    );
}

#[test]
fn a_lantern_rests_below_or_hangs_above_as_its_state_says() {
    let index = catalog(
        r#""support": [
            {"ids": ["lantern"], "when": {"hanging": "false"}, "defaults": {"hanging": "false"},
             "on": "below", "needs": "center"},
            {"ids": ["lantern"], "when": {"hanging": "true"}, "defaults": {"hanging": "false"},
             "on": "above", "needs": "center"},
            {"ids": ["redstone_wire"], "on": "below", "needs": "sturdy", "also_on": ["hopper"]}
        ]"#,
    )
    .expect("loads");
    let lantern = |pairs: &[(&str, &str)]| index.constraint("minecraft:lantern", &props(pairs));
    let resting = |host| Constraint::Rests {
        host,
        needs: Support::Center,
    };
    assert_eq!(lantern(&[]), Some(resting(Face::Down)));
    assert_eq!(lantern(&[("hanging", "true")]), Some(resting(Face::Up)));
    assert!(index.rests_on_any_face("minecraft:redstone_wire", &props(&[]), "minecraft:hopper"));
    assert!(!index.rests_on_any_face("minecraft:redstone_wire", &props(&[]), "minecraft:glass"));
    assert!(!index.rests_on_any_face("minecraft:lantern", &props(&[]), "minecraft:hopper"));
}

#[test]
fn a_state_outside_every_when_has_no_rule() {
    let index = catalog(
        r#""support": [{"ids": ["oak_door"], "when": {"half": "lower"},
            "defaults": {"half": "lower"}, "on": "below", "needs": "sturdy"}]"#,
    )
    .expect("loads");
    assert_eq!(
        index.constraint("minecraft:oak_door", &props(&[("half", "upper")])),
        None,
    );
    assert_eq!(
        index.constraint("minecraft:oak_door", &props(&[])),
        Some(Constraint::Rests {
            host: Face::Down,
            needs: Support::Sturdy,
        }),
    );
}

#[test]
fn a_centre_row_gives_its_faces_in_the_states_it_names() {
    let index = catalog(
        r#""center_support": [{"ids": ["chain"], "when": {"axis": "y"},
            "defaults": {"axis": "y"}, "faces": ["up", "down"]}]"#,
    )
    .expect("loads");
    let gives =
        |pairs: &[(&str, &str)], face| index.gives_centre("minecraft:chain", &props(pairs), face);
    assert!(gives(&[], Face::Up));
    assert!(gives(&[("axis", "y")], Face::Down));
    assert!(!gives(&[("axis", "x")], Face::Up));
    assert!(!gives(&[], Face::North));
}

#[test]
fn a_boundary_names_its_edition_and_reason() {
    let index = catalog(
        r#""semantic_sensitivity": [{"edition": "java", "boundary": "1.17",
            "reason": " cauldron split ", "ids": ["cauldron"]}]"#,
    )
    .expect("loads");
    assert_eq!(
        index
            .boundaries_of(Edition::Java, "minecraft:cauldron")
            .collect::<Vec<_>>(),
        [Boundary {
            version: "1.17",
            reason: "cauldron split",
        }],
    );
    assert_eq!(
        index
            .boundaries_of(Edition::Bedrock, "minecraft:cauldron")
            .count(),
        0
    );
}

#[test]
fn an_entity_box_is_read_in_blocks() {
    let index = catalog(
        r#""entity_aabb": [{"entities": ["item_frame"], "width": 0.75, "height": 0.75,
            "depth": 0.0625}]"#,
    )
    .expect("loads");
    assert_eq!(
        index.entity_box("minecraft:item_frame"),
        Some(EntityBox {
            width: 0.75,
            height: 0.75,
            depth: Some(0.0625),
        }),
    );
    assert_eq!(index.entity_box("minecraft:armor_stand"), None);
}

fn malformed(table: &'static str, id: &str, problem: &str) -> ConstraintsError {
    ConstraintsError::MalformedRow {
        table,
        id: format!("minecraft:{id}"),
        problem: problem.to_owned(),
    }
}

fn assert_refused(cases: Vec<(&str, ConstraintsError)>) {
    for (tables, refusal) in cases {
        assert_eq!(catalog(tables).err(), Some(refusal), "{tables}");
    }
}

#[test]
fn a_row_that_names_nothing_or_no_block_is_refused() {
    assert_refused(vec![
        (
            r#""gravity": [{"ids": []}]"#,
            ConstraintsError::EmptyRow {
                table: "gravity",
                index: 0,
                missing: "block",
            },
        ),
        (
            r#""entity_aabb": [{"entities": [], "width": 1, "height": 1}]"#,
            ConstraintsError::EmptyRow {
                table: "entity_aabb",
                index: 0,
                missing: "entity",
            },
        ),
        (
            r#""gravity": [{"ids": ["sand_castle"]}]"#,
            ConstraintsError::UnknownId {
                table: "gravity",
                id: "minecraft:sand_castle".to_owned(),
            },
        ),
        (
            r#""gravity": [{"ids": ["sand"], "when": {}}]"#,
            ConstraintsError::Parse {
                message: "unknown field `when`, expected `ids` at line 1 column 117".to_owned(),
            },
        ),
    ]);
}

#[test]
fn a_rule_whose_states_or_reach_are_wrong_is_refused() {
    assert_refused(vec![
        (
            r#""support": [{"ids": ["lantern"], "when": {"hanging": "maybe"},
                "defaults": {"hanging": "maybe"}, "on": "below", "needs": "center"}]"#,
            malformed(
                "support",
                "lantern",
                "no version gives the block `hanging=maybe`",
            ),
        ),
        (
            r#""support": [{"ids": ["lantern"], "when": {"hanging": "true"},
                "on": "above", "needs": "center"}]"#,
            malformed(
                "support",
                "lantern",
                "the row reads `hanging`, which `defaults` does not give",
            ),
        ),
        (
            r#""attachment": [{"ids": ["lever"], "hangs_by": "facing",
                "defaults": {"face": "wall", "facing": "north"}}]"#,
            malformed(
                "attachment",
                "lever",
                "`defaults` gives `face`, which the row does not read",
            ),
        ),
        (
            r#""support": [{"ids": ["sand"], "on": "below", "needs": "sturdy"}],
               "gravity": [{"ids": ["sand"]}]"#,
            ConstraintsError::OverlappingRules {
                id: "minecraft:sand".to_owned(),
                first: "support",
                second: "gravity",
            },
        ),
        (
            r#""center_support": [{"ids": ["chain"], "faces": ["top"]}]"#,
            malformed("center_support", "chain", "`top` is no face"),
        ),
    ]);
}

#[test]
fn a_boundary_or_box_that_is_wrong_is_refused() {
    assert_refused(vec![
        (
            r#""semantic_sensitivity": [{"edition": "bedrock", "boundary": "1.17",
                "reason": "cauldron split", "ids": ["cauldron"]}]"#,
            ConstraintsError::UnknownBoundary {
                edition: Edition::Bedrock,
                boundary: "1.17".to_owned(),
            },
        ),
        (
            r#""semantic_sensitivity": [{"edition": "java", "boundary": "1.17",
                "reason": "  ", "ids": ["cauldron"]}]"#,
            malformed("semantic_sensitivity", "cauldron", "it gives no reason"),
        ),
        (
            r#""entity_aabb": [{"entities": ["villager"], "width": 0.6, "height": -1}]"#,
            malformed(
                "entity_aabb",
                "villager",
                "`height` is not a positive number of blocks",
            ),
        ),
        (
            r#""entity_aabb": [{"entities": ["villager"], "width": 0.6, "height": 1.95},
                {"entities": ["villager"], "width": 0.6, "height": 1.95}]"#,
            malformed(
                "entity_aabb",
                "villager",
                "another row gives the entity a box",
            ),
        ),
    ]);
}

#[test]
fn a_schema_newer_than_this_build_is_refused() {
    let text =
        r#"{"schema_version": 2, "name": "t", "description": "d", "namespace": "minecraft"}"#;
    assert_eq!(
        parse_constraints(
            text,
            builtin_pack(Edition::Java),
            builtin_pack(Edition::Bedrock)
        )
        .err(),
        Some(ConstraintsError::UnsupportedSchemaVersion {
            got: 2,
            supported: SUPPORTED_CONSTRAINTS_SCHEMA,
        }),
    );
}
