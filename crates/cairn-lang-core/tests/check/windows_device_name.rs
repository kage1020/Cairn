//! A `place id=` or `struct` name that Windows reserves for a device names an artifact no Windows
//! host can write, so `check` refuses it on every host; a site name or port id never names a whole
//! artifact, so it may spell one.

use cairn_lang_core::{Diagnostic, DiagnosticCode, Severity, artifact_stem};

use crate::common::{PRELUDE, codes, diagnose, lowered, notes, one_of_code, slice};

/// [`PRELUDE`] and a site of `rows`.
fn site_of(rows: &[&str]) -> String {
    let mut source = format!("{PRELUDE}site s:\n");
    for row in rows {
        source.push_str("  ");
        source.push_str(row);
        source.push('\n');
    }
    source
}

/// Assert `place id=<written>` is refused as the device `id`, and as nothing else.
fn assert_place_id_refused_as_a_device(written: &str, id: &str) {
    let row = format!("place id={written} use=hut theme=plain at=origin");
    let source = site_of(&[&row]);
    assert_eq!(codes(&source), ["E_INVALID_PLACE_ID"], "{id}");
    let found = one_of_code(&source, DiagnosticCode::InvalidPlaceId);
    assert_eq!(slice(&source, &found), row, "{id}");
    assert_eq!(
        found.primary,
        format!(
            "`place id={id}` in site `s` is not a usable id: it is a device name Windows reserves"
        ),
    );
}

#[test]
fn a_place_id_named_con_prn_aux_or_nul_is_refused_in_any_case() {
    for id in ["CON", "con", "Prn", "aUX", "nul"] {
        assert_place_id_refused_as_a_device(id, id);
    }
}

#[test]
fn a_place_id_named_com_or_lpt_and_a_digit_is_refused_in_any_case() {
    for id in ["COM0", "com1", "Com9", "LPT0", "lpt5", "lPt9"] {
        assert_place_id_refused_as_a_device(id, id);
    }
}

/// Only a string literal can carry the superscript.
#[test]
fn a_place_id_named_com_or_lpt_and_a_superscript_digit_is_refused_in_any_case() {
    for id in ["COM¹", "com²", "LPT³", "Lpt¹"] {
        assert_place_id_refused_as_a_device(&format!("\"{id}\""), id);
    }
}

#[test]
fn the_place_id_refusal_says_why_and_how_to_fix_it() {
    let source = site_of(&["place id=nul use=hut theme=plain at=origin"]);
    let found = one_of_code(&source, DiagnosticCode::InvalidPlaceId);
    assert_eq!(found.severity(), Severity::Error);
    assert_eq!(
        notes(&found),
        [
            "a place id is the stem of the artifact file written into `--out`, and Windows \
             reserves `CON`, `PRN`, `AUX`, `NUL`, and `COM` or `LPT` followed by one digit, in \
             any letter case, as device names no file can take, so the id is refused on every \
             host, not only on Windows; rename it, for example to `nul_1`"
        ],
    );
    assert_eq!(found.notes[0].span, None);
}

#[test]
fn the_place_id_the_refusal_recommends_is_usable() {
    let source = site_of(&["place id=nul_1 use=hut theme=plain at=origin"]);
    assert_eq!(codes(&source), Vec::<&str>::new());
}

#[test]
fn a_place_id_that_only_resembles_a_device_is_usable() {
    for id in ["console", "CON1", "NULL", "COM", "COM10", "\"COM⁴\""] {
        let source = site_of(&[&format!("place id={id} use=hut theme=plain at=origin")]);
        assert_eq!(codes(&source), Vec::<&str>::new(), "{id}");
    }
}

/// `(code, line)` of every finding, with each note's line after its finding's.
fn findings_with_note_lines(source: &str) -> Vec<(&'static str, usize, Vec<usize>)> {
    let line_of = |at: usize| source[..at].matches('\n').count() + 1;
    diagnose(source)
        .iter()
        .map(|d: &Diagnostic| {
            let note_lines = d
                .notes
                .iter()
                .filter_map(|n| n.span.as_ref().map(|span| line_of(span.start)))
                .collect();
            (d.code.as_str(), line_of(d.span.start), note_lines)
        })
        .collect()
}

/// The first site row of [`site_of`] is on this line.
const FIRST_ROW: usize = 11;

#[test]
fn an_origin_naming_a_place_refused_as_a_device_is_deferred_to_that_row() {
    let source = site_of(&[
        "place id=nul use=hut theme=plain at=origin",
        "place id=b use=hut theme=plain east_of=nul gap=4",
    ]);
    assert_eq!(
        findings_with_note_lines(&source),
        [
            ("E_INVALID_PLACE_ID", FIRST_ROW, vec![]),
            ("W_DEFERRED_PLACE", FIRST_ROW + 1, vec![FIRST_ROW]),
        ],
    );
}

#[test]
fn a_connect_naming_a_place_refused_as_a_device_is_deferred_to_that_row() {
    let source = site_of(&[
        "place id=b use=hut theme=plain at=origin",
        "place id=nul use=hut theme=plain east_of=b gap=4",
        "connect b.entry to nul.entry path=@oak_planks",
    ]);
    assert_eq!(
        findings_with_note_lines(&source),
        [
            ("E_INVALID_PLACE_ID", FIRST_ROW + 1, vec![]),
            ("W_DEFERRED_CONNECT", FIRST_ROW + 2, vec![FIRST_ROW + 1]),
        ],
    );
    let deferred = one_of_code(&source, DiagnosticCode::DeferredConnect);
    assert_eq!(slice(&source, &deferred), "nul.entry");
    assert_eq!(
        deferred.primary,
        "`connect` target `nul.entry` names a `place` row refused for its id; no walkway laid",
    );
    assert_eq!(
        notes(&deferred),
        [
            "that row carries its own diagnostic saying what is wrong with its id; renaming it, \
             and this endpoint with it, lays this walkway"
        ],
    );
}

#[test]
fn a_connect_above_the_place_refused_as_a_device_names_no_prior_place() {
    let source = site_of(&[
        "place id=b use=hut theme=plain at=origin",
        "connect b.entry to nul.entry path=@oak_planks",
        "place id=nul use=hut theme=plain east_of=b gap=4",
    ]);
    let found: Vec<(&str, usize)> = findings_with_note_lines(&source)
        .into_iter()
        .map(|(code, line, _)| (code, line))
        .collect();
    assert_eq!(
        found,
        [
            ("E_UNRESOLVED_PLACE_REF", FIRST_ROW + 1),
            ("E_INVALID_PLACE_ID", FIRST_ROW + 2),
        ],
    );
}

/// A struct alone, so no `def` goes unused.
fn struct_named(name: &str) -> String {
    format!(
        "theme plain:\n  slot floor -> @oak_planks\n\nstruct {name} size=3x3\n  floor mat_slot=floor\n"
    )
}

#[test]
fn a_struct_named_con_prn_aux_or_nul_is_refused_in_any_case() {
    for name in ["CON", "con", "Prn", "aUX", "nul"] {
        assert_struct_refused_as_a_device(name);
    }
}

#[test]
fn a_struct_named_com_or_lpt_and_a_digit_is_refused_in_any_case() {
    for name in ["COM0", "com1", "Com9", "LPT0", "lpt5", "lPt9"] {
        assert_struct_refused_as_a_device(name);
    }
}

fn assert_struct_refused_as_a_device(name: &str) {
    let source = struct_named(name);
    assert_eq!(codes(&source), ["E_INVALID_STRUCT_NAME"], "{name}");
    let found = one_of_code(&source, DiagnosticCode::InvalidStructName);
    assert_eq!(slice(&source, &found), name);
    assert_eq!(
        found.primary,
        format!("`struct {name}` is not a usable name: it is a device name Windows reserves"),
    );
}

#[test]
fn the_struct_name_refusal_says_why_and_how_to_fix_it() {
    let found = one_of_code(&struct_named("Aux"), DiagnosticCode::InvalidStructName);
    assert_eq!(found.severity(), Severity::Error);
    assert_eq!(
        notes(&found),
        [
            "a struct's name is the stem of the artifact file written into `--out`, and Windows \
             reserves `CON`, `PRN`, `AUX`, `NUL`, and `COM` or `LPT` followed by one digit, in \
             any letter case, as device names no file can take, so the name is refused on every \
             host, not only on Windows; rename it, for example to `Aux_1`"
        ],
    );
    assert_eq!(found.notes[0].span, None);
    assert_eq!(codes(&struct_named("Aux_1")), Vec::<&str>::new());
}

#[test]
fn a_struct_whose_name_only_resembles_a_device_is_not_refused() {
    for name in ["console", "CON1", "NULL", "COM", "COM10", "lpt_1"] {
        assert_eq!(codes(&struct_named(name)), Vec::<&str>::new(), "{name}");
    }
}

/// Each is one part of a walkway's name, which a device name never is.
#[test]
fn a_site_and_a_port_may_spell_a_device() {
    let source = "theme plain:\n  slot floor -> @oak_planks\n  slot wall  -> @cobblestone\n\n\
         def hut size=3x3:\n  floor id=floor mat_slot=floor\n  \
         walls id=walls class=outer mat_slot=wall height=3\n  door id=nul side=front at=center\n\n\
         site con:\n  place id=a use=hut theme=plain at=origin\n  \
         place id=b use=hut theme=plain east_of=a gap=4\n  connect a.nul to b.nul path=@oak_planks\n";
    assert_eq!(codes(source), Vec::<&str>::new());
    let out = lowered(source);
    assert_eq!(out.diagnostics, Vec::new());
    let stems: Vec<String> = out.structures.keys().map(|k| artifact_stem(k)).collect();
    assert_eq!(stems, ["a", "b", "con_walkway_a_nul__b_nul"]);
}
