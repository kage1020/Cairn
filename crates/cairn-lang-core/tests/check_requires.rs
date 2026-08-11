//! What `@requires` accepts, and what it says about the rest.
//!
//! The directive declares a Minecraft capability floor. Every expression
//! this pass does not understand used to be dropped in silence: the floor
//! simply did not exist, `cairn info` printed `0.0 .. latest`, and the
//! author's constraint was gone without a word. A floor that quietly
//! evaporates is worse than no floor, because it is still written in the
//! file and still read by whoever opens it.
//!
//! Only `version>=X` is defined (spec syntax §5.3, versioning-editions
//! §10.4). Anything else is reported rather than ignored — including the
//! shapes an author reasonably reaches for first, since "not supported" is
//! an answer and silence is not.

use cairn_lang_core::check::DiagnosticCode;
use cairn_lang_core::{check, lower, parse};

/// The body every fixture shares, so the header is the only variable.
const BODY: &str = "struct s size=2x2\n  floor mat_slot=f\n";

fn diagnostics(header: &str) -> Vec<cairn_lang_core::Diagnostic> {
    let source = format!("{header}{BODY}");
    let module = parse(&source).expect("the fixtures all parse");
    let ir = lower(&module);
    check(&module, &ir, None)
}

/// Codes reported for a source, as strings, so a failure names them.
fn codes(header: &str) -> Vec<&'static str> {
    diagnostics(header)
        .iter()
        .map(|d| d.code.as_str())
        .collect()
}

/// The `E_INVALID_REQUIRES` message for a header that has exactly one.
fn message(header: &str) -> String {
    let found = diagnostics(header);
    let invalid: Vec<_> = found
        .iter()
        .filter(|d| d.code == DiagnosticCode::InvalidRequires)
        .collect();
    assert_eq!(
        invalid.len(),
        1,
        "{header:?} should report exactly one invalid requirement, got {:?}",
        found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
    );
    invalid[0].primary.clone()
}

/// The strictest floor the module declares, as `cairn info` derives it.
fn floor(header: &str) -> String {
    let source = format!("{header}{BODY}");
    let module = parse(&source).expect("parse");
    cairn_lang_core::resolve::declared_version_floor(&module)
        .map_or_else(|| "(none)".to_owned(), |f| f.version)
}

// -- the grammar ----------------------------------------------------------

/// The reported case: a space around the operator made the floor vanish.
/// `version >= 1.21` is what a human writes first, and the constraint was
/// dropped with `cairn check` exiting 0 and `cairn info` printing `0.0`.
#[test]
fn whitespace_around_the_operator_does_not_change_the_floor() {
    for header in [
        "@requires version>=1.21\n",
        "@requires version >= 1.21\n",
        "@requires version>= 1.21\n",
        "@requires version >=1.21\n",
        "@requires  version  >=  1.21  \n",
    ] {
        assert_eq!(floor(header), "1.21", "{header:?}");
        assert!(
            !codes(header).contains(&"E_INVALID_REQUIRES"),
            "{header:?} is well formed",
        );
    }
}

/// Repeating the directive composes rather than conflicts: the floors fold
/// to the strictest. Spec syntax §5.3 exempts `@requires` from
/// `E_DUPLICATE_HEADER` for exactly this reason.
#[test]
fn repeated_requirements_fold_to_the_strictest() {
    let header = "@requires version>=1.20\n@requires version>=1.21\n@requires version>=1.19\n";
    assert_eq!(floor(header), "1.21");
    assert!(codes(header).is_empty(), "{:?}", codes(header));
}

// -- what it refuses ------------------------------------------------------

/// The other reported case. `version<1.20` is not a constraint Cairn can
/// intersect — only `>=` is defined — and it used to be dropped, so the
/// file read as if it declared an upper bound that was never enforced or
/// even acknowledged.
#[test]
fn an_operator_other_than_at_least_is_refused() {
    for (header, operator) in [
        ("@requires version<1.20\n", "<"),
        ("@requires version<=1.20\n", "<="),
        ("@requires version>1.20\n", ">"),
        ("@requires version==1.20\n", "=="),
        ("@requires version=1.20\n", "="),
    ] {
        let text = message(header);
        assert!(
            text.contains(operator) && text.contains(">="),
            "{header:?} should name the operator it found and the one that works: {text}",
        );
    }
}

#[test]
fn an_empty_payload_is_refused() {
    for header in ["@requires version>=\n", "@requires version >=  \n"] {
        let text = message(header);
        assert!(
            text.contains("version>="),
            "{header:?} should show the shape that works: {text}",
        );
    }
}

/// A non-numeric segment cannot be ordered against a target, and the old
/// lexicographic fallback ordered it wrongly rather than refusing it.
#[test]
fn a_non_numeric_segment_is_refused() {
    for (header, segment) in [
        ("@requires version>=1.a\n", "a"),
        ("@requires version>=1..2\n", ""),
        ("@requires version>=x\n", "x"),
    ] {
        let text = message(header);
        assert!(
            text.contains("1.a") || text.contains("1..2") || text.contains('x'),
            "{header:?} should quote the version it could not read: {text}",
        );
        if !segment.is_empty() {
            assert!(
                text.contains(segment),
                "{header:?} should name the segment `{segment}`: {text}",
            );
        }
    }
}

/// The overflow the audit found: `4294967296` is all ASCII digits, so it
/// passed the old check, and `compare_versions` then fell back to a
/// lexicographic compare that sorted it *below* `999`. Refusing it here is
/// the first of the two guards; `compare_versions` gets the other, because
/// it is public and can be reached without this pass.
#[test]
fn a_segment_too_large_to_compare_is_refused() {
    let text = message("@requires version>=4294967296\n");
    assert!(
        text.contains("4294967296"),
        "the message should name the segment that does not fit: {text}",
    );
}

#[test]
fn trailing_tokens_are_refused() {
    let text = message("@requires version>=1.21 extra\n");
    assert!(
        text.contains("extra"),
        "the message should name what followed the version: {text}",
    );
}

#[test]
fn an_expression_that_is_not_a_version_requirement_is_refused() {
    for header in ["@requires nonsense\n", "@requires 1.21\n"] {
        let text = message(header);
        assert!(
            text.contains("version>="),
            "{header:?} should show the shape that works: {text}",
        );
    }
}

/// A refused requirement declares no floor. Reporting the error and then
/// also honouring half of the expression would be the worst of both.
#[test]
fn a_refused_requirement_declares_no_floor() {
    assert_eq!(floor("@requires version<1.20\n"), "(none)");
    // And it does not suppress a well-formed one on another line.
    let mixed = "@requires version<1.20\n@requires version>=1.21\n";
    assert_eq!(floor(mixed), "1.21");
    assert_eq!(codes(mixed), vec!["E_INVALID_REQUIRES"]);
}

/// The diagnostic has to land on the directive, or an editor underlines
/// the wrong line.
#[test]
fn the_diagnostic_covers_the_directive_that_is_wrong() {
    let header = "@cairn 2026.7\n@requires version<1.20\n";
    let source = format!("{header}{BODY}");
    let module = parse(&source).expect("parse");
    let ir = lower(&module);
    let found = check(&module, &ir, None);
    let invalid = found
        .iter()
        .find(|d| d.code == DiagnosticCode::InvalidRequires)
        .expect("reported");
    assert_eq!(&source[invalid.span.clone()], "@requires version<1.20");
}
