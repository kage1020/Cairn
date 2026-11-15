//! A `beam` lays pillar blocks on their side along the top row of a wall, or over a `door` or
//! `window`, along its wall unless its line turns it (`spec/blockstate` "Derive by default,
//! promote on override").

use cairn_lang_core::DiagnosticCode;

use crate::common::{DRAWN_ROLES, Finding, blocks_where, findings, lowered, only_structure};

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n  slot frame -> @spruce_log\n\n";

/// A 7x5 struct with a floor and 3-row walls, `members` after them; with no overhang its front
/// wall is `z=4`, its back `z=0`, its left `x=0` and its right `x=6`.
fn hall(members: &str) -> String {
    format!(
        "{THEME}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         {members}\n"
    )
}

/// A block as `id[properties]`, with its cell.
type Placed = ((u32, u32, u32), String);

/// The logs `members` lays in [`hall`], which it must lay without a finding.
fn laid(members: &str) -> Vec<Placed> {
    let src = hall(members);
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), Vec::<Finding<'_>>::new(), "{members}");
    blocks_where(only_structure(&out), |id| id.ends_with("_log"))
}

/// A finding as [`Finding`] holds it, owned.
type Owned = (DiagnosticCode, String, String, Vec<String>);

/// The findings `src` raises, owned, and the logs it lays.
fn lowering(src: &str) -> (Vec<Owned>, Vec<Placed>) {
    let out = lowered(src);
    let found = findings(src, &out)
        .into_iter()
        .map(|(code, at, primary, notes)| {
            (
                code,
                at.to_owned(),
                primary.to_owned(),
                notes.into_iter().map(str::to_owned).collect(),
            )
        })
        .collect();
    (
        found,
        blocks_where(only_structure(&out), |id| id.ends_with("_log")),
    )
}

/// The findings `members` raises in [`hall`], which must lay no log.
fn refused(members: &str) -> Vec<Owned> {
    let (found, logs) = lowering(&hall(members));
    assert_eq!(logs, [], "{members}");
    found
}

/// A finding as [`Owned`] holds it, from borrowed parts.
fn owned(code: DiagnosticCode, at: &str, primary: &str, notes: &[&str]) -> Owned {
    (
        code,
        at.to_owned(),
        primary.to_owned(),
        notes.iter().map(|note| (*note).to_owned()).collect(),
    )
}

/// `id[axis=axis]` in each cell of `cells`.
fn logs(id: &str, axis: &str, cells: &[(u32, u32, u32)]) -> Vec<Placed> {
    cells
        .iter()
        .map(|&cell| (cell, format!("minecraft:{id}[axis={axis}]")))
        .collect()
}

#[test]
fn a_beam_on_a_wall_top_lies_along_it_between_its_corners() {
    assert_eq!(
        laid("beam kind=pillar mat_slot=frame at=front.top"),
        logs(
            "spruce_log",
            "x",
            &[(1, 3, 4), (2, 3, 4), (3, 3, 4), (4, 3, 4), (5, 3, 4)]
        ),
    );
    assert_eq!(
        laid("beam kind=pillar mat_slot=frame at=left.top"),
        logs("spruce_log", "z", &[(0, 3, 1), (0, 3, 2), (0, 3, 3)]),
    );
}

#[test]
fn a_beam_without_a_material_is_built_from_the_default_beam() {
    assert_eq!(
        laid("beam kind=pillar at=right.top"),
        logs("oak_log", "z", &[(6, 3, 1), (6, 3, 2), (6, 3, 3)]),
    );
}

#[test]
fn a_written_axis_replaces_the_one_along_the_wall() {
    assert_eq!(
        laid("beam kind=pillar at=back.top axis=y"),
        logs(
            "oak_log",
            "y",
            &[(1, 3, 0), (2, 3, 0), (3, 3, 0), (4, 3, 0), (5, 3, 0)]
        ),
    );
}

#[test]
fn a_beam_under_a_level_tops_the_walls_that_level_starts() {
    let members = "level y=3\n    walls mat_slot=wall height=2\n    \
                   beam kind=pillar at=left.top";
    assert_eq!(
        laid(members),
        logs("oak_log", "z", &[(0, 5, 1), (0, 5, 2), (0, 5, 3)]),
    );
}

#[test]
fn a_beam_outside_a_level_tops_its_own_walls_where_the_levels_walls_touch_them() {
    let members = "level y=3\n    walls mat_slot=wall height=2\n  beam kind=pillar at=left.top";
    assert_eq!(
        laid(members),
        logs("oak_log", "z", &[(0, 3, 1), (0, 3, 2), (0, 3, 3)]),
    );
}

#[test]
fn a_beam_over_a_door_lies_on_the_row_above_it_one_column_past_each_side() {
    let members = "walls mat_slot=wall height=4\n  door id=entry side=front at=center\n  \
                   beam kind=pillar at=entry.top";
    assert_eq!(
        laid(members),
        logs("oak_log", "x", &[(2, 3, 4), (3, 3, 4), (4, 3, 4)]),
    );
}

#[test]
fn a_beam_over_a_window_lies_over_every_opening_its_line_cuts() {
    // Along the back wall `offset=` grows toward the west, so `u` is `6 - x`.
    let series = "window id=w side=back y=1 offset=1 size=1x1 repeat=2 step=2\n  \
                  beam kind=pillar at=w.top";
    assert_eq!(
        laid(series),
        logs(
            "oak_log",
            "x",
            &[(2, 2, 0), (3, 2, 0), (4, 2, 0), (5, 2, 0)]
        ),
    );
    let mirrored = "window id=w side=front y=1 offset=1 size=1x1 sym=true\n  \
                    beam kind=pillar at=w.top";
    assert_eq!(
        laid(mirrored),
        logs(
            "oak_log",
            "x",
            &[(1, 2, 4), (2, 2, 4), (4, 2, 4), (5, 2, 4)]
        ),
    );
}

#[test]
fn a_beam_whose_kind_or_anchor_names_nothing_is_refused() {
    const ANCHORS: &str = "write `at=<side>.top` to lay the beam along the top row of a wall, \
                           where `<side>` is front, back, left or right, or `at=<id>.top` to lay \
                           it over the `door` or `window` with that `id=`";
    const KINDS: &str =
        "`kind=` takes `pillar`, a run of pillar blocks such as logs laid on their side";
    for (members, at, primary, notes) in [
        (
            "beam at=front.top",
            "beam at=front.top",
            "beam has no `kind=`, so nothing says what it builds",
            vec![KINDS],
        ),
        (
            "beam kind=piller at=front.top",
            "beam kind=piller at=front.top",
            "unknown beam `kind=piller`",
            vec![KINDS, "did you mean `kind=pillar`?"],
        ),
        (
            "beam kind=pillar",
            "beam kind=pillar",
            "beam has no `at=`, so nothing says where it lies",
            vec![ANCHORS],
        ),
        (
            "beam kind=pillar at=front",
            "beam kind=pillar at=front",
            "beam `at=` must be a two-segment reference, not identifier `front`",
            vec![ANCHORS],
        ),
        (
            "beam kind=3 at=front.top",
            "beam kind=3 at=front.top",
            "beam `kind=` must be `pillar`, not integer `3`",
            vec![KINDS],
        ),
        (
            "beam kind=pillar at=front.top.x",
            "beam kind=pillar at=front.top.x",
            "beam `at=front.top.x` has 3 segments, and a beam's anchor has two",
            vec![ANCHORS],
        ),
        (
            "beam kind=pillar at=front.bottom",
            "beam kind=pillar at=front.bottom",
            "beam `at=front.bottom` does not end in `.top`, the top a beam rests on",
            vec![ANCHORS, "did you mean `at=front.top`?"],
        ),
        (
            "beam kind=pillar at=frnt.bottom",
            "beam kind=pillar at=frnt.bottom",
            "beam `at=frnt.bottom` does not end in `.top`, the top a beam rests on",
            vec![ANCHORS, "did you mean `at=front.top`?"],
        ),
        (
            "beam kind=pillar at=porch.bottom",
            "beam kind=pillar at=porch.bottom",
            "beam `at=porch.bottom` does not end in `.top`, the top a beam rests on",
            vec![ANCHORS],
        ),
        (
            "door id=entry side=front at=center\n  beam kind=pillar at=entry.bottom",
            "beam kind=pillar at=entry.bottom",
            "beam `at=entry.bottom` does not end in `.top`, the top a beam rests on",
            vec![ANCHORS, "did you mean `at=entry.top`?"],
        ),
        (
            "door id=entry side=front at=center\n  beam kind=pillar at=entr.top",
            "beam kind=pillar at=entr.top",
            "beam `at=entr.top`: `entr` is no wall side, and no `door` or `window` in this body \
             has `id=entr`",
            vec![ANCHORS, "did you mean `at=entry.top`?"],
        ),
        (
            "beam kind=pillar at=front.top axis=w",
            "beam kind=pillar at=front.top axis=w",
            "unknown beam `axis=w`",
            vec!["`axis=` takes x or y or z", "did you mean `axis=x`?"],
        ),
    ] {
        assert_eq!(
            refused(members),
            [(
                DiagnosticCode::InvalidMember,
                at.to_owned(),
                primary.to_owned(),
                notes.into_iter().map(str::to_owned).collect::<Vec<_>>(),
            )],
            "{members}",
        );
    }
}

/// The note on a beam whose level has no walls of its own to rest on.
const OWN_WALLS: &str = "a beam along a wall's top rests on the walls of its own level; add a \
                         `walls` member with a positive `height=` beside the beam, or move the \
                         beam beside the walls it rests on";

#[test]
fn a_beam_with_no_wall_row_to_rest_on_is_refused() {
    let src = format!(
        "{THEME}struct s size=7x5\n  floor mat_slot=wall\n  beam kind=pillar at=front.top\n"
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::InvalidMember,
            "beam kind=pillar at=front.top",
            "beam `at=front.top` has no wall to rest on: no `walls` member with a positive \
             `height=` rises from the struct's base",
            vec![OWN_WALLS],
        )],
    );
    assert_eq!(
        refused("level y=3\n    beam kind=pillar at=front.top"),
        [owned(
            DiagnosticCode::InvalidMember,
            "beam kind=pillar at=front.top",
            "beam `at=front.top` has no wall to rest on: no `walls` member with a positive \
             `height=` rises from `level y=3`",
            &[OWN_WALLS],
        )],
        "the walls under the level below do not reach up to hold it",
    );
    assert_eq!(
        refused("window id=w side=front y=3 offset=2 size=1x1\n  beam kind=pillar at=w.top"),
        [(
            DiagnosticCode::InvalidMember,
            "beam kind=pillar at=w.top".to_owned(),
            "beam `at=w.top` would lie on row 4, above the window, and no `walls` member \
             reaches that row"
                .to_owned(),
            vec!["raise the `walls` to reach row 4, or lower the window".to_owned()],
        )],
    );
}

#[test]
fn a_beam_on_walls_that_paint_nothing_defers() {
    let src =
        format!("{THEME}struct s size=7x5\n  walls height=3\n  beam kind=pillar at=front.top\n");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::DeferredMember,
            "beam kind=pillar at=front.top",
            "beam `at=front.top` rests on row 3 of a `walls` member that paints nothing",
            vec!["the beam is laid once that member's material resolves to a block"],
        )],
    );
    assert_eq!(
        refused("walls height=5\n  beam kind=pillar at=front.top"),
        [owned(
            DiagnosticCode::DeferredMember,
            "beam kind=pillar at=front.top",
            "beam `at=front.top` rests on row 5 of a `walls` member that paints nothing",
            &["the beam is laid once that member's material resolves to a block"],
        )],
        "the painting walls stop below the row the beam's level is written to reach",
    );
}

/// The deferral of a beam at `at=<id>.top` over the `keyword` with that id, which is not placed.
fn over_unplaced(id: &str, keyword: &str) -> Owned {
    owned(
        DiagnosticCode::DeferredMember,
        &format!("beam kind=pillar at={id}.top"),
        &format!(
            "beam `at={id}.top` rests on a {keyword} that is not placed, so nothing says where \
             its top is"
        ),
        &[&format!(
            "the beam is laid once the {keyword} is; see the finding on the {keyword}'s line"
        )],
    )
}

/// An opening's line, and the code, primary and notes of the finding that keeps it unplaced.
type Unplaced<'a> = (&'a str, DiagnosticCode, &'a str, &'a [&'a str]);

/// Assert that a beam at `at=d.top` over each `keyword` line is deferred after the finding on
/// that line, and lays nothing.
fn defers_over_each(keyword: &str, openings: &[Unplaced<'_>]) {
    for &(opening, code, primary, notes) in openings {
        let line = opening.lines().last().map_or(opening, str::trim_start);
        assert_eq!(
            refused(&format!("{opening}\n  beam kind=pillar at=d.top")),
            [
                owned(code, line, primary, notes),
                over_unplaced("d", keyword)
            ],
            "{opening}",
        );
    }
}

#[test]
fn a_beam_over_a_door_its_own_line_does_not_place_defers_and_lays_nothing() {
    defers_over_each(
        "door",
        &[
            (
                "door id=d side=sideways at=center",
                DiagnosticCode::InvalidMember,
                "unknown `side=sideways` (expected one of front, back, left, right)",
                &[],
            ),
            (
                "door id=d side=front at=middle",
                DiagnosticCode::InvalidMember,
                "door has `at=middle`, which is not one of center, left, right — use \
                 `at=center | left | right`",
                &[],
            ),
            (
                "door id=d side=front at=center hinge=sideways",
                DiagnosticCode::InvalidMember,
                "unknown door `hinge=sideways`",
                &["`hinge=` takes one of left, right"],
            ),
            (
                "level y=2\n    door id=d side=front at=center",
                DiagnosticCode::InvalidMember,
                "door stands at y=3..=4, and the wall course holding y=3 ends there (the walls \
                 occupy y=1..=3)",
                &[
                    "a door block is two rows tall; raise the `walls` so their course holds the \
                     row above the one the door opens at",
                ],
            ),
        ],
    );
}

#[test]
fn a_beam_over_a_window_its_own_line_does_not_place_defers_and_lays_nothing() {
    defers_over_each(
        "window",
        &[
            (
                "window id=d side=front offset=1 size=1x1",
                DiagnosticCode::InvalidMember,
                "window has no `y=`",
                &[
                    "write `y=N`, where `N` is a non-negative integer: the window's lowest row, \
                     counted up from 0, the base row of the struct or of the window's `level`",
                ],
            ),
            (
                "window id=d side=front y=1 size=1x1 repeat=0",
                DiagnosticCode::InvalidMember,
                "window `repeat=0` would stamp no instances; drop the window instead",
                &[],
            ),
            (
                "window id=d side=front y=1 size=1x1 repeat=2",
                DiagnosticCode::InvalidMember,
                "window `repeat=` requires a positive `step=` so instances do not overlap",
                &[],
            ),
            (
                "window id=d side=front y=1 offset=6 size=2x1",
                DiagnosticCode::InvalidMember,
                "window extends beyond the `front` wall (offset=6 size=2x1 repeat=1 step=0, wall \
                 length=7)",
                &[
                    "the window ends at `offset + size.w`, which must be at most 7; lower \
                     `offset=` or narrow `size=`",
                ],
            ),
            (
                "window id=d side=front y=2 offset=1 size=1x3",
                DiagnosticCode::InvalidMember,
                "window rows y=2..=4 are not all inside one wall course (size=1x3; the walls \
                 occupy y=1..=3)",
                &[
                    "move the window with `y=`, shorten it with `size=`, or raise the walls' \
                     `height=`, so one course holds every row it cuts",
                ],
            ),
            (
                "window id=d side=front y=1 size=1x1 repeat=2 step=2 sym=true",
                DiagnosticCode::DeferredMember,
                "window with both `repeat=` and `sym=true` is not yet supported",
                &[DRAWN_ROLES],
            ),
        ],
    );
}

#[test]
fn a_beam_over_a_window_whose_material_binds_no_block_defers_and_lays_nothing() {
    assert_eq!(
        refused("window id=d side=front y=1 size=1x1 mat_slot=nope\n  beam kind=pillar at=d.top"),
        [over_unplaced("d", "window")],
    );
}

#[test]
fn a_beam_over_an_opening_on_a_wall_with_no_cell_between_its_corners_is_refused() {
    let src = format!(
        "{THEME}struct s size=2x5\n  walls mat_slot=wall height=4\n  \
         door id=d side=front at=center\n  beam kind=pillar at=d.top\n  \
         beam kind=pillar at=front.top\n"
    );
    let corners = "a beam runs between the corners of the wall it lies on, so that wall needs a \
                   length of at least 3";
    assert_eq!(
        lowering(&src),
        (
            vec![
                owned(
                    DiagnosticCode::InvalidMember,
                    "beam kind=pillar at=d.top",
                    "beam `at=d.top`: the front wall is 2 long, so it has no cell between its \
                     corners",
                    &[corners],
                ),
                owned(
                    DiagnosticCode::InvalidMember,
                    "beam kind=pillar at=front.top",
                    "beam `at=front.top`: the front wall is 2 long, so it has no cell between \
                     its corners",
                    &[corners],
                ),
            ],
            Vec::new(),
        ),
    );
}

#[test]
fn an_axis_of_the_wrong_kind_is_ignored_and_the_beam_lies_along_its_wall() {
    let src = hall("beam kind=pillar at=front.top axis=3");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::IgnoredArgument,
            "3",
            "`axis=` must be `x`, `y` or `z`, not integer `3`; the value was ignored",
            vec!["the beam lies along its wall, as with no `axis=`"],
        )],
    );
    assert_eq!(
        blocks_where(only_structure(&out), |id| id.ends_with("_log")),
        logs(
            "oak_log",
            "x",
            &[(1, 3, 4), (2, 3, 4), (3, 3, 4), (4, 3, 4), (5, 3, 4)]
        ),
    );
}

#[test]
fn a_beam_over_an_opening_under_a_level_lies_above_it_wherever_the_beam_is_written() {
    let upper = "level y=3\n    walls mat_slot=wall height=4\n    \
                 door id=d side=front at=center\n    window id=w side=back y=1 offset=3 size=1x1";
    let mut over_both = [
        logs("oak_log", "x", &[(2, 6, 4), (3, 6, 4), (4, 6, 4)]),
        logs("oak_log", "x", &[(2, 5, 0), (3, 5, 0), (4, 5, 0)]),
    ]
    .concat();
    over_both.sort();
    assert_eq!(
        laid(&format!(
            "{upper}\n  beam kind=pillar at=d.top\n  beam kind=pillar at=w.top"
        )),
        over_both,
    );
    let src = format!(
        "{THEME}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=4\n  \
         door id=d side=front at=center\n  level y=3\n    beam kind=pillar at=d.top\n"
    );
    assert_eq!(
        lowering(&src),
        (
            Vec::new(),
            logs("oak_log", "x", &[(2, 3, 4), (3, 3, 4), (4, 3, 4)]),
        ),
        "the beam's own level does not move it",
    );
}

#[test]
fn a_side_name_is_read_before_an_opening_id() {
    assert_eq!(
        laid("door id=front side=back at=center\n  beam kind=pillar at=front.top"),
        logs(
            "oak_log",
            "x",
            &[(1, 3, 4), (2, 3, 4), (3, 3, 4), (4, 3, 4), (5, 3, 4)]
        ),
    );
}

#[test]
fn a_window_cut_through_a_beam_on_the_wall_top_takes_its_cell() {
    assert_eq!(
        laid("beam kind=pillar at=front.top\n  window side=front y=3 offset=2 size=1x1"),
        logs(
            "oak_log",
            "x",
            &[(1, 3, 4), (3, 3, 4), (4, 3, 4), (5, 3, 4)]
        ),
    );
}

#[test]
fn a_beam_over_a_window_whose_mirror_is_skipped_covers_the_window_alone() {
    let src =
        hall("window id=w side=front y=1 offset=2 size=3x1 sym=true\n  beam kind=pillar at=w.top");
    let (found, logs_laid) = lowering(&src);
    assert_eq!(
        found
            .iter()
            .map(|(code, at, _, _)| (*code, at.as_str()))
            .collect::<Vec<_>>(),
        [(
            DiagnosticCode::DeferredMember,
            "window id=w side=front y=1 offset=2 size=3x1 sym=true"
        )],
        "the window's own finding on its skipped mirror, and none on the beam",
    );
    assert_eq!(
        logs_laid,
        logs(
            "oak_log",
            "x",
            &[(1, 2, 4), (2, 2, 4), (3, 2, 4), (4, 2, 4), (5, 2, 4)]
        ),
    );
}

#[test]
fn a_beam_under_an_overhanging_roof_lies_on_the_wall_the_overhang_moves_in() {
    let src = format!(
        "{THEME}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         roof kind=flat mat_slot=wall overhang=1\n  beam kind=pillar at=front.top\n"
    );
    assert_eq!(
        lowering(&src),
        (
            Vec::new(),
            logs(
                "oak_log",
                "x",
                &[(2, 3, 5), (3, 3, 5), (4, 3, 5), (5, 3, 5), (6, 3, 5)]
            ),
        ),
    );
}
