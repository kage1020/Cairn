//! A `key=` whose value is present but not readable is not the same as a
//! `key=` left off, and the build has to say which one it saw.
//!
//! `spec/lint` "Error vs warning" calls this an unreadable value. The pass
//! drops the value and uses the default, which is a `W_IGNORED_ARGUMENT`:
//! the rule forbids *silent* substitution. Four readers used to treat a
//! value of the wrong shape as if the key were absent. They built the
//! default and reported nothing:
//!
//! - window `sym=`, where anything but a bare `true` or `false` meant "no
//!   mirror";
//! - stair `facing=` / `half=` / `shape=`, where a quoted or numeric value
//!   became the default state;
//! - place `gap=`, where a non-integer became `0`, and an origin past `i32`
//!   saturated onto the edge, so two rows could land on one coordinate.
//!
//! Every test here pins both halves of the contract. The build equals the
//! build of the same line with the key left off, so the default really is
//! what was used. And exactly one finding names the key and the value that
//! was written, so the substitution is not silent.

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

// --- window `sym=` ---------------------------------------------------------

/// The issue's own window: a 7-wide front wall, so a mirrored `offset=1`
/// window lands at `offset=5` and the two are told apart.
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
        let ir = lowered(&window(&format!(" sym={written}")));
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
    }
}

#[test]
fn an_unreadable_sym_on_a_window_that_is_not_cut_is_not_reported() {
    // The finding says the window is drawn. A window refused for its
    // geometry is not, and its repair is already in the refusal.
    let src = window(" sym=yes").replace("offset=1", "offset=9");
    let ir = lowered(&src);
    let codes: Vec<&str> = findings(&ir).iter().map(|(code, _)| *code).collect();
    assert_eq!(codes, vec!["W_DEFERRED_MEMBER"], "{:#?}", ir.diagnostics);
}

// --- stair `facing=` / `half=` / `shape=` ----------------------------------

/// The issue's own eave band.
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
fn every_unreadable_stair_state_is_reported_once() {
    let ir = lowered(&stair(
        " facing=\"in\" half=\"bottom\" shape=\"outer_left\"",
    ));
    assert_eq!(only_structure(&ir), only_structure(&lowered(&stair(""))));
    assert_eq!(
        findings(&ir),
        vec![
            (
                "W_IGNORED_ARGUMENT",
                "`half=` must be `top` or `bottom`, not string `\"bottom\"`; the value was ignored",
            ),
            (
                "W_IGNORED_ARGUMENT",
                "`facing=` must be `out` or `in`, not string `\"in\"`; the value was ignored",
            ),
            (
                "W_IGNORED_ARGUMENT",
                "`shape=` must be `straight`, `outer_left`, or `outer_right`, not string \
                 `\"outer_left\"`; the value was ignored",
            ),
        ],
    );
}

#[test]
fn an_unreadable_stair_state_on_a_stair_that_is_not_built_is_not_reported() {
    // An unknown identifier names no state and defers the stair; a second
    // finding about a sibling argument nothing was built from would bill
    // one repair twice. The same holds for a stair refused after the reads,
    // here for want of an overhang.
    let unknown = lowered(&stair(" half=sideways facing=1"));
    assert_eq!(
        findings(&unknown),
        vec![(
            "W_DEFERRED_MEMBER",
            "stair `half=sideways` is not yet supported (use `top` or `bottom`)",
        )],
    );

    let no_overhang = lowered(&stair(" facing=1").replace(" overhang=1", ""));
    let codes: Vec<&str> = findings(&no_overhang)
        .iter()
        .map(|(code, _)| *code)
        .collect();
    assert_eq!(
        codes,
        vec!["W_DEFERRED_MEMBER"],
        "{:#?}",
        no_overhang.diagnostics
    );
}

// --- place `gap=` ----------------------------------------------------------

/// The issue's own site, with `rows` appended after `a`.
fn site(rows: &str) -> String {
    format!(
        "def box size=3x3:\n  \
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

#[test]
fn an_unreadable_gap_places_the_row_edge_to_edge_and_says_so() {
    let touching = lowered(&site("  place id=b use=box theme=t east_of=a\n"));
    assert_eq!(findings(&touching), vec![]);
    assert_eq!(origin(&touching, "b"), Some((3, 0, 0)));
    // Guard: a readable gap moves the row, so `(3, 0, 0)` below is the
    // default and not a reader that never looked at `gap=`.
    let spaced = lowered(&site("  place id=b use=box theme=t east_of=a gap=4\n"));
    assert_eq!(origin(&spaced, "b"), Some((7, 0, 0)));

    for (selector, at) in [("east_of", (3, 0, 0)), ("north_of", (0, 0, -3))] {
        for (written, described) in [("wide", "identifier `wide`"), ("\"4\"", "string `\"4\"`")] {
            let ir = lowered(&site(&format!(
                "  place id=b use=box theme=t {selector}=a gap={written}\n"
            )));
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
        }
    }
}

#[test]
fn an_origin_past_i32_refuses_the_row_instead_of_saturating() {
    // `a` is 3 wide, so `gap=2147483644` puts `b` at exactly `i32::MAX`
    // and one more leaves the range.
    let edge = lowered(&site(
        "  place id=b use=box theme=t east_of=a gap=2147483644\n",
    ));
    assert_eq!(findings(&edge), vec![]);
    assert_eq!(origin(&edge, "b"), Some((i32::MAX, 0, 0)));

    for (row, reported) in [
        ("east_of=a gap=2147483645", "x=2147483648"),
        ("east_of=a gap=3000000000", "x=3000000003"),
        ("north_of=a gap=2147483646", "z=-2147483649"),
    ] {
        let ir = lowered(&site(&format!("  place id=b use=box theme=t {row}\n")));
        assert_eq!(origin(&ir, "b"), None, "{row}");
        assert_eq!(
            findings(&ir),
            vec![(
                "W_DEFERRED_MEMBER",
                format!(
                    "this placement's origin works out to {reported}, past the -2147483648 to \
                     2147483647 range a placement's origin is recorded in; shorten the `gap=` \
                     on this row or on a row it is placed relative to"
                )
                .as_str(),
            )],
            "{row}",
        );
    }
}

#[test]
fn rows_past_the_range_are_not_stacked_on_one_coordinate() {
    // The issue's own chain: `b` saturated to `i32::MAX`, and `c`, placed
    // east of it, saturated onto the same coordinate. Neither is placed
    // now, and `c` is refused as a row whose anchor did not lower — with no
    // finding about its own unreadable `gap=`, since it is not placed.
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
        vec!["W_DEFERRED_MEMBER", "W_DEFERRED_MEMBER"],
        "{:#?}",
        ir.diagnostics
    );
    assert!(
        ir.diagnostics[1].primary.contains("did not lower"),
        "{:?}",
        ir.diagnostics[1].primary,
    );
}

#[test]
fn a_row_refused_for_its_origin_reports_nothing_about_its_body() {
    // `north_of` needs the new body's depth, so the range is only known
    // once the body is lowered. The body's own findings are held until
    // then: a row that is not placed says so once, as a row whose anchor
    // did not lower does, and says nothing about a body it never built.
    // `bad` is placed only by the refused row, because a finding repeated
    // word for word by a second placement of one `def` is reported once.
    let with_b = |row: &str| {
        lowered(&format!(
            "def box size=3x3:\n  \
             floor id=f mat_slot=stone\n\n\
             def bad size=3x3:\n  \
             floor id=f mat_slot=stone\n  \
             walls mat_slot=stone height=2\n  \
             window side=front offset=1 size=1x1\n\n\
             theme t:\n  \
             slot stone -> @stone\n\n\
             site s:\n  \
             place id=a use=box theme=t at=origin\n  \
             place id=b use=bad theme=t {row}\n"
        ))
    };
    // Guard: placed in range, `bad` does report its window.
    let placed = with_b("north_of=a gap=1");
    assert_eq!(origin(&placed, "b"), Some((0, 0, -4)));
    assert_eq!(
        findings(&placed),
        vec![("W_DEFERRED_MEMBER", "window has no `y=`")],
    );

    let ir = with_b("north_of=a gap=9999999999");
    assert_eq!(origin(&ir, "b"), None);
    assert_eq!(
        findings(&ir),
        vec![(
            "W_DEFERRED_MEMBER",
            "this placement's origin works out to z=-10000000002, past the -2147483648 to \
             2147483647 range a placement's origin is recorded in; shorten the `gap=` on this \
             row or on a row it is placed relative to",
        )],
    );
}

#[test]
fn an_unreadable_gap_on_a_row_refused_for_its_origin_is_not_reported() {
    // `b` sits exactly on `i32::MAX`, so `c` leaves the range even at the
    // `gap=0` its unreadable `gap=` falls back to. The finding would say
    // the row is placed; it is not.
    let ir = lowered(&site(
        "  place id=b use=box theme=t east_of=a gap=2147483644\n  \
         place id=c use=box theme=t east_of=b gap=wide\n",
    ));
    assert_eq!(origin(&ir, "b"), Some((i32::MAX, 0, 0)));
    assert_eq!(origin(&ir, "c"), None);
    let codes: Vec<&str> = findings(&ir).iter().map(|(code, _)| *code).collect();
    assert_eq!(codes, vec!["W_DEFERRED_MEMBER"], "{:#?}", ir.diagnostics);
}
