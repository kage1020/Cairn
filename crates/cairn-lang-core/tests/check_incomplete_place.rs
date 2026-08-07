//! Acceptance tests for `E_INCOMPLETE_PLACE`.
//!
//! `resolve_site_placements` reads three keys off a `place` row before it
//! can build anything: `id=` for the scope key and the output `.nbt`,
//! `use=` for the def to instantiate, `theme=` for the materials. Each read
//! used to `continue` on a miss, so a row short of any of them produced no
//! placement, no voxels, and no diagnostic at either stage — `cairn check`
//! exited 0 on a site whose buildings were simply absent.
//!
//! The keys are required rather than auto-filled. `spec/syntax.md` §5.5
//! auto-addresses a *geometry member*, whose address derives from its role
//! and position; a `place` names the file the compiler writes (§9.3.4) and
//! the second half of the scope key `east_of=` and `connect` parse back
//! out, so an invented name would be one the author never wrote and cannot
//! refer to.

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::{Diagnostic, DiagnosticCode, Severity, check, lower, parse, resolve};

fn diagnose(source: &str) -> Vec<Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    check(&module, &ir, None)
}

fn incomplete_only(source: &str) -> Vec<Diagnostic> {
    diagnose(source)
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::IncompletePlace)
        .collect()
}

fn one(source: &str) -> Diagnostic {
    let mut found = incomplete_only(source);
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

/// A site whose first row anchors at the origin, so the row under test is
/// the only thing that can be wrong.
fn site_with(row: &str) -> String {
    format!("{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  {row}\n")
}

/// Each key on its own, and the message names the one that is missing
/// rather than a generic "incomplete".
#[test]
fn ip_1_each_absent_key_is_reported_by_name() {
    for (row, key) in [
        (
            "place           use=hut theme=plain east_of=anchor gap=4",
            "`id=`",
        ),
        (
            "place id=b               theme=plain east_of=anchor gap=4",
            "`use=`",
        ),
        (
            "place id=b      use=hut              east_of=anchor gap=4",
            "`theme=`",
        ),
    ] {
        let src = site_with(row);
        let d = one(&src);
        assert_eq!(d.severity(), Severity::Error, "for `{row}`");
        assert_eq!(
            d.primary,
            format!("`place` in site `duo` is missing {key}, so no placement is built for it"),
        );
    }
}

/// A row short of every key is one finding, not three: the author fixes
/// one line once rather than re-running the compiler to discover the next
/// omission.
#[test]
fn ip_2_every_missing_key_is_named_at_once() {
    let src = site_with("place east_of=anchor gap=4");
    let d = one(&src);
    assert_eq!(
        d.primary,
        "`place` in site `duo` is missing `id=`, `use=`, and `theme=`, so no placement is built for it",
    );
    assert_eq!(
        notes(&d).len(),
        3,
        "one note per key, saying what each is for: {:#?}",
        notes(&d),
    );
}

/// The negative space. Without this, a pass that reported every `place`
/// row would satisfy the two tests above.
#[test]
fn ip_3_a_complete_row_is_not_reported() {
    let src = site_with("place id=b use=hut theme=plain east_of=anchor gap=4");
    assert!(
        incomplete_only(&src).is_empty(),
        "got {:#?}",
        incomplete_only(&src),
    );
}

/// Absence is read off the surface key, not off the lifted value. A
/// mistyped key is present — calling it missing would send the author to
/// add a key that is already on the line, and `E_TYPE_MISMATCH_LABEL`
/// already names it.
#[test]
fn ip_4_a_mistyped_key_is_not_a_missing_one() {
    for row in [
        "place id=3 use=hut theme=plain east_of=anchor gap=4",
        "place id=b use=3   theme=plain east_of=anchor gap=4",
        "place id=b use=hut theme=7     east_of=anchor gap=4",
    ] {
        let src = site_with(row);
        assert!(
            incomplete_only(&src).is_empty(),
            "`{row}` is mistyped, not incomplete, got {:#?}",
            incomplete_only(&src),
        );
        assert!(
            diagnose(&src)
                .iter()
                .any(|d| d.code == DiagnosticCode::TypeMismatchLabel),
            "`{row}` is still reported, by the code that can say what is wrong with it",
        );
    }
}

/// The span underlines the offending row. A renderer pointing at the
/// `site` header would send the author to a line that is correct.
#[test]
fn ip_5_the_span_underlines_the_offending_row() {
    let src = site_with("place id=b use=hut east_of=anchor gap=4");
    let d = one(&src);
    assert_eq!(
        &src[d.span.clone()],
        "place id=b use=hut east_of=anchor gap=4"
    );
}

/// An incomplete row that still has an `id=` keeps its registration, so a
/// later `east_of=` naming it resolves exactly as it did. Dropping the
/// registration would turn one mistake into a cascade of unresolved
/// references down the rest of the site.
#[test]
fn ip_6_an_incomplete_row_with_an_id_still_registers_its_name() {
    let src = format!(
        "{PRELUDE}site trio:\n  place id=anchor use=hut theme=plain at=origin\n  \
         place id=gap theme=plain east_of=anchor gap=4\n  \
         place id=tail use=hut theme=plain east_of=gap gap=4\n"
    );
    let codes: Vec<&str> = diagnose(&src).iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes.iter().filter(|c| **c == "E_INCOMPLETE_PLACE").count(),
        1,
        "only the row missing a key is reported, got {codes:?}",
    );
    assert!(
        !codes.contains(&"E_UNRESOLVED_PLACE_REF"),
        "`east_of=gap` must still resolve, got {codes:?}",
    );
}

/// A `connect` naming an incomplete place keeps its own cascade. The two
/// findings are not a duplicate: one says which row is short of a key, the
/// other which walkway went missing because of it — the same pairing an
/// unresolved `use=DEF` already produces.
#[test]
fn ip_7_a_connect_naming_the_row_still_cascades() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         place id=peer theme=plain east_of=anchor gap=4\n  \
         connect anchor.entry to peer.entry path=@gravel\n"
    );
    let codes: Vec<&str> = diagnose(&src).iter().map(|d| d.code.as_str()).collect();
    assert!(codes.contains(&"E_INCOMPLETE_PLACE"), "got {codes:?}");
    assert!(codes.contains(&"W_DEFERRED_CONNECT"), "got {codes:?}");
}

/// The premise, measured on the build rather than on the diagnostic: the
/// row really does produce no placement. Every assertion above filters to
/// a code and never lowers, so none of them would notice if an incomplete
/// row started building something.
#[test]
fn ip_8_an_incomplete_row_builds_nothing() {
    let build = |source: &str| {
        let module = parse(source).expect("parse");
        let ir = lower(&module);
        let resolution = resolve(&ir, None);
        lower_to_block_array(&ir, &resolution, None)
            .placements
            .len()
    };
    let complete = site_with("place id=b use=hut theme=plain east_of=anchor gap=4");
    assert_eq!(build(&complete), 2, "baseline: both rows build");
    for row in [
        "place           use=hut theme=plain east_of=anchor gap=4",
        "place id=b               theme=plain east_of=anchor gap=4",
        "place id=b      use=hut              east_of=anchor gap=4",
    ] {
        assert_eq!(
            build(&site_with(row)),
            1,
            "`{row}` must not reach the build",
        );
    }
}
