//! `E_INCOMPLETE_PLACE`: a `place` row needs `id=`, `use=` and `theme=` before
//! anything can be built from it, and a row short of any of them is an error
//! naming every key it lacks. The keys are never filled in: `id=` is the name
//! `east_of=`, `connect` and the output `.nbt` use, so an invented one would be
//! a name the author cannot point at.

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::{DiagnosticCode, Severity, lower, parse, resolve};

use crate::common::{PRELUDE, diagnose, notes, of_code, one_of_code};

/// A site whose first row anchors at the origin, so the row under test is
/// the only thing that can be wrong.
fn site_with(row: &str) -> String {
    format!("{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  {row}\n")
}

/// Every arity of the missing-key list, with the exact sentence each
/// renders. Table-driven because the join has three branches and the two-key
/// one — the shape a user hits most, writing the position first and the body
/// later — is the one that read `` `id=`, and `use=` `` until a reviewer
/// caught it: the serial comma is a three-or-more rule.
#[test]
fn the_missing_key_list_reads_at_every_arity() {
    for (row, listed) in [
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
        (
            "place id=b                           east_of=anchor gap=4",
            "`use=` and `theme=`",
        ),
        (
            "place                    theme=plain east_of=anchor gap=4",
            "`id=` and `use=`",
        ),
        (
            "place           use=hut              east_of=anchor gap=4",
            "`id=` and `theme=`",
        ),
        (
            "place                                east_of=anchor gap=4",
            "`id=`, `use=`, and `theme=`",
        ),
    ] {
        let src = site_with(row);
        let d = one_of_code(&src, DiagnosticCode::IncompletePlace);
        assert_eq!(d.severity(), Severity::Error, "for `{row}`");
        let subject = if row.contains("id=b") {
            "`place id=b`"
        } else {
            "`place`"
        };
        assert_eq!(
            d.primary,
            format!("{subject} in site `duo` is missing {listed}, so no placement is built for it"),
        );
    }
}

/// Each note says what its own key is for. Without this the note bodies are
/// pinned by nothing — `every_missing_key_is_named_at_once` counts them and `diagnostic_text.rs` only
/// checks formatting — so a build emitting the `theme=` sentence on a
/// `use=`-less row passes the whole suite.
#[test]
fn each_note_explains_the_key_it_belongs_to() {
    for (row, needle) in [
        (
            "place           use=hut theme=plain east_of=anchor gap=4",
            "`id=` is the name every `east_of=` and `connect` refers to",
        ),
        (
            "place id=b               theme=plain east_of=anchor gap=4",
            "`use=DEF` names the `def` this placement instantiates",
        ),
        (
            "place id=b      use=hut              east_of=anchor gap=4",
            "`theme=NAME` names the theme",
        ),
    ] {
        let src = site_with(row);
        let d = one_of_code(&src, DiagnosticCode::IncompletePlace);
        assert_eq!(notes(&d).len(), 1, "for `{row}`");
        assert!(
            notes(&d)[0].starts_with(needle),
            "for `{row}`, got {:?}",
            notes(&d)[0],
        );
    }
}

/// The structured payload carries the key set, so a quick-fix does not have
/// to parse the sentence back apart — which is what
/// `spec/lint` "Machine-readable payload" exists to stop consumers doing.
#[test]
fn the_payload_lists_the_missing_keys() {
    let src = site_with("place east_of=anchor gap=4");
    let d = one_of_code(&src, DiagnosticCode::IncompletePlace);
    let value = serde_json::to_value(&d.data).expect("serialise payload");
    assert_eq!(
        value,
        serde_json::json!({"kind": "incomplete_place", "missing": ["id", "use", "theme"]}),
    );
}

/// A row short of every key is one finding, not three: the author fixes
/// one line once rather than re-running the compiler to discover the next
/// omission.
#[test]
fn every_missing_key_is_named_at_once() {
    let src = site_with("place east_of=anchor gap=4");
    assert_eq!(
        of_code(&src, DiagnosticCode::IncompletePlace).len(),
        1,
        "one finding, not one per key",
    );
    assert_eq!(
        notes(&one_of_code(&src, DiagnosticCode::IncompletePlace)).len(),
        3,
        "and one note per key, saying what each is for",
    );
}

/// The negative space: a site whose every row is complete says nothing.
/// The tests above already rule out a report-everything implementation —
/// `one_of_code(, DiagnosticCode::IncompletePlace)` counts across the whole source, and `site_with`'s anchor row is
/// complete — so what this adds is the case with no incomplete row at all.
#[test]
fn a_complete_row_is_not_reported() {
    let src = site_with("place id=b use=hut theme=plain east_of=anchor gap=4");
    assert!(
        of_code(&src, DiagnosticCode::IncompletePlace).is_empty(),
        "got {:#?}",
        of_code(&src, DiagnosticCode::IncompletePlace),
    );
}

/// Absence is read off the surface key, not off the lifted value. A
/// mistyped key is present — calling it missing would send the author to
/// add a key that is already on the line, and `E_TYPE_MISMATCH_LABEL`
/// already names it.
#[test]
fn a_mistyped_key_is_not_a_missing_one() {
    for row in [
        "place id=3 use=hut theme=plain east_of=anchor gap=4",
        "place id=b use=3   theme=plain east_of=anchor gap=4",
        "place id=b use=hut theme=7     east_of=anchor gap=4",
    ] {
        let src = site_with(row);
        assert!(
            of_code(&src, DiagnosticCode::IncompletePlace).is_empty(),
            "`{row}` is mistyped, not incomplete, got {:#?}",
            of_code(&src, DiagnosticCode::IncompletePlace),
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
fn the_span_underlines_the_offending_row() {
    let src = site_with("place id=b use=hut east_of=anchor gap=4");
    let d = one_of_code(&src, DiagnosticCode::IncompletePlace);
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
fn an_incomplete_row_with_an_id_still_registers_its_name() {
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
fn a_connect_naming_the_row_still_cascades() {
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
fn an_incomplete_row_builds_nothing() {
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
    // The boundary the anchor row hides: a site whose only row is
    // incomplete builds nothing at all, not "one fewer than expected".
    let alone = format!("{PRELUDE}site solo:\n  place id=lonely theme=plain at=origin\n");
    assert_eq!(build(&alone), 0);
}

/// Every incomplete row in a site is reported, not just the first. This is
/// the row-wise version of the loop the code closes: a `reported_once` flag
/// would leave the second author-written row as invisible as it was before.
#[test]
fn every_incomplete_row_is_reported() {
    let src = format!(
        "{PRELUDE}site many:\n  place id=anchor use=hut theme=plain at=origin\n  \
         place id=b theme=plain east_of=anchor gap=4\n  \
         place id=c use=hut east_of=anchor gap=8\n  \
         place east_of=anchor gap=12\n"
    );
    assert_eq!(
        of_code(&src, DiagnosticCode::IncompletePlace).len(),
        3,
        "got {:#?}",
        of_code(&src, DiagnosticCode::IncompletePlace),
    );
}

/// The finding coexists with the other `place`-row errors rather than
/// masking them or being masked. Each pair is a different check on a
/// `place` row, and each is on the same line the author has
/// to edit — so batching them is the same promise the multi-key message
/// makes, extended across codes.
#[test]
fn it_coexists_with_the_other_place_row_errors() {
    for (row, other) in [
        // The id guards run after the completeness check and report on
        // their own.
        (
            "place id=\"bad.id\" theme=plain east_of=anchor gap=4",
            "E_INVALID_PLACE_ID",
        ),
        (
            "place id=anchor theme=plain east_of=anchor gap=4",
            "E_DUPLICATE_PLACE_ID",
        ),
        // The origin check sits behind the `id=` gate, so a row with no
        // `id=` at all is the shape that could lose it: adding the `id=`
        // this code asks for would surface a brand-new error on the line
        // just fixed.
        (
            "place use=hut theme=plain at=middle",
            "E_INVALID_PLACE_ORIGIN",
        ),
        ("place id=b use=hut theme=plain", "E_INVALID_PLACE_ORIGIN"),
    ] {
        let src = site_with(row);
        let codes: Vec<&str> = diagnose(&src).iter().map(|d| d.code.as_str()).collect();
        assert!(
            codes.contains(&other),
            "`{row}` should also report {other}, got {codes:?}",
        );
    }
}
