//! A sensor or actuator stands one block against a wall, turned as its kind is mounted
//! (`spec/redstone` "Signal binding").

use cairn_lang_core::DiagnosticCode;

use crate::common::{Finding, blocks_where, findings, lowered, only_structure};

const THEME: &str = "theme t:\n  \
                     slot wall  -> @cobblestone\n  \
                     slot knob  -> @stone_button\n  \
                     slot plank -> @oak_planks\n\n";

/// Every component keyword, with the noun its findings call it by.
const COMPONENTS: [(&str, &str); 7] = [
    ("lever", "lever"),
    ("button", "button"),
    ("daylight", "daylight detector"),
    ("observer", "observer"),
    ("lamp", "lamp"),
    ("piston", "piston"),
    ("dispenser", "dispenser"),
];

/// A 7x5 struct with 3-row walls under a flat roof one voxel past them, `members` after the roof;
/// the overhang puts its back wall at `z=1` and its left at `x=1`.
fn storeroom(members: &str) -> String {
    format!(
        "{THEME}struct s size=7x5\n  walls mat_slot=wall height=3\n  \
         roof kind=flat mat_slot=wall overhang=1\n  {members}\n"
    )
}

/// The blocks other than the walls and roof that `members` sets in [`storeroom`], each with its
/// cell, asserting the lowering found nothing.
fn built(members: &str) -> Vec<((u32, u32, u32), String)> {
    let out = lowered(&storeroom(members));
    assert_eq!(out.diagnostics, [], "{members}");
    components(&out)
}

/// The blocks other than air, the walls and the roof in `out`'s one structure.
fn components(out: &cairn_lang_core::block_array::BlockArrayIr) -> Vec<((u32, u32, u32), String)> {
    blocks_where(only_structure(out), |id| {
        !matches!(id, "minecraft:air" | "minecraft:cobblestone")
    })
}

/// The cell `at=inside.back offset=2 y=1` names in [`storeroom`].
const BACK: (u32, u32, u32) = (5, 1, 2);

#[test]
fn a_lever_or_button_hangs_on_the_wall_facing_away_from_it() {
    for (args, cell, facing) in [
        ("at=inside.back offset=2 y=1", BACK, "south"),
        ("at=inside.front offset=1 y=1", (2, 1, 4), "north"),
        ("at=inside.left offset=1 y=1", (2, 1, 2), "east"),
        ("at=inside.right offset=1 y=1", (6, 1, 4), "west"),
        ("at=back.outside offset=1 y=1", (6, 1, 0), "north"),
        ("at=front.outside offset=1 y=1", (2, 1, 6), "south"),
        ("at=left.outside offset=1 y=1", (0, 1, 2), "west"),
        ("at=right.outside offset=1 y=1", (8, 1, 4), "east"),
    ] {
        for (keyword, id) in [("lever", "lever"), ("button", "oak_button")] {
            assert_eq!(
                built(&format!("{keyword} {args}")),
                [(cell, format!("minecraft:{id}[face=wall,facing={facing}]"))],
                "{keyword} {args}",
            );
        }
    }
}

#[test]
fn an_observer_piston_or_dispenser_faces_away_from_the_wall_unless_its_line_turns_it() {
    // Along the back wall, `offset=` grows toward the west, so `left` faces east.
    for (written, facing) in [
        ("", "south"),
        (" facing=up", "up"),
        (" facing=down", "down"),
        (" facing=in", "south"),
        (" facing=out", "north"),
        (" facing=left", "east"),
        (" facing=right", "west"),
    ] {
        for keyword in ["observer", "piston", "dispenser"] {
            assert_eq!(
                built(&format!("{keyword} at=inside.back offset=2 y=1{written}")),
                [(BACK, format!("minecraft:{keyword}[facing={facing}]"))],
                "{keyword}{written}",
            );
        }
    }
}

#[test]
fn a_daylight_detector_or_lamp_is_its_block_with_no_states() {
    for (keyword, id) in [
        ("daylight", "minecraft:daylight_detector"),
        ("lamp", "minecraft:redstone_lamp"),
    ] {
        assert_eq!(
            built(&format!("{keyword} at=inside.back offset=2 y=1")),
            [(BACK, id.to_owned())],
            "{keyword}",
        );
    }
}

#[test]
fn sticky_true_builds_a_sticky_piston() {
    for (sticky, id) in [("true", "sticky_piston"), ("false", "piston")] {
        assert_eq!(
            built(&format!(
                "piston at=inside.back offset=2 y=1 facing=up sticky={sticky}"
            )),
            [(BACK, format!("minecraft:{id}[facing=up]"))],
            "sticky={sticky}",
        );
    }
}

#[test]
fn only_facing_is_recorded_as_intent() {
    let line = "piston at=inside.back offset=2 y=1 facing=up sticky=true";
    let src = storeroom(line);
    let module = cairn_lang_core::parse(&src).expect("parse");
    let ir = cairn_lang_core::lower(&module);
    let member = &ir.structs[0].members[2];
    let intent: Vec<(&str, &str)> = member
        .intent_state
        .iter()
        .map(|(key, value)| (key.as_str(), &src[value.span.clone()]))
        .collect();
    assert_eq!(intent, [("facing", "up")]);
}

#[test]
fn an_unknown_facing_refuses_the_component() {
    let takes = "`facing=` takes one of up, down, in, out, left, right";
    for (line, primary, notes) in [
        (
            "observer at=inside.back offset=2 y=1 facing=sideways",
            "unknown observer `facing=sideways`",
            vec![takes],
        ),
        (
            "dispenser at=inside.back offset=2 y=1 facing=north",
            "unknown dispenser `facing=north`",
            vec![
                takes,
                "`facing=` names a way from the wall the dispenser stands by, and `north` from \
                 the back wall is `facing=out`",
            ],
        ),
        (
            "piston at=inside.left offset=1 y=1 facing=west powered_by=sig.push",
            "unknown piston `facing=west`",
            vec![
                takes,
                "`facing=` names a way from the wall the piston stands by, and `west` from the \
                 left wall is `facing=out`",
                "its `powered_by=` binding still reaches the netlist, with no piston placed for \
                 it to drive",
            ],
        ),
        (
            "observer at=back offset=2 y=1 facing=east -> sig.seen",
            "unknown observer `facing=east`",
            vec![
                takes,
                "`facing=` names a way from the wall the observer stands by, not a compass \
                 direction",
                "its signal binding still reaches the netlist, with no observer placed to drive it",
            ],
        ),
    ] {
        let src = storeroom(line);
        let out = lowered(&src);
        let expected: Vec<Finding<'_>> =
            vec![(DiagnosticCode::InvalidMember, line, primary, notes)];
        assert_eq!(findings(&src, &out), expected);
        assert_eq!(components(&out), [], "{line}");
    }
}

#[test]
fn a_facing_or_sticky_that_is_no_word_is_ignored() {
    let src = storeroom("piston at=inside.back offset=2 y=1 facing=1 sticky=yes");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::IgnoredArgument,
                "1",
                "`facing=` must be `up`, `down`, `in`, `out`, `left` or `right`, not integer \
                 `1`; the value was ignored",
                vec!["the piston faces away from the wall it stands by"],
            ),
            (
                DiagnosticCode::IgnoredArgument,
                "yes",
                "`sticky=` must be `true` or `false`, not identifier `yes`; the value was ignored",
                vec!["the piston is built without a sticky head, as `sticky=false` builds it"],
            ),
        ],
    );
    assert_eq!(
        components(&out),
        [(BACK, "minecraft:piston[facing=south]".to_owned())]
    );

    let line = "piston at=inside.back offset=2 y=1 facing=north sticky=1";
    let src = storeroom(line);
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::InvalidMember,
                line,
                "unknown piston `facing=north`",
                vec![
                    "`facing=` takes one of up, down, in, out, left, right",
                    "`facing=` names a way from the wall the piston stands by, and `north` from \
                     the back wall is `facing=out`",
                ],
            ),
            (
                DiagnosticCode::IgnoredArgument,
                "1",
                "`sticky=` must be `true` or `false`, not integer `1`; the value was ignored",
                vec!["this piston is not built either way — see the finding on the same line"],
            ),
        ],
    );
}

#[test]
fn a_button_bound_to_a_button_material_is_built_from_it() {
    assert_eq!(
        built("button at=inside.back offset=2 y=1 mat_slot=knob"),
        [(
            BACK,
            "minecraft:stone_button[face=wall,facing=south]".to_owned()
        )],
    );
}

#[test]
fn a_button_bound_outside_the_button_family_is_refused_and_built_from_the_default_button() {
    let src = storeroom("button at=inside.back offset=2 y=1 mat_slot=plank");
    let out = lowered(&src);
    assert_eq!(
        out.diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
        [DiagnosticCode::IncompatibleMaterial],
    );
    let refusal = &out.diagnostics[0];
    assert_eq!(&src[refusal.span.clone()], "@oak_planks");
    assert_eq!(
        refusal.primary,
        "button derives `face` / `facing` from the wall it hangs on and `minecraft:oak_planks` \
         is not a button, so those states have nowhere to go",
    );
    assert_eq!(
        serde_json::to_value(&refusal.data).expect("serialises")["required"],
        "button",
    );
    assert_eq!(
        components(&out),
        [(
            BACK,
            "minecraft:oak_button[face=wall,facing=south]".to_owned()
        )]
    );
}

#[test]
fn a_patch_line_binds_without_building_a_second_block() {
    for (keyword, key) in [
        ("lamp", "lit_by"),
        ("piston", "powered_by"),
        ("dispenser", "fired_by"),
    ] {
        let members =
            format!("{keyword} id=it at=inside.back offset=2 y=1\n  {keyword}[id=it] {key}=sig.go");
        let out = lowered(&storeroom(&members));
        assert_eq!(out.diagnostics, [], "{keyword}");
        assert_eq!(components(&out).len(), 1, "{keyword}");
    }
}

#[test]
fn a_component_is_refused_or_deferred_wherever_a_plate_is() {
    // Past the wall, at its corner, above the walls, and outside a struct with no overhang.
    let unroofed = |line: &str| {
        format!("{THEME}struct s size=7x5\n  walls mat_slot=wall height=3\n  {line}\n")
    };
    for args in [
        "at=inside.back offset=9 y=1",
        "at=inside.back offset=0 y=1",
        "at=inside.back offset=2 y=7",
        "at=back.outside offset=2 y=1",
        "at=back offset=2 y=1",
    ] {
        let as_found = |keyword: &str| {
            let src = unroofed(&format!("{keyword} {args}"));
            lowered(&src)
                .diagnostics
                .iter()
                .map(|d| {
                    let notes: Vec<String> = d.notes.iter().map(|n| n.message.clone()).collect();
                    (d.code, d.primary.clone(), notes)
                })
                .collect::<Vec<_>>()
        };
        let plate = as_found("pressure_plate");
        assert_ne!(plate, [], "{args}");
        for (keyword, noun) in COMPONENTS {
            let renamed = |text: &str| {
                text.replace("pressure_plate", keyword)
                    .replace("plate", noun)
            };
            let expected: Vec<_> = plate
                .iter()
                .map(|(code, primary, notes)| {
                    (
                        *code,
                        renamed(primary),
                        notes.iter().map(|n| renamed(n)).collect::<Vec<_>>(),
                    )
                })
                .collect();
            assert_eq!(as_found(keyword), expected, "{keyword} {args}");
        }
    }
}
