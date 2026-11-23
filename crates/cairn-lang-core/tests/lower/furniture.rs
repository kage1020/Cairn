//! A body's `place` builds a def inside it, turned to face away from its wall or the way its
//! `facing=` names, from the file's defs or the furniture library (`spec/components-editing-sites`
//! "A `def` placed in a body").

use cairn_lang_core::DiagnosticCode;

use crate::common::{Finding, blocks_where, findings, lowered, only_structure};

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n  slot post -> @spruce_log\n\n";

/// A 7x5 struct with a floor and 3-row walls under `theme`, `members` after them.
fn hall_under(theme: &str, members: &str) -> String {
    format!(
        "{theme}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         {members}\n"
    )
}

/// A block as `id[properties]`, or its bare id, with its cell.
type Placed = ((u32, u32, u32), String);

/// The blocks other than cobblestone and air that `src` builds, with no finding.
fn stood_in(src: &str) -> Vec<Placed> {
    let out = lowered(src);
    assert_eq!(findings(src, &out), Vec::<Finding<'_>>::new(), "{src}");
    blocks_where(only_structure(&out), |id| {
        !matches!(id, "minecraft:cobblestone" | "minecraft:air")
    })
}

/// [`stood_in`] the hall under [`THEME`].
fn stood(members: &str) -> Vec<Placed> {
    stood_in(&hall_under(THEME, members))
}

/// The findings `members` raises in the hall under `theme`, which builds nothing else.
fn refused_under(theme: &str, members: &str) -> Vec<(DiagnosticCode, String, String, Vec<String>)> {
    let src = hall_under(theme, members);
    let out = lowered(&src);
    let placed = blocks_where(only_structure(&out), |id| {
        !matches!(id, "minecraft:cobblestone" | "minecraft:air")
    });
    assert_eq!(placed, [], "{members}");
    findings(&src, &out)
        .into_iter()
        .map(|(code, at, primary, notes)| {
            (
                code,
                at.to_owned(),
                primary.to_owned(),
                notes.into_iter().map(str::to_owned).collect(),
            )
        })
        .collect()
}

/// [`refused_under`] [`THEME`].
fn refused(members: &str) -> Vec<(DiagnosticCode, String, String, Vec<String>)> {
    refused_under(THEME, members)
}

fn at(cell: (u32, u32, u32), block: &str) -> Placed {
    (cell, block.to_owned())
}

/// The note every refusal of a line's `at=` closes with.
const AT_FORMS: &str = "write `at=inside.<side>` to stand the def against the inside of a wall, \
                        where `<side>` is front, back, left or right, or `at=X,Y,Z` to stand it \
                        at a cell";

#[test]
fn a_library_table_stands_at_the_cell_it_names() {
    assert_eq!(
        stood("place use=table at=3,1,2"),
        [
            at((3, 1, 2), "minecraft:oak_fence"),
            at((3, 2, 2), "minecraft:oak_pressure_plate"),
        ],
    );
}

#[test]
fn a_chair_turns_its_back_away_from_the_way_it_faces() {
    let chair = |facing: &str| stood(&format!("place use=chair at=3,1,2{facing}"));
    let stair = |facing: &str| {
        [at(
            (3, 1, 2),
            &format!("minecraft:oak_stairs[facing={facing}]"),
        )]
    };
    assert_eq!(chair(""), stair("north"));
    assert_eq!(chair(" facing=south"), stair("north"));
    assert_eq!(chair(" facing=west"), stair("east"));
    assert_eq!(chair(" facing=north"), stair("south"));
    assert_eq!(chair(" facing=east"), stair("west"));
}

#[test]
fn by_a_wall_a_def_stands_one_cell_in_with_its_back_to_the_wall() {
    let chair = |side: &str| stood(&format!("place use=chair at=inside.{side} offset=2 y=1"));
    assert_eq!(
        chair("back"),
        [at((4, 1, 1), "minecraft:oak_stairs[facing=north]")],
    );
    assert_eq!(
        chair("front"),
        [at((2, 1, 3), "minecraft:oak_stairs[facing=south]")],
    );
    assert_eq!(
        chair("left"),
        [at((1, 1, 2), "minecraft:oak_stairs[facing=west]")],
    );
    assert_eq!(
        chair("right"),
        [at((5, 1, 2), "minecraft:oak_stairs[facing=east]")],
    );
}

#[test]
fn a_fireplace_spans_three_columns_of_its_wall() {
    let hearth = "minecraft:bricks";
    assert_eq!(
        stood("place use=fireplace at=inside.front offset=2 y=1"),
        [
            at((2, 1, 3), hearth),
            at((2, 2, 3), hearth),
            at((2, 3, 3), hearth),
            at((3, 1, 3), "minecraft:campfire"),
            at((3, 3, 3), hearth),
            at((4, 1, 3), hearth),
            at((4, 2, 3), hearth),
            at((4, 3, 3), hearth),
        ],
    );
}

#[test]
fn a_def_of_the_file_turns_its_cells_and_their_axes() {
    let theme = "theme t:\n  slot wall -> @cobblestone\n  slot beam -> @spruce_log[axis=x]\n  \
                 slot cap -> @oak_planks\n\n";
    let src = format!(
        "{theme}def bench size=2x1:\n  fill mat_slot=beam from=0,0,0 to=0,0,0\n  \
         fill mat_slot=cap from=1,0,0 to=1,0,0\n\n{}",
        hall_under("", "place use=bench at=2,1,1 facing=west"),
    );
    assert_eq!(
        stood_in(&src),
        [
            at((2, 1, 1), "minecraft:spruce_log[axis=z]"),
            at((2, 1, 2), "minecraft:oak_planks"),
        ],
    );
}

#[test]
fn the_body_theme_rebinds_a_library_slot() {
    let theme =
        "theme t:\n  slot wall -> @cobblestone\n  slot chair -> @spruce_stairs[facing=north]\n\n";
    assert_eq!(
        stood_in(&hall_under(theme, "place use=chair at=3,1,2 facing=west")),
        [at((3, 1, 2), "minecraft:spruce_stairs[facing=east]")],
    );
}

#[test]
fn a_def_of_the_file_shadows_the_library_def_of_its_name() {
    let src = format!(
        "def table size=1x1:\n  fill mat_slot=post from=0,0,0 to=0,0,0\n\n{}",
        hall_under(THEME, "place use=table at=3,1,2"),
    );
    assert_eq!(stood_in(&src), [at((3, 1, 2), "minecraft:spruce_log")]);
}

#[test]
fn a_body_with_no_theme_builds_the_library_from_its_own() {
    assert_eq!(
        stood_in("struct s size=3x3\n  place use=lantern at=1,0,1\n"),
        [at((1, 0, 1), "minecraft:lantern[hanging=true]")],
    );
}

#[test]
fn a_body_with_no_theme_builds_nothing_of_a_def_of_the_file() {
    let src = "def post size=1x1:\n  fill mat_slot=post from=0,0,0 to=0,0,0\n\n\
               struct s size=3x3\n  place use=post at=1,0,1\n";
    let out = lowered(src);
    assert_eq!(
        findings(src, &out)
            .into_iter()
            .map(|(code, at, primary, _)| (code, at, primary))
            .collect::<Vec<_>>(),
        [(
            DiagnosticCode::NoThemeBound,
            "struct s size=3x3\n  place use=post at=1,0,1",
            "struct `s` has no theme bound, so no `mat_slot=` in it resolves to a block",
        )],
    );
    assert_eq!(
        blocks_where(only_structure(&out), |id| id != "minecraft:air"),
        []
    );
}

#[test]
fn a_place_without_an_at_is_refused() {
    assert_eq!(
        refused("place use=table"),
        [(
            DiagnosticCode::InvalidMember,
            "place use=table".to_owned(),
            "place has no `at=`, so nothing says where the def stands".to_owned(),
            vec![AT_FORMS.to_owned()],
        )],
    );
}

#[test]
fn a_def_by_a_wall_stands_strictly_inside_the_ring() {
    let ring = |wall: &str, last: u32, rows: u32| {
        format!(
            "a def by a wall stands strictly inside the wall ring: along the {wall} wall, \
             columns 1 to {last}, and at most {rows} rows into the struct"
        )
    };
    assert_eq!(
        refused("place use=table at=inside.back offset=0 y=1"),
        [(
            DiagnosticCode::InvalidMember,
            "place use=table at=inside.back offset=0 y=1".to_owned(),
            "def `table` is 1 wide and 1 deep, so by the back wall at `offset=0` it takes \
             columns 0 to 0 and 1 rows in"
                .to_owned(),
            vec![ring("back", 5, 3)],
        )],
    );
    assert_eq!(
        refused("place use=fireplace at=inside.left offset=2 y=1"),
        [(
            DiagnosticCode::InvalidMember,
            "place use=fireplace at=inside.left offset=2 y=1".to_owned(),
            "def `fireplace` is 3 wide and 1 deep, so by the left wall at `offset=2` it takes \
             columns 2 to 4 and 1 rows in"
                .to_owned(),
            vec![ring("left", 3, 5)],
        )],
    );
}

#[test]
fn a_def_stands_only_inside_a_wall() {
    assert_eq!(
        refused("place use=chair at=back.outside offset=2"),
        [(
            DiagnosticCode::InvalidMember,
            "place use=chair at=back.outside offset=2".to_owned(),
            "place `at=back.outside` stands the def outside its wall, and a def stands only \
             inside one"
                .to_owned(),
            vec!["write `at=inside.back` to stand it against the inside of that wall".to_owned()],
        )],
    );
}

#[test]
fn a_def_reaching_past_the_footprint_is_refused() {
    assert_eq!(
        refused("place use=fireplace at=5,1,1"),
        [(
            DiagnosticCode::InvalidMember,
            "place use=fireplace at=5,1,1".to_owned(),
            "def `fireplace` takes 3x1 cells as it faces, so at `at=5,1,1` it reaches past the \
             struct's `size=7x5` footprint"
                .to_owned(),
            vec![
                "the low corner of its footprint lies at `at=`, and the struct's `x` runs from 0 \
                 to 6 and its `z` from 0 to 4"
                    .to_owned(),
            ],
        )],
    );
    // Turned, it takes one column and three rows.
    assert_eq!(
        stood("place use=fireplace at=5,1,1 facing=west")
            .into_iter()
            .map(|(cell, _)| cell)
            .collect::<Vec<_>>(),
        [
            (5, 1, 1),
            (5, 1, 2),
            (5, 1, 3),
            (5, 2, 1),
            (5, 2, 3),
            (5, 3, 1),
            (5, 3, 2),
            (5, 3, 3),
        ],
    );
}

#[test]
fn a_def_reaching_above_the_struct_is_refused() {
    assert_eq!(
        refused("place use=bookcase at=3,2,2"),
        [(
            DiagnosticCode::InvalidMember,
            "place use=bookcase at=3,2,2".to_owned(),
            "def `bookcase` placed here reaches row 4, which does not fit in the struct (dims.y=4)"
                .to_owned(),
            vec![
                "the struct's top row is 3".to_owned(),
                "stand the def lower, or raise the `walls` to reach its rows".to_owned(),
            ],
        )],
    );
}

#[test]
fn a_def_above_walls_that_build_nothing_is_deferred() {
    // `shell` is bound to no block, so the walls paint nothing and the struct is one row tall.
    let src = format!(
        "{THEME}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=shell height=3\n  \
         place use=table at=3,1,2\n"
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out)
            .into_iter()
            .filter(|(code, ..)| *code == DiagnosticCode::DeferredMember)
            .collect::<Vec<_>>(),
        [(
            DiagnosticCode::DeferredMember,
            "place use=table at=3,1,2",
            "def `table` placed here reaches row 2, which does not fit in the struct (dims.y=1)",
            vec![
                "a `walls` member that would reach these rows paints nothing; the def is built \
                 once that member's material resolves to a block",
            ],
        )],
    );
}

#[test]
fn a_facing_that_is_no_compass_word_is_refused() {
    assert_eq!(
        refused("place use=chair at=3,1,2 facing=up"),
        [(
            DiagnosticCode::InvalidMember,
            "place use=chair at=3,1,2 facing=up".to_owned(),
            "place `facing=` must be a compass word, not identifier `up`".to_owned(),
            vec![
                "`facing=` names the way the def's front looks: `north`, `east`, `south` or \
                 `west`, and `south` when absent"
                    .to_owned(),
            ],
        )],
    );
}

#[test]
fn a_def_a_body_cannot_build_is_refused_at_its_line() {
    let src = format!(
        "def booth size=2x2:\n  walls mat_slot=wall height=2\n\n{}",
        hall_under(THEME, "place use=booth at=2,1,1"),
    );
    let out = lowered(&src);
    let found: Vec<_> = findings(&src, &out)
        .into_iter()
        .filter(|(code, ..)| *code == DiagnosticCode::InvalidMember)
        .collect();
    assert_eq!(
        found,
        [(
            DiagnosticCode::InvalidMember,
            "place use=booth at=2,1,1",
            "def `booth` holds a `walls` line, which a def placed in a body cannot build",
            vec![
                "a def placed in a body is built from `fill` lines that each write a \
                 `mat_slot=` and a box inside its footprint; a def of any other line is placed \
                 from a `site`",
                "the line it cannot build",
            ],
        )],
    );
    let sizeless = format!(
        "def stool:\n  fill mat_slot=post from=0,0,0 to=0,0,0\n\n{}",
        hall_under(THEME, "place use=stool at=2,1,1"),
    );
    let out = lowered(&sizeless);
    assert_eq!(
        findings(&sizeless, &out)
            .into_iter()
            .filter(|(code, ..)| *code == DiagnosticCode::InvalidMember)
            .collect::<Vec<_>>(),
        [(
            DiagnosticCode::InvalidMember,
            "place use=stool at=2,1,1",
            "def `stool` declares no `size=`, so nothing says how much floor it takes",
            vec!["give it a footprint, as in `def stool size=1x1:`"],
        )],
    );
}
