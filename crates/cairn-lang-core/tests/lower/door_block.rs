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
fn three_doors_in_a_row_hinge_the_end_one_away_from_its_neighbour() {
    let src = format!(
        "{THEME}struct s size=3x5\n  walls mat_slot=wall height=3\n  \
         door side=front at=left\n  door side=front at=center\n  door side=front at=right\n"
    );
    let out = lowered(&src);
    assert_eq!(out.diagnostics, []);
    let ba = only_structure(&out);
    // The middle door has a door on both sides, so neither neighbour moves its hinge.
    for (x, hinge) in [(0, "left"), (1, "left"), (2, "right")] {
        assert_eq!(door_at(ba, x, 1, 4), oak_door("north", hinge), "x={x}");
    }
}

#[test]
fn a_door_at_a_wall_end_is_hinged_on_the_jamb_with_wall_on_every_side() {
    // `size=9x7` puts the back wall at z=0, the left at x=0 and the right at x=8; each wall's
    // left end is the one on the left as seen from outside.
    for (side, left_end, right_end, facing) in [
        ("front", (0, 6), (8, 6), "north"),
        ("back", (8, 0), (0, 0), "south"),
        ("left", (0, 0), (0, 6), "east"),
        ("right", (8, 6), (8, 0), "west"),
    ] {
        for (at, (x, z), hinge) in [("left", left_end, "right"), ("right", right_end, "left")] {
            let out = lowered(&hut(&format!("door side={side} at={at}")));
            assert_eq!(out.diagnostics, [], "{side} {at}");
            assert_eq!(
                door_at(only_structure(&out), x, 1, z),
                oak_door(facing, hinge),
                "{side} {at}",
            );
        }
    }
}

#[test]
fn a_written_right_hinge_is_built_where_the_wall_gives_left() {
    let out = lowered(&hut("door side=front at=center hinge=right"));
    assert_eq!(out.diagnostics, []);
    assert_eq!(front_door(&out, 4), oak_door("north", "right"));
}

/// The `W_PHASE_CONFLICT` the line `overriding` raises over an earlier one, with the note on a
/// door it splits when there is one.
fn phase_conflict<'a>(
    overriding: &'a str,
    primary: &'a str,
    split: Option<&'a str>,
) -> Finding<'a> {
    let mut notes = vec!["overwritten member declared here"];
    notes.extend(split);
    notes.extend([
        "If the override is deliberate this is the local-override rule of spec/compilation \
         \"Phase evaluation\" and the later line wins as written. If it is not, move one of the \
         two so their footprints do not meet.",
        "Members in different phases may overlap freely — a `door` cut through a `walls` is the \
         evaluation model working — so this is only reported for two members the same phase \
         evaluates.",
    ]);
    (DiagnosticCode::PhaseConflict, overriding, primary, notes)
}

#[test]
fn two_doors_in_one_opening_contest_the_hinge_each_would_build() {
    let derived = "door side=front at=left";
    let over_derived = "`door` overwrites 2 voxels that `door` painted in the same phase, so which \
                        block the build keeps is decided by the order the two lines are written in";
    // At this corner the derived hinge is `right`, so a written `left` differs from it either way.
    for (first, second, hinge) in [
        ("door side=front at=left hinge=left", derived, "right"),
        (derived, "door side=front at=left hinge=left", "left"),
    ] {
        let src = hut(&format!("{first}\n  {second}"));
        let out = lowered(&src);
        assert_eq!(
            findings(&src, &out),
            [phase_conflict(second, over_derived, None)],
            "{first} / {second}",
        );
        assert_eq!(
            front_door(&out, 0),
            oak_door("north", hinge),
            "{first} / {second}"
        );
    }
    // A written `right` builds what the derived door would have.
    let src = hut(&format!("{derived}\n  door side=front at=left hinge=right"));
    let out = lowered(&src);
    assert_eq!(out.diagnostics, []);
    assert_eq!(front_door(&out, 0), oak_door("north", "right"));
}

#[test]
fn a_window_over_half_a_door_leaves_the_other_half_and_says_so() {
    let window = "window side=front offset=0 y=2 size=1x1 mat_slot=glass";
    let src = hut(&format!("door side=front at=left\n  {window}"));
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [phase_conflict(
            window,
            "`window` overwrites 1 voxel that `door` painted in the same phase, so which block \
             the build keeps is decided by the order the two lines are written in",
            Some(
                "the `door` is left with one of its two blocks, and the game removes a door \
                 block whose other half is missing"
            ),
        )],
    );
    let ba = only_structure(&out);
    assert_eq!(
        door_at(ba, 0, 1, 6),
        [
            oak_door("north", "right")[0].clone(),
            "minecraft:glass_pane[]".to_owned(),
        ],
    );
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
            open_yes(
                "this door is not built either way — see the finding on the same line, or on \
                 the material its `mat_slot=` names"
            ),
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

#[test]
fn a_material_outside_the_door_family_is_reported_beside_a_refused_or_deferred_course() {
    let line = "door side=front at=center mat_slot=plank";
    let material: Finding<'_> = (
        DiagnosticCode::IncompatibleMaterial,
        "@oak_planks",
        "door derives `facing` / `half` / `hinge` from where it stands and `minecraft:oak_planks` \
         is not a door, so those states have nowhere to go",
        vec![
            "bind the slot to a `*_door` material — the registry pack's `door.default` resolves \
             to one",
            "reached through `mat_slot=plank`, so every member reading that slot has it too",
        ],
    );
    let src = format!("{THEME}struct s size=9x7\n  walls mat_slot=wall height=1\n  {line}\n");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            material.clone(),
            (
                DiagnosticCode::InvalidMember,
                line,
                "door stands at y=1..=2, and the wall course holding y=1 ends there (the walls \
                 occupy y=1..=1)",
                vec![
                    "a door block is two rows tall; raise the `walls` so their course holds the \
                     row above the one the door opens at",
                ],
            ),
        ],
    );

    let src = format!(
        "{THEME}struct s size=9x7\n  {line}\n\n  level id=up\n    walls mat_slot=wall height=4\n"
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            material,
            (
                DiagnosticCode::DeferredMember,
                line,
                "door has no wall to cut into: every `walls` member on this struct sits under a \
                 `level` that is not placed",
                vec![
                    "a `level` that is not placed holds a `walls` member that may reach the rows \
                     the door stands in; the door is set once that `level` is placed",
                ],
            ),
        ],
    );
}
