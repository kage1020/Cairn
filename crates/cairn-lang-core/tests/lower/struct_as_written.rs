//! A member's own line is judged against the struct as written: every member at the size its line
//! asks for, whether or not it builds (`spec/lint` "Lowering").

use cairn_lang_core::DiagnosticCode;

use crate::common::{Finding, findings, lowered};

const THEME: &str = "theme t:\n  \
                     slot wall -> @cobblestone\n  \
                     slot roof -> @spruce_stairs\n  \
                     slot glass -> @glass_pane\n\n";

/// [`THEME`] and a struct of `size` with walls up to row 3, `lines` after them.
fn walled(size: &str, lines: &str) -> String {
    format!("{THEME}struct s size={size}\n  walls mat_slot=wall height=3\n  {lines}\n")
}

/// Each finding of `found` without its notes.
fn primaries<'a>(found: &[Finding<'a>]) -> Vec<(DiagnosticCode, &'a str, &'a str)> {
    found
        .iter()
        .map(|&(code, line, primary, _)| (code, line, primary))
        .collect()
}

#[test]
fn an_eave_stair_at_the_rise_of_a_roof_under_a_raised_level_waits_on_that_roof() {
    let roof = "roof kind=gable mat_slot=roof overhang=1";
    let stair = "stair kind=stairs mat_slot=roof side=front y=4";
    let src = walled("9x9", &format!("level id=up y=1\n    {roof}\n  {stair}"));
    let out = lowered(&src);
    let found = findings(&src, &out);
    assert_eq!(
        primaries(&found),
        [
            (
                DiagnosticCode::DeferredMember,
                roof,
                "level-scoped `roof` is not yet supported",
            ),
            (
                DiagnosticCode::DeferredMember,
                stair,
                "eave `stair` has no overhang to sit in: the roof written with an `overhang=` \
                 draws nothing",
            ),
        ],
    );
    assert_eq!(found[1].3, ["the stair is built once that roof draws"]);
}

#[test]
fn a_member_beside_a_roof_under_a_raised_level_waits_on_the_rows_that_roof_would_add() {
    let stair = "stair kind=stairs side=front y=5 mat_slot=roof";
    let plate = "pressure_plate at=front.outside y=5";
    let src = walled(
        "5x5",
        &format!(
            "level y=1\n    roof kind=gable overhang=1 mat_slot=roof\n  \
             roof kind=flat mat_slot=roof overhang=1\n  {stair}\n  {plate}"
        ),
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out)[1..],
        [
            (
                DiagnosticCode::DeferredMember,
                stair,
                "stair y=5 does not fit in the struct (dims.y=5)",
                vec![
                    "a `roof` that would rise to this row draws nothing; the stair is built once \
                      that roof draws"
                ],
            ),
            (
                DiagnosticCode::DeferredMember,
                plate,
                "pressure_plate y=5 does not fit in the struct (dims.y=5)",
                vec![
                    "a `roof` that would rise to this row draws nothing; the plate fits once \
                      that roof draws"
                ],
            ),
        ],
    );
}

#[test]
fn an_eave_stair_beside_a_roof_whose_kind_does_not_read_waits_up_to_the_tallest_roof() {
    let roof = "roof kind=gabel mat_slot=roof overhang=1";
    let within = "stair kind=stairs mat_slot=roof side=front y=4";
    let above = "stair kind=stairs mat_slot=roof side=back y=99";
    let src = walled("9x9", &format!("{roof}\n  {within}\n  {above}"));
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::InvalidMember,
                roof,
                "unknown roof `kind=gabel` (expected one of gable, shed, hip, flat)",
                vec![],
            ),
            (
                DiagnosticCode::DeferredMember,
                within,
                "stair y=4 does not fit in the struct (dims.y=4)",
                vec![
                    "a `roof` whose `kind=` or `slope_to=` does not read may rise to this row; \
                     the stair is built once that roof draws",
                ],
            ),
            (
                DiagnosticCode::InvalidMember,
                above,
                "stair y=99 does not fit in the struct (dims.y=4)",
                vec![
                    "the struct's top row is 3",
                    "a `roof` whose `kind=` or `slope_to=` does not read may raise it to row 14",
                    "lower the stair or its `level`, or raise the `walls` to reach its row",
                ],
            ),
        ],
    );
}

#[test]
fn an_eave_stair_above_walls_that_paint_nothing_names_the_rows_they_would_hold() {
    let above = "stair kind=stairs side=front y=20";
    let within = "stair kind=stairs side=back y=6";
    let src = format!(
        "struct hut size=5x5\n  walls mat_slot=wall height=3\n  \
         roof kind=gable overhang=1 mat_slot=roof\n  {above}\n  {within}\n"
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out)[2..],
        [
            (
                DiagnosticCode::InvalidMember,
                above,
                "stair y=20 does not fit in the struct (dims.y=5)",
                vec![
                    "the struct's top row is 7",
                    "rows 5 to 7 wait on a `walls` member that paints nothing",
                    "lower the stair or its `level`, or raise the `walls` to reach its row",
                ],
            ),
            (
                DiagnosticCode::DeferredMember,
                within,
                "stair y=6 does not fit in the struct (dims.y=5)",
                vec![
                    "a `walls` member that would reach this row paints nothing; the stair is \
                     built once that member's material resolves to a block",
                ],
            ),
        ],
    );
}

#[test]
fn an_unreadable_overhang_counts_as_none() {
    let stair = "stair kind=stairs side=front y=3 mat_slot=roof";
    let src = walled(
        "5x5",
        &format!("roof kind=gable overhang=\"1\" mat_slot=roof\n  {stair}"),
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::IgnoredArgument,
                "\"1\"",
                "`overhang=` must be a non-negative integer that fits in u32, not string `\"1\"`; \
                 the value was ignored",
                vec!["the roof is drawn flush with the wall line"],
            ),
            (
                DiagnosticCode::InvalidMember,
                stair,
                "eave `stair` has no overhang to sit in: no roof on this struct has an \
                 `overhang=` of at least 1",
                vec![
                    "an eave band sits one voxel outside the wall, in the ring a roof's \
                     `overhang=` adds; give a `roof` in the struct's body an `overhang=` of at \
                     least 1",
                ],
            ),
        ],
    );
}

#[test]
fn a_member_beside_walls_under_a_level_with_no_y_waits_on_that_level() {
    let door = "door side=front at=center";
    let window = "window side=back offset=1 y=1 size=2x2 mat_slot=glass";
    let plate = "pressure_plate at=inside.front offset=2 y=5";
    let stair = "stair kind=stairs side=front y=5 mat_slot=roof";
    let src = format!(
        "{THEME}struct s size=7x7\n  {door}\n  {window}\n  {plate}\n  {stair}\n  \
         roof kind=flat overhang=1 mat_slot=roof\n\n  level id=l\n    walls mat_slot=wall height=3\n"
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::DeferredMember,
                door,
                "door has no wall to cut into: every `walls` member on this struct sits under a \
                 `level` that is not placed",
                vec![
                    "a `level` that is not placed holds a `walls` member that may reach the row \
                     the door opens at; the door is cut once that `level` is placed",
                ],
            ),
            (
                DiagnosticCode::DeferredMember,
                window,
                "window at y=1 size=2x2 has no wall to cut into: every `walls` member on this \
                 struct sits under a `level` that is not placed",
                vec![
                    "a `level` that is not placed holds a `walls` member that may reach the \
                     window's rows; the window is cut once that `level` is placed",
                ],
            ),
            (
                DiagnosticCode::DeferredMember,
                plate,
                "pressure_plate y=5 does not fit in the struct (dims.y=2)",
                vec![
                    "a `level` that is not placed holds a `walls` member that may reach this row; \
                     the plate fits once that `level` is placed",
                ],
            ),
            (
                DiagnosticCode::DeferredMember,
                stair,
                "stair y=5 does not fit in the struct (dims.y=2)",
                vec![
                    "a `level` that is not placed holds a `walls` member that may reach this row; \
                     the stair is built once that `level` is placed",
                ],
            ),
        ],
    );
}

#[test]
fn an_actuator_patch_binding_a_door_under_a_level_with_no_y_waits_on_that_level() {
    let patch = "door[id=up] opened_by=sig.open";
    let src = walled(
        "7x7",
        &format!("{patch}\n\n  level id=l\n    door id=up side=front at=center"),
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::DeferredMember,
            patch,
            "door actuator patch binds a `door` under a `level` that is not placed, so no door \
             is built for it to open",
            vec!["the door is built once that `level` is placed"],
        )],
    );
}

#[test]
fn an_opening_beside_walls_with_no_positive_height_is_offered_one() {
    let walls = "walls mat_slot=wall height=0";
    let door = "door side=front at=center";
    let window = "window side=back offset=1 y=1 size=2x2";
    let src = format!("{THEME}struct s size=5x5\n  {walls}\n  {door}\n  {window}\n");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::InvalidMember,
                walls,
                "walls `height=` must be a positive integer that fits in u32, not integer `0`",
                vec![
                    "write `height=N`, where `N` is a positive integer: the walls fill the `N` \
                     rows above the base row",
                ],
            ),
            (
                DiagnosticCode::InvalidMember,
                door,
                "door has no wall to cut into: this struct declares no `walls` with a positive \
                 `height=`",
                vec![
                    "give the `walls` a positive `height=`, or add a `walls` member whose course \
                     holds the row the door opens at",
                ],
            ),
            (
                DiagnosticCode::InvalidMember,
                window,
                "window at y=1 size=2x2 has no wall to cut into: this struct declares no `walls` \
                 with a positive `height=`",
                vec![
                    "give the `walls` a positive `height=`, or add a `walls` member whose course \
                     holds the window's rows",
                ],
            ),
        ],
    );
}
