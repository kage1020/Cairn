//! Acceptance tests for the `member_scope` pass of
//! `cairn_lang_core::check`.
//!
//! `intent::role_of` is one global table, so every keyword classifies to
//! its role in every body. Nothing downstream puts the distinction back:
//! `block_array` buckets the geometry roles and the site passes match
//! `place` / `connect`, so a keyword written into the wrong body falls off
//! the end of both.
//!
//! The two halves failed differently, which is why both are covered here.
//! A `place` in a `def` body at least earned a `W_DEFERRED_MEMBER` during
//! lowering — invisible to `cairn check`, which does not lower, but
//! visible to `cairn lower`. A `floor` among a site's placements had no
//! reporter at either stage: the resolver's loop and the lowering loop
//! both `continue` past a non-`place` row without a word, so the member
//! produced no voxels and no diagnostic anywhere.

use cairn_lang_core::{Diagnostic, DiagnosticCode, Severity, check, lower, parse};

fn diagnose(source: &str) -> Vec<Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    check(&module, &ir, None)
}

fn misplaced_only(source: &str) -> Vec<Diagnostic> {
    diagnose(source)
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::MisplacedMember)
        .collect()
}

fn one(source: &str) -> Diagnostic {
    let mut found = misplaced_only(source);
    assert_eq!(found.len(), 1, "expected one finding, got {found:#?}");
    found.remove(0)
}

fn notes(diag: &Diagnostic) -> Vec<&str> {
    diag.notes.iter().map(|n| n.message.as_str()).collect()
}

const PRELUDE: &str = "theme plain:\n  \
slot floor -> @oak_planks\n  \
slot wall  -> @cobblestone\n\n\
def hut size=3x3:\n  \
floor id=floor mat_slot=floor\n  \
walls id=walls class=outer mat_slot=wall height=3\n  \
door  id=entry side=front at=center\n\n";

/// Every keyword the role table knows, with a well-formed argument list
/// for each so the finding under test is the only thing wrong with the
/// line. `connect` names the two places `site_with` declares.
const GEOMETRY_ROWS: &[&str] = &[
    "floor mat_slot=floor",
    "walls mat_slot=wall height=3",
    "door id=d side=front at=center",
    "window id=w side=front y=1 offset=1 size=1x1",
    "roof kind=flat mat_slot=wall",
    "stair kind=stairs mat_slot=wall",
    "level y=0",
    "pressure_plate id=p at=front.outside",
    "circuit region=floor void=1",
];

const SITE_ROWS: &[&str] = &[
    "place id=extra use=hut theme=plain at=origin",
    "connect anchor.entry to peer.entry path=@gravel",
];

fn struct_with(row: &str) -> String {
    format!("{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  {row}\n")
}

fn site_with(row: &str) -> String {
    format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n  {row}\n"
    )
}

/// The bug as reported: `place` and `connect` written into a `def` body.
/// `cairn check` exited 0 on this source.
#[test]
fn ms_1_a_site_row_in_a_geometry_body_is_an_error() {
    for row in SITE_ROWS {
        let src = struct_with(row);
        let d = one(&src);
        assert_eq!(d.severity, Severity::Error, "for `{row}`");
        assert!(
            d.primary
                .starts_with("nothing in a `struct` or `def` body reads `"),
            "for `{row}`, got {:?}",
            d.primary,
        );
    }
}

/// The half with no reporter at all: a geometry keyword among a site's
/// placements produced no voxels and no diagnostic at either stage.
#[test]
fn ms_2_a_geometry_row_in_a_site_body_is_an_error() {
    for row in GEOMETRY_ROWS {
        let src = site_with(row);
        let d = one(&src);
        assert_eq!(d.severity, Severity::Error, "for `{row}`");
        assert!(
            d.primary.starts_with("nothing in a `site` body reads `"),
            "for `{row}`, got {:?}",
            d.primary,
        );
    }
}

/// The negative space. Without this, a pass that reported every member
/// would satisfy the two tests above.
#[test]
fn ms_3_a_row_the_body_reads_is_not_reported() {
    for row in GEOMETRY_ROWS {
        let src = struct_with(row);
        assert!(
            misplaced_only(&src).is_empty(),
            "`{row}` belongs in a `struct` body, got {:#?}",
            misplaced_only(&src),
        );
    }
    for row in SITE_ROWS {
        let src = site_with(row);
        assert!(
            misplaced_only(&src).is_empty(),
            "`{row}` belongs in a `site` body, got {:#?}",
            misplaced_only(&src),
        );
    }
}

/// A `level y=N` groups members inside the geometry body it already sits
/// in — `flatten_members` splices its children into the same phase
/// buckets — so nesting does not open a body with different rules.
#[test]
fn ms_4_the_body_kind_carries_through_a_level() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  level y=0\n    \
         place id=extra use=hut theme=plain at=origin\n"
    );
    let d = one(&src);
    assert!(d.primary.contains("reads `place`"), "got {:?}", d.primary);
}

/// One finding per misplaced subtree. Everything indented under a row
/// this body cannot read goes with it, so descending would bill one
/// mistake once per line underneath.
#[test]
fn ms_5_a_subtree_is_reported_once_at_its_root() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         walls mat_slot=wall height=3\n    floor mat_slot=floor\n    \
         door id=d side=front at=center\n"
    );
    let d = one(&src);
    assert!(
        d.primary.contains("reads `walls`"),
        "the outermost misplaced row is the one reported, got {:?}",
        d.primary,
    );
    assert!(
        notes(&d).contains(&"the 2 members indented under it go with it"),
        "the lost children have to be counted here — `nesting` stays quiet \
         about them precisely because this finding covers them, got {:#?}",
        notes(&d),
    );
}

/// `nesting` and `member_scope` do not both fire on one row. The nesting
/// advice ("dedent these") does not fit a row that cannot be in this body
/// at any indentation.
#[test]
fn ms_6_nesting_does_not_also_report_a_misplaced_row() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         walls mat_slot=wall height=3\n    floor mat_slot=floor\n"
    );
    let codes: Vec<&str> = diagnose(&src).iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes
            .iter()
            .filter(|c| **c == "E_UNSUPPORTED_NESTING")
            .count(),
        0,
        "got {codes:?}",
    );
}

/// An unknown keyword is `keyword_allowlist`'s finding in either body:
/// the repair is the word, and there is no role to measure against the
/// body's readers.
#[test]
fn ms_7_an_unknown_keyword_is_left_to_the_allowlist() {
    for src in [
        struct_with("frame id=f side=front"),
        site_with("frame id=f"),
    ] {
        assert!(
            misplaced_only(&src).is_empty(),
            "got {:#?}",
            misplaced_only(&src),
        );
        assert!(
            diagnose(&src)
                .iter()
                .any(|d| d.code == DiagnosticCode::UnknownKeyword),
            "the allowlist still owns it",
        );
    }
}

/// `logic` and `assert` lines are not members — `intent::lower` sorts
/// them into `MemberBody`'s own fields and redstone synthesis reads them
/// from either body — so this pass must not touch them.
#[test]
fn ms_8_logic_and_assert_lines_are_not_members() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         logic sig.open = sig.step\n"
    );
    assert!(
        misplaced_only(&src).is_empty(),
        "got {:#?}",
        misplaced_only(&src),
    );
}

/// The advice for a `level` in a `site` body is written separately: its
/// children are usually `place` rows that belong exactly where they are,
/// one indent to the left, and the generic "move it into a struct"
/// advice would cost the author the placement.
#[test]
fn ms_9_a_level_in_a_site_body_says_dedent_rather_than_relocate() {
    let src = format!(
        "{PRELUDE}site duo:\n  level y=0\n    \
         place id=anchor use=hut theme=plain at=origin\n"
    );
    let d = one(&src);
    assert!(
        notes(&d).contains(
            &"a `site` body has no grouping construct: dedent the rows to the site's own indentation and drop the `level`"
        ),
        "got {:#?}",
        notes(&d),
    );
}

/// The "expected one of" note is the closed set for the body the author
/// is actually in, not the whole keyword table.
#[test]
fn ms_10_the_candidate_list_is_scoped_to_the_body() {
    let geometry = one(&struct_with("place id=extra use=hut theme=plain at=origin"));
    assert!(
        notes(&geometry).contains(
            &"expected one of: floor, walls, door, window, roof, stair, level, pressure_plate, circuit"
        ),
        "got {:#?}",
        notes(&geometry),
    );
    let site = one(&site_with("floor mat_slot=floor"));
    assert!(
        notes(&site).contains(&"expected one of: place, connect"),
        "got {:#?}",
        notes(&site),
    );
}

/// The span underlines the offending row, not the whole body. A renderer
/// pointing at the `struct` header would send the author to a line that
/// is correct.
#[test]
fn ms_11_the_span_underlines_the_offending_row() {
    let src = site_with("floor id=stray mat_slot=floor");
    let d = one(&src);
    assert_eq!(&src[d.span.clone()], "floor id=stray mat_slot=floor");
}
