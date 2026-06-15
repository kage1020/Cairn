//! Lexer acceptance tests.

use cairn_core::error::{LexError, Position};
use cairn_core::lex::{TokenKind, lex};

fn kinds(source: &str) -> Vec<TokenKind> {
    lex(source)
        .expect("lex")
        .into_iter()
        .map(|t| t.kind)
        .collect()
}

#[test]
fn lexes_simple_command() {
    let kinds = kinds("walls class=outer mat_slot=wall height=4\n");
    assert_eq!(
        kinds,
        vec![
            TokenKind::Ident("walls".into()),
            TokenKind::Ident("class".into()),
            TokenKind::Eq,
            TokenKind::Ident("outer".into()),
            TokenKind::Ident("mat_slot".into()),
            TokenKind::Eq,
            TokenKind::Ident("wall".into()),
            TokenKind::Ident("height".into()),
            TokenKind::Eq,
            TokenKind::Int {
                value: 4,
                lexeme: "4".into()
            },
            TokenKind::Newline,
        ]
    );
}

#[test]
fn lexes_size_literal() {
    let kinds = kinds("struct cottage size=9x7\n");
    assert!(kinds.contains(&TokenKind::Size(9, 7)));
}

#[test]
fn lexes_header_and_at_token() {
    let kinds = kinds("@cairn 2026.06\n  slot floor -> @oak_planks\n");
    assert_eq!(kinds[0], TokenKind::At);
    assert_eq!(kinds[1], TokenKind::Ident("cairn".into()));
    assert_eq!(
        kinds[2],
        TokenKind::Int {
            value: 2026,
            lexeme: "2026".into()
        }
    );
    assert_eq!(kinds[3], TokenKind::Dot);
    assert_eq!(
        kinds[4],
        TokenKind::Int {
            value: 6,
            lexeme: "06".into()
        }
    );
    assert_eq!(kinds[5], TokenKind::Newline);
    assert_eq!(kinds[6], TokenKind::Indent);
    assert_eq!(kinds[7], TokenKind::Ident("slot".into()));
    assert_eq!(kinds[8], TokenKind::Ident("floor".into()));
    assert_eq!(kinds[9], TokenKind::Arrow);
    assert_eq!(kinds[10], TokenKind::At);
    assert_eq!(kinds[11], TokenKind::Ident("oak_planks".into()));
}

#[test]
fn lexes_indent_and_dedent() {
    let source = "theme medieval:\n  slot floor -> @oak_planks\nstruct cottage size=9x7\n";
    let kinds = kinds(source);
    let indents = kinds
        .iter()
        .filter(|k| matches!(k, TokenKind::Indent))
        .count();
    let dedents = kinds
        .iter()
        .filter(|k| matches!(k, TokenKind::Dedent))
        .count();
    assert_eq!(indents, 1, "one indent for the theme body");
    assert_eq!(dedents, 1, "one dedent before struct");
}

#[test]
fn skips_comments_and_blank_lines() {
    let source = "\
# leading comment
@cairn 2026.06

# blank above
struct keep size=11x9
";
    let kinds = kinds(source);
    assert_eq!(kinds[0], TokenKind::At);
    assert_eq!(kinds[1], TokenKind::Ident("cairn".into()));
}

#[test]
fn tabs_are_rejected() {
    let err = lex("\tstruct\n").unwrap_err();
    assert!(matches!(err, LexError::TabIndent { .. }));
}

#[test]
fn odd_indent_is_rejected() {
    let err = lex("theme medieval:\n   slot floor -> @x\n").unwrap_err();
    assert!(matches!(err, LexError::OddIndent { got: 3, .. }));
}

#[test]
fn double_indent_is_rejected() {
    // Going from 0 to 4 spaces in one step is an indent of 2 levels.
    let err = lex("struct keep\n    floor mat_slot=floor\n").unwrap_err();
    assert!(matches!(err, LexError::OddIndent { got: 4, .. }));
}

#[test]
fn lexes_arrow_and_comparison() {
    let kinds = kinds("@requires version>=1.20\n");
    assert!(kinds.iter().any(|k| matches!(k, TokenKind::GreaterEq)));
}

#[test]
fn lexes_string_list() {
    let kinds = kinds("@intended_targets [\"1.20.4\",\"1.21.4\"]\n");
    assert!(
        kinds
            .iter()
            .any(|k| matches!(k, TokenKind::Str(s) if s == "1.20.4"))
    );
    assert!(
        kinds
            .iter()
            .any(|k| matches!(k, TokenKind::Str(s) if s == "1.21.4"))
    );
}

#[test]
fn tracks_line_and_column() {
    let tokens = lex("theme medieval:\n  slot floor -> @oak_planks\n").expect("lex");
    let slot_token = tokens
        .iter()
        .find(|t| matches!(&t.kind, TokenKind::Ident(s) if s == "slot"))
        .expect("slot token");
    assert_eq!(slot_token.position, Position { line: 2, col: 3 });
}
