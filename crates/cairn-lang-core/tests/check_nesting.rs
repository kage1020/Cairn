//! Acceptance tests for the `nesting` pass of `cairn_lang_core::check`.
//!
//! The parser attaches an indented body to any command, but only one
//! shape has a reader: a `level y=N` sitting directly in a `struct` or
//! `def` body, which `block_array::flatten_members` unwraps so the
//! children join the phase buckets. Everything else is dropped before a
//! block is placed.
//!
//! The failure was inverted. `check::connect_arity` recurses into
//! children, so a *malformed* nested `connect` was reported loudly,
//! while a well-formed one produced no walkway and no diagnostic. The
//! more correct the row, the quieter it failed.
//!
//! Two things these tests deliberately do *not* claim. Dropped is not
//! inert in a `struct` or `def` body — a nested member still receives a
//! theme binding and still contributes to redstone synthesis, so the
//! diagnostic is about blocks, not about the member vanishing. And the
//! "no voxels" half of that is pinned in `check_nesting_lowering.rs`,
//! which runs the lowering these tests do not.

use cairn_lang_core::{Diagnostic, DiagnosticCode, Severity, check, lower, parse};

fn diagnose(source: &str) -> Vec<Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    check(&module, &ir, None)
}

fn nesting_only(source: &str) -> Vec<Diagnostic> {
    diagnose(source)
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::UnsupportedNesting)
        .collect()
}

fn one(source: &str) -> Diagnostic {
    let mut found = nesting_only(source);
    assert_eq!(found.len(), 1, "expected one finding, got {found:#?}");
    found.remove(0)
}

fn slice<'a>(source: &'a str, diag: &Diagnostic) -> &'a str {
    &source[diag.span.clone()]
}

/// The advice note — the one without a span. The `declared here` note
/// carries a span and quotes the same keyword, so an assertion that
/// searched every note would pass on either.
fn advice(diag: &Diagnostic) -> &str {
    diag.notes.iter().find(|n| n.span.is_none()).map_or_else(
        || panic!("no span-less advice note in {diag:#?}"),
        |n| n.message.as_str(),
    )
}

const PRELUDE: &str = "theme plain:\n  \
slot floor -> @oak_planks\n  \
slot wall  -> @cobblestone\n\n\
def hut size=3x3:\n  \
floor id=floor mat_slot=floor\n  \
walls id=walls class=outer mat_slot=wall height=3\n  \
door  id=entry side=front at=center\n\n";

/// Every shape a site body can nest. The four differ only in which
/// keyword the message has to quote — `walk` has one loop and one
/// report call, so what these buy is the quoting, not four code paths.
#[test]
fn ne_1_every_nested_row_in_a_site_body_is_reported() {
    let cases = [
        (
            "place",
            "site duo:\n  place id=anchor use=hut theme=plain at=origin\n    \
             place id=peer use=hut theme=plain east_of=anchor gap=4\n",
        ),
        (
            "connect",
            "site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
             place id=peer use=hut theme=plain east_of=anchor gap=4\n  \
             connect anchor.entry to peer.entry path=@gravel\n    \
             place id=third use=hut theme=plain east_of=peer gap=4\n",
        ),
    ];
    for (keyword, body) in cases {
        let src = format!("{PRELUDE}{body}");
        let d = one(&src);
        assert_eq!(d.severity(), Severity::Error);
        assert_eq!(
            d.primary,
            format!(
                "`{keyword}` does not group members: the 1 member indented under it is not part of the site"
            ),
        );
        assert!(
            advice(&d).contains("site"),
            "the advice should name the scope, got: {}",
            advice(&d),
        );
    }
}

/// A member indented under another member in a `struct` body produces
/// no voxels. The `door` here is the shape that reads most like working
/// code: dedent it by two spaces and the wall gains its opening.
#[test]
fn ne_2_a_member_nested_under_a_geometry_member_is_reported() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n    \
         door id=d side=front at=center\n"
    );
    let d = one(&src);
    assert_eq!(d.severity(), Severity::Error);
    assert_eq!(
        d.primary,
        "`walls` does not group members: the 1 member indented under it produces no blocks",
    );
    assert_eq!(slice(&src, &d), "door id=d side=front at=center");
    assert!(
        advice(&d).contains("level"),
        "the advice should point at the one construct that does group members, got: {}",
        advice(&d),
    );
    assert!(
        d.notes.iter().any(|n| n.message.contains("redstone")),
        "a note should say what the members still take part in, got: {:#?}",
        d.notes,
    );
}

/// Same rule in a `def` body. `def` is what a `site` instantiates, so
/// it is the more likely place to meet this — and it is a separate loop
/// in `run`, which a struct-only suite leaves unexecuted.
#[test]
fn ne_2b_the_rule_reaches_def_bodies_too() {
    let src = format!(
        "{PRELUDE}def lodge size=5x5:\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n    \
         door id=d side=front at=center\n"
    );
    let d = one(&src);
    assert_eq!(
        d.primary,
        "`walls` does not group members: the 1 member indented under it produces no blocks",
    );
}

/// `level y=N` in a geometry body is the exception, and the pass has to
/// recurse *through* it: a member nested under a member nested under a
/// `level` is still lost.
#[test]
fn ne_3_level_groups_its_children_and_the_walk_continues_inside() {
    let ok = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         level y=0\n    walls class=outer mat_slot=wall height=3\n    \
         door id=d side=front at=center\n"
    );
    assert!(nesting_only(&ok).is_empty(), "got {:#?}", nesting_only(&ok));

    let nested = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         level y=0\n    walls class=outer mat_slot=wall height=3\n      \
         door id=d side=front at=center\n"
    );
    let d = one(&nested);
    assert!(
        d.primary.starts_with("`walls` does not group members"),
        "got: {}",
        d.primary,
    );
}

/// A `level` in a *site* body is not the exception. `lower_site`
/// iterates the rows for `Place` and `Connect` and a `Level` is
/// neither, so nothing unwraps it.
#[test]
fn ne_4_level_in_a_site_body_is_reported_as_a_misplaced_row_not_as_nesting() {
    let src = format!(
        "{PRELUDE}site duo:\n  level y=0\n    \
         place id=anchor use=hut theme=plain at=origin\n"
    );
    assert!(
        nesting_only(&src).is_empty(),
        "a `site` body has no reader for `level` at all, so the finding is \
         about the row rather than about what is indented under it — \
         reporting both would bill one mistake twice, got {:#?}",
        nesting_only(&src),
    );
    assert_eq!(
        diagnose(&src)
            .iter()
            .filter(|d| d.code == DiagnosticCode::MisplacedMember)
            .count(),
        1,
        "`member_scope` owns the row, and its advice is the one that fits: \
         the indented `place` belongs in the site, one indent to the left",
    );
}

/// One diagnostic per dropped subtree, and the span reaches the bottom
/// of it. A `Member`'s own span stops at its header line, so an
/// underline built from it alone would leave the grandchildren the
/// author has to move outside the range.
#[test]
fn ne_5_a_subtree_is_reported_once_at_its_root_and_underlined_whole() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n    \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n      \
         connect anchor.entry to peer.entry path=@gravel\n"
    );
    let d = one(&src);
    let text = slice(&src, &d);
    assert!(
        text.starts_with("place id=peer"),
        "the span should start at the outermost dropped member, got {text:?}",
    );
    assert!(
        text.ends_with("path=@gravel"),
        "the span should reach the end of the dropped subtree, got {text:?}",
    );
}

/// Two members under one parent are one mistake with one fix, so they
/// earn one diagnostic covering the whole indented run.
#[test]
fn ne_6_sibling_nested_members_are_underlined_as_one_run() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n    \
         door id=d side=front at=center\n    \
         window id=w side=front y=1 offset=1 size=1x1\n"
    );
    let d = one(&src);
    assert_eq!(
        d.primary,
        "`walls` does not group members: the 2 members indented under it produce no blocks",
    );
    let text = slice(&src, &d);
    assert!(
        text.starts_with("door") && text.ends_with("size=1x1"),
        "the span should cover both members, got {text:?}",
    );
}

/// A `level` inside a `level` loses its body too, and `cairn check` has
/// to say so on its own: `check` does not lower, so the
/// `W_DEFERRED_MEMBER` the block-array pass raises for this shape never
/// reaches it. A nested `level` inside a `def` that no site places is
/// not lowered at all, so that one has no other reporter anywhere.
#[test]
fn ne_7_a_level_inside_a_level_is_reported_by_check_not_left_to_lowering() {
    for (label, body) in [
        (
            "struct",
            "struct s size=5x5\n  floor mat_slot=floor\n  \
             level y=0\n    level y=1\n      walls class=outer mat_slot=wall height=3\n",
        ),
        (
            "unplaced def",
            "def lodge size=5x5:\n  floor mat_slot=floor\n  \
             level y=0\n    level y=1\n      walls class=outer mat_slot=wall height=3\n",
        ),
    ] {
        let src = format!("{PRELUDE}{body}");
        let d = one(&src);
        assert_eq!(
            d.primary,
            "a `level` inside another `level` does not group members: the 1 member indented under it produces no blocks",
            "{label}",
        );
    }
}

/// A `level` with no usable `y=` has nowhere to put its children, so
/// the body is lost for a reason of its own — and the message says
/// which, because "does not group members" would be false advice about
/// a construct that does.
#[test]
fn ne_7b_a_level_without_a_usable_offset_is_reported() {
    // No negative case: `-` lexes (a version label's pre-release suffix
    // needs it to) but no value position reads one, so `y=-3` fails in
    // the parser and never reaches a check pass. The overflow case does
    // reach it, and is the reason the reader is `u32::try_from` rather
    // than a cast.
    for y in ["", " y=abc", " y=99999999999", " y=plain"] {
        let src = format!(
            "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
             level{y}\n    walls class=outer mat_slot=wall height=3\n"
        );
        let d = one(&src);
        assert_eq!(
            d.primary,
            "`level` has no `y=` offset to place its children at: the 1 member indented under it produces no blocks",
            "for `level{y}`",
        );
        assert!(advice(&d).contains("y=N"), "for `level{y}`");
    }
}

/// An unknown keyword is `keyword_allowlist`'s finding. Its repair is
/// the keyword, so adding "move these into a `level`" alongside would
/// point the author at the wrong line.
#[test]
fn ne_7c_an_unknown_keyword_parent_is_left_to_the_allowlist() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         room name=hall\n    walls class=outer mat_slot=wall height=3\n"
    );
    assert!(
        nesting_only(&src).is_empty(),
        "got {:#?}",
        nesting_only(&src)
    );
    let codes: Vec<&str> = diagnose(&src).iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E_UNKNOWN_KEYWORD"),
        "the keyword itself must still be reported, got {codes:?}",
    );
}

/// The two findings the module doc names as the inversion: a malformed
/// nested `connect` earns both its arity error and the nesting error,
/// once each, and the well-formed one earns only the nesting error.
#[test]
fn ne_7d_a_malformed_nested_connect_earns_both_findings_once() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n  \
         connect anchor.entry to peer.entry path=@gravel\n    \
         connect 1 to 2 path=@gravel\n"
    );
    let codes: Vec<&str> = diagnose(&src).iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes
            .iter()
            .filter(|c| **c == "E_UNSUPPORTED_NESTING")
            .count(),
        1,
        "got {codes:?}",
    );
    assert_eq!(
        codes.iter().filter(|c| **c == "E_CONNECT_ARITY").count(),
        2,
        "both endpoints of the nested row are still checked, got {codes:?}",
    );
}

/// The shipped examples must stay clean. `themed-tower.crn` is the one
/// that nests, and it nests the legal way.
#[test]
fn ne_8_shipped_examples_declare_no_unsupported_nesting() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples");
    let mut checked = 0;
    for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
        let path = entry
            .unwrap_or_else(|e| panic!("entry in {}: {e}", dir.display()))
            .path();
        if path.extension().is_none_or(|e| e != "crn") {
            continue;
        }
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let found = nesting_only(&src);
        assert!(found.is_empty(), "{}: got {found:#?}", path.display());
        checked += 1;
    }
    assert!(checked > 0, "no examples were checked in {}", dir.display());
}

/// A flat body is the normal case and must stay silent — the negative
/// space every assertion above depends on.
#[test]
fn ne_9_flat_bodies_are_silent() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n  \
         door id=d side=front at=center\n\n\
         site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n  \
         connect anchor.entry to peer.entry path=@gravel\n"
    );
    let found = nesting_only(&src);
    assert!(found.is_empty(), "got {found:#?}");
}
