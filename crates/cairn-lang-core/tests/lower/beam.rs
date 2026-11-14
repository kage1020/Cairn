//! A `beam` lays pillar blocks on their side along the top row of a wall, or over a `door` or
//! `window`, along its wall unless its line turns it (`spec/blockstate` "Derive by default,
//! promote on override").

use cairn_lang_core::DiagnosticCode;

use crate::common::{Finding, blocks_where, findings, lowered, only_structure};

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

/// The findings `members` raises in [`hall`].
fn refused(members: &str) -> Vec<(DiagnosticCode, String, String, Vec<String>)> {
    let src = hall(members);
    let out = lowered(&src);
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
            "beam kind=pillar at=front.bottom",
            "beam kind=pillar at=front.bottom",
            "beam `at=front.bottom` does not end in `.top`, the top a beam rests on",
            vec![ANCHORS, "did you mean `at=front.top`?"],
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
            "beam `at=front.top` has no wall to rest on: no `walls` member reaches row 1",
            vec!["add a `walls` member that reaches this row to the struct's body"],
        )],
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
fn a_beam_on_walls_that_paint_nothing_or_an_opening_that_is_not_placed_defers() {
    let src =
        format!("{THEME}struct s size=7x5\n  walls height=3\n  beam kind=pillar at=front.top\n");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::DeferredMember,
            "beam kind=pillar at=front.top",
            "beam `at=front.top` rests on row 1 of a `walls` member that paints nothing",
            vec!["the beam is laid once that member's material resolves to a block"],
        )],
    );
    let found = refused("door id=d side=sideways at=center\n  beam kind=pillar at=d.top");
    assert_eq!(
        found
            .iter()
            .map(|(code, at, primary, notes)| (*code, at.as_str(), primary.as_str(), notes.clone()))
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::InvalidMember,
                "door id=d side=sideways at=center",
                "unknown `side=sideways` (expected one of front, back, left, right)",
                Vec::new(),
            ),
            (
                DiagnosticCode::DeferredMember,
                "beam kind=pillar at=d.top",
                "beam `at=d.top` rests on a door whose line does not place it, so nothing says \
                 where its top is",
                vec![
                    "the beam is laid once the door's line reads; see the finding on it".to_owned()
                ],
            ),
        ],
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
