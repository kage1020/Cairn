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

use cairn_lang_core::Edition;
use cairn_lang_core::check::DiagnosticCode;
use cairn_lang_core::resolve::compare_versions;
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

/// The strictest edition-neutral floor the module declares, as
/// `cairn info` renders it on the `registry compatibility` row.
fn floor(header: &str) -> String {
    floors_for(header, None)
        .into_iter()
        .reduce(|best, next| {
            if compare_versions(&next.version, &best.version).is_gt() {
                next
            } else {
                best
            }
        })
        .map_or_else(|| "(none)".to_owned(), |f| f.version)
}

/// Every floor a build of `edition` is held to, in source order.
fn floors_for(
    header: &str,
    edition: Option<cairn_lang_core::Edition>,
) -> Vec<cairn_lang_core::resolve::VersionFloor> {
    let source = format!("{header}{BODY}");
    let module = parse(&source).expect("parse");
    cairn_lang_core::resolve::declared_version_floors(&module, edition)
}

/// The version labels of those floors, which is what most of the tests
/// below are actually about.
fn versions_for(header: &str, edition: Option<cairn_lang_core::Edition>) -> Vec<String> {
    floors_for(header, edition)
        .into_iter()
        .map(|f| f.version)
        .collect()
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

/// A snapshot, a pre-release, and a date-based label are real Minecraft
/// versions, and `spec/versioning-editions.md` §10.1 says two of the three
/// will be how releases are spelled. The directive accepts them: whether
/// one can be *ordered* is the target edition's `DataVersion` table's
/// answer, asked at the command that pins an edition, and refusing them
/// here would pre-empt it with the wrong one.
#[test]
fn a_label_the_spec_says_will_exist_is_not_refused_by_the_directive() {
    for (header, version) in [
        ("@requires version>=1.21.4-rc1\n", "1.21.4-rc1"),
        ("@requires version>=24w14a\n", "24w14a"),
        ("@requires version>=2026.1\n", "2026.1"),
    ] {
        assert!(
            !codes(header).contains(&"E_INVALID_REQUIRES"),
            "{header:?}: {:?}",
            codes(header),
        );
        assert_eq!(floor(header), version, "{header:?}");
    }
}

/// A `-` used to die in the lexer, before any pass could have an opinion:
/// a header's value is the raw source between its tokens, so a character
/// the lexer refuses never reaches the reader that would accept it.
#[test]
fn a_pre_release_label_reaches_the_directive_at_all() {
    let source = format!("@requires version>=1.21.4-rc1\n{BODY}");
    parse(&source).expect("a pre-release suffix lexes");
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

// -- the edition scope ---------------------------------------------------
//
// Java releases run `1.20.4 / 1.21 / 1.21.4` and Bedrock `1.21.0 / 1.21.40
// / 1.21.60`, so `1.21.4` names Java's newest release and no Bedrock
// release at all. A floor may say which numbering it is written in.

/// A scoped floor is in its own edition's build.
#[test]
fn a_scoped_floor_is_collected_for_the_edition_it_names() {
    let header = "@requires java version>=1.21.4\n";
    assert_eq!(versions_for(header, Some(Edition::Java)), ["1.21.4"]);
    assert!(codes(header).is_empty(), "{:?}", codes(header));
}

/// And inert in the other's — not violated by it. A file declaring a floor
/// per edition would otherwise be unbuildable everywhere.
#[test]
fn a_scoped_floor_is_not_a_floor_on_the_other_edition() {
    let header = "@requires java version>=1.21.4\n@requires bedrock version>=1.21.40\n";
    assert_eq!(versions_for(header, Some(Edition::Java)), ["1.21.4"]);
    assert_eq!(versions_for(header, Some(Edition::Bedrock)), ["1.21.40"]);
}

/// An unscoped floor is a floor on whatever is being built, so it is in
/// both — and beside a scoped one, both apply to that edition.
#[test]
fn an_unscoped_floor_is_in_every_edition() {
    let header = "@requires version>=1.21\n@requires bedrock version>=1.21.40\n";
    assert_eq!(versions_for(header, Some(Edition::Java)), ["1.21"]);
    assert_eq!(
        versions_for(header, Some(Edition::Bedrock)),
        ["1.21", "1.21.40"],
    );
}

/// The edition-neutral question — the `registry compatibility` row, which
/// `cairn info` prints once for a file it may be reporting on for both
/// editions — takes only the unscoped floors. A floor written in Java's
/// numbering says nothing about a file's Bedrock range.
#[test]
fn the_neutral_row_reads_only_the_unscoped_floors() {
    assert_eq!(floor("@requires java version>=1.21.4\n"), "(none)");
    assert_eq!(
        floor("@requires java version>=1.21.4\n@requires version>=1.20\n"),
        "1.20",
    );
}

/// A typo in the scope is a floor with a typo in it, and says so. Reading
/// it as "not a version requirement" would point at the half that is
/// right.
#[test]
fn a_scope_that_is_not_an_edition_is_named() {
    let text = message("@requires jaba version>=1.21\n");
    assert!(text.contains("jaba"), "should quote the scope: {text}");
    assert!(
        text.contains("java") && text.contains("bedrock"),
        "should name the editions that work: {text}",
    );
    assert_eq!(floor("@requires jaba version>=1.21\n"), "(none)");
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
