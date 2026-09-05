//! What `@cairn` accepts, and what it says when it cannot read the value.
//!
//! The directive declares the Cairn language version a file was written
//! against. `spec/index.md` calls it "provenance only, so a future compiler
//! can parse and warn correctly" — a job that needs the value to be
//! readable as a version, which nothing checked. `@cairn banana` and
//! `@cairn 2026.06.1.2` compiled in silence, and so did a file declaring a
//! language newer than the compiler reading it, which is the one case where
//! the header has something to say about the findings around it.
//!
//! Both codes are warnings. The value reaches no pass, no palette and no
//! lockfile field, so `spec/lint` §11.3's error test — leaving it alone
//! yields something other than what the source asked for — is not met: the
//! artifact is identical either way. `@requires` is an error for the
//! opposite reason, its floor being folded into the compatible range and
//! the `--target` gate.

use cairn_lang_core::calver::{LanguageVersion, parse_language_version};
use cairn_lang_core::check::{DiagnosticCode, Severity};
use cairn_lang_core::{CAIRN_VERSION, Diagnostic, check, lower, parse};

/// The body every fixture shares, so the header is the only variable.
const BODY: &str = "struct s size=2x2\n  floor mat_slot=f\n";

fn diagnostics(header: &str) -> Vec<Diagnostic> {
    let source = format!("{header}{BODY}");
    let module = parse(&source).expect("the fixtures all parse");
    let ir = lower(&module);
    check(&module, &ir, None)
}

fn codes(header: &str) -> Vec<&'static str> {
    diagnostics(header)
        .iter()
        .map(|d| d.code.as_str())
        .collect()
}

/// The one finding of `code` this header earns.
fn only(header: &str, code: DiagnosticCode) -> Diagnostic {
    let found = diagnostics(header);
    let mut matching: Vec<_> = found.iter().filter(|d| d.code == code).cloned().collect();
    assert_eq!(
        matching.len(),
        1,
        "{header:?} should report exactly one {}, got {:?}",
        code.as_str(),
        codes(header),
    );
    matching.remove(0)
}

// -- the shape ------------------------------------------------------------

#[test]
fn the_spelling_every_shipped_file_uses_is_accepted() {
    // AC1. `2026.06` is calver's `YYYY.0M`; the crate's own release version
    // is `YYYY.M`. Both are in this repo today and both are read.
    for header in [
        "@cairn 2026.06\n",
        "@cairn 2026.6\n",
        "@cairn 2026.6.2\n",
        "@cairn 2026.12\n",
        "@cairn 2026.01.0\n",
    ] {
        assert!(
            !codes(header).contains(&"W_INVALID_CAIRN_VERSION"),
            "{header:?} is a language version",
        );
    }
}

#[test]
fn a_value_that_is_not_a_calver_is_reported() {
    // AC2. Each of these reaches a different named failure, so a consumer
    // choosing a repair does not have to read the sentence.
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
    // The message reaches an author, and `` `` `` in a sentence tells
    // them nothing about which part of the line to fix.
    let found = only("@cairn 2026.\n", DiagnosticCode::InvalidCairnVersion);
    assert!(
        found.primary.contains("empty component") && !found.primary.contains("``"),
        "the empty component is described: {}",
        found.primary,
    );
}

#[test]
fn a_second_word_on_the_line_is_reported_as_one() {
    // `@cairn` takes the rest of the line, so an interior space is the
    // one way a second word arrives. It used to travel into the payload
    // as a "component" of `06 draft`, which a quick-fix consumer would
    // have used as a replacement range.
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
    // The span is the whole `@cairn …` line, the way `@requires` anchors.
    let header = "@cairn banana\n";
    let found = only(header, DiagnosticCode::InvalidCairnVersion);
    let source = format!("{header}{BODY}");
    assert_eq!(&source[found.span.clone()], "@cairn banana");
}

// -- the compiler's own version ------------------------------------------

#[test]
fn a_version_newer_than_this_compiler_is_reported() {
    // AC3. The one thing an older compiler can usefully say about a file
    // written against a newer language.
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
    // AC4. Including the boundary: `CAIRN_VERSION` itself is not future.
    for header in [
        "@cairn 2026.06\n".to_owned(),
        "@cairn 1970.1\n".to_owned(),
        format!("@cairn {CAIRN_VERSION}\n"),
    ] {
        assert!(
            !codes(&header).contains(&"W_FUTURE_CAIRN_VERSION"),
            "{header:?} is not a future version",
        );
    }
}

#[test]
fn an_absent_patch_compares_as_zero() {
    // AC4's boundary, asked of the comparison directly so no test pins a
    // verdict against a constant `release-plz` moves.
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
    // AC7. The pass compares against `CAIRN_VERSION`; a release that made
    // that string unparseable would leave the comparison with nothing on
    // one side, and no fixture would notice.
    parse_language_version(CAIRN_VERSION)
        .unwrap_or_else(|e| panic!("`{CAIRN_VERSION}` must be a language version: {e}"));
}

// -- the two codes do not overlap ----------------------------------------

#[test]
fn a_value_that_is_not_a_version_has_no_version_to_compare() {
    // AC5. A malformed value cannot also be a future one; reporting both
    // would tell an author to upgrade over a string that names nothing.
    let found = codes("@cairn banana\n");
    assert!(found.contains(&"W_INVALID_CAIRN_VERSION"), "{found:?}");
    assert!(!found.contains(&"W_FUTURE_CAIRN_VERSION"), "{found:?}");
}

#[test]
fn each_repeated_header_is_judged_on_its_own_line() {
    // AC6. The duplicate pass reports the repeat; this pass still reads
    // both values, because the author has two lines to fix.
    let header = "@cairn banana\n@cairn 9999.12\n";
    let found = codes(header);
    assert!(found.contains(&"E_DUPLICATE_HEADER"), "{found:?}");
    let invalid = only(header, DiagnosticCode::InvalidCairnVersion);
    let future = only(header, DiagnosticCode::FutureCairnVersion);
    let source = format!("{header}{BODY}");
    assert_eq!(&source[invalid.span.clone()], "@cairn banana");
    assert_eq!(&source[future.span.clone()], "@cairn 9999.12");
}

// -- what the finding carries --------------------------------------------

#[test]
fn both_codes_carry_a_payload_a_consumer_can_act_on() {
    // AC9. `spec/lint` §11.2 tells consumers not to parse the prose, so
    // the parts a quick-fix needs travel beside it.
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
    // The directive is optional, and an absent one is not a finding.
    let found = codes("");
    assert!(!found.contains(&"W_INVALID_CAIRN_VERSION"), "{found:?}");
    assert!(!found.contains(&"W_FUTURE_CAIRN_VERSION"), "{found:?}");
}
