//! `@cairn`: `W_INVALID_CAIRN_VERSION` for a value that is not a calver, `W_FUTURE_CAIRN_VERSION`
//! for one newer than this compiler, and a note on the parse error of a file that does not parse.

use cairn_lang_core::ast::Header;
use cairn_lang_core::calver::{LanguageVersion, parse_language_version};
use cairn_lang_core::check::{DiagnosticCode, LineStarts, Severity};
use cairn_lang_core::{CAIRN_VERSION, Diagnostic, diagnose_parse_failure, parse};

use crate::common::{codes, diagnose, with_header};

/// The one finding of `code` this header earns.
fn only(header: &str, code: DiagnosticCode) -> Diagnostic {
    let found = diagnose(&with_header(header));
    let mut matching: Vec<_> = found.iter().filter(|d| d.code == code).cloned().collect();
    assert_eq!(
        matching.len(),
        1,
        "{header:?} should report exactly one {}, got {:?}",
        code.as_str(),
        codes(&with_header(header)),
    );
    matching.remove(0)
}

// -- the shape ------------------------------------------------------------

#[test]
fn the_spelling_every_shipped_file_uses_is_accepted() {
    // `2026.06` is calver's `YYYY.0M`, and the crate's own release version is `YYYY.M`.
    for header in [
        "@cairn 2026.06\n",
        "@cairn 2026.6\n",
        "@cairn 2026.6.2\n",
        "@cairn 2026.12\n",
        "@cairn 2026.01.0\n",
    ] {
        assert!(
            !codes(&with_header(header)).contains(&"W_INVALID_CAIRN_VERSION"),
            "{header:?} is a language version",
        );
    }
}

#[test]
fn a_value_that_is_not_a_calver_is_reported() {
    for (header, fragment) in [
        ("@cairn banana\n", "two or three"),
        ("@cairn 2026\n", "two or three"),
        ("@cairn 2026.06.1.2\n", "two or three"),
        ("@cairn 2026.\n", "digits"),
        ("@cairn .6\n", "digits"),
        ("@cairn 2026.6.x\n", "digits"),
        ("@cairn 20261.6\n", "four digits"),
        ("@cairn 2026.13\n", "1 and 12"),
        ("@cairn 2026.0\n", "1 and 12"),
        ("@cairn 2026.6.99999999999999999999\n", "too large"),
        ("@cairn 2026.06 draft\n", "follows the version"),
    ] {
        let found = only(header, DiagnosticCode::InvalidCairnVersion);
        assert_eq!(found.severity(), Severity::Warning, "{header:?}");
        assert!(
            found.primary.contains(fragment),
            "{header:?} should say why: {}",
            found.primary,
        );
    }
}

#[test]
fn an_empty_component_is_described_rather_than_quoted() {
    let found = only("@cairn 2026.\n", DiagnosticCode::InvalidCairnVersion);
    assert!(
        found.primary.contains("empty component") && !found.primary.contains("``"),
        "the empty component is described: {}",
        found.primary,
    );
}

#[test]
fn a_second_word_on_the_line_is_reported_as_one() {
    let found = only(
        "@cairn 2026.06 draft\n",
        DiagnosticCode::InvalidCairnVersion,
    );
    let json = serde_json::to_value(found.data.expect("a payload")).expect("serialises");
    assert_eq!(json["reason"], "trailing_tokens");
    assert_eq!(json["found"], "draft");
}

#[test]
fn the_finding_lands_on_the_directive_that_carries_it() {
    let header = "@cairn banana\n";
    let found = only(header, DiagnosticCode::InvalidCairnVersion);
    let source = with_header(header);
    assert_eq!(&source[found.span.clone()], "@cairn banana");
}

// -- the compiler's own version ------------------------------------------

#[test]
fn a_version_newer_than_this_compiler_is_reported() {
    let header = "@cairn 9999.12\n";
    let found = only(header, DiagnosticCode::FutureCairnVersion);
    assert_eq!(found.severity(), Severity::Warning);
    assert!(
        found.primary.contains("9999.12") && found.primary.contains(CAIRN_VERSION),
        "the finding names both versions: {}",
        found.primary,
    );
}

#[test]
fn a_version_this_compiler_is_at_or_past_is_not_reported() {
    // Including the boundary: `CAIRN_VERSION` itself is not future.
    for header in [
        "@cairn 2026.06\n".to_owned(),
        "@cairn 1970.1\n".to_owned(),
        format!("@cairn {CAIRN_VERSION}\n"),
    ] {
        assert!(
            !codes(&with_header(&header)).contains(&"W_FUTURE_CAIRN_VERSION"),
            "{header:?} is not a future version",
        );
    }
}

#[test]
fn an_absent_patch_compares_as_zero() {
    // Asked of the comparison directly, so no verdict pins a version `release-plz` moves.
    let two = parse_language_version("2026.9").expect("a language version");
    let three = parse_language_version("2026.9.2").expect("a language version");
    assert_eq!(two, LanguageVersion::new(2026, 9, 0));
    assert!(two < three, "an absent patch is the earliest of its month");
    assert!(
        !two.is_newer_than(&three),
        "`2026.9` is not newer than `2026.9.2`",
    );
    assert!(three.is_newer_than(&two));
}

#[test]
fn the_leading_zero_does_not_change_the_month() {
    let padded = parse_language_version("2026.06").expect("a language version");
    let bare = parse_language_version("2026.6").expect("a language version");
    assert_eq!(padded, bare);
}

#[test]
fn this_compilers_own_version_is_a_language_version() {
    parse_language_version(CAIRN_VERSION)
        .unwrap_or_else(|e| panic!("`{CAIRN_VERSION}` must be a language version: {e}"));
}

// -- the two codes do not overlap ----------------------------------------

#[test]
fn a_value_that_is_not_a_version_has_no_version_to_compare() {
    let found = codes(&with_header("@cairn banana\n"));
    assert!(found.contains(&"W_INVALID_CAIRN_VERSION"), "{found:?}");
    assert!(!found.contains(&"W_FUTURE_CAIRN_VERSION"), "{found:?}");
}

#[test]
fn each_repeated_header_is_judged_on_its_own_line() {
    let header = "@cairn banana\n@cairn 9999.12\n";
    let found = codes(&with_header(header));
    assert!(found.contains(&"E_DUPLICATE_HEADER"), "{found:?}");
    let invalid = only(header, DiagnosticCode::InvalidCairnVersion);
    let future = only(header, DiagnosticCode::FutureCairnVersion);
    let source = with_header(header);
    assert_eq!(&source[invalid.span.clone()], "@cairn banana");
    assert_eq!(&source[future.span.clone()], "@cairn 9999.12");
}

// -- what the finding carries --------------------------------------------

#[test]
fn both_codes_carry_a_payload_a_consumer_can_act_on() {
    // `spec/lint` "Machine-readable payload".
    let invalid = only("@cairn 2026.13\n", DiagnosticCode::InvalidCairnVersion);
    let json = serde_json::to_value(invalid.data.expect("a payload")).expect("serialises");
    assert_eq!(json["kind"], "invalid_cairn_version");
    assert_eq!(json["reason"], "month_out_of_range");
    assert_eq!(json["found"], "13");

    let future = only("@cairn 9999.12\n", DiagnosticCode::FutureCairnVersion);
    let json = serde_json::to_value(future.data.expect("a payload")).expect("serialises");
    assert_eq!(json["kind"], "future_cairn_version");
    assert_eq!(json["declared"], "9999.12");
    assert_eq!(json["compiler"], CAIRN_VERSION);
}

#[test]
fn a_module_with_no_cairn_header_is_silent() {
    let found = codes(&with_header(""));
    assert!(!found.contains(&"W_INVALID_CAIRN_VERSION"), "{found:?}");
    assert!(!found.contains(&"W_FUTURE_CAIRN_VERSION"), "{found:?}");
}

// -- the file the header explains best -----------------------------------
// `check` never sees a source that does not parse, so the gap is a note on the parse error.

/// The diagnostic a source that does not parse renders as.
fn parse_failure(source: &str) -> Diagnostic {
    let err = parse(source).expect_err("the fixture is supposed to be refused");
    diagnose_parse_failure(source, &LineStarts::new(source), &err)
}

/// Whether the parse failure for `source` carries the version note.
fn notes_the_version_gap(source: &str) -> bool {
    parse_failure(source)
        .notes
        .iter()
        .any(|n| n.message.contains("newer than this build"))
}

#[test]
fn a_new_syntactic_form_under_a_future_header_says_the_gap_may_explain_it() {
    // A directive and a top-level item this build does not have.
    for source in [
        "@cairn 9999.12\n@materials \"x\"\n",
        "@cairn 9999.12\nmaterials m:\n  slot a -> @b\n",
    ] {
        let diagnostic = parse_failure(source);
        assert_eq!(diagnostic.code.as_str(), "E_PARSE", "{source:?}");
        assert_eq!(
            diagnostic.notes.len(),
            1,
            "one finding, one note: {:?}",
            diagnostic.notes,
        );
        let note = &diagnostic.notes[0];
        assert!(
            note.message.contains("9999.12") && note.message.contains(CAIRN_VERSION),
            "the note names both versions: {}",
            note.message,
        );
        // At the header, not the failure: the failure already has the primary.
        let span = note.span.clone().expect("the note points at the header");
        assert_eq!(&source[span], "@cairn 9999.12");
    }
}

#[test]
fn a_source_that_does_not_even_lex_still_has_its_header_read() {
    assert!(notes_the_version_gap(
        "@cairn 9999.12\nstruct s size=3x3\n  floor a=%\n",
    ));
}

#[test]
fn a_header_this_build_can_read_adds_nothing() {
    for header in [format!("@cairn {CAIRN_VERSION}\n"), String::new()] {
        let source = format!("{header}@materials \"x\"\n");
        assert!(
            parse_failure(&source).notes.is_empty(),
            "should be silent: {source:?}",
        );
    }
}

/// `W_INVALID_CAIRN_VERSION` is a check pass's, which does not run on a source that fails to parse.
#[test]
fn a_header_that_names_no_version_says_it_cannot_judge_the_gap() {
    let source = "@cairn banana\n@materials \"x\"\n";
    let diagnostic = parse_failure(source);
    assert!(!notes_the_version_gap(source), "no gap is being claimed");
    let note = diagnostic.notes.first().expect("the note");
    assert!(
        note.message
            .contains("`@cairn banana` does not name a language version"),
        "{}",
        note.message,
    );
    let span = note.span.clone().expect("a span");
    assert_eq!(&source[span], "@cairn banana");
}

/// A lone `\r` is a line break Cairn accepts.
#[test]
fn a_file_broken_by_lone_carriage_returns_is_still_read() {
    assert!(notes_the_version_gap(
        "@cairn 9999.12\rstruct s size=3x3\r  floor a=%\r",
    ));
}

#[test]
fn a_leading_byte_order_mark_does_not_hide_the_header() {
    assert!(notes_the_version_gap(
        "\u{feff}@cairn 9999.12\nstruct s size=3x3\n  floor a=%\n",
    ));
}

/// The lexer takes only `' '` between a directive and its value.
#[test]
fn whitespace_the_lexer_refuses_declares_no_version() {
    for source in [
        "@cairn\t9999.12\nstruct s size=2x2\n  floor mat_slot=f\n",
        "@cairn\u{a0}9999.12\nstruct s size=2x2\n  floor mat_slot=f\n",
        // One token, so a second one is not part of the value either.
        "@cairn 9999.12 spare\nstruct s size=2x2\n  floor a=%\n",
    ] {
        assert!(
            parse_failure(source).notes.is_empty(),
            "the file declares nothing here: {source:?}",
        );
    }
}

#[test]
fn a_cairn_line_that_is_not_a_header_is_not_read() {
    for source in [
        // Below a declaration, where the header block has ended.
        "struct s size=3x3\n  floor mat_slot=f\n@cairn 9999.12\n@materials \"x\"\n",
        // Indented, which is a body row to the lexer rather than a header.
        "  @cairn 9999.12\n@materials \"x\"\n",
    ] {
        assert!(
            !notes_the_version_gap(source),
            "should be silent: {source:?}",
        );
    }
}

#[test]
fn the_header_is_read_past_the_layout_a_file_may_open_with() {
    let source = "# what this file is\n\n@requires \">=2026.9\"\n@cairn 9999.12 # written against\n\n@materials \"x\"\n";
    let diagnostic = parse_failure(source);
    let note = diagnostic.notes.first().expect("the note");
    assert!(note.message.contains("9999.12"), "{}", note.message);
    let span = note.span.clone().expect("a span");
    assert_eq!(&source[span], "@cairn 9999.12");
}

#[test]
fn a_file_that_parses_reports_the_gap_through_the_pass_instead() {
    let found = codes(&with_header("@cairn 9999.12\n"));
    assert!(found.contains(&"W_FUTURE_CAIRN_VERSION"), "{found:?}");
    assert!(parse(&with_header("@cairn 9999.12\n")).is_ok());
}

#[test]
fn the_note_underlines_what_the_parser_calls_the_header() {
    for source in [
        "@cairn 9999.12\n",
        "# a comment first\n\n@requires \">=2026.9\"\n@cairn 9999.12 # and a trailing one\n",
    ] {
        let module = parse(source).expect("this half of the fixture has to parse");
        let header = module
            .headers
            .iter()
            .find(|h| matches!(h, Header::Cairn { .. }))
            .expect("the fixture declares one");
        // The same source with a body the lexer refuses.
        let unparsable = format!("{source}struct s size=3x3\n  floor a=%\n");
        let note = parse_failure(&unparsable)
            .notes
            .into_iter()
            .next()
            .expect("the note");
        assert_eq!(
            note.span.expect("a span"),
            *header.span(),
            "the scan and the parser disagree about the header in {source:?}",
        );
    }
}
