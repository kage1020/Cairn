//! Lowering records each member's `resolved_state` beside the grid it builds
//! (`spec/blockstate` "`intent_state` and `resolved_state`"): every state the member's cells hold
//! once the body is painted, the members in the order of the first cell each owns.

use cairn_lang_core::block_array::{BlockArrayIr, CellRun};
use serde_json::json;

use crate::common::lowered;

const PRELUDE: &str = "theme t:\n  \
                       slot wall -> @cobblestone\n  \
                       slot eave -> @oak_stairs\n\n";

type Cells = Vec<(u32, u32, u32)>;

/// A recorded member as `(role, id, [(state, cells)])`, the state as the palette prints it.
type Built = (String, Option<String>, Vec<(String, Cells)>);

/// Each member recorded under `key`.
fn built(ir: &BlockArrayIr, key: &str) -> Vec<Built> {
    ir.members[key]
        .iter()
        .map(|member| {
            let blocks = member
                .resolved_state
                .iter()
                .map(|block| {
                    let state = if block.state.properties.is_empty() {
                        block.state.id.clone()
                    } else {
                        format!("{}[{}]", block.state.id, block.state.properties_text())
                    };
                    (state, block.cells().collect())
                })
                .collect();
            (member.role.clone(), member.id.clone(), blocks)
        })
        .collect()
}

fn member(role: &str, id: Option<&str>, blocks: &[(&str, Cells)]) -> Built {
    (
        role.to_owned(),
        id.map(str::to_owned),
        blocks
            .iter()
            .map(|(state, cells)| ((*state).to_owned(), cells.clone()))
            .collect(),
    )
}

#[test]
fn an_eave_stair_records_the_facing_its_out_resolved_to() {
    let ir = lowered(&format!(
        "{PRELUDE}struct s size=3x1\n  \
         walls mat_slot=wall height=1\n  \
         roof kind=flat mat_slot=wall overhang=1\n  \
         stair id=eave kind=stairs mat_slot=eave side=front half=top facing=out shape=outer_left\n"
    ));

    // The eave hangs a row below the walls, so its first cell comes first.
    assert_eq!(
        built(&ir, "struct::s"),
        vec![
            member(
                "stair",
                Some("eave"),
                &[(
                    "minecraft:oak_stairs[facing=south,half=top,shape=outer_left]",
                    vec![(1, 0, 2), (2, 0, 2), (3, 0, 2)],
                )],
            ),
            member(
                "walls",
                None,
                &[(
                    "minecraft:cobblestone",
                    vec![(1, 1, 1), (2, 1, 1), (3, 1, 1)],
                )],
            ),
            member(
                "roof",
                None,
                &[(
                    "minecraft:cobblestone",
                    (0..3)
                        .flat_map(|z| (0..5).map(move |x| (x, 2, z)))
                        .collect(),
                )],
            ),
        ],
    );
}

#[test]
fn reordering_lines_that_share_no_cell_leaves_the_record_as_it_was() {
    let body = |first: &str, second: &str| {
        lowered(&format!(
            "{PRELUDE}struct s size=3x3\n  walls mat_slot=wall height=1\n  {first}\n  {second}\n"
        ))
    };
    let front = "window id=f side=front y=1 offset=1 size=1x1 mat_slot=eave";
    let back = "window id=b side=back y=1 offset=1 size=1x1 mat_slot=eave";
    let ir = body(front, back);
    assert_eq!(
        serde_json::to_value(&ir).expect("the IR serializes"),
        serde_json::to_value(body(back, front)).expect("the IR serializes"),
    );
    let ids: Vec<Option<&str>> = ir.members["struct::s"]
        .iter()
        .map(|member| member.id.as_deref())
        .collect();
    assert_eq!(ids, [None, Some("b"), Some("f")]);
}

#[test]
fn a_member_whose_every_cell_a_later_member_repaints_records_nothing() {
    // The second course paints the first's cells with the same block, and still takes them.
    let ir = lowered(&format!(
        "{PRELUDE}struct s size=1x1\n  \
         walls id=first mat_slot=wall height=1\n  \
         walls id=second mat_slot=wall height=1\n"
    ));
    assert_eq!(
        built(&ir, "struct::s"),
        vec![member(
            "walls",
            Some("second"),
            &[("minecraft:cobblestone", vec![(0, 1, 0)])],
        )],
    );
}

#[test]
fn a_member_that_holds_several_states_lists_them_by_the_first_cell_of_each() {
    let ir = lowered(&format!(
        "{PRELUDE}struct s size=1x1\n  \
         walls mat_slot=wall height=1\n  \
         roof kind=gable mat_slot=eave overhang=1\n"
    ));
    let roof: Vec<(String, Cells)> = built(&ir, "struct::s")
        .into_iter()
        .find(|(role, _, _)| role == "roof")
        .expect("the roof builds")
        .2;
    let row = |y: u32, z: u32| (0..3).map(|x| (x, y, z)).collect::<Cells>();
    // The slopes face the ridge, and the ridge, a row up, caps them.
    assert_eq!(
        roof,
        [
            (
                "minecraft:oak_stairs[facing=south,half=bottom,shape=straight]".to_owned(),
                row(2, 0),
            ),
            (
                "minecraft:oak_stairs[facing=north,half=bottom,shape=straight]".to_owned(),
                row(2, 2),
            ),
            (
                "minecraft:oak_stairs[facing=south,half=top,shape=straight]".to_owned(),
                row(3, 1),
            ),
        ],
    );
}

#[test]
fn the_same_def_placed_under_two_themes_records_each_places_own_blocks() {
    let ir = lowered(&format!(
        "{PRELUDE}theme u:\n  slot wall -> @oak_planks\n\n\
         def hut size=1x1:\n  \
         walls mat_slot=wall height=1\n\n\
         site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=u east_of=a gap=2\n"
    ));
    assert_eq!(
        built(&ir, "site::s::a"),
        vec![member(
            "walls",
            None,
            &[("minecraft:cobblestone", vec![(0, 1, 0)])]
        )],
    );
    assert_eq!(
        built(&ir, "site::s::b"),
        vec![member(
            "walls",
            None,
            &[("minecraft:oak_planks", vec![(0, 1, 0)])]
        )],
    );
}

#[test]
fn a_place_id_written_twice_records_the_members_of_the_body_kept() {
    let ir = lowered(&format!(
        "{PRELUDE}def hut size=1x1:\n  \
         walls mat_slot=wall height=1\n\n\
         def shed size=1x1:\n  \
         floor mat_slot=eave\n\n\
         site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         place id=a use=shed theme=t at=origin\n"
    ));
    let keys: Vec<&str> = ir.structures.keys().map(String::as_str).collect();
    assert_eq!(keys, ["site::s::a"]);
    assert_eq!(
        ir.structures["site::s::a"].palette.entries[1].id, "minecraft:cobblestone",
        "the first row's body is the one kept",
    );
    assert_eq!(
        built(&ir, "site::s::a"),
        vec![member(
            "walls",
            None,
            &[("minecraft:cobblestone", vec![(0, 1, 0)])]
        )],
    );
}

#[test]
fn a_cell_is_recorded_on_the_member_that_painted_it_last() {
    let ir = lowered(&format!(
        "{PRELUDE}struct s size=3x1\n  \
         walls id=shell mat_slot=wall height=2\n  \
         door id=entry side=front at=center\n"
    ));

    assert_eq!(
        built(&ir, "struct::s"),
        vec![
            member(
                "walls",
                Some("shell"),
                &[(
                    "minecraft:cobblestone",
                    vec![(0, 1, 0), (2, 1, 0), (0, 2, 0), (2, 2, 0)],
                )],
            ),
            member(
                "door",
                Some("entry"),
                &[
                    (
                        "minecraft:oak_door[facing=north,half=lower,hinge=left,open=false]",
                        vec![(1, 1, 0)],
                    ),
                    (
                        "minecraft:oak_door[facing=north,half=upper,hinge=left,open=false]",
                        vec![(1, 2, 0)],
                    ),
                ],
            ),
        ],
    );
}

#[test]
fn a_member_that_builds_no_cell_records_nothing() {
    let ir = lowered(&format!(
        "{PRELUDE}struct s size=3x1\n  \
         floor mat_slot=wall\n  \
         door id=entry side=front at=center\n"
    ));

    let roles: Vec<&str> = ir.members["struct::s"]
        .iter()
        .map(|member| member.role.as_str())
        .collect();
    assert_eq!(roles, ["floor"]);
}

#[test]
fn each_place_records_its_own_members_under_its_structure_key() {
    let ir = lowered(&format!(
        "{PRELUDE}def hut size=1x1:\n  \
         walls mat_slot=wall height=1\n\n\
         site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=2\n"
    ));

    let keys: Vec<&str> = ir.members.keys().map(String::as_str).collect();
    assert_eq!(keys, ["site::s::a", "site::s::b"]);
    let expected = vec![member(
        "walls",
        None,
        &[("minecraft:cobblestone", vec![(0, 1, 0)])],
    )];
    assert_eq!(built(&ir, "site::s::a"), expected);
    assert_eq!(built(&ir, "site::s::b"), expected);
}

#[test]
fn the_record_serializes_under_members_keyed_like_structures() {
    let ir = lowered(&format!(
        "{PRELUDE}struct s size=1x1\n  \
         walls id=shell mat_slot=wall height=1\n"
    ));

    let json = serde_json::to_value(&ir).expect("the IR serializes");
    assert_eq!(
        json["members"],
        json!({
            "struct::s": [{
                "role": "walls",
                "id": "shell",
                "resolved_state": [{
                    "state": { "id": "minecraft:cobblestone" },
                    "runs": [{ "from": [0, 1, 0], "len": 1 }],
                }],
                "geometry": {
                    "anchor": [0, 1, 0],
                    "declared_bbox": { "min": [0, 1, 0], "max": [0, 1, 0] },
                    "actual_bbox": { "min": [0, 1, 0], "max": [0, 1, 0] },
                },
            }],
        }),
    );
}

#[test]
fn cells_in_a_row_along_x_record_as_one_run() {
    let ir = lowered(&format!(
        "{PRELUDE}struct s size=3x1\n  \
         walls mat_slot=wall height=2\n"
    ));

    let runs: Vec<Vec<CellRun>> = ir.members["struct::s"]
        .iter()
        .flat_map(|member| member.resolved_state.iter().map(|block| block.runs.clone()))
        .collect();
    assert_eq!(
        runs,
        [vec![
            CellRun {
                from: (0, 1, 0),
                len: 3
            },
            CellRun {
                from: (0, 2, 0),
                len: 3
            },
        ]],
    );
}
