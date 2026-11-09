//! A `chest` stands one chest block against a wall, facing away from it unless its line turns it
//! (`spec/blockstate` "Derive by default, promote on override").

use cairn_lang_core::DiagnosticCode;

use crate::common::{Finding, chests, findings, lowered, only_structure};

const THEME: &str = "theme t:\n  \
                     slot wall  -> @cobblestone\n  \
                     slot trap  -> @trapped_chest\n  \
                     slot plank -> @oak_planks\n\n";

/// A 7x5 struct with 3-row walls under a flat roof one voxel past them, `members` after the roof;
/// the overhang puts its back wall at `z=1` and its left at `x=1`.
fn storeroom(members: &str) -> String {
    format!(
        "{THEME}struct s size=7x5\n  walls mat_slot=wall height=3\n  \
         roof kind=flat mat_slot=wall overhang=1\n  {members}\n"
    )
}

/// The chests `members` sets in [`storeroom`], each with its cell.
fn stored(members: &str) -> Vec<((u32, u32, u32), String)> {
    let out = lowered(&storeroom(members));
    assert_eq!(out.diagnostics, [], "{members}");
    chests(only_structure(&out))
}

/// A chest of the default block facing `facing`, with the `extra` states after it.
fn chest(facing: &str, extra: &str) -> String {
    format!("minecraft:chest[facing={facing}{extra}]")
}

#[test]
fn a_chest_faces_away_from_the_wall_it_stands_by() {
    for (line, cell, facing) in [
        ("chest at=inside.back offset=2 y=1", (5, 1, 2), "south"),
        ("chest at=inside.front offset=1 y=1", (2, 1, 4), "north"),
        ("chest at=inside.left offset=1 y=1", (2, 1, 2), "east"),
        ("chest at=inside.right offset=1 y=1", (6, 1, 4), "west"),
        ("chest at=back.outside offset=1 y=1", (6, 1, 0), "north"),
        ("chest at=front.outside offset=1 y=1", (2, 1, 6), "south"),
        ("chest at=left.outside offset=1 y=1", (0, 1, 2), "west"),
        ("chest at=right.outside offset=1 y=1", (8, 1, 4), "east"),
    ] {
        assert_eq!(stored(line), [(cell, chest(facing, ""))], "{line}");
    }
}

#[test]
fn a_written_facing_turns_the_chest_toward_its_own_side_of_the_wall() {
    // Along the back wall, `offset=` grows toward the west, so `left` faces east.
    for (facing, built) in [
        ("in", "south"),
        ("out", "north"),
        ("left", "east"),
        ("right", "west"),
    ] {
        assert_eq!(
            stored(&format!(
                "chest at=inside.back offset=2 y=1 facing={facing}"
            )),
            [((5, 1, 2), chest(built, ""))],
            "facing={facing}",
        );
    }
}

#[test]
fn size_single_writes_the_chests_type_and_both_are_recorded_as_intent() {
    let line = "chest at=inside.back offset=2 y=1 facing=left size=single";
    assert_eq!(stored(line), [((5, 1, 2), chest("east", ",type=single"))]);
    let src = storeroom(line);
    let module = cairn_lang_core::parse(&src).expect("parse");
    let ir = cairn_lang_core::lower(&module);
    let member = &ir.structs[0].members[2];
    let intent: Vec<(&str, &str)> = member
        .intent_state
        .iter()
        .map(|(key, value)| (key.as_str(), &src[value.span.clone()]))
        .collect();
    assert_eq!(intent, [("facing", "left"), ("size", "single")]);
}

#[test]
fn an_unknown_facing_size_or_waterlogged_refuses_the_chest_and_each_is_reported() {
    for (line, primary, notes) in [
        (
            "chest at=inside.back offset=2 y=1 facing=up",
            "unknown chest `facing=up`",
            vec!["`facing=` takes one of in, out, left, right"],
        ),
        (
            "chest at=inside.back offset=2 y=1 facing=north",
            "unknown chest `facing=north`",
            vec![
                "`facing=` takes one of in, out, left, right",
                "`facing=` names a way from the wall the chest stands by, and `north` from the \
                 back wall is `facing=out`",
            ],
        ),
        (
            "chest at=inside.back offset=2 y=1 size=double",
            "unknown chest `size=double`",
            vec![
                "`size=` takes `single`, which keeps the chest from pairing; leave it out to let \
                 the chest pair with the one beside it",
            ],
        ),
    ] {
        let src = storeroom(line);
        let out = lowered(&src);
        let expected: Vec<Finding<'_>> =
            vec![(DiagnosticCode::InvalidMember, line, primary, notes)];
        assert_eq!(findings(&src, &out), expected);
        assert_eq!(chests(only_structure(&out)), [], "{line}");
    }

    let line = "chest at=inside.back offset=2 y=1 facing=up size=double waterlogged=yes";
    let src = storeroom(line);
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::InvalidMember,
                line,
                "unknown chest `facing=up`",
                vec!["`facing=` takes one of in, out, left, right"],
            ),
            (
                DiagnosticCode::InvalidMember,
                line,
                "unknown chest `size=double`",
                vec![
                    "`size=` takes `single`, which keeps the chest from pairing; leave it out to \
                     let the chest pair with the one beside it",
                ],
            ),
            (
                DiagnosticCode::InvalidMember,
                line,
                "unknown chest `waterlogged=yes`",
                vec![
                    "`waterlogged=` takes `auto`, which lets a water `fill` set it, or `true` or \
                     `false`, which keep it",
                ],
            ),
        ],
    );
    assert_eq!(chests(only_structure(&out)), []);
}

#[test]
fn a_facing_or_size_that_is_no_word_is_ignored() {
    let src = storeroom("chest at=inside.back offset=2 y=1 facing=1 size=2");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::IgnoredArgument,
                "1",
                "`facing=` must be `in`, `out`, `left` or `right`, not integer `1`; the value was \
                 ignored",
                vec!["the chest faces away from the wall it stands by"],
            ),
            (
                DiagnosticCode::IgnoredArgument,
                "2",
                "`size=` must be `single`, not integer `2`; the value was ignored",
                vec!["the chest pairs with the chest beside it, as it does without `size=`"],
            ),
        ],
    );
    assert_eq!(
        chests(only_structure(&out)),
        [((5, 1, 2), chest("south", ""))]
    );

    let line = "chest at=inside.back offset=2 y=1 facing=up size=2";
    let src = storeroom(line);
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::InvalidMember,
                line,
                "unknown chest `facing=up`",
                vec!["`facing=` takes one of in, out, left, right"],
            ),
            (
                DiagnosticCode::IgnoredArgument,
                "2",
                "`size=` must be `single`, not integer `2`; the value was ignored",
                vec!["this chest is not built either way — see the finding on the same line"],
            ),
        ],
    );
}

#[test]
fn a_chest_bound_to_a_chest_material_is_built_from_it() {
    assert_eq!(
        stored("chest at=inside.back offset=2 y=1 mat_slot=trap"),
        [(
            (5, 1, 2),
            "minecraft:trapped_chest[facing=south]".to_owned()
        )],
    );
}

#[test]
fn a_chest_bound_outside_the_chest_family_is_refused_and_built_from_the_default_chest() {
    let src = storeroom("chest at=inside.back offset=2 y=1 mat_slot=plank");
    let out = lowered(&src);
    assert_eq!(
        out.diagnostics.iter().map(|d| d.code).collect::<Vec<_>>(),
        [DiagnosticCode::IncompatibleMaterial],
    );
    let refusal = &out.diagnostics[0];
    assert_eq!(&src[refusal.span.clone()], "@oak_planks");
    assert_eq!(
        refusal.primary,
        "chest derives `facing` / `type` from the wall it stands by and the chest beside it and \
         `minecraft:oak_planks` is not a chest, so those states have nowhere to go",
    );
    assert_eq!(
        serde_json::to_value(&refusal.data).expect("serialises")["required"],
        "chest",
    );
    assert_eq!(
        chests(only_structure(&out)),
        [((5, 1, 2), chest("south", ""))]
    );
}

#[test]
fn a_chest_is_refused_or_deferred_wherever_a_plate_is() {
    // Past the wall, at its corner, above the walls, outside a struct with no overhang, and with
    // no anchor.
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
        let as_chest = |text: &str| {
            text.replace("pressure_plate", "chest")
                .replace("plate", "chest")
        };
        let plate: Vec<_> = as_found("pressure_plate")
            .into_iter()
            .map(|(code, primary, notes)| {
                (
                    code,
                    as_chest(&primary),
                    notes.iter().map(|n| as_chest(n)).collect(),
                )
            })
            .collect();
        assert_ne!(plate, [], "{args}");
        assert_eq!(as_found("chest"), plate, "{args}");
    }
}
