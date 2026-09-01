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

/// A space around the operator used to make the floor vanish.
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

/// `version<1.20` is not a constraint Cairn can intersect — only `>=` is
/// defined — and it used to be dropped, so the file read as if it declared
/// an upper bound that was never enforced or even acknowledged.
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

/// A component that is not a number cannot be ordered against a target,
/// and a lexicographic fallback orders it wrongly rather than refusing it.
///
/// Each fixture carries the exact text its own message must quote. A
/// disjunction over the three would pass on any message mentioning any of
/// them, including three copies of one wrong message.
#[test]
fn a_component_that_is_not_a_number_is_refused() {
    for (header, version, component) in [
        ("@requires version>=1.a\n", "1.a", "a"),
        ("@requires version>=x\n", "x", "x"),
        ("@requires version>=1.2.beta\n", "1.2.beta", "beta"),
    ] {
        let text = message(header);
        assert!(
            text.contains(version),
            "{header:?} should quote the version it could not read: {text}",
        );
        assert!(
            text.contains(component),
            "{header:?} should name the component `{component}`: {text}",
        );
    }
    // An empty component is its own shape of mistake and gets its own
    // sentence — there is no fragment to quote back.
    let text = message("@requires version>=1..2\n");
    assert!(
        text.contains("1..2") && text.contains("empty component"),
        "{text}",
    );
}

/// A snapshot label is a real Minecraft version, so the refusal has to say
/// Cairn cannot order it rather than that it is a number out of range.
#[test]
fn a_snapshot_label_is_refused_for_the_reason_it_is_refused() {
    let text = message("@requires version>=24w14a\n");
    assert!(text.contains("24w14a"), "{text}");
    assert!(
        !text.contains("4294967295"),
        "a snapshot label is not a number out of range: {text}",
    );
}

/// `4294967296` is all ASCII digits, so a digit-only check passes it to a
/// comparison that cannot order it. Refusing it here is the first of two
/// guards; `compare_versions` gets the other, because it is public and can
/// be reached without this pass.
#[test]
fn a_component_too_large_to_compare_is_refused() {
    let text = message("@requires version>=4294967296\n");
    assert!(
        text.contains("4294967296"),
        "the message should name the component that does not fit: {text}",
    );
}

/// The payload carries the failure as data, so a quick-fix does not have to
/// take the sentence apart. `spec/lint.md` §11.2 asks for exactly that, and
/// this one code covers several mistakes whose repairs have nothing in
/// common — replacing `<` with `>=` is a one-character edit a tool can
/// offer, while a snapshot label cannot be repaired at all today.
#[test]
fn the_finding_carries_the_failure_as_data() {
    use cairn_lang_core::check::DiagnosticData;

    for (header, reason, found) in [
        ("@requires version<1.20\n", "unsupported_operator", "<"),
        ("@requires version>=1.a\n", "component_not_a_number", "a"),
        (
            "@requires version>=4294967296\n",
            "component_too_large",
            "4294967296",
        ),
        (
            "@requires version>=1.21 extra\n",
            "trailing_tokens",
            "extra",
        ),
        ("@requires version>=\n", "empty_version", ""),
        ("@requires nonsense\n", "not_a_version_requirement", ""),
    ] {
        let payload = diagnostics(header)
            .into_iter()
            .find(|d| d.code == DiagnosticCode::InvalidRequires)
            .and_then(|d| d.data)
            .unwrap_or_else(|| panic!("{header:?} should carry a payload"));
        assert_eq!(
            payload,
            DiagnosticData::InvalidRequires {
                reason: reason.to_owned(),
                found: found.to_owned(),
            },
            "{header:?}",
        );
    }
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
