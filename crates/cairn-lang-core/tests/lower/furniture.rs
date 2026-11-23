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

/// A finding as `(code, the text it spans, primary, notes)`.
type Owned = (DiagnosticCode, String, String, Vec<String>);

/// Every finding lowering `src` raises.
fn found(src: &str) -> Vec<Owned> {
    let out = lowered(src);
    findings(src, &out)
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

/// The findings `members` raises in the hall under `theme`, which builds nothing else.
fn refused_under(theme: &str, members: &str) -> Vec<Owned> {
    let src = hall_under(theme, members);
    let placed = blocks_where(only_structure(&lowered(&src)), |id| {
        !matches!(id, "minecraft:cobblestone" | "minecraft:air")
    });
    assert_eq!(placed, [], "{members}");
    found(&src)
}

/// One `E_INVALID_MEMBER` on `line` with `primary` and `notes`.
fn invalid(line: &str, primary: &str, notes: &[&str]) -> Vec<Owned> {
    vec![(
        DiagnosticCode::InvalidMember,
        line.to_owned(),
        primary.to_owned(),
        notes.iter().map(|note| (*note).to_owned()).collect(),
    )]
}

/// [`refused_under`] [`THEME`].
fn refused(members: &str) -> Vec<Owned> {
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
            at((3, 1, 3), "minecraft:campfire[facing=north]"),
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
    let theme = "theme t:\n  slot wall -> @cobblestone\n  slot chair_seat -> @spruce_stairs[facing=north]\n\n";
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
    assert_eq!(
        found(&src),
        [(
            DiagnosticCode::DeferredMember,
            "place use=table at=3,1,2".to_owned(),
            "def `table` placed here reaches row 2, which does not fit in the struct (dims.y=1)"
                .to_owned(),
            vec![
                "a `walls` member that would reach these rows paints nothing; the def is built \
                 once that member's material resolves to a block"
                    .to_owned(),
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
    assert_eq!(
        found(&src),
        invalid(
            "place use=booth at=2,1,1",
            "def `booth` holds a `walls` line, which a def placed in a body cannot build",
            &[UNBUILDABLE, "the line it cannot build"],
        ),
    );
    let sizeless = format!(
        "def stool:\n  fill mat_slot=post from=0,0,0 to=0,0,0\n\n{}",
        hall_under(THEME, "place use=stool at=2,1,1"),
    );
    assert_eq!(
        found(&sizeless),
        invalid(
            "place use=stool at=2,1,1",
            "def `stool` declares no `size=`, so nothing says how much floor it takes",
            &["give it a footprint, as in `def stool size=1x1:`"],
        ),
    );
}

/// The note on a def a body cannot build, saying what one is built from.
const UNBUILDABLE: &str = "a def placed in a body is built from `fill` lines that each write a \
                           `mat_slot=` and a box inside its footprint; a def of any other line \
                           is placed from a `site`";

/// A theme binding four planks, one to each of the slots `a` to `d`.
const FOUR: &str = "theme t:\n  slot wall -> @cobblestone\n  slot a -> @oak_planks\n  \
                    slot b -> @spruce_planks\n  slot c -> @birch_planks\n  \
                    slot d -> @jungle_planks\n\n";

/// The blocks a def of the file `def` builds, placed by `place` in the hall under [`FOUR`].
fn four_placed(def: &str, place: &str) -> Vec<Placed> {
    stood_in(&format!("{FOUR}{def}\n\n{}", hall_under("", place)))
}

#[test]
fn a_deep_def_at_a_cell_turns_both_its_extents() {
    let quad = "def quad size=2x2:\n  fill mat_slot=a from=0,0,0 to=0,0,0\n  \
                fill mat_slot=b from=1,0,0 to=1,0,0\n  fill mat_slot=c from=0,0,1 to=0,0,1\n  \
                fill mat_slot=d from=1,0,1 to=1,0,1";
    let faced =
        |facing: &str| four_placed(quad, &format!("place use=quad at=2,1,1 facing={facing}"));
    let planks = |[low, near, far, high]: [&str; 4]| {
        vec![
            at((2, 1, 1), &format!("minecraft:{low}_planks")),
            at((2, 1, 2), &format!("minecraft:{near}_planks")),
            at((3, 1, 1), &format!("minecraft:{far}_planks")),
            at((3, 1, 2), &format!("minecraft:{high}_planks")),
        ]
    };
    assert_eq!(faced("south"), planks(["oak", "birch", "spruce", "jungle"]));
    assert_eq!(faced("west"), planks(["birch", "jungle", "oak", "spruce"]));
    assert_eq!(faced("north"), planks(["jungle", "spruce", "birch", "oak"]));
    assert_eq!(faced("east"), planks(["spruce", "oak", "jungle", "birch"]));
}

#[test]
fn a_deep_def_by_each_wall_stands_its_back_row_against_it() {
    let shelf = "def shelf size=1x2:\n  fill mat_slot=a from=0,0,0 to=0,0,0\n  \
                 fill mat_slot=b from=0,0,1 to=0,0,1";
    let by = |side: &str| {
        four_placed(
            shelf,
            &format!("place use=shelf at=inside.{side} offset=2 y=1"),
        )
    };
    let (back, front) = ("minecraft:oak_planks", "minecraft:spruce_planks");
    assert_eq!(by("back"), [at((4, 1, 1), back), at((4, 1, 2), front)]);
    assert_eq!(by("front"), [at((2, 1, 2), front), at((2, 1, 3), back)]);
    assert_eq!(by("left"), [at((1, 1, 2), back), at((2, 1, 2), front)]);
    assert_eq!(by("right"), [at((4, 1, 2), front), at((5, 1, 2), back)]);
}

#[test]
fn the_library_s_blocks_turn_with_the_def_and_its_cubes_do_not() {
    let faced = |piece: &str, x: u32| format!("place use={piece} at={x},1,1 facing=west");
    let members = [
        faced("furnace", 1),
        faced("lectern", 2),
        faced("barrel", 3),
        faced("crafting_table", 4),
        faced("wall_torch", 5),
    ]
    .join("\n  ");
    assert_eq!(
        stood(&members),
        [
            at((1, 1, 1), "minecraft:furnace[facing=west]"),
            at((2, 1, 1), "minecraft:lectern[facing=west]"),
            at((3, 1, 1), "minecraft:barrel[facing=up]"),
            at((4, 1, 1), "minecraft:crafting_table"),
            at((5, 1, 1), "minecraft:wall_torch[facing=west]"),
        ],
    );
}

#[test]
fn an_overhang_moves_a_placed_def_with_the_footprint() {
    let src = format!(
        "{THEME}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         roof kind=flat mat_slot=wall overhang=1\n  place use=table at=3,1,2\n  \
         place use=chair at=inside.back offset=2 y=1\n"
    );
    assert_eq!(
        stood_in(&src),
        [
            at((4, 1, 3), "minecraft:oak_fence"),
            at((4, 2, 3), "minecraft:oak_pressure_plate"),
            at((5, 1, 2), "minecraft:oak_stairs[facing=north]"),
        ],
    );
}

#[test]
fn a_place_under_a_level_stands_on_the_level_s_row() {
    let src = format!(
        "{THEME}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=5\n  \
         level id=upper y=2\n    place use=table at=3,0,2\n    \
         place use=chair at=inside.back offset=2 y=1\n"
    );
    assert_eq!(
        stood_in(&src),
        [
            at((3, 2, 2), "minecraft:oak_fence"),
            at((3, 3, 2), "minecraft:oak_pressure_plate"),
            at((4, 3, 1), "minecraft:oak_stairs[facing=north]"),
        ],
    );
}

#[test]
fn a_place_that_takes_a_cell_from_another_of_its_phase_conflicts() {
    let conflict = |line: &str, earlier: &str| {
        vec![(
            DiagnosticCode::PhaseConflict,
            line.to_owned(),
            format!(
                "`place` overwrites 1 voxel that `{earlier}` painted in the same phase, so which \
                 block the build keeps is decided by the order the two lines are written in"
            ),
            vec![
                "overwritten member declared here".to_owned(),
                "If the override is deliberate this is the local-override rule of \
                 spec/compilation \"Phase evaluation\" and the later line wins as written. If it \
                 is not, move one of the two so their footprints do not meet."
                    .to_owned(),
                "Members in different phases may overlap freely — a `door` cut through a `walls` \
                 is the evaluation model working — so this is only reported for two members the \
                 same phase evaluates."
                    .to_owned(),
            ],
        )]
    };
    let chair = "place use=chair at=3,1,2";
    let src = hall_under(THEME, &format!("place use=table at=3,1,2\n  {chair}"));
    assert_eq!(found(&src), conflict(chair, "place"));
    let chair = "place use=chair at=inside.back offset=2 y=1";
    let src = hall_under(
        THEME,
        &format!("chest at=inside.back offset=2 y=1\n  {chair}"),
    );
    assert_eq!(found(&src), conflict(chair, "chest"));
}

#[test]
fn a_tall_def_is_refused_before_its_cells_are_listed() {
    let src = format!(
        "def tall size=1x1:\n  fill mat_slot=post from=0,0,0 to=0,4000000000,0\n\n{}",
        hall_under(THEME, "place use=tall at=2,1,2"),
    );
    assert_eq!(
        found(&src),
        invalid(
            "place use=tall at=2,1,2",
            "def `tall` placed here reaches row 4000000001, which does not fit in the struct \
             (dims.y=4)",
            &[
                "the struct's top row is 3",
                "stand the def lower, or raise the `walls` to reach its rows",
            ],
        ),
    );
}

#[test]
fn a_def_on_a_cell_of_an_opening_is_refused() {
    let door = "door side=front at=center";
    let window = "window side=back offset=3 y=2 size=1x1 mat_slot=wall";
    for (opening, place, keyword) in [
        (door, "place use=bookcase at=3,1,4", "door"),
        (window, "place use=bookcase at=3,1,0", "window"),
    ] {
        let src = hall_under(THEME, &format!("{opening}\n  {place}"));
        assert_eq!(
            found(&src),
            invalid(
                place,
                &format!(
                    "def `bookcase` placed here takes a cell the `{keyword}` holds, and would \
                     erase it"
                ),
                &[
                    &format!("the `{keyword}` whose cell it takes"),
                    "stand the def clear of every `door`, `window` and `arch`",
                ],
            ),
            "{opening}",
        );
        let bookshelves = blocks_where(only_structure(&lowered(&src)), |id| {
            id == "minecraft:bookshelf"
        });
        assert_eq!(bookshelves, [], "{opening}");
    }
}

#[test]
fn a_def_a_body_cannot_build_is_refused_for_each_kind_of_line() {
    let refusal = |def: &str, what: &str| {
        let src = format!("{def}\n\n{}", hall_under(THEME, "place use=odd at=2,1,1"));
        assert_eq!(
            found(&src),
            invalid(
                "place use=odd at=2,1,1",
                &format!("def `odd` holds {what}, which a def placed in a body cannot build"),
                &[UNBUILDABLE, "the line it cannot build"],
            ),
            "{def}",
        );
    };
    refusal(
        "def odd size=1x1:\n  fill mat_slot=post from=0,0,0 to=1,0,0",
        "a `fill` reaching past its 1x1 footprint",
    );
    refusal(
        "def odd size=1x1:\n  fill mat_slot=post from=0,0,0 to=up",
        "a `fill` whose `from=` and `to=` are not two coordinate triples",
    );
    refusal(
        "def odd size=1x1:\n  fill fluid=water from=0,0,0 to=0,0,0",
        "a water `fill`",
    );
    refusal(
        "def odd size=1x1:\n  fill from=0,0,0 to=0,0,0",
        "a `fill` with no `mat_slot=`",
    );
    refusal(
        "def odd size=1x1:\n  place use=table at=0,0,0",
        "a `place` line",
    );
}

#[test]
fn a_place_that_stands_its_def_nowhere_is_refused() {
    assert_eq!(
        refused("place at=3,1,2"),
        invalid(
            "place at=3,1,2",
            "place has no `use=`, so nothing says which def it builds",
            &["name a def of this file or of the furniture library, as in `use=table`"],
        ),
    );
    assert_eq!(
        refused("place use=table at=origin"),
        invalid(
            "place use=table at=origin",
            "place `at=` must be `inside.<side>` or a coordinate triple, not identifier `origin`",
            &[AT_FORMS],
        ),
    );
    assert_eq!(
        refused("place use=table at=inside.top"),
        invalid(
            "place use=table at=inside.top",
            "place `at=inside.top`: `top` is not a wall side",
            &["a side is one of front, back, left, right, as in `at=inside.front`"],
        ),
    );
    assert_eq!(
        refused("place use=table at=inside.back offset=wide y=1"),
        invalid(
            "place use=table at=inside.back offset=wide y=1",
            "place `offset=` must be a non-negative integer that fits in u32, not identifier \
             `wide`",
            &["`offset=` counts cells along the wall from 0, its left end seen from outside"],
        ),
    );
    assert_eq!(
        refused("place use=table at=inside.back offset=2 y=high"),
        invalid(
            "place use=table at=inside.back offset=2 y=high",
            "place `y=` must be a non-negative integer that fits in u32, not identifier `high`",
            &["`y=` counts rows up from 0, the base row of the struct or of the def's `level`"],
        ),
    );
}

#[test]
fn a_misspelt_anchor_is_offered_only_the_inside_of_a_wall() {
    assert_eq!(
        refused("place use=table at=insde.back offset=2 y=1"),
        invalid(
            "place use=table at=insde.back offset=2 y=1",
            "place `at=insde.back` is not an anchor",
            &[AT_FORMS, "did you mean `at=inside.back`?"],
        ),
    );
    assert_eq!(
        refused("place use=table at=bakc.outside offset=2 y=1"),
        invalid(
            "place use=table at=bakc.outside offset=2 y=1",
            "place `at=bakc.outside`: `bakc` is not a wall side",
            &[
                "a side is one of front, back, left, right, as in `at=inside.front`",
                "did you mean `at=inside.back`?",
            ],
        ),
    );
}

#[test]
fn a_slot_of_the_file_named_like_a_library_part_leaves_the_library_alone() {
    let theme = "theme t:\n  slot wall -> @cobblestone\n  slot torch -> @torch\n\n";
    assert_eq!(
        stood_in(&hall_under(
            theme,
            "fill mat_slot=torch from=1,1,1 to=1,1,1\n  place use=wall_torch at=inside.back offset=3 y=2",
        )),
        [
            at((1, 1, 1), "minecraft:torch"),
            at((3, 2, 1), "minecraft:wall_torch[facing=south]"),
        ],
    );
}
