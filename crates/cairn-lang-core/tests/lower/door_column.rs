//! A `door` names its column with an `at=` anchor or with `offset=N`, a column between its wall's
//! corners, and a walkway port reads the column the door stands in (`spec/syntax` "Selectors").

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::block_array::{BlockArrayIr, Footprint};

use crate::common::{
    door_at, findings, lowered, lowered_with_resolver_diagnostics, oak_door, only_structure,
};

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n  slot gravel -> @gravel\n\n";

/// The columns `offset=` takes on a 9-long wall.
const NINE: &str = "`offset=` takes a column between the wall's corners, from 1 to 7";

/// The note on a door whose `at=` names no anchor, on a 9-long wall.
fn no_anchor_note() -> String {
    format!(
        "name the door's column with `at=center`, `at=left` or `at=right`, or with `offset=N` \
         counted from 0 at the wall's left end seen from outside; {NINE}"
    )
}

/// A struct of `size` with 4-row walls and `member` after them.
fn hut(size: &str, member: &str) -> String {
    format!("{THEME}struct s size={size}\n  walls mat_slot=wall height=4\n  {member}\n")
}

/// The one finding lowering `member` in [`hut`] of `size` draws, after its code and line.
fn refusal(size: &str, member: &str) -> (String, Vec<String>) {
    let src = hut(size, member);
    let out = lowered(&src);
    let found = findings(&src, &out);
    assert_eq!(found.len(), 1, "{member}: {found:#?}");
    let (code, line, primary, notes) = &found[0];
    assert_eq!((*code, *line), (DiagnosticCode::InvalidMember, member));
    (
        (*primary).to_owned(),
        notes.iter().map(|note| (*note).to_owned()).collect(),
    )
}

#[test]
fn an_offset_counts_from_the_left_end_of_the_wall_seen_from_outside() {
    // 9x7: the front runs x up at z=6, the back x down at z=0, the left z up at x=0 and the right
    // z down at x=8.
    for (member, (x, z), facing) in [
        ("door side=front offset=2", (2, 6), "north"),
        ("door side=back offset=2", (6, 0), "south"),
        ("door side=left offset=2", (0, 2), "east"),
        ("door side=right offset=2", (8, 4), "west"),
    ] {
        let out = lowered(&hut("9x7", member));
        assert_eq!(out.diagnostics, [], "{member}");
        assert_eq!(
            door_at(only_structure(&out), x, 1, z),
            oak_door(facing, "left"),
            "{member}",
        );
    }
}

#[test]
fn an_offset_and_the_anchor_naming_its_column_build_the_same_door() {
    let by_offset = lowered(&hut("9x7", "door side=front offset=4"));
    let by_anchor = lowered(&hut("9x7", "door side=front at=center"));
    assert_eq!(by_offset.diagnostics, []);
    let (offset, anchor) = (only_structure(&by_offset), only_structure(&by_anchor));
    assert_eq!(
        (&offset.palette, &offset.voxels),
        (&anchor.palette, &anchor.voxels),
    );
}

#[test]
fn adjacent_offset_doors_hinge_on_their_outer_jambs_and_a_written_hinge_holds() {
    for (members, left, right) in [
        (
            "door side=front offset=3\n  door side=front offset=4",
            oak_door("north", "left"),
            oak_door("north", "right"),
        ),
        (
            "door side=front offset=3 hinge=right",
            oak_door("north", "right"),
            [
                "minecraft:cobblestone[]".to_owned(),
                "minecraft:cobblestone[]".to_owned(),
            ],
        ),
    ] {
        let out = lowered(&hut("9x7", members));
        assert_eq!(out.diagnostics, [], "{members}");
        let array = only_structure(&out);
        assert_eq!(
            (door_at(array, 3, 1, 6), door_at(array, 4, 1, 6)),
            (left, right),
            "{members}"
        );
    }
}

#[test]
fn an_offset_takes_a_column_between_the_corners_and_no_other() {
    for offset in 1..=3 {
        let out = lowered(&hut("5x5", &format!("door side=front offset={offset}")));
        assert_eq!(out.diagnostics, [], "offset={offset}");
        assert_eq!(
            door_at(only_structure(&out), offset, 1, 4),
            oak_door("north", "left"),
            "offset={offset}",
        );
    }
    let between = "`offset=` takes a column between the wall's corners, from 1 to 3";
    for (offset, primary, note) in [
        (
            0_u64,
            "door has `offset=0`, the front wall's corner with the left wall",
            format!("{between}; `at=left` stands the door in this corner"),
        ),
        (
            4,
            "door has `offset=4`, the front wall's corner with the right wall",
            format!("{between}; `at=right` stands the door in this corner"),
        ),
        (
            5,
            "door has `offset=5`, past the end of the front wall (length 5)",
            between.to_owned(),
        ),
        (
            4_294_967_295,
            "door has `offset=4294967295`, past the end of the front wall (length 5)",
            between.to_owned(),
        ),
    ] {
        let member = format!("door side=front offset={offset}");
        assert_eq!(
            refusal("5x5", &member),
            (primary.to_owned(), vec![note]),
            "{member}",
        );
    }
}

#[test]
fn a_refused_offset_cuts_nothing() {
    let refused = lowered(&hut("5x5", "door side=front offset=0"));
    let bare = lowered(&hut("5x5", ""));
    let codes: Vec<DiagnosticCode> = refused.diagnostics.iter().map(|d| d.code).collect();
    assert_eq!(codes, [DiagnosticCode::InvalidMember]);
    let (refused, bare) = (only_structure(&refused), only_structure(&bare));
    assert_eq!(
        (&refused.palette, &refused.voxels),
        (&bare.palette, &bare.voxels),
    );
}

#[test]
fn a_corner_offset_names_the_wall_it_meets_at_either_end_of_each_wall() {
    // 9x7: the front and back are 9 long, the left and right 7.
    let seven = "`offset=` takes a column between the wall's corners, from 1 to 5";
    for (member, primary, between, anchor) in [
        (
            "door side=front offset=0",
            "the front wall's corner with the left wall",
            NINE,
            "left",
        ),
        (
            "door side=front offset=8",
            "the front wall's corner with the right wall",
            NINE,
            "right",
        ),
        (
            "door side=back offset=0",
            "the back wall's corner with the right wall",
            NINE,
            "left",
        ),
        (
            "door side=back offset=8",
            "the back wall's corner with the left wall",
            NINE,
            "right",
        ),
        (
            "door side=left offset=0",
            "the left wall's corner with the back wall",
            seven,
            "left",
        ),
        (
            "door side=left offset=6",
            "the left wall's corner with the front wall",
            seven,
            "right",
        ),
        (
            "door side=right offset=0",
            "the right wall's corner with the front wall",
            seven,
            "left",
        ),
        (
            "door side=right offset=6",
            "the right wall's corner with the back wall",
            seven,
            "right",
        ),
    ] {
        let offset = member.rsplit('=').next().expect("an offset");
        assert_eq!(
            refusal("9x7", member),
            (
                format!("door has `offset={offset}`, {primary}"),
                vec![format!(
                    "{between}; `at={anchor}` stands the door in this corner"
                )],
            ),
            "{member}",
        );
    }
}

#[test]
fn a_short_wall_names_the_columns_between_its_corners() {
    for (size, member, primary, note) in [
        (
            "3x5",
            "door side=front offset=0",
            "door has `offset=0`, the front wall's corner with the left wall",
            "`offset=` takes the one column between the wall's corners, 1; `at=left` stands the \
             door in this corner",
        ),
        (
            "2x5",
            "door side=front offset=1",
            "door has `offset=1`, the front wall's corner with the right wall",
            "the wall is 2 long, so no column lies between its corners; `at=right` stands the \
             door in this corner",
        ),
        (
            "5x2",
            "door side=left offset=0",
            "door has `offset=0`, the left wall's corner with the back wall",
            "the wall is 2 long, so no column lies between its corners; `at=left` stands the \
             door in this corner",
        ),
        (
            "1x5",
            "door side=front offset=0",
            "door has `offset=0`, the front wall's corner with the left wall",
            "the wall is 1 long, so no column lies between its corners; `at=left` stands the \
             door in this corner",
        ),
    ] {
        assert_eq!(
            refusal(size, member),
            (primary.to_owned(), vec![note.to_owned()]),
            "{size} {member}",
        );
    }
}

#[test]
fn a_door_names_its_column_once_and_by_a_whole_number() {
    let both = format!(
        "keep one of `at=` and `offset=`: `at=` takes `center`, `left` or `right`, and `offset=N` \
         counts from 0 at the wall's left end seen from outside; {NINE}"
    );
    let not_whole = |described: &str| {
        format!(
            "door has an `offset=` that is {described}, not a non-negative integer that fits in \
             u32"
        )
    };
    for (member, primary, note) in [
        (
            "door side=front at=center offset=2",
            "door names its column twice, with `at=` and with `offset=`".to_owned(),
            both.clone(),
        ),
        (
            "door side=front at=center offset=0",
            "door names its column twice, with `at=` and with `offset=`".to_owned(),
            both.clone(),
        ),
        (
            "door side=front at=center offset=x",
            "door names its column twice, with `at=` and with `offset=`".to_owned(),
            both,
        ),
        (
            "door side=front",
            "door has no `at=` or `offset=`".to_owned(),
            no_anchor_note(),
        ),
        (
            "door side=front at=middle",
            "door has `at=middle`, which is not one of center, left, right".to_owned(),
            no_anchor_note(),
        ),
        (
            "door side=front offset=x",
            not_whole("identifier `x`"),
            NINE.to_owned(),
        ),
        (
            "door side=front offset=1.5",
            not_whole("decimal `1.5`"),
            NINE.to_owned(),
        ),
        (
            "door side=front offset=4294967296",
            not_whole("integer `4294967296`"),
            NINE.to_owned(),
        ),
    ] {
        assert_eq!(refusal("9x7", member), (primary, vec![note]), "{member}");
    }
}

#[test]
fn a_numbered_at_is_told_the_key_that_names_its_column() {
    const ANCHORS: &str = "`at=` takes `center`, `left` or `right`";
    for (size, number, note) in [
        (
            "9x7",
            2,
            format!(
                "a column by number is `offset=2`, counted from 0 at the wall's left end seen \
                 from outside; {ANCHORS}"
            ),
        ),
        (
            "9x7",
            0,
            format!("{ANCHORS}; `at=left` stands the door in column 0"),
        ),
        (
            "9x7",
            8,
            format!("{ANCHORS}; `at=right` stands the door in column 8"),
        ),
        ("9x7", 12, format!("{ANCHORS}; {NINE}")),
        (
            "2x7",
            1,
            format!("{ANCHORS}; `at=right` stands the door in column 1"),
        ),
        (
            "3x7",
            1,
            format!(
                "a column by number is `offset=1`, counted from 0 at the wall's left end seen \
                 from outside; {ANCHORS}"
            ),
        ),
    ] {
        let member = format!("door side=front at={number}");
        assert_eq!(
            refusal(size, &member),
            (
                format!("door has `at={number}`, which is not one of center, left, right"),
                vec![note],
            ),
            "{size} {member}",
        );
    }
}

/// Two 5x3 `def`s side by side, `west`'s front door at `offset=` `column` joined to `east`'s.
fn duo(column: u32) -> BlockArrayIr {
    lowered_with_resolver_diagnostics(&format!(
        "{THEME}def hut size=5x3:\n  \
         walls mat_slot=wall height=3\n  \
         door id=entry side=front offset={column}\n\n\
         def shed size=5x3:\n  \
         walls mat_slot=wall height=3\n  \
         door id=entry side=front at=center\n\n\
         site s:\n  \
         place id=west use=hut theme=t at=origin\n  \
         place id=east use=shed theme=t east_of=west gap=4\n  \
         connect west.entry to east.entry path=@gravel\n",
    ))
}

#[test]
fn a_port_on_a_door_at_an_offset_stands_outside_that_column() {
    // `east` stands at x=9, so the port outside its centre door is at x=11.
    for (column, origin, length) in [(1, (1, 0, 3), 11), (3, (3, 0, 3), 9)] {
        let out = duo(column);
        assert!(
            out.diagnostics.is_empty(),
            "offset={column}: {:#?}",
            out.diagnostics
        );
        let walkway = out
            .walkways
            .get("walkway::s::west.entry__east.entry")
            .expect("walkway laid");
        assert_eq!(
            (walkway.origin, walkway.footprint),
            (origin, Footprint { x: length, z: 1 }),
            "offset={column}",
        );
    }
}
