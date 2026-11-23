//! A `column` stands a run of its material upright from a cell, and its `repeat=` and `step=` stand
//! a series of them (`spec/blockstate` "Derive by default, promote on override").

use cairn_lang_core::DiagnosticCode;

use crate::common::{Finding, blocks_where, findings, lowered, only_structure};

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n  slot post -> @spruce_log\n\n";

/// A 7x5 struct with a floor and 3-row walls, `members` after them.
fn hall(members: &str) -> String {
    hall_under(THEME, members)
}

/// [`hall`] under `theme` in place of [`THEME`].
fn hall_under(theme: &str, members: &str) -> String {
    format!(
        "{theme}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         {members}\n"
    )
}

/// A block as `id[properties]`, or its bare id, with its cell.
type Placed = ((u32, u32, u32), String);

/// The blocks other than cobblestone and air that `members` builds in [`hall`], with no finding.
fn stood(members: &str) -> Vec<Placed> {
    stood_in(&hall(members))
}

/// [`stood`], for the whole source `src`.
fn stood_in(src: &str) -> Vec<Placed> {
    let out = lowered(src);
    assert_eq!(findings(src, &out), Vec::<Finding<'_>>::new(), "{src}");
    blocks_where(only_structure(&out), |id| {
        !matches!(id, "minecraft:cobblestone" | "minecraft:air")
    })
}

/// The findings `members` raises in [`hall`], as code, text pointed at, primary and notes.
fn refused(members: &str) -> Vec<(DiagnosticCode, String, String, Vec<String>)> {
    let src = hall(members);
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

/// `id` in each cell of `cells`.
fn blocks(id: &str, cells: &[(u32, u32, u32)]) -> Vec<Placed> {
    cells.iter().map(|&cell| (cell, id.to_owned())).collect()
}

#[test]
fn a_column_rises_its_height_from_the_cell_it_stands_on() {
    assert_eq!(
        stood("column at=3,1,2 height=3 mat_slot=post"),
        blocks("minecraft:spruce_log", &[(3, 1, 2), (3, 2, 2), (3, 3, 2)]),
    );
}

#[test]
fn a_column_without_a_material_is_built_from_the_default_column() {
    assert_eq!(
        stood("column at=2,1,1 height=2"),
        blocks("minecraft:stone_bricks", &[(2, 1, 1), (2, 2, 1)]),
    );
}

#[test]
fn a_series_stands_each_column_a_step_past_the_one_before() {
    assert_eq!(
        stood("column at=1,1,2 height=1 repeat=3 step=2,0,0 mat_slot=post"),
        blocks("minecraft:spruce_log", &[(1, 1, 2), (3, 1, 2), (5, 1, 2)]),
    );
}

#[test]
fn a_column_without_a_series_moves_nothing_by_its_step() {
    assert_eq!(
        stood("column at=1,1,2 height=1 step=2,0,0 mat_slot=post"),
        blocks("minecraft:spruce_log", &[(1, 1, 2)]),
    );
}

#[test]
fn a_column_keeps_the_state_its_material_writes() {
    let theme = "theme t:\n  slot wall -> @cobblestone\n  slot post -> @spruce_log[axis=x]\n\n";
    assert_eq!(
        stood_in(&hall_under(theme, "column at=3,1,2 height=2 mat_slot=post")),
        blocks("minecraft:spruce_log[axis=x]", &[(3, 1, 2), (3, 2, 2)]),
    );
}

#[test]
fn a_column_under_a_level_rises_from_the_row_its_level_raises_it_to() {
    assert_eq!(
        stood("level y=1\n    column at=3,0,2 height=2 mat_slot=post"),
        blocks("minecraft:spruce_log", &[(3, 1, 2), (3, 2, 2)]),
    );
}

#[test]
fn a_column_over_the_walls_is_a_conflict_in_their_phase_and_the_later_line_wins() {
    let src = hall("column at=0,1,0 height=3 mat_slot=post");
    let out = lowered(&src);
    let codes: Vec<(DiagnosticCode, &str)> = findings(&src, &out)
        .into_iter()
        .map(|(code, at, _, _)| (code, at))
        .collect();
    assert_eq!(
        codes,
        [(
            DiagnosticCode::PhaseConflict,
            "column at=0,1,0 height=3 mat_slot=post"
        )],
    );
    assert_eq!(
        blocks_where(only_structure(&out), |id| id.ends_with("_log")),
        blocks("minecraft:spruce_log", &[(0, 1, 0), (0, 2, 0), (0, 3, 0)]),
    );
}

#[test]
fn a_column_with_no_height_or_one_that_is_no_positive_whole_number_is_refused() {
    let form = "write `height=N`, where `N` is a positive whole number: the column rises `N` rows \
                from the cell its `at=` names, that cell included";
    assert_eq!(
        refused("column at=3,1,2 mat_slot=post"),
        [(
            DiagnosticCode::InvalidMember,
            "column at=3,1,2 mat_slot=post".to_owned(),
            "column has no `height=`, so nothing says how many rows it rises".to_owned(),
            vec![form.to_owned()],
        )],
    );
    for (height, described) in [("0", "integer `0`"), ("up", "identifier `up`")] {
        let line = format!("column at=3,1,2 height={height} mat_slot=post");
        assert_eq!(
            refused(&line),
            [(
                DiagnosticCode::InvalidMember,
                line.clone(),
                format!(
                    "column `height=` must be a positive integer that fits in u32, not {described}"
                ),
                vec![form.to_owned()],
            )],
        );
    }
}

#[test]
fn a_repeat_or_step_that_does_not_read_is_refused() {
    let form = "write `repeat=N step=dx,dy,dz`: the column is written `N` times, each copy \
                `dx,dy,dz` past the one before";
    for (series, primary) in [
        (
            "repeat=0 step=2,0,0",
            "column `repeat=` must be a positive integer that fits in u32, not integer `0`",
        ),
        (
            "repeat=x step=2,0,0",
            "column `repeat=` must be a positive integer that fits in u32, not identifier `x`",
        ),
        (
            "repeat=2 step=2",
            "column `step=` must be a coordinate triple whose components fit in u32, not integer \
             `2`",
        ),
    ] {
        let line = format!("column at=1,1,2 height=1 {series} mat_slot=post");
        assert_eq!(
            refused(&line),
            [(
                DiagnosticCode::InvalidMember,
                line.clone(),
                primary.to_owned(),
                vec![form.to_owned()],
            )],
        );
    }
}

#[test]
fn a_series_without_a_step_to_space_it_is_refused() {
    let form = "write `repeat=N step=dx,dy,dz`: the column is written `N` times, each copy \
                `dx,dy,dz` past the one before";
    for (line, primary) in [
        (
            "column at=1,1,2 height=1 repeat=2 mat_slot=post",
            "column has `repeat=2` and no `step=`, so nothing says where each copy goes",
        ),
        (
            "column at=1,1,2 height=1 repeat=2 step=0,0,0 mat_slot=post",
            "column `step=0,0,0` puts each of its 2 copies in the cells of the first",
        ),
    ] {
        assert_eq!(
            refused(line),
            [(
                DiagnosticCode::InvalidMember,
                line.to_owned(),
                primary.to_owned(),
                vec![form.to_owned()],
            )],
        );
    }
}

#[test]
fn a_column_rising_above_the_struct_is_refused() {
    assert_eq!(
        refused("column at=3,1,2 height=9 mat_slot=post"),
        [(
            DiagnosticCode::InvalidMember,
            "column at=3,1,2 height=9 mat_slot=post".to_owned(),
            "column `at=3,1,2` with `height=9` (its top in row 9) does not fit in the struct \
             (dims.y=4)"
                .to_owned(),
            vec![
                "the cell's `y` runs from 0 to 3, the struct's top row".to_owned(),
                "lower the column's `at=` or `height=`, or raise the `walls` to reach its rows"
                    .to_owned(),
            ],
        )],
    );
}

#[test]
fn a_column_rising_above_the_struct_under_a_level_names_the_row_its_top_reaches() {
    let line = "column at=3,1,2 height=3 mat_slot=post";
    assert_eq!(
        refused(&format!("level y=1\n    {line}")),
        [(
            DiagnosticCode::InvalidMember,
            line.to_owned(),
            "column `at=3,1,2` with `height=3` under `level y=1` (its top in row 4) does not fit \
             in the struct (dims.y=4)"
                .to_owned(),
            vec![
                "the cell's `y` runs from 0 to 2 under `level y=1`, the struct's top row being 3"
                    .to_owned(),
                "lower the column's `at=` or `height=`, or raise the `walls` to reach its rows"
                    .to_owned(),
            ],
        )],
    );
}

#[test]
fn a_column_whose_top_lies_past_the_largest_row_names_the_row_it_reaches() {
    let line = "column at=1,5,1 height=4294967295 mat_slot=post";
    assert_eq!(
        refused(line),
        [(
            DiagnosticCode::InvalidMember,
            line.to_owned(),
            "column `at=1,5,1` with `height=4294967295` (its top in row 4294967299) does not fit \
             in the struct (dims.y=4)"
                .to_owned(),
            vec![
                "the cell's `y` runs from 0 to 3, the struct's top row".to_owned(),
                "lower the column's `at=` or `height=`, or raise the `walls` to reach its rows"
                    .to_owned(),
            ],
        )],
    );
}

#[test]
fn a_series_reaching_past_the_footprint_is_refused() {
    let line = "column at=1,1,2 height=1 repeat=4 step=2,0,0 mat_slot=post";
    assert_eq!(
        refused(line),
        [(
            DiagnosticCode::InvalidMember,
            line.to_owned(),
            "column `at=1,1,2`, whose last of 4 copies lands at 7,1,2, lies outside the \
             struct's `size=7x5` footprint"
                .to_owned(),
            vec![
                "the cell's `x` runs from 0 to 6 and its `z` from 0 to 4".to_owned(),
                "lower the `repeat=` or `step=`".to_owned(),
            ],
        )],
    );
}
