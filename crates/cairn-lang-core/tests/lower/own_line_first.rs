//! A member whose own line is wrong is refused, even when a member it needs builds nothing or the
//! pass does not lower what its line writes yet (`spec/lint` "Lowering").

use cairn_lang_core::DiagnosticCode;

use crate::common::{Finding, findings, lowered};

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n  slot roof -> @spruce_stairs\n\n";

/// A 5x5 struct with no theme, so its walls up to row 3 paint nothing, `lines` after them.
fn unpainted(lines: &str) -> String {
    format!("struct s size=5x5\n  walls mat_slot=wall height=3\n  {lines}\n")
}

/// The findings of `found` after the scope's `W_NO_THEME_BOUND`, which leaves its walls unpainted.
fn after_no_theme<'a, 'f>(found: &'f [Finding<'a>]) -> &'f [Finding<'a>] {
    assert_eq!(found[0].0, DiagnosticCode::NoThemeBound, "{found:#?}");
    &found[1..]
}

#[test]
fn a_door_above_every_course_is_refused_beside_walls_that_paint_nothing() {
    let src = unpainted("level y=3\n    door side=front at=center");
    let out = lowered(&src);
    assert_eq!(
        after_no_theme(&findings(&src, &out)),
        [(
            DiagnosticCode::InvalidMember,
            "door side=front at=center",
            "door opens at y=4, which is not inside any wall course (the walls occupy y=1..=3)",
            vec![
                "a door opens one row above the base of its `level`, or of the body outside one; \
                 move it under a `level` whose walls hold that row, or add walls there"
            ],
        )],
    );
}

#[test]
fn a_door_whose_at_names_no_anchor_is_refused_beside_walls_that_paint_nothing() {
    let src = unpainted("door side=front at=middle");
    let out = lowered(&src);
    assert_eq!(
        after_no_theme(&findings(&src, &out)),
        [(
            DiagnosticCode::InvalidMember,
            "door side=front at=middle",
            "door has `at=middle`, which is not one of center, left, right — use `at=center | \
             left | right`",
            vec![],
        )],
    );
}

#[test]
fn a_window_past_its_wall_is_refused_beside_walls_that_paint_nothing() {
    let src = unpainted("window side=front offset=3 y=1 size=3x1");
    let out = lowered(&src);
    assert_eq!(
        after_no_theme(&findings(&src, &out)),
        [(
            DiagnosticCode::InvalidMember,
            "window side=front offset=3 y=1 size=3x1",
            "window extends beyond the `front` wall (offset=3 size=3x1 repeat=1 step=0, wall \
             length=5)",
            vec![
                "the window ends at `offset + size.w`, which must be at most 5; lower `offset=` \
                 or narrow `size=`"
            ],
        )],
    );
}

#[test]
fn a_window_outside_every_course_is_refused_beside_walls_that_paint_nothing() {
    let src = unpainted("window side=front offset=1 y=3 size=1x2");
    let out = lowered(&src);
    assert_eq!(
        after_no_theme(&findings(&src, &out)),
        [(
            DiagnosticCode::InvalidMember,
            "window side=front offset=1 y=3 size=1x2",
            "window rows y=3..=4 are not all inside one wall course (size=1x2; the walls occupy \
             y=1..=3)",
            vec![
                "move the window with `y=`, shorten it with `size=`, or raise the walls' \
                 `height=`, so one course holds every row it cuts"
            ],
        )],
    );
}

#[test]
fn an_eave_stair_wrong_in_itself_is_refused_beside_a_roof_that_draws_nothing() {
    let roof = "roof overhang=2 mat_slot=roof";
    let no_kind = (
        DiagnosticCode::InvalidMember,
        roof,
        "missing `kind=` (expected one of gable, shed, hip, flat)",
        vec![],
    );
    let struct_with = |stair: &str| {
        format!("{THEME}struct s size=5x5\n  walls mat_slot=wall height=3\n  {roof}\n  {stair}\n")
    };
    let stair = "stair kind=stairs mat_slot=roof side=front y=99";
    let src = struct_with(stair);
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            no_kind.clone(),
            (
                DiagnosticCode::InvalidMember,
                stair,
                "stair y=99 does not fit in the struct (dims.y=4)",
                vec![
                    "the struct's top row is 3",
                    "a `roof` whose `kind=` or `slope_to=` does not read may raise it to row 12",
                    "lower the stair or its `level`, or raise the `walls` to reach its row",
                ],
            ),
        ],
    );
    let stair = "stair kind=stairs mat_slot=roof side=front half=middle";
    let src = struct_with(stair);
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            no_kind,
            (
                DiagnosticCode::InvalidMember,
                stair,
                "unknown stair `half=middle`",
                vec!["`half=` takes one of top, bottom"],
            ),
        ],
    );
}

#[test]
fn a_window_series_wrong_in_itself_is_refused_before_its_mirror_is_deferred() {
    let past_the_wall = |window: &'static str| {
        (
            DiagnosticCode::InvalidMember,
            window,
            "window extends beyond the `front` wall (offset=7 size=5x5 repeat=2 step=3, wall \
             length=9)",
            vec![
                "the last instance ends at `offset + step * (repeat - 1) + size.w`, which must be \
                 at most 9; lower `offset=`, `step=` or `repeat=`, or narrow `size=`",
            ],
        )
    };
    let struct_with = |window: &str| {
        format!("{THEME}struct s size=9x7\n  walls mat_slot=wall height=3\n  {window}\n")
    };
    let window = "window side=front offset=7 y=2 size=5x5 repeat=2 step=3 sym=true";
    let src = struct_with(window);
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), [past_the_wall(window)]);
    let window = "window side=front offset=7 y=2 size=5x5 repeat=2 step=3 sym=yes";
    let src = struct_with(window);
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            past_the_wall(window),
            (
                DiagnosticCode::IgnoredArgument,
                "yes",
                "`sym=` must be `true` or `false`, not identifier `yes`; the value was ignored",
                vec!["this window is not cut either way — see the finding on the same line"],
            ),
        ],
    );
}

#[test]
fn a_roof_under_a_raised_level_is_refused_for_its_shape_before_it_is_deferred() {
    let src = format!(
        "{THEME}struct s size=9x7\n  walls mat_slot=wall height=3\n  level y=1\n    \
         roof kind=banana\n    roof kind=shed\n    roof kind=flat\n"
    );
    let out = lowered(&src);
    let found = findings(&src, &out);
    assert_eq!(
        found[..2],
        [
            (
                DiagnosticCode::InvalidMember,
                "roof kind=banana",
                "unknown roof `kind=banana` (expected one of gable, shed, hip, flat)",
                vec![],
            ),
            (
                DiagnosticCode::InvalidMember,
                "roof kind=shed",
                "shed roof requires `slope_to=` (one of front, back, left, right)",
                vec![],
            ),
        ],
    );
    assert_eq!(
        found[2..]
            .iter()
            .map(|&(code, line, primary, _)| (code, line, primary))
            .collect::<Vec<_>>(),
        [(
            DiagnosticCode::DeferredMember,
            "roof kind=flat",
            "level-scoped `roof` is not yet supported",
        )],
    );
}
