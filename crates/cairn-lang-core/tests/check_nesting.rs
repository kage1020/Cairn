//! Acceptance tests for the `nesting` pass of `cairn_lang_core::check`.
//!
//! The parser attaches an indented body to any command, but only one
//! role reads one back: `level y=N` inside a `struct` or `def` groups
//! its children for phase-bucketing. Everywhere else the body is
//! dropped — `block_array::flatten_members` returns the parent and never
//! looks at `member.children`, and site resolution walks
//! `site.placements` without descending at all.
//!
//! The result was an inversion. `check::connect_arity` recurses into
//! children, so a *malformed* nested `connect` was reported loudly,
//! while a well-formed one produced no walkway and no diagnostic. The
//! more correct the row, the quieter the failure.
//!
//! Dropped is not the same as inert: a nested member still receives a
//! theme binding (`resolve_members` recurses) and still contributes
//! sensors, actuators, and `logic` bindings to redstone synthesis
//! (`collect_member` recurses). What it never produces is geometry — no
//! voxels in a struct body, no placement or walkway in a site body.
//! That is what these tests pin.

use cairn_lang_core::{Diagnostic, DiagnosticCode, Severity, check, lower, parse};

fn diagnose(source: &str) -> Vec<Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}"));
    let ir = lower(&module);
    check(&module, &ir, None)
}

fn nesting_only(source: &str) -> Vec<Diagnostic> {
    diagnose(source)
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::UnsupportedNesting)
        .collect()
}

fn slice<'a>(source: &'a str, diag: &Diagnostic) -> &'a str {
    &source[diag.span.clone()]
}

const PRELUDE: &str = "theme plain:\n  \
slot floor -> @oak_planks\n  \
slot wall  -> @cobblestone\n\n\
def hut size=3x3:\n  \
floor id=floor mat_slot=floor\n  \
walls id=walls class=outer mat_slot=wall height=3\n  \
door  id=entry side=front at=center\n\n";

/// Every shape a site body can nest, since the four rows are two roles
/// in two positions and each is a separate call site in the walk.
///
/// All four resolve to nothing: `resolve_site_placements` iterates
/// `site.placements` and never descends, so the nested row is neither a
/// placement nor a walkway.
#[test]
fn ne_1_every_nested_row_in_a_site_body_is_reported() {
    let cases = [
        (
            "connect under place",
            "site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
             place id=peer use=hut theme=plain east_of=anchor gap=4\n    \
             connect anchor.entry to peer.entry path=@gravel\n",
        ),
        (
            "place under place",
            "site duo:\n  place id=anchor use=hut theme=plain at=origin\n    \
             place id=peer use=hut theme=plain east_of=anchor gap=4\n",
        ),
        (
            "place under connect",
            "site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
             place id=peer use=hut theme=plain east_of=anchor gap=4\n  \
             connect anchor.entry to peer.entry path=@gravel\n    \
             place id=third use=hut theme=plain east_of=peer gap=4\n",
        ),
        (
            "connect under connect",
            "site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
             place id=peer use=hut theme=plain east_of=anchor gap=4\n  \
             connect anchor.entry to peer.entry path=@gravel\n    \
             connect peer.entry to anchor.entry path=@gravel\n",
        ),
    ];
    for (label, body) in cases {
        let src = format!("{PRELUDE}{body}");
        let found = nesting_only(&src);
        assert_eq!(found.len(), 1, "{label}: got {found:#?}");
        assert_eq!(found[0].severity, Severity::Error);
        assert!(
            found[0].primary.contains("site"),
            "{label}: the message should name the scope, got: {}",
            found[0].primary,
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
    let found = nesting_only(&src);
    assert_eq!(found.len(), 1, "got {found:#?}");
    let d = &found[0];
    assert_eq!(d.severity, Severity::Error);
    assert!(
        slice(&src, d).starts_with("door"),
        "the span should cover the dropped members, got {:?}",
        slice(&src, d),
    );
    assert!(
        d.notes.iter().any(|n| n.message.contains("level")),
        "the note should point at the one construct that does group members, got: {:#?}",
        d.notes,
    );
}

/// `level y=N` is the exception, and the only one. Without this the
/// pass would reject the shape `examples/themed-tower.crn` is built
/// from.
#[test]
fn ne_3_level_in_a_geometry_body_groups_its_children_silently() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         level y=0\n    walls class=outer mat_slot=wall height=3\n    \
         door id=d side=front at=center\n"
    );
    let found = nesting_only(&src);
    assert!(found.is_empty(), "got {found:#?}");
}

/// A `level` in a *site* body is not the exception. Site lowering never
/// looks at it, so its children are dropped exactly like any other
/// nested row — the role is only meaningful where something reads it.
#[test]
fn ne_4_level_in_a_site_body_does_not_group_anything() {
    let src = format!(
        "{PRELUDE}site duo:\n  level y=0\n    \
         place id=anchor use=hut theme=plain at=origin\n"
    );
    let found = nesting_only(&src);
    assert_eq!(found.len(), 1, "got {found:#?}");
}

/// One diagnostic per dropped subtree, not one per dropped member. The
/// inner body is already inside something that will not be built, so
/// reporting it again would count the same mistake twice.
#[test]
fn ne_5_a_subtree_is_reported_once_at_its_root() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n    \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n      \
         connect anchor.entry to peer.entry path=@gravel\n"
    );
    let found = nesting_only(&src);
    assert_eq!(found.len(), 1, "got {found:#?}");
    assert!(
        slice(&src, &found[0]).starts_with("place id=peer"),
        "the span should start at the outermost dropped member, got {:?}",
        slice(&src, &found[0]),
    );
}

/// Two members under one parent are one mistake with one fix, so they
/// earn one diagnostic covering the whole indented run — the same shape
/// `connect_arity` uses for trailing extras.
#[test]
fn ne_6_sibling_nested_members_are_underlined_as_one_run() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n    \
         door id=d side=front at=center\n    \
         window id=w side=front y=1 offset=1 size=1x1\n"
    );
    let found = nesting_only(&src);
    assert_eq!(found.len(), 1, "got {found:#?}");
    let text = slice(&src, &found[0]);
    assert!(
        text.starts_with("door") && text.contains("window"),
        "the span should cover both members, got {text:?}",
    );
    assert!(
        found[0].primary.contains('2'),
        "the message should say how many members are dropped, got: {}",
        found[0].primary,
    );
}

/// A `level` nested inside a `level` is left to block-array lowering,
/// which already reports it as a deferred member and names every
/// dropped subtree. Reporting it here as well would give one mistake
/// two codes from two layers.
#[test]
fn ne_7_a_level_inside_a_level_is_left_to_the_lowering_pass() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         level y=0\n    level y=1\n      walls class=outer mat_slot=wall height=3\n"
    );
    let found = nesting_only(&src);
    assert!(found.is_empty(), "got {found:#?}");
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
    for entry in std::fs::read_dir(&dir).expect("read examples dir") {
        let path = entry.expect("dir entry").path();
        if path.extension().is_none_or(|e| e != "crn") {
            continue;
        }
        let src = std::fs::read_to_string(&path).expect("read example");
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
