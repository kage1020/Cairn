//! `E_INCOMPLETE_PLACE`: a `place` row short of `id=`, `use=` or `theme=` is an error naming every
//! key it lacks, and no key is filled in.

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::{DiagnosticCode, Severity, lower, parse, resolve};

use crate::common::{PRELUDE, diagnose, notes, of_code, one_of_code};

/// A site whose first row anchors at the origin, so only the row under test can be wrong.
fn site_with(row: &str) -> String {
    format!("{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  {row}\n")
}

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

/// The other tests count the notes, and `check/diagnostic_text.rs` checks only their formatting.
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

/// `spec/lint` "Machine-readable payload".
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

#[test]
fn a_complete_row_is_not_reported() {
    let src = site_with("place id=b use=hut theme=plain east_of=anchor gap=4");
    assert!(
        of_code(&src, DiagnosticCode::IncompletePlace).is_empty(),
        "got {:#?}",
        of_code(&src, DiagnosticCode::IncompletePlace),
    );
}

/// Absence is read off the surface key, not off the lifted value.
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

#[test]
fn the_span_underlines_the_offending_row() {
    let src = site_with("place id=b use=hut east_of=anchor gap=4");
    let d = one_of_code(&src, DiagnosticCode::IncompletePlace);
    assert_eq!(
        &src[d.span.clone()],
        "place id=b use=hut east_of=anchor gap=4"
    );
}

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
    // The boundary the anchor row hides: an only row builds nothing at all.
    let alone = format!("{PRELUDE}site solo:\n  place id=lonely theme=plain at=origin\n");
    assert_eq!(build(&alone), 0);
}

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

#[test]
fn it_coexists_with_the_other_place_row_errors() {
    for (row, other) in [
        // The id guards run after the completeness check and report on their own.
        (
            "place id=\"bad.id\" theme=plain east_of=anchor gap=4",
            "E_INVALID_PLACE_ID",
        ),
        (
            "place id=anchor theme=plain east_of=anchor gap=4",
            "E_DUPLICATE_PLACE_ID",
        ),
        // The origin check sits behind the `id=` gate, so a row with no `id=` could lose it.
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
