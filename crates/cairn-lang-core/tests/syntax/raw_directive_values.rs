//! `@cairn`, `@requires` and a part's `requires` take raw text to end of line; anywhere else the
//! same text is refused with the error [`lex`] reports.

use cairn_lang_core::ast::Header;
use cairn_lang_core::check::DiagnosticCode;
use cairn_lang_core::error::ParseError;
use cairn_lang_core::lex::lex;
use cairn_lang_core::{check, lower, parse};

/// The body under every header fixture, so the header is the only variable.
const BODY: &str = "\ntheme t:\n  slot wall -> @stone\n";

/// The codes `check` reports for a source that has to parse.
fn codes(source: &str) -> Vec<DiagnosticCode> {
    let module = parse(source)
        .unwrap_or_else(|err| panic!("{source:?} should parse, got: {}", err.user_message()));
    let ir = lower(&module);
    check(&module, &ir, None)
        .into_iter()
        .map(|d| d.code)
        .collect()
}

/// Assert `parse` refuses `source` with exactly the error `lex` reports.
fn refused_as_lex_refuses(source: &str) {
    let lexed = lex(source).expect_err("the fixture holds something no token starts with");
    let parsed = parse(source).expect_err("the stretch is not in a raw value");
    assert!(
        matches!(parsed, ParseError::Lex(_)),
        "{source:?}: expected the lex error, got {parsed:?}",
    );
    assert_eq!(
        (parsed.position(), parsed.user_message()),
        (lexed.position(), lexed.user_message()),
        "{source:?}",
    );
}

#[test]
fn an_unlexable_cairn_value_is_a_warning_rather_than_a_refused_build() {
    for header in [
        "@cairn 2026.6+build\n",
        "@cairn 2026/06\n",
        "@cairn 2026.6 draft!\n",
        "@cairn 2026.6 \"draft\n",
        "@cairn 2x2y\n",
    ] {
        let source = format!("{header}{BODY}");
        assert!(lex(&source).is_err(), "{header:?} should still fail `lex`");
        assert_eq!(
            codes(&source),
            [DiagnosticCode::InvalidCairnVersion],
            "{header:?}",
        );
    }
}

#[test]
fn an_unlexable_requires_value_is_a_floor_that_declares_nothing() {
    for source in [
        format!("@requires version~=1.21\n{BODY}"),
        format!("@requires version>=1.21+x\n{BODY}"),
        "theme t:\n  requires version~=1.21\n  slot wall -> @stone\n".to_owned(),
        "def d size=2x2:\n  requires version~=1.21\n".to_owned(),
    ] {
        assert!(lex(&source).is_err(), "{source:?} should still fail `lex`");
        // The unused `def` has its own warning; the floor is the point.
        let found = codes(&source);
        assert_eq!(
            found
                .iter()
                .filter(|&&code| code == DiagnosticCode::InvalidRequires)
                .count(),
            1,
            "{source:?}: {found:?}",
        );
    }
}

/// A raw value is text, so a `{` after its `=` opens no compound the lexer could refuse.
#[test]
fn a_brace_after_an_equals_sign_in_a_raw_value_is_text() {
    for (header, raw) in [
        ("@requires feature={a:1}\n", "feature={a:1}"),
        ("@requires feature={a\n", "feature={a"),
        ("@requires feature={a:01} >=1.21\n", "feature={a:01} >=1.21"),
    ] {
        let module = parse(&format!("{header}{BODY}")).expect("a raw value takes any text");
        let Header::Requires { requirement, .. } = &module.headers[0] else {
            panic!("expected a `@requires` header, got {:?}", module.headers);
        };
        assert_eq!(requirement.as_str(), raw, "{header:?}");
    }
}

#[test]
fn a_comment_after_the_value_is_still_a_comment() {
    assert_eq!(codes(&format!("@cairn 2026.6 # $ ~\n{BODY}")), []);
}

/// An unterminated quote runs to end of line. tree-sitter reads this `#` as a comment; the two
/// parsers disagree here.
#[test]
fn a_comment_inside_an_unterminated_quote_is_part_of_the_value() {
    let source = format!("@cairn 2026.6 \"draft # note\n{BODY}");
    let module = parse(&source).expect("the quote is the value's to judge");
    let Header::Cairn { version, .. } = &module.headers[0] else {
        panic!("expected a `@cairn` header, got {:?}", module.headers);
    };
    assert_eq!(version.as_str(), "2026.6 \"draft # note");
    assert_eq!(codes(&source), [DiagnosticCode::InvalidCairnVersion]);
}

#[test]
fn the_same_stretch_anywhere_else_is_refused() {
    for source in [
        // Whitespace other than a space.
        "@cairn 2026.6\tdraft\ntheme t:\n  slot wall -> @stone\n",
        // A tab inside a stretch the lexer refused.
        "@cairn 2026.6 \"dr\taft\ntheme t:\n  slot wall -> @stone\n",
        // Not a raw value at all.
        "theme t:\n  slot wall -> @st$one\n",
        // `@intended_targets` re-reads its value as tokens.
        "@intended_targets [$]\ntheme t:\n  slot wall -> @stone\n",
        // An unknown directive is not judged by any pass.
        "@foo $\ntheme t:\n  slot wall -> @stone\n",
        // A `struct` reads no floors, so this `requires` is an ordinary member line.
        "struct s size=3x3\n  requires version~=1.21\n",
        // A stray ahead of an indentation failure is still the first error.
        "theme t:\n  slot $ -> @stone\n\tslot x -> @y\n",
        // And ahead of a parse error later in the file.
        "theme t:\n  slot $ -> @stone\n  slot x ->\n",
        // And behind one: the lex refusal is still the one reported.
        "theme t:\n  slot x ->\n  slot $ -> @stone\n",
    ] {
        refused_as_lex_refuses(source);
    }
}

#[test]
fn a_stray_after_a_taken_value_is_still_refused() {
    let source = "@cairn 2026.6+build\ntheme t:\n  slot wall -> @st$one\n";
    let err = parse(source).expect_err("`$` is in a material name");
    assert!(matches!(err, ParseError::Lex(_)), "{err:?}");
    assert_eq!(err.user_message(), "unexpected character `$` (U+0024)");
    assert_eq!(
        (err.position().line.get(), err.position().col.get()),
        (3, 19)
    );
}

#[test]
fn an_indentation_failure_after_a_taken_value_is_the_error_reported() {
    let source = "@cairn 2026.6+build\ntheme t:\n\tslot wall -> @stone\n";
    let err = parse(source).expect_err("a tab is not indentation");
    assert!(
        err.user_message().contains("tab character"),
        "{}",
        err.user_message(),
    );
    assert_eq!(err.position().line.get(), 3);
}

/// The parse error, as `(line, column, message)`, for a source that does not parse.
fn refusal(source: &str) -> (u32, u32, String) {
    let err = parse(source).expect_err("the fixture does not parse");
    let at = err.position();
    (at.line.get(), at.col.get(), err.user_message())
}

#[test]
fn a_raw_value_after_a_parse_error_is_not_refused_in_its_place() {
    for (source, expected) in [
        (
            "def d size=3x3\n  window size=\n  requires version~=1.21\n",
            (2, 15, "unexpected end of line in value position"),
        ),
        (
            "@intended_targets [oak]\n@requires version~=1.21\nstruct s size=3x3\n",
            (
                1,
                20,
                "@intended_targets expects a string such as `\"1.21.4\"`, got `oak`",
            ),
        ),
        (
            "@crain 2026.6\n@cairn 2026.6+build\nstruct s size=3x3\n",
            (1, 1, "unknown directive `@crain`"),
        ),
        (
            "theme t\n  slot a ->\n  requires version>=1.21+x\n",
            (2, 12, "unexpected end of line in value position"),
        ),
    ] {
        let (line, col, message) = expected;
        assert_eq!(
            refusal(source),
            (line, col, message.to_owned()),
            "{source:?}"
        );
    }
}
