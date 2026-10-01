//! A directive value the lexer has no token for is the directive's to judge.
//!
//! `@cairn`, `@requires` and a part's `requires` take their value as raw
//! text to end of line, and a check pass decides what it means:
//! `W_INVALID_CAIRN_VERSION` for a `@cairn` that names no version,
//! `E_INVALID_REQUIRES` for a floor that declares none (`spec/syntax`
//! "Headers"). The whole file used to be lexed before the parser started,
//! so a value holding a character no token starts with never reached
//! either pass: `@cairn 2026.6 draft` was a warning and
//! `@cairn 2026.6 draft!` refused the build.
//!
//! The other half is that nothing else moved. The same stretch anywhere a
//! raw value does not take it is refused with the error [`lex`] reports
//! for it, at the same position, ahead of whatever the parse found.

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

/// A comment ends the value, so what follows it is no part of the text a
/// raw value takes — and was never lexed at all.
#[test]
fn a_comment_after_the_value_is_still_a_comment() {
    assert_eq!(codes(&format!("@cairn 2026.6 # $ ~\n{BODY}")), []);
}

/// Except where a quote opens ahead of the `#`. An unterminated string
/// runs to end of line, so the comment is part of the stretch the lexer
/// refused, and the value takes it whole.
///
/// This pins today's reading, not an intended one. The tree-sitter grammar
/// reads the same source the other way: `directive_literal` stops at the
/// `#` and the rest is a comment. Both parsers accept the file; only the
/// value text differs.
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
fn the_same_stretch_anywhere_else_is_refused_as_before() {
    for source in [
        // Whitespace other than a space separates nothing to the lexer,
        // and is no part of a value either.
        "@cairn 2026.6\tdraft\ntheme t:\n  slot wall -> @stone\n",
        // A tab inside a stretch the lexer refused is still a tab, and
        // still no part of a value.
        "@cairn 2026.6 \"dr\taft\ntheme t:\n  slot wall -> @stone\n",
        // Not a raw value at all.
        "theme t:\n  slot wall -> @st$one\n",
        // `@intended_targets` re-reads its value as tokens.
        "@intended_targets [$]\ntheme t:\n  slot wall -> @stone\n",
        // An unknown directive is not judged by any pass.
        "@foo $\ntheme t:\n  slot wall -> @stone\n",
        // A `struct` reads no floors, so a `requires` line that declares
        // none is an ordinary member line there, and gives its value back.
        "struct s size=3x3\n  requires version~=1.21\n",
        // A stray ahead of an indentation failure is still the first error.
        "theme t:\n  slot $ -> @stone\n\tslot x -> @y\n",
        // And ahead of a parse error later in the file.
        "theme t:\n  slot $ -> @stone\n  slot x ->\n",
        // And behind one: the whole file used to be lexed first.
        "theme t:\n  slot x ->\n  slot $ -> @stone\n",
    ] {
        refused_as_lex_refuses(source);
    }
}

/// A taken value does not excuse a later stray: the stray is the error,
/// where `lex` alone stops at the value.
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

/// A value the parser took raw does not stand in for the indentation
/// failure after it, which is the file's only real error.
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
