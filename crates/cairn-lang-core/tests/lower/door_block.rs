//! A `door` sets a door block into the opening it cuts, its states derived from where it stands
//! unless its line writes them (`spec/blockstate` "Derive by default, promote on override").

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::block_array::BlockArrayIr;
use serde_json::json;

use crate::common::{Finding, door_at, findings, lowered, oak_door, only_structure, state_at};

const THEME: &str = "theme t:\n  \
                     slot wall  -> @cobblestone\n  \
                     slot glass -> @glass_pane\n  \
                     slot iron  -> @iron_door\n  \
                     slot plank -> @oak_planks\n\n";

/// A 9x7 struct with 4-row walls and `members` after them; its front wall is at `z=6`.
fn hut(members: &str) -> String {
    format!("{THEME}struct s size=9x7\n  walls mat_slot=wall height=4\n  {members}\n")
}

/// The door standing in the front wall of [`hut`] at `x`.
fn front_door(out: &BlockArrayIr, x: u32) -> [String; 2] {
    door_at(only_structure(out), x, 1, 6)
}

#[test]
fn a_door_is_hinged_on_the_side_with_more_wall_whichever_line_comes_first() {
    // The window takes the upper row left of the centre door, so the wall on its right is taller.
    let door = "door side=front at=center";
    let window = "window side=front offset=3 y=2 size=1x1 mat_slot=glass";
    for members in [format!("{door}\n  {window}"), format!("{window}\n  {door}")] {
        let out = lowered(&hut(&members));
        assert_eq!(out.diagnostics, [], "{members}");
        assert_eq!(front_door(&out, 4), oak_door("north", "right"), "{members}");
    }
    let mirrored = lowered(&hut(&format!(
        "{door}\n  window side=front offset=5 y=2 size=1x1 mat_slot=glass"
    )));
    assert_eq!(front_door(&mirrored, 4), oak_door("north", "left"));
}

#[test]
fn two_doors_side_by_side_are_hinged_on_their_outer_jambs() {
    let src = format!(
        "{THEME}struct s size=2x5\n  walls mat_slot=wall height=3\n  \
         door side=front at=left\n  door side=front at=right\n"
    );
    let out = lowered(&src);
    assert_eq!(out.diagnostics, []);
    let ba = only_structure(&out);
    assert_eq!(door_at(ba, 0, 1, 4), oak_door("north", "left"));
    assert_eq!(door_at(ba, 1, 1, 4), oak_door("north", "right"));
}

#[test]
fn a_door_faces_into_the_struct_from_every_side() {
    // `size=9x7` puts the back wall at z=0, the left at x=0 and the right at x=8.
    for (side, (x, z), facing) in [
        ("front", (4, 6), "north"),
        ("back", (4, 0), "south"),
        ("left", (0, 3), "east"),
        ("right", (8, 3), "west"),
    ] {
        let out = lowered(&hut(&format!("door side={side} at=center")));
        assert_eq!(out.diagnostics, [], "{side}");
        assert_eq!(
            door_at(only_structure(&out), x, 1, z),
            oak_door(facing, "left"),
            "{side}",
        );
    }
}

#[test]
fn a_written_hinge_and_open_are_built_as_written_and_recorded_as_intent() {
    // At the left corner the derived hinge is `right`, so the written `left` is what is built.
    let src = hut("door side=front at=left hinge=left open=true");
    let out = lowered(&src);
    assert_eq!(out.diagnostics, []);
    assert_eq!(
        front_door(&out, 0),
        ["lower", "upper"].map(|half| format!(
            "minecraft:oak_door[facing=north,half={half},hinge=left,open=true]"
        )),
    );
    let module = cairn_lang_core::parse(&src).expect("parse");
    let ir = cairn_lang_core::lower(&module);
    let door = &ir.structs[0].members[1];
    let intent: Vec<(&str, &str)> = door
        .intent_state
        .iter()
        .map(|(key, value)| (key.as_str(), &src[value.span.clone()]))
        .collect();
    assert_eq!(intent, [("hinge", "left"), ("open", "true")]);
}

#[test]
fn the_door_records_both_halves_as_its_resolved_state() {
    let out = lowered(&hut("door id=entry side=front at=left"));
    let door = out.members["struct::s"]
        .iter()
        .find(|member| member.role == "door")
        .expect("the door is recorded");
    assert_eq!(
        serde_json::to_value(&door.resolved_state).expect("serialises"),
        json!([
            {
                "state": {
                    "id": "minecraft:oak_door",
                    "properties": {
                        "facing": "north", "half": "lower", "hinge": "right", "open": "false"
                    },
                },
                "runs": [{ "from": [0, 1, 6], "len": 1 }],
            },
            {
                "state": {
                    "id": "minecraft:oak_door",
                    "properties": {
                        "facing": "north", "half": "upper", "hinge": "right", "open": "false"
                    },
                },
                "runs": [{ "from": [0, 2, 6], "len": 1 }],
            },
        ]),
    );
}

#[test]
fn an_unknown_hinge_refuses_the_door_and_leaves_the_wall_whole() {
    let line = "door side=front at=center hinge=up";
    let src = hut(line);
    let out = lowered(&src);
    let expected: Vec<Finding<'_>> = vec![(
        DiagnosticCode::InvalidMember,
        line,
        "unknown door `hinge=up`",
        vec!["`hinge=` takes one of left, right"],
    )];
    assert_eq!(findings(&src, &out), expected);
    let ba = only_structure(&out);
    for y in 1..=2 {
        assert_eq!(state_at(ba, 4, y, 6).id, "minecraft:cobblestone", "y={y}");
    }
}

/// The `W_IGNORED_ARGUMENT` an `open=yes` raises, its note saying what is built instead.
fn open_yes(note: &'static str) -> Finding<'static> {
    (
        DiagnosticCode::IgnoredArgument,
        "yes",
        "`open=` must be `true` or `false`, not identifier `yes`; the value was ignored",
        vec![note],
    )
}

#[test]
fn an_open_that_is_no_boolean_is_ignored_and_the_door_built_closed() {
    let src = hut("door side=front at=center open=yes");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [open_yes(
            "the door is built closed, as `open=false` builds it"
        )],
    );
    assert_eq!(front_door(&out, 4), oak_door("north", "left"));

    let line = "door side=front at=center hinge=up open=yes";
    let src = hut(line);
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::InvalidMember,
                line,
                "unknown door `hinge=up`",
                vec!["`hinge=` takes one of left, right"],
            ),
            open_yes("this door is not built either way — see the finding on the same line"),
        ],
    );
}

#[test]
fn a_door_bound_to_a_door_material_is_built_from_it() {
    let out = lowered(&hut("door side=front at=center mat_slot=iron"));
    assert_eq!(out.diagnostics, []);
    assert_eq!(
        front_door(&out, 4),
        ["lower", "upper"].map(|half| format!(
            "minecraft:iron_door[facing=north,half={half},hinge=left,open=false]"
        )),
    );
}

#[test]
fn a_door_bound_outside_the_door_family_is_refused_and_built_from_the_default_door() {
    let src = hut("door side=front at=center mat_slot=plank");
    let out = lowered(&src);
    let refusal = out
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::IncompatibleMaterial)
        .expect("the material is refused");
    assert_eq!(
        out.diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
        [DiagnosticCode::IncompatibleMaterial],
    );
    assert_eq!(
        refusal.primary,
        "door derives `facing` / `half` / `hinge` from where it stands and `minecraft:oak_planks` \
         is not a door, so those states have nowhere to go",
    );
    assert_eq!(
        serde_json::to_value(&refusal.data).expect("serialises")["required"],
        "door",
    );
    assert_eq!(front_door(&out, 4), oak_door("north", "left"));
}
