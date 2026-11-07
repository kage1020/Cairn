//! A Tier 0 part sets one block in the cell its `at=` names, turned and stated as its line says
//! (`spec/redstone` "Tier 0 parts").

use cairn_lang_core::{BlockArrayIr, DiagnosticCode};

use crate::common::{Finding, blocks_where, findings, lowered, only_structure};

/// A 7x5 struct with 3-row walls under a flat roof one voxel past them, `members` after the roof;
/// the overhang puts its back wall at `z=1` and its left at `x=1`, and moves a triple's cell one
/// voxel along `x` and `z`.
fn room(members: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n\n\
         struct s size=7x5\n  walls mat_slot=wall height=3\n  \
         roof kind=flat mat_slot=wall overhang=1\n  {members}\n"
    )
}

/// A block as `id[properties]`, with its cell.
type Placed = ((u32, u32, u32), String);

/// The redstone blocks `out` holds, each with its cell.
fn parts_of(out: &BlockArrayIr) -> Vec<Placed> {
    blocks_where(only_structure(out), |id| {
        [
            "repeater",
            "comparator",
            "redstone_torch",
            "redstone_wall_torch",
            "redstone_wire",
        ]
        .iter()
        .any(|part| id == format!("minecraft:{part}"))
    })
}

/// The redstone blocks `members` sets in [`room`], each with its cell, and the findings.
fn built(members: &str) -> (Vec<Placed>, Vec<String>) {
    let src = room(members);
    let out = lowered(&src);
    let parts = parts_of(&out);
    let found = findings(&src, &out)
        .into_iter()
        .map(|(code, at, primary, _)| format!("{}: {at}: {primary}", code.as_str()))
        .collect();
    (parts, found)
}

/// The parts `members` sets, which it must set without a finding.
fn placed(members: &str) -> Vec<Placed> {
    let (parts, found) = built(members);
    assert_eq!(found, Vec::<String>::new(), "{members}");
    parts
}

#[test]
fn a_part_at_a_triple_stands_in_that_cell_turned_and_stated_as_its_line_says() {
    for (line, block) in [
        (
            "repeater at=3,1,2 facing=east",
            "minecraft:repeater[delay=1,facing=west]",
        ),
        (
            "repeater at=3,1,2 facing=north delay=4",
            "minecraft:repeater[delay=4,facing=south]",
        ),
        (
            "comparator at=3,1,2 facing=south",
            "minecraft:comparator[facing=north,mode=compare]",
        ),
        (
            "comparator at=3,1,2 facing=west mode=subtract",
            "minecraft:comparator[facing=east,mode=subtract]",
        ),
        ("redstone_torch at=3,1,2", "minecraft:redstone_torch"),
        (
            "redstone_torch at=3,1,2 facing=east",
            "minecraft:redstone_wall_torch[facing=east]",
        ),
        ("redstone_dust at=3,1,2", "minecraft:redstone_wire"),
        (
            "redstone_dust at=3,1,2 connect=none",
            "minecraft:redstone_wire[east=none,north=none,south=none,west=none]",
        ),
        (
            "redstone_dust at=3,1,2 connect=all",
            "minecraft:redstone_wire[east=side,north=side,south=side,west=side]",
        ),
    ] {
        assert_eq!(placed(line), [((4, 1, 3), block.to_owned())], "{line}");
    }
}

#[test]
fn a_part_by_a_wall_drives_or_points_away_from_it_unless_its_line_turns_it() {
    // Along the back wall, `offset=` grows toward the west, so `left` drives east.
    for (line, cell, block) in [
        (
            "repeater at=inside.back offset=2 y=1",
            (5, 1, 2),
            "minecraft:repeater[delay=1,facing=north]",
        ),
        (
            "repeater at=inside.back offset=2 y=1 facing=left",
            (5, 1, 2),
            "minecraft:repeater[delay=1,facing=west]",
        ),
        (
            "comparator at=front.outside offset=1 y=1",
            (2, 1, 6),
            "minecraft:comparator[facing=north,mode=compare]",
        ),
        (
            "comparator at=inside.left offset=1 y=1 facing=in",
            (2, 1, 2),
            "minecraft:comparator[facing=west,mode=compare]",
        ),
        (
            "redstone_torch at=inside.back offset=2 y=2",
            (5, 2, 2),
            "minecraft:redstone_wall_torch[facing=south]",
        ),
        (
            "redstone_torch at=right.outside offset=1 y=1",
            (8, 1, 4),
            "minecraft:redstone_wall_torch[facing=east]",
        ),
        (
            "redstone_dust at=inside.front offset=1 y=1",
            (2, 1, 4),
            "minecraft:redstone_wire",
        ),
    ] {
        assert_eq!(placed(line), [(cell, block.to_owned())], "{line}");
    }
}

#[test]
fn a_part_whose_line_does_not_say_where_or_which_way_is_refused_and_not_placed() {
    for (line, primary) in [
        (
            "repeater facing=east",
            "repeater has no `at=`, so nothing says where it stands",
        ),
        (
            "redstone_dust at=center",
            "redstone_dust `at=` must be a coordinate triple or a wall anchor, not identifier \
             `center`",
        ),
        (
            "redstone_torch at=3,1,2 y=1",
            "redstone_torch `y=` beside a coordinate triple, which names the cell whole",
        ),
        (
            "repeater at=3,1,2",
            "repeater at a coordinate triple has no `facing=`, so nothing says which way it drives",
        ),
        (
            "comparator at=3,1,2 facing=in",
            "unknown comparator `facing=in` at a coordinate triple",
        ),
        (
            "repeater at=inside.back offset=2 y=1 facing=east",
            "unknown repeater `facing=east`",
        ),
        (
            "redstone_torch at=inside.back offset=2 y=1 facing=east",
            "redstone_torch `facing=east` by a wall: the torch hangs on the wall's face, so \
             `facing=` has nothing to choose",
        ),
        (
            "repeater at=3,1,2 facing=east delay=5",
            "repeater `delay=5` is no delay a repeater takes",
        ),
        (
            "comparator at=3,1,2 facing=east mode=add",
            "unknown comparator `mode=add`",
        ),
        (
            "redstone_dust at=3,1,2 connect=some",
            "unknown redstone_dust `connect=some`",
        ),
    ] {
        let (parts, found) = built(line);
        assert_eq!(parts, [], "{line}");
        assert_eq!(
            found,
            [format!("E_INVALID_MEMBER: {line}: {primary}")],
            "{line}"
        );
    }
}

#[test]
fn each_refusal_says_what_the_key_takes() {
    let notes_of = |line: &str| {
        let src = room(line);
        let out = lowered(&src);
        let found: Vec<Finding<'_>> = findings(&src, &out);
        let [(DiagnosticCode::InvalidMember, _, _, notes)] = found.as_slice() else {
            panic!("expected one refusal of `{line}`, got {found:#?}");
        };
        notes
            .iter()
            .map(|note| (*note).to_owned())
            .collect::<Vec<_>>()
    };
    let compass = "`facing=` takes one of north, south, east, west: north toward the `back` wall \
                   and east toward the `right` one";
    assert_eq!(
        notes_of("comparator at=3,1,2 facing=in"),
        [
            compass.to_owned(),
            "`in` names a way from a wall, and a part at a coordinate triple stands by none; \
             anchor it by a wall with `at=<side>.outside` or `at=inside.<side>` to turn it that \
             way"
            .to_owned(),
        ],
    );
    assert_eq!(
        notes_of("repeater at=3,1,2 facing=esat"),
        [compass.to_owned(), "did you mean `facing=east`?".to_owned()],
    );
    assert_eq!(
        notes_of("repeater at=3,1,2"),
        [
            compass.to_owned(),
            "no wall stands by a part at a triple for it to drive away from".to_owned(),
        ],
    );
    assert_eq!(
        notes_of("repeater at=3,1,2 facing=east delay=0"),
        ["`delay=` takes a whole number from 1 to 4, the ticks the repeater holds a signal"],
    );
    assert_eq!(
        notes_of("comparator at=3,1,2 facing=east mode=substract"),
        [
            "`mode=` takes compare or subtract",
            "did you mean `mode=subtract`?",
        ],
    );
    assert_eq!(
        notes_of("redstone_dust at=3,1,2 offset=1"),
        [
            "drop `offset=`, or set the part by a wall with `at=<side>.outside` or \
             `at=inside.<side>`, which reads them"
        ],
    );
    assert_eq!(
        notes_of("redstone_dust"),
        [
            "write `at=` as the coordinate triple `x,y,z` of the cell the part stands in, as in \
             `at=1,1,1`, or as a wall anchor, `at=<side>.outside` or `at=inside.<side>`"
        ],
    );
}

#[test]
fn a_value_of_the_wrong_kind_is_ignored_and_the_part_built_without_it() {
    for (line, at, primary, consequence, block) in [
        (
            "repeater at=3,1,2 facing=east delay=fast",
            "fast",
            "`delay=` must be a whole number from 1 to 4, not identifier `fast`; the value was \
             ignored",
            "the repeater is built with `delay=1`",
            ((4, 1, 3), "minecraft:repeater[delay=1,facing=west]"),
        ),
        (
            "redstone_torch at=inside.back offset=2 y=2 facing=3",
            "3",
            "`facing=` must be `north`, `south`, `east` or `west`, not integer `3`; the value was \
             ignored",
            "the redstone torch hangs on the wall it stands by, pointing away from it",
            ((5, 2, 2), "minecraft:redstone_wall_torch[facing=south]"),
        ),
        (
            "redstone_dust at=3,1,2 connect=2",
            "2",
            "`connect=` must be `none` or `all`, not integer `2`; the value was ignored",
            "the redstone dust takes its sides from the blocks beside it",
            ((4, 1, 3), "minecraft:redstone_wire"),
        ),
    ] {
        let src = room(line);
        let out = lowered(&src);
        assert_eq!(
            findings(&src, &out),
            [(
                DiagnosticCode::IgnoredArgument,
                at,
                primary,
                vec![consequence]
            )],
            "{line}",
        );
        let (cell, block) = block;
        assert_eq!(parts_of(&out), [(cell, block.to_owned())], "{line}");
    }
}

#[test]
fn a_part_stands_under_a_level_at_the_levels_height() {
    assert_eq!(
        placed("level y=1\n    redstone_dust at=3,0,2 connect=none"),
        [(
            (4, 1, 3),
            "minecraft:redstone_wire[east=none,north=none,south=none,west=none]".to_owned()
        )],
    );
}
