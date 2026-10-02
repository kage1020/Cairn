//! A `key=` whose value is present but not readable is not the same as a
//! `key=` left off, and the build has to say which one it saw.
//!
//! `spec/lint` "Error vs warning" calls this an unreadable value. The pass
//! drops the value and uses the default, which is a `W_IGNORED_ARGUMENT`:
//! the rule forbids *silent* substitution. Five keys, read by three
//! members, used to treat a value of the wrong shape as if the key were
//! absent, building the default and reporting nothing:
//!
//! - window `sym=`, where anything but a bare `true` or `false` meant "no
//!   mirror";
//! - stair `facing=` / `half=` / `shape=`, where a quoted or numeric value
//!   became the default state;
//! - place `gap=`, where a non-integer became `0`.
//!
//! The tests for a member that is built pin both halves of that contract:
//! the build equals the build of the same line with the key left off, so
//! the default really is what was used, and exactly one finding names the
//! key and the value that was written. A member that is refused reports
//! the unreadable value all the same, with a note saying it is not built
//! either way, so the author learns about both in one compile.
//!
//! Separately, an origin that works out past `i32` used to saturate onto
//! the range's edge, so two `place` rows could land on one coordinate. It
//! is refused now, as is a body whose far edge is past the range, and a
//! refused row's body, or that of a row placed relative to a refused one,
//! still reports what is wrong with its `def` and theme.

use cairn_lang_core::block_array::BlockArrayIr;

mod common;
use common::{lowered, only_structure};

/// `(code, primary)` for every finding, in report order.
fn findings(ir: &BlockArrayIr) -> Vec<(&'static str, &str)> {
    ir.diagnostics
        .iter()
        .map(|d| (d.code.as_str(), d.primary.as_str()))
        .collect()
}

/// The source text finding `i` underlines.
fn underlined<'s>(source: &'s str, ir: &BlockArrayIr, i: usize) -> &'s str {
    &source[ir.diagnostics[i].span.clone()]
}

// --- window `sym=` ---------------------------------------------------------

/// A window at `offset=1` on a 7-wide front wall, so its mirror lands at
/// `offset=5` and a mirrored build differs from an unmirrored one.
fn window(sym: &str) -> String {
    format!(
        "struct s size=7x3\n  \
         walls  mat_slot=wall height=3\n  \
         window side=front offset=1 y=1 size=1x1{sym} mat_slot=glass\n\n\
         theme t:\n  \
         slot wall  -> @cobblestone\n  \
         slot glass -> @glass_pane\n"
    )
}

#[test]
fn an_unreadable_sym_draws_the_unmirrored_window_and_says_so() {
    let unmirrored = lowered(&window(""));
    let mirrored = lowered(&window(" sym=true"));
    assert_eq!(findings(&unmirrored), vec![]);
    assert_eq!(findings(&mirrored), vec![]);
    // Guard: without this, the equality below would hold for a reader that
    // ignored `sym=` altogether.
    assert_ne!(only_structure(&unmirrored), only_structure(&mirrored));

    for (written, described) in [
        ("yes", "identifier `yes`"),
        ("\"true\"", "string `\"true\"`"),
        ("1", "integer `1`"),
    ] {
        let source = window(&format!(" sym={written}"));
        let ir = lowered(&source);
        assert_eq!(
            only_structure(&ir),
            only_structure(&unmirrored),
            "sym={written} must draw what a window with no `sym=` draws",
        );
        assert_eq!(
            findings(&ir),
            vec![(
                "W_IGNORED_ARGUMENT",
                format!("`sym=` must be `true` or `false`, not {described}; the value was ignored")
                    .as_str(),
            )],
            "sym={written}",
        );
        assert_eq!(
            ir.diagnostics[0].notes[0].message,
            "the window is drawn without its mirror, as `sym=false` would draw it",
        );
        // The value, not the line: the span `check::arguments` gives the
        // same code.
        assert_eq!(underlined(&source, &ir, 0), written);
    }
}

#[test]
fn an_unreadable_sym_on_a_window_that_is_not_cut_is_still_reported() {
    // Refused for its geometry: the refusal and the unreadable value are
    // both repairs, and both arrive in one compile. The note says the
    // window is not cut, rather than that it is drawn unmirrored.
    let src = window(" sym=yes").replace("offset=1", "offset=9");
    let ir = lowered(&src);
    let codes: Vec<&str> = findings(&ir).iter().map(|(code, _)| *code).collect();
    assert_eq!(
        codes,
        vec!["W_DEFERRED_MEMBER", "W_IGNORED_ARGUMENT"],
        "{:#?}",
        ir.diagnostics
    );
    assert_eq!(
        ir.diagnostics[1].notes[0].message,
        "this window is not cut either way — see the finding on the same line",
    );
}

#[test]
fn an_unreadable_sym_on_a_repeated_window_cuts_nothing() {
    // `sym=true` refuses a window with `repeat=`, and `sym=false` builds
    // it. Reading an unreadable `sym=` as `false` would build three
    // windows the source may have refused, so nothing is cut, as for
    // `sym=true`.
    let repeated = |sym: &str| window(&format!(" repeat=3 step=2{sym}"));
    let refused = lowered(&repeated(" sym=true"));
    let built = lowered(&repeated(" sym=false"));
    assert_ne!(
        only_structure(&refused),
        only_structure(&built),
        "guard: `sym=` decides whether this window is cut",
    );
    let ir = lowered(&repeated(" sym=yes"));
    assert_eq!(only_structure(&ir), only_structure(&refused));
    assert_eq!(
        findings(&ir),
        vec![
            (
                "W_DEFERRED_MEMBER",
                "window with `repeat=` is not cut while its `sym=` is unreadable: `sym=true` \
                 with `repeat=` is not yet supported, and `sym=false` is not what was written",
            ),
            (
                "W_IGNORED_ARGUMENT",
                "`sym=` must be `true` or `false`, not identifier `yes`; the value was ignored",
            ),
        ],
    );
    assert_eq!(
        ir.diagnostics[1].notes[0].message,
        "this window has `repeat=`, which builds only with `sym=false`, so it is not cut while \
         `sym=` is unreadable — see the finding on the same line",
    );
}

// --- `side=` / shed `slope_to=` ---------------------------------------------

#[test]
fn a_side_of_the_wrong_shape_is_refused_with_the_value_written() {
    // A quoted `"front"` names the side the author meant; the message has
    // to show the quotes, or it reads as if `front` itself were refused.
    let ir = lowered(&window("").replace("side=front", "side=\"front\""));
    assert_eq!(
        findings(&ir),
        vec![(
            "W_DEFERRED_MEMBER",
            "`side=` must be one of front, back, left, right, not string `\"front\"`",
        )],
    );
    let shed = lowered(
        "struct s size=5x5\n  \
         walls mat_slot=wall height=3\n  \
         roof  kind=shed slope_to=\"front\" mat_slot=roof\n\n\
         theme t:\n  \
         slot wall -> @cobblestone\n  \
         slot roof -> @oak_stairs\n",
    );
    assert_eq!(
        findings(&shed),
        vec![(
            "W_DEFERRED_MEMBER",
            "shed `slope_to=` must be one of front, back, left, right, not string `\"front\"`",
        )],
    );
}

// --- stair `facing=` / `half=` / `shape=` ----------------------------------

/// An eave band on the front wall of a struct whose roof draws a
/// one-block overhang, the least an eave needs to sit outside the wall.
fn stair(args: &str) -> String {
    format!(
        "struct s size=5x5\n  \
         walls mat_slot=wall height=3\n  \
         roof  kind=flat mat_slot=wall overhang=1\n  \
         stair kind=stairs side=front{args} mat_slot=eave\n\n\
         theme t:\n  \
         slot wall -> @cobblestone\n  \
         slot eave -> @oak_stairs\n"
    )
}

#[test]
fn an_unreadable_stair_state_builds_the_default_and_says_so() {
    let defaults = lowered(&stair(""));
    assert_eq!(findings(&defaults), vec![]);

    for (key, readable, expected, default) in [
        ("facing", "in", "`out` or `in`", "out"),
        ("half", "bottom", "`top` or `bottom`", "top"),
        (
            "shape",
            "outer_left",
            "`straight`, `outer_left`, or `outer_right`",
            "straight",
        ),
    ] {
        // Guard: the identifier the author meant changes the build, so the
        // equality below is not true of a reader that ignores `key=`.
        let meant = lowered(&stair(&format!(" {key}={readable}")));
        assert_eq!(findings(&meant), vec![]);
        assert_ne!(only_structure(&meant), only_structure(&defaults), "{key}");

        for (written, described) in [
            (
                format!("\"{readable}\""),
                format!("string `\"{readable}\"`"),
            ),
            ("2".to_owned(), "integer `2`".to_owned()),
        ] {
            let ir = lowered(&stair(&format!(" {key}={written}")));
            assert_eq!(
                only_structure(&ir),
                only_structure(&defaults),
                "{key}={written} must build what a stair with no `{key}=` builds",
            );
            assert_eq!(
                findings(&ir),
                vec![(
                    "W_IGNORED_ARGUMENT",
                    format!("`{key}=` must be {expected}, not {described}; the value was ignored")
                        .as_str(),
                )],
                "{key}={written}",
            );
            assert_eq!(
                ir.diagnostics[0].notes[0].message,
                format!("the stair is built with the default `{key}={default}`"),
            );
        }
    }
}

#[test]
fn every_unreadable_stair_state_is_reported_once_on_its_own_value() {
    // Each finding underlines its own value, so the three are reported in
    // the order the line writes them and an editor can tell them apart.
    let source = stair(" shape=\"outer_left\" facing=\"in\" half=\"bottom\"");
    let ir = lowered(&source);
    assert_eq!(only_structure(&ir), only_structure(&lowered(&stair(""))));
    assert_eq!(
        findings(&ir),
        vec![
            (
                "W_IGNORED_ARGUMENT",
                "`shape=` must be `straight`, `outer_left`, or `outer_right`, not string \
                 `\"outer_left\"`; the value was ignored",
            ),
            (
                "W_IGNORED_ARGUMENT",
                "`facing=` must be `out` or `in`, not string `\"in\"`; the value was ignored",
            ),
            (
                "W_IGNORED_ARGUMENT",
                "`half=` must be `top` or `bottom`, not string `\"bottom\"`; the value was ignored",
            ),
        ],
    );
    let spans: Vec<&str> = (0..3).map(|i| underlined(&source, &ir, i)).collect();
    assert_eq!(spans, vec!["\"outer_left\"", "\"in\"", "\"bottom\""]);
}

#[test]
fn an_unreadable_stair_state_on_a_stair_that_is_not_built_is_still_reported() {
    // An identifier the stair does not support defers it. The unreadable
    // siblings are further repairs, and they arrive in the same compile
    // rather than one compile each after the first is made.
    const NOT_BUILT: &str = "this stair is not built either way — see the finding on the same line";
    let unknown = lowered(&stair(" half=sideways facing=1 shape=\"straight\""));
    assert_eq!(
        findings(&unknown),
        vec![
            (
                "W_DEFERRED_MEMBER",
                "stair `half=sideways` is not yet supported (use `top` or `bottom`)",
            ),
            (
                "W_IGNORED_ARGUMENT",
                "`facing=` must be `out` or `in`, not integer `1`; the value was ignored",
            ),
            (
                "W_IGNORED_ARGUMENT",
                "`shape=` must be `straight`, `outer_left`, or `outer_right`, not string \
                 `\"straight\"`; the value was ignored",
            ),
        ],
    );
    assert_eq!(unknown.diagnostics[1].notes[0].message, NOT_BUILT);
    assert_eq!(unknown.diagnostics[2].notes[0].message, NOT_BUILT);

    // The same holds for a stair refused after the state arguments, here
    // for want of an overhang.
    let no_overhang = lowered(&stair(" facing=1").replace(" overhang=1", ""));
    let codes: Vec<&str> = findings(&no_overhang)
        .iter()
        .map(|(code, _)| *code)
        .collect();
    assert_eq!(
        codes,
        vec!["W_DEFERRED_MEMBER", "W_IGNORED_ARGUMENT"],
        "{:#?}",
        no_overhang.diagnostics
    );
    assert_eq!(no_overhang.diagnostics[1].notes[0].message, NOT_BUILT);
}

// --- place `gap=` ----------------------------------------------------------

/// A site whose first row puts a 3-wide, 5-deep `box` at the origin, with
/// `rows` appended after it. The body is not square, so `east_of` stepping
/// by the width and `north_of` stepping by the depth are told apart.
fn site(rows: &str) -> String {
    format!(
        "def box size=3x5:\n  \
         floor id=f mat_slot=stone\n\n\
         theme t:\n  \
         slot stone -> @stone\n\n\
         site s:\n  \
         place id=a use=box theme=t at=origin\n{rows}"
    )
}

fn origin(ir: &BlockArrayIr, id: &str) -> Option<(i32, i32, i32)> {
    ir.placements
        .get(&format!("site::s::{id}"))
        .map(|p| p.origin)
}

/// The note on an unreadable `gap=` whose row is refused for a reason no
/// `gap=` reaches.
const NOT_PLACED: &str = "this row is not placed either way — see the finding on the same line";

/// The note on an unreadable `gap=` whose row is refused for its origin.
/// Only `gap=0` was tried, so it claims nothing about any other value.
const NOT_PLACED_AT_ZERO: &str = "this row is not placed at `gap=0`, the value its origin was \
     worked out with — see the finding on the same line";

#[test]
fn an_unreadable_gap_places_the_row_edge_to_edge_and_says_so() {
    let touching = lowered(&site("  place id=b use=box theme=t east_of=a\n"));
    assert_eq!(findings(&touching), vec![]);
    assert_eq!(origin(&touching, "b"), Some((3, 0, 0)));
    // Guard: a readable gap moves the row, so `(3, 0, 0)` below is the
    // default and not a reader that never looked at `gap=`.
    let spaced = lowered(&site("  place id=b use=box theme=t east_of=a gap=4\n"));
    assert_eq!(origin(&spaced, "b"), Some((7, 0, 0)));

    for (selector, at) in [("east_of", (3, 0, 0)), ("north_of", (0, 0, -5))] {
        for (written, described) in [("wide", "identifier `wide`"), ("\"4\"", "string `\"4\"`")] {
            let source = site(&format!(
                "  place id=b use=box theme=t {selector}=a gap={written}\n"
            ));
            let ir = lowered(&source);
            assert_eq!(origin(&ir, "b"), Some(at), "{selector} gap={written}");
            assert_eq!(
                findings(&ir),
                vec![(
                    "W_IGNORED_ARGUMENT",
                    format!("`gap=` must be an integer, not {described}; the value was ignored")
                        .as_str(),
                )],
                "{selector} gap={written}",
            );
            assert_eq!(
                ir.diagnostics[0].notes[0].message,
                "the row is placed as `gap=0` places it, edge to edge with the place it is \
                 relative to",
            );
            assert_eq!(underlined(&source, &ir, 0), written);
        }
    }
}

#[test]
fn a_placement_past_i32_refuses_the_row_instead_of_saturating() {
    // `a` and `b` are 3 wide, so `gap=2147483642` puts `b`'s last column
    // at exactly `i32::MAX`, and one more leaves the range.
    let edge = lowered(&site(
        "  place id=b use=box theme=t east_of=a gap=2147483642\n",
    ));
    assert_eq!(findings(&edge), vec![]);
    assert_eq!(origin(&edge, "b"), Some((i32::MAX - 2, 0, 0)));
    // `b` is 5 deep, so `gap=2147483643` puts it at exactly `i32::MIN`.
    let floor = lowered(&site(
        "  place id=b use=box theme=t north_of=a gap=2147483643\n",
    ));
    assert_eq!(findings(&floor), vec![]);
    assert_eq!(origin(&floor, "b"), Some((0, 0, i32::MIN)));

    let origin_past = |reported: &str| {
        format!(
            "this placement's origin works out to {reported}, past the -2147483648 to \
             2147483647 range a placement's origin is recorded in; shorten the `gap=` on this \
             row or on a row it is placed relative to"
        )
    };
    // The origin is in range and the body is not: its cells would have no
    // world coordinate, so the row is refused all the same.
    let body_past = |reported: &str| {
        format!(
            "this placement's body reaches {reported}, past the -2147483648 to 2147483647 \
             range a placement's cells are addressed in; shorten the `gap=` on this row or on \
             a row it is placed relative to"
        )
    };
    for (row, primary) in [
        ("east_of=a gap=2147483643", body_past("x=2147483648")),
        ("east_of=a gap=2147483644", body_past("x=2147483649")),
        ("east_of=a gap=2147483645", origin_past("x=2147483648")),
        ("east_of=a gap=3000000000", origin_past("x=3000000003")),
        ("north_of=a gap=2147483644", origin_past("z=-2147483649")),
    ] {
        let ir = lowered(&site(&format!("  place id=b use=box theme=t {row}\n")));
        assert_eq!(origin(&ir, "b"), None, "{row}");
        assert_eq!(
            findings(&ir),
            vec![("W_DEFERRED_MEMBER", primary.as_str())],
            "{row}",
        );
    }
}

#[test]
fn rows_past_the_range_are_not_stacked_on_one_coordinate() {
    // Saturating used to put `b` on `i32::MAX` and `c`, placed east of it,
    // on the same coordinate. Neither is placed now: `b` for its origin,
    // and `c` as a row whose anchor did not lower. `c`'s unreadable `gap=`
    // is still reported, with a note saying the row is not placed.
    let ir = lowered(&site(
        "  place id=b use=box theme=t east_of=a gap=2147483647\n  \
         place id=c use=box theme=t east_of=b gap=wide\n",
    ));
    assert_eq!(origin(&ir, "b"), None);
    assert_eq!(origin(&ir, "c"), None);
    assert_eq!(origin(&ir, "a"), Some((0, 0, 0)));
    let codes: Vec<&str> = findings(&ir).iter().map(|(code, _)| *code).collect();
    assert_eq!(
        codes,
        vec![
            "W_DEFERRED_MEMBER",
            "W_DEFERRED_MEMBER",
            "W_IGNORED_ARGUMENT"
        ],
        "{:#?}",
        ir.diagnostics
    );
    assert!(
        ir.diagnostics[1].primary.contains("did not lower"),
        "{:?}",
        ir.diagnostics[1].primary,
    );
    assert_eq!(ir.diagnostics[2].notes[0].message, NOT_PLACED);
}

/// A site placing `bad`, whose eave band is bound to a material that is
/// not a stair: a defect in the `def` and theme that has nothing to do
/// with where a row lands. `bad` is placed only by row `b`, because a
/// finding repeated word for word by a second placement of one `def` is
/// reported once.
fn with_bad_body(row: &str) -> BlockArrayIr {
    lowered(&format!(
        "def box size=5x5:\n  \
         floor id=f mat_slot=wall\n\n\
         def bad size=5x5:\n  \
         walls mat_slot=wall height=3\n  \
         roof  kind=flat mat_slot=wall overhang=1\n  \
         stair kind=stairs side=front mat_slot=wall\n\n\
         theme t:\n  \
         slot wall -> @cobblestone\n\n\
         site s:\n  \
         place id=a use=box theme=t at=origin\n  \
         place id=b use=bad theme=t {row}\n"
    ))
}

#[test]
fn a_row_refused_for_its_origin_still_reports_its_body() {
    let incompatible = |ir: &BlockArrayIr| {
        ir.diagnostics
            .iter()
            .find(|d| d.code.as_str() == "E_INCOMPATIBLE_MATERIAL")
            .cloned()
    };
    // Guard: placed in range, `bad` reports its stair's material.
    let placed = with_bad_body("east_of=a gap=1");
    assert!(origin(&placed, "b").is_some());
    let codes: Vec<&str> = findings(&placed).iter().map(|(code, _)| *code).collect();
    assert_eq!(
        codes,
        vec!["E_INCOMPATIBLE_MATERIAL"],
        "{:#?}",
        placed.diagnostics
    );

    // Lowering the body takes nothing from the origin, so the refused row
    // reports the same finding, beside the refusal.
    let refused = with_bad_body("east_of=a gap=3000000000");
    assert_eq!(origin(&refused, "b"), None);
    let codes: Vec<&str> = findings(&refused).iter().map(|(code, _)| *code).collect();
    // Source order: the finding sits on the theme's `slot` line, above
    // the site.
    assert_eq!(
        codes,
        vec!["E_INCOMPATIBLE_MATERIAL", "W_DEFERRED_MEMBER"],
        "{:#?}",
        refused.diagnostics
    );
    assert!(
        refused.diagnostics[1]
            .primary
            .contains("origin works out to x=3000000005"),
        "{:?}",
        refused.diagnostics[1].primary,
    );
    assert_eq!(incompatible(&refused), incompatible(&placed));
}

#[test]
fn a_row_whose_anchor_did_not_lower_still_reports_its_body() {
    // `b` is refused for its origin, so `c`, placed east of it, has no
    // anchor. `bad` is placed by `c` alone; its stair's material is a
    // defect in the `def` and theme wherever `c` would have landed.
    let ir = lowered(
        "def box size=5x5:\n  \
         floor id=f mat_slot=wall\n\n\
         def bad size=5x5:\n  \
         walls mat_slot=wall height=3\n  \
         roof  kind=flat mat_slot=wall overhang=1\n  \
         stair kind=stairs side=front mat_slot=wall\n\n\
         theme t:\n  \
         slot wall -> @cobblestone\n\n\
         site s:\n  \
         place id=a use=box theme=t at=origin\n  \
         place id=b use=box theme=t east_of=a gap=3000000000\n  \
         place id=c use=bad theme=t east_of=b\n",
    );
    assert_eq!(origin(&ir, "c"), None);
    let codes: Vec<&str> = findings(&ir).iter().map(|(code, _)| *code).collect();
    assert_eq!(
        codes,
        vec![
            "E_INCOMPATIBLE_MATERIAL",
            "W_DEFERRED_MEMBER",
            "W_DEFERRED_MEMBER"
        ],
        "{:#?}",
        ir.diagnostics
    );
    assert!(
        ir.diagnostics[2].primary.contains("did not lower"),
        "{:?}",
        ir.diagnostics[2].primary,
    );
}

#[test]
fn an_unreadable_gap_on_a_row_refused_for_its_origin_is_still_reported() {
    // `b`'s last column sits exactly on `i32::MAX`, so `c` leaves the
    // range at the `gap=0` its unreadable `gap=` falls back to. The note
    // names that value only: it does not say whether some other `gap=`
    // would place the row.
    let ir = lowered(&site(
        "  place id=b use=box theme=t east_of=a gap=2147483642\n  \
         place id=c use=box theme=t east_of=b gap=wide\n",
    ));
    assert_eq!(origin(&ir, "b"), Some((i32::MAX - 2, 0, 0)));
    assert_eq!(origin(&ir, "c"), None);
    let codes: Vec<&str> = findings(&ir).iter().map(|(code, _)| *code).collect();
    assert_eq!(
        codes,
        vec!["W_DEFERRED_MEMBER", "W_IGNORED_ARGUMENT"],
        "{:#?}",
        ir.diagnostics
    );
    assert_eq!(ir.diagnostics[1].notes[0].message, NOT_PLACED_AT_ZERO);
}
