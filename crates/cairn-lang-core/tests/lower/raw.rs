//! A `raw_block` or `raw_fill` writes the block id its `mat=` names into the cells its triples
//! name, as given, after every other member (`spec/materials-themes` "Raw blocks").

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::block_array::BlockArrayIr;

use crate::common::{
    BIN_THEME, Finding, Wet, bin, findings, layer, lowered, lowered_with, only_structure,
};

/// A block a member resolved to, as `id[properties]`, and its cells.
type Built = (String, Vec<(u32, u32, u32)>);

/// Each block the members written with `role` resolved to.
fn built_by(out: &BlockArrayIr, role: &str) -> Vec<Built> {
    out.members["struct::s"]
        .iter()
        .filter(|member| member.role == role)
        .flat_map(|member| &member.resolved_state)
        .map(|block| {
            let state = &block.state;
            let printed = if state.properties.is_empty() {
                state.id.clone()
            } else {
                format!("{}[{}]", state.id, state.properties_text())
            };
            (printed, block.cells.clone())
        })
        .collect()
}

/// [`bin`]'s row `y = 1` with nothing in the footprint but the walls.
const EMPTY_ROW: [&str; 6] = [
    ".......", ".#####.", ".#...#.", ".#...#.", ".#####.", ".......",
];

#[test]
fn a_raw_block_writes_its_block_into_the_one_cell_its_at_names() {
    let out = lowered(&bin("raw_block mat=minecraft:stone at=2,1,1"));
    assert_eq!(out.diagnostics, []);
    assert_eq!(
        layer(only_structure(&out), 1),
        [
            ".......", ".#####.", ".#.=.#.", ".#...#.", ".#####.", ".......",
        ],
    );
    assert_eq!(
        built_by(&out, "raw_block"),
        [("minecraft:stone".to_owned(), vec![(3, 1, 2)])],
    );
}

#[test]
fn a_raw_fill_writes_its_block_into_every_cell_of_its_box_with_its_corners_in_either_order() {
    let out = lowered(&bin("raw_fill mat=minecraft:stone from=3,1,2 to=1,0,1"));
    assert_eq!(out.diagnostics, []);
    let array = only_structure(&out);
    assert_eq!(
        layer(array, 0),
        [
            ".......", ".......", "..===..", "..===..", ".......", ".......",
        ],
    );
    assert_eq!(
        layer(array, 1),
        [
            ".......", ".#####.", ".#===#.", ".#===#.", ".#####.", ".......",
        ],
    );
}

#[test]
fn a_raw_member_under_a_level_is_measured_from_the_level() {
    let out = lowered(&bin(
        "level y=1\n    raw_fill mat=minecraft:stone from=1,0,1 to=3,0,1",
    ));
    assert_eq!(out.diagnostics, []);
    let array = only_structure(&out);
    assert_eq!(layer(array, 1)[2], ".#===#.");
    assert_eq!(layer(array, 0)[2], ".......");
}

#[test]
fn a_raw_member_keeps_the_state_literal_it_writes() {
    let out = lowered(&bin(
        "raw_block mat=minecraft:oak_stairs[facing=east, half=top] at=1,1,1",
    ));
    assert_eq!(out.diagnostics, []);
    assert_eq!(
        built_by(&out, "raw_block"),
        [(
            "minecraft:oak_stairs[facing=east,half=top]".to_owned(),
            vec![(2, 1, 2)],
        )],
    );
}

#[test]
fn a_raw_member_replaces_what_another_phase_put_in_its_cells_air_included() {
    let src = bin("raw_fill mat=minecraft:air from=0,1,0 to=4,1,0\n  \
         raw_block mat=minecraft:stone at=0,1,3");
    let out = lowered(&src);
    assert_eq!(
        out.diagnostics,
        [],
        "members in different phases overlap freely"
    );
    assert_eq!(
        layer(only_structure(&out), 1),
        [
            ".......", ".......", ".#...#.", ".#...#.", ".=####.", ".......",
        ],
    );
    let walls: usize = built_by(&out, "walls")
        .iter()
        .map(|(_, cells)| cells.len())
        .sum();
    assert_eq!(
        walls,
        2 * 14 - 6,
        "the walls keep the cells no raw member wrote"
    );
}

#[test]
fn water_poured_before_a_raw_block_neither_stays_in_its_cell_nor_waterlogs_it() {
    let src = bin("window side=front offset=1 y=1 size=3x1 mat_slot=pane\n  \
         fill fluid=water from=0,1,0 to=4,1,3\n  \
         raw_block mat=minecraft:glass_pane at=1,1,1");
    let out = lowered_with(&src, &Wet);
    assert_eq!(out.diagnostics, []);
    assert_eq!(
        layer(only_structure(&out), 1),
        [
            ".......", ".#####.", ".#p~~#.", ".#~~~#.", ".#PPP#.", ".......",
        ],
    );
    assert_eq!(
        built_by(&out, "fill")
            .iter()
            .map(|(_, cells)| cells.len())
            .sum::<usize>(),
        5,
    );
}

#[test]
fn two_raw_members_writing_one_cell_contest_it_and_the_later_line_wins() {
    let src = bin("raw_block mat=minecraft:stone at=2,1,1\n  \
         raw_fill mat=minecraft:air from=1,1,1 to=3,1,1");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::PhaseConflict,
            "raw_fill mat=minecraft:air from=1,1,1 to=3,1,1",
            "`raw_fill` overwrites 1 voxel that `raw_block` painted in the same phase, so which \
             block the build keeps is decided by the order the two lines are written in",
            vec![
                "overwritten member declared here",
                "If the override is deliberate this is the local-override rule of \
                 spec/compilation \"Phase evaluation\" and the later line wins as written. If it \
                 is not, move one of the two so their footprints do not meet.",
                "Members in different phases may overlap freely — a `door` cut through a `walls` \
                 is the evaluation model working — so this is only reported for two members the \
                 same phase evaluates.",
            ],
        )],
    );
    assert_eq!(layer(only_structure(&out), 1), EMPTY_ROW);
}

#[test]
fn a_raw_member_builds_in_a_scope_with_no_theme() {
    let src = "struct s size=3x3\n  raw_fill mat=minecraft:stone from=0,0,0 to=2,0,2\n";
    let out = lowered(src);
    assert_eq!(out.diagnostics, []);
    assert_eq!(layer(only_structure(&out), 0), ["===", "===", "==="]);
}

#[test]
fn a_raw_member_sizes_no_row_of_the_volume() {
    let src = "struct s size=3x3\n  raw_block mat=minecraft:stone at=1,1,1\n";
    let out = lowered(src);
    assert_eq!(
        findings(src, &out),
        [(
            DiagnosticCode::InvalidMember,
            "raw_block mat=minecraft:stone at=1,1,1",
            "raw_block `at=1,1,1` does not fit in the struct (dims.y=1)",
            vec![
                "the cell's `y` runs from 0 to 0, the struct's top row",
                "lower the raw_block's `at=`, or raise the `walls` to reach its rows",
            ],
        )],
    );
    assert_eq!(only_structure(&out).dims.y, 1);
}

#[test]
fn a_raw_member_whose_line_names_no_block_or_no_cell_is_refused() {
    const BLOCK_ID: &str = "write `mat=` as the block id itself, `namespace:path`, as in \
                            `mat=minecraft:stone`";
    const AT: &str = "write `at=` as the coordinate triple `x,y,z` of the cell the block goes in, \
                      as in `at=1,1,1`";
    const CORNERS: &str = "write `from=` and `to=` as two opposite corners of the box, each a \
                           coordinate triple `x,y,z`, as in `from=1,1,1 to=3,2,3`";
    for (line, primary, notes) in [
        (
            "raw_block at=1,1,1",
            "raw_block has no `mat=`, so nothing says which block it places",
            vec![BLOCK_ID],
        ),
        (
            "raw_block mat=@stone at=1,1,1",
            "raw_block `mat=` must be a block id, not token `@stone`",
            vec![
                BLOCK_ID,
                "`@stone` is a canonical token, which a theme binds to a `mat_slot=`; write \
                 `mat=minecraft:stone` to place the block it names",
            ],
        ),
        (
            "raw_block mat=@floor.wood at=1,1,1",
            "raw_block `mat=` must be a block id, not token `@floor.wood`",
            vec![BLOCK_ID],
        ),
        (
            "raw_fill mat=stone from=1,1,1 to=1,1,1",
            "raw_fill `mat=` must be a block id, not identifier `stone`",
            vec![BLOCK_ID],
        ),
        (
            "raw_block mat=minecraft:stone",
            "raw_block has no `at=`, so nothing says which cell its block goes in",
            vec![AT],
        ),
        (
            "raw_block mat=minecraft:stone at=1",
            "raw_block `at=` must be a coordinate triple, not integer `1`",
            vec![AT],
        ),
        (
            "raw_fill mat=minecraft:stone from=1,1,1",
            "raw_fill has no `to=`, so nothing says which cells its box holds",
            vec![CORNERS],
        ),
        (
            "raw_block mat=minecraft:stone at=5,1,1",
            "raw_block `at=5,1,1` lies outside the struct's `size=5x4` footprint",
            vec!["the cell's `x` runs from 0 to 4 and its `z` from 0 to 3"],
        ),
        (
            "raw_fill mat=minecraft:stone from=1,1,1 to=1,1,4",
            "raw_fill `to=1,1,4` lies outside the struct's `size=5x4` footprint",
            vec!["a corner's `x` runs from 0 to 4 and its `z` from 0 to 3"],
        ),
    ] {
        let src = bin(line);
        let out = lowered(&src);
        let expected: Vec<Finding<'_>> =
            vec![(DiagnosticCode::InvalidMember, line, primary, notes)];
        assert_eq!(findings(&src, &out), expected, "{line}");
        assert_eq!(
            layer(only_structure(&out), 1),
            EMPTY_ROW,
            "{line} builds nothing"
        );
    }
}

#[test]
fn a_raw_member_above_the_struct_is_refused_and_one_a_roof_may_reach_is_deferred() {
    let src = bin("raw_block mat=minecraft:stone at=1,9,1");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::InvalidMember,
            "raw_block mat=minecraft:stone at=1,9,1",
            "raw_block `at=1,9,1` does not fit in the struct (dims.y=4)",
            vec![
                "the cell's `y` runs from 0 to 3, the struct's top row",
                "lower the raw_block's `at=`, or raise the `walls` to reach its rows",
            ],
        )],
    );

    let src = format!(
        "{BIN_THEME}struct s size=5x4\n  walls mat_slot=wall height=2\n  roof mat_slot=wall\n  \
         level y=3\n    raw_block mat=minecraft:stone at=1,0,1\n"
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::InvalidMember,
                "roof mat_slot=wall",
                "missing `kind=` (expected one of gable, shed, hip, flat)",
                vec![],
            ),
            (
                DiagnosticCode::DeferredMember,
                "raw_block mat=minecraft:stone at=1,0,1",
                "raw_block `at=1,0,1` under `level y=3` (row 3) does not fit in the struct \
                 (dims.y=3)",
                vec![
                    "a `roof` whose `kind=` or `slope_to=` does not read may rise to this row; \
                     the block is placed once that roof draws",
                ],
            ),
        ],
    );
}
