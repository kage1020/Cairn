//! `@requires` declares a capability floor, and only `version>=X` is defined (`spec/syntax`
//! "Headers"); any other expression is reported rather than dropped.

use cairn_lang_core::Edition;
use cairn_lang_core::check::DiagnosticCode;
use cairn_lang_core::resolve::compare_versions;
use cairn_lang_core::{check, lower, parse};

use crate::common::{codes, diagnose, with_header};

/// The `E_INVALID_REQUIRES` message for a header that has exactly one.
fn message(header: &str) -> String {
    let found = diagnose(&with_header(header));
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

/// The strictest edition-neutral floor, as `cairn info` renders the `registry compatibility` row.
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
    let source = with_header(header);
    let module = parse(&source).expect("parse");
    match edition {
        Some(edition) => cairn_lang_core::resolve::declared_version_floors(&module, edition),
        None => cairn_lang_core::resolve::unscoped_version_floors(&module),
    }
}

/// The version labels of those floors.
fn versions_for(header: &str, edition: Option<cairn_lang_core::Edition>) -> Vec<String> {
    floors_for(header, edition)
        .into_iter()
        .map(|f| f.version)
        .collect()
}

// -- the grammar ----------------------------------------------------------

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
            !codes(&with_header(header)).contains(&"E_INVALID_REQUIRES"),
            "{header:?} is well formed",
        );
    }
}

/// `spec/syntax` "Headers" exempts `@requires` from `E_DUPLICATE_HEADER`.
#[test]
fn repeated_requirements_fold_to_the_strictest() {
    let header = "@requires version>=1.20\n@requires version>=1.21\n@requires version>=1.19\n";
    assert_eq!(floor(header), "1.21");
    assert!(
        codes(&with_header(header)).is_empty(),
        "{:?}",
        codes(&with_header(header))
    );
}

// -- what it refuses ------------------------------------------------------

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
    let text = message("@requires version>=1..2\n");
    assert!(
        text.contains("1..2") && text.contains("empty component"),
        "{text}",
    );
}

/// `spec/versioning-editions` "The target is a compile-time parameter" counts these as versions.
#[test]
fn a_label_the_spec_says_will_exist_is_not_refused_by_the_directive() {
    for (header, version) in [
        ("@requires version>=1.21.4-rc1\n", "1.21.4-rc1"),
        ("@requires version>=24w14a\n", "24w14a"),
        ("@requires version>=2026.1\n", "2026.1"),
    ] {
        assert!(
            !codes(&with_header(header)).contains(&"E_INVALID_REQUIRES"),
            "{header:?}: {:?}",
            codes(&with_header(header)),
        );
        assert_eq!(floor(header), version, "{header:?}");
    }
}

#[test]
fn a_pre_release_label_reaches_the_directive_at_all() {
    let source = with_header("@requires version>=1.21.4-rc1\n");
    parse(&source).expect("a pre-release suffix lexes");
}

/// `4294967296` is all ASCII digits, so a digit-only check would pass it.
#[test]
fn a_component_too_large_to_compare_is_refused() {
    let text = message("@requires version>=4294967296\n");
    assert!(
        text.contains("4294967296"),
        "the message should name the component that does not fit: {text}",
    );
}

/// `spec/lint` "Machine-readable payload".
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
        let payload = diagnose(&with_header(header))
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
// `1.21.4` names Java's newest release and no Bedrock release at all.

#[test]
fn a_scoped_floor_is_collected_for_the_edition_it_names() {
    let header = "@requires java version>=1.21.4\n";
    assert_eq!(versions_for(header, Some(Edition::Java)), ["1.21.4"]);
    assert!(
        codes(&with_header(header)).is_empty(),
        "{:?}",
        codes(&with_header(header))
    );
}

#[test]
fn a_scoped_floor_is_not_a_floor_on_the_other_edition() {
    let header = "@requires java version>=1.21.4\n@requires bedrock version>=1.21.40\n";
    assert_eq!(versions_for(header, Some(Edition::Java)), ["1.21.4"]);
    assert_eq!(versions_for(header, Some(Edition::Bedrock)), ["1.21.40"]);
}

#[test]
fn an_unscoped_floor_is_in_every_edition() {
    let header = "@requires version>=1.21\n@requires bedrock version>=1.21.40\n";
    assert_eq!(versions_for(header, Some(Edition::Java)), ["1.21"]);
    assert_eq!(
        versions_for(header, Some(Edition::Bedrock)),
        ["1.21", "1.21.40"],
    );
}

#[test]
fn the_neutral_row_reads_only_the_unscoped_floors() {
    assert_eq!(floor("@requires java version>=1.21.4\n"), "(none)");
    assert_eq!(
        floor("@requires java version>=1.21.4\n@requires version>=1.20\n"),
        "1.20",
    );
}

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

#[test]
fn a_refused_requirement_declares_no_floor() {
    assert_eq!(floor("@requires version<1.20\n"), "(none)");
    // A well-formed floor on another line still stands.
    let mixed = "@requires version<1.20\n@requires version>=1.21\n";
    assert_eq!(floor(mixed), "1.21");
    assert_eq!(codes(&with_header(mixed)), vec!["E_INVALID_REQUIRES"]);
}

#[test]
fn the_diagnostic_covers_the_directive_that_is_wrong() {
    let header = "@cairn 2026.7\n@requires version<1.20\n";
    let source = with_header(header);
    let module = parse(&source).expect("parse");
    let ir = lower(&module);
    let found = check(&module, &ir, None);
    let invalid = found
        .iter()
        .find(|d| d.code == DiagnosticCode::InvalidRequires)
        .expect("reported");
    assert_eq!(&source[invalid.span.clone()], "@requires version<1.20");
}
