//! `E_UNEXPECTED_POSITIONAL`: everything after a command keyword is `key=value`
//! (`spec/syntax` "Lexical"), so a bare value on any row but `connect` is an
//! error at the value. A dropped `=` is the case that matters: `walls
//! mat_slot=wall height 3` builds no wall, and the missing `height=` alone does
//! not say why.

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::{DiagnosticCode, Severity, lower, parse, resolve};

use crate::common::{PRELUDE, def_with, diagnose, of_code, one_of_code, struct_with};

fn duo_with(row: &str) -> String {
    format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n  {row}\n"
    )
}

/// The spec's own forbidden example.
#[test]
fn the_spec_forbidden_form_is_an_error() {
    for src in [
        struct_with("window front G 2 2 2x2 mat_slot=wall"),
        def_with("window front G 2 2 2x2 mat_slot=wall"),
    ] {
        let d = one_of_code(&src, DiagnosticCode::UnexpectedPositional);
        assert_eq!(d.severity(), Severity::Error);
        assert_eq!(
            d.primary,
            "`window` reads only `key=value` arguments: the 5 bare values on this line are dropped",
        );
    }
}

/// `height 3` reads as two bare values, not as a misspelt `height=3`.
#[test]
fn a_dropped_equals_sign_is_reported() {
    let src = struct_with("walls mat_slot=wall height 3");
    let d = one_of_code(&src, DiagnosticCode::UnexpectedPositional);
    assert_eq!(&src[d.span.clone()], "height 3");
}

/// `connect FROM.PORT to TO.PORT` is the one form that reads positionals.
/// A malformed row is `connect_arity`'s, whose advice is about the endpoint
/// shape rather than `key=value`.
#[test]
fn connect_is_exempt_well_formed_or_not() {
    for row in [
        "connect anchor.entry to peer.entry path=@gravel",
        "connect anchor.entry to peer.entry c.exit path=@gravel",
        "connect anchor.entry xxx peer.entry path=@gravel",
        "connect anchor to peer.entry path=@gravel",
    ] {
        let src = duo_with(row);
        assert!(
            of_code(&src, DiagnosticCode::UnexpectedPositional).is_empty(),
            "`{row}` is `connect_arity`'s, got {:#?}",
            of_code(&src, DiagnosticCode::UnexpectedPositional),
        );
    }
    let over_arity = duo_with("connect anchor.entry to peer.entry c.exit path=@gravel");
    assert!(
        diagnose(&over_arity)
            .iter()
            .any(|d| d.code == DiagnosticCode::ConnectArity),
        "the arity pass still owns the over-arity row",
    );
}

/// The parser appends to `positional` whenever the next token is not
/// `key=`, so a bare value can sit *after* an argument, not only before
/// the first `key=value`.
#[test]
fn a_positional_after_an_argument_is_still_reported() {
    let src = struct_with("window id=w side=front y=1 offset=1 size=2 x2 mat_slot=wall");
    let d = one_of_code(&src, DiagnosticCode::UnexpectedPositional);
    assert_eq!(&src[d.span.clone()], "x2");
}

/// The argument caught between two bare values is part of the line the
/// author rewrites.
#[test]
fn the_span_covers_the_whole_run() {
    let src = struct_with("window front side=front G mat_slot=wall");
    let d = one_of_code(&src, DiagnosticCode::UnexpectedPositional);
    assert_eq!(&src[d.span.clone()], "front side=front G");
}

#[test]
fn a_fully_keyed_line_is_not_reported() {
    for row in [
        "floor mat_slot=floor",
        "walls mat_slot=wall height=3",
        "window id=w side=front y=1 offset=1 size=1x1",
        "level y=0",
    ] {
        let src = struct_with(row);
        assert!(
            of_code(&src, DiagnosticCode::UnexpectedPositional).is_empty(),
            "`{row}` is well-formed, got {:#?}",
            of_code(&src, DiagnosticCode::UnexpectedPositional),
        );
    }
}

/// An unknown keyword is `keyword_allowlist`'s finding: there is no
/// reader whose argument form the bare values could be measured against,
/// and the repair is the word.
#[test]
fn an_unknown_keyword_is_left_to_the_allowlist() {
    let src = struct_with("frame front 2");
    assert!(
        of_code(&src, DiagnosticCode::UnexpectedPositional).is_empty(),
        "got {:#?}",
        of_code(&src, DiagnosticCode::UnexpectedPositional),
    );
    assert!(
        diagnose(&src)
            .iter()
            .any(|d| d.code == DiagnosticCode::UnknownKeyword),
    );
}

/// This pass descends through a subtree `member_scope` and `nesting` have
/// already reported, because dedenting the row leaves it just as broken.
#[test]
fn a_positional_inside_a_dropped_body_is_still_reported() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls mat_slot=wall height=3\n    window front G mat_slot=wall\n"
    );
    let d = one_of_code(&src, DiagnosticCode::UnexpectedPositional);
    assert_eq!(&src[d.span.clone()], "front G");
    assert!(
        diagnose(&src)
            .iter()
            .any(|d| d.code == DiagnosticCode::UnsupportedNesting),
        "the dropped body is still reported by `nesting` — the two \
         findings have different repairs",
    );
}

/// Singular and plural both appear in the message, and only a rendered
/// string shows that the verb agrees with the count.
#[test]
fn the_verb_agrees_with_the_count() {
    let one_value = one_of_code(
        &struct_with("roof flat mat_slot=wall"),
        DiagnosticCode::UnexpectedPositional,
    );
    assert!(
        one_value
            .primary
            .ends_with("the 1 bare value on this line is dropped"),
        "got {:?}",
        one_value.primary,
    );
    let two_values = one_of_code(
        &struct_with("roof flat gable mat_slot=wall"),
        DiagnosticCode::UnexpectedPositional,
    );
    assert!(
        two_values
            .primary
            .ends_with("the 2 bare values on this line are dropped"),
        "got {:?}",
        two_values.primary,
    );
}

/// A `place` row takes the same treatment as a geometry row: `place hut`
/// (the `use=` key forgotten) is a plausible typo and drops the whole
/// placement.
#[test]
fn a_site_row_is_covered_too() {
    let src = duo_with("place extra theme=plain at=origin");
    let d = one_of_code(&src, DiagnosticCode::UnexpectedPositional);
    assert_eq!(&src[d.span.clone()], "extra");
}

/// The premise the message is written on, measured on the build: `walls`
/// has no default height, so a lost `=` costs the whole wall.
#[test]
fn a_dropped_equals_costs_the_whole_member() {
    let solid = |source: &str| {
        let module = parse(source).expect("parse");
        let ir = lower(&module);
        let resolution = resolve(&ir, None);
        let built = lower_to_block_array(&ir, &resolution, None);
        built.structures["struct::s"]
            .voxels
            .iter()
            .filter(|c| c.0 != 0)
            .count()
    };
    let keyed = solid(&struct_with("walls mat_slot=wall height=3"));
    let dropped = solid(&struct_with("walls mat_slot=wall height 3"));
    let no_walls = solid(&struct_with("floor mat_slot=floor"));
    assert!(
        keyed > dropped,
        "the wall should be missing: keyed={keyed} dropped={dropped}",
    );
    assert_eq!(
        dropped, no_walls,
        "missing entirely, not shortened — there is no default height",
    );
}
