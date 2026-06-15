//! Lexer acceptance tests.

use cairn_lang_core::error::{LexError, Position};
use cairn_lang_core::lex::{TokenKind, lex};

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

#[test]
fn accepts_crlf_line_endings() {
    // Same as the simple-command test but with Windows-style line endings —
    // must not regress on `core.autocrlf=true` checkouts.
    let kinds = kinds("walls class=outer height=4\r\nfloor mat_slot=floor\r\n");
    assert!(
        kinds
            .iter()
            .any(|k| matches!(k, TokenKind::Ident(s) if s == "walls"))
    );
    assert!(
        kinds
            .iter()
            .any(|k| matches!(k, TokenKind::Ident(s) if s == "floor"))
    );
    let newlines = kinds
        .iter()
        .filter(|k| matches!(k, TokenKind::Newline))
        .count();
    assert_eq!(newlines, 2);
}

#[test]
fn accepts_lone_cr_line_endings() {
    // Pre-OSX Mac line endings — lenient acceptance.
    let kinds = kinds("walls height=4\rfloor mat_slot=floor\r");
    let newlines = kinds
        .iter()
        .filter(|k| matches!(k, TokenKind::Newline))
        .count();
    assert_eq!(newlines, 2);
}

#[test]
fn col_counts_characters_not_bytes_in_strings() {
    // Multi-byte (3-byte UTF-8) characters inside a string must not throw
    // off the column counter for the next token.
    let source = "@intended_targets [\"日本語\"]\n@cairn 2026.06\n";
    let tokens = lex(source).expect("lex");
    let cairn = tokens
        .iter()
        .find(|t| matches!(&t.kind, TokenKind::Ident(s) if s == "cairn"))
        .expect("cairn token");
    assert_eq!(cairn.position, Position { line: 2, col: 2 });
}

#[test]
fn non_ascii_outside_string_is_reported_as_character() {
    // A non-ASCII character not inside a string literal should error with
    // the actual `char`, not a corrupted single byte.
    let err = lex("struct あ size=1x1\n").unwrap_err();
    match err {
        LexError::UnexpectedChar { ch, .. } => assert_eq!(ch, 'あ'),
        other => panic!("expected UnexpectedChar, got {other:?}"),
    }
}

#[test]
fn unterminated_string_is_reported() {
    let err = lex("@intended_targets [\"open\n").unwrap_err();
    assert!(matches!(err, LexError::UnterminatedString { .. }));
}

#[test]
fn dedent_emits_per_level_closed() {
    // `struct s\n  level y=0\n    door\nstruct t\n` exits two indent levels
    // on the line `struct t`, so two Dedent tokens are emitted on it.
    let source = "struct s\n  level y=0\n    door side=front\nstruct t\n";
    let kinds = kinds(source);
    let dedents = kinds
        .iter()
        .filter(|k| matches!(k, TokenKind::Dedent))
        .count();
    assert_eq!(dedents, 2);
}
