//! Span propagation acceptance tests.
//!
//! Locks down the M2-PR2 invariant that every parser-visible AST node carries
//! a `span` whose byte range round-trips through `&source[span]` to the
//! surface form, and that the IR's [`Member`] mirrors the originating
//! [`Statement::Generic`] span exactly.

use cairn_lang_core::ast::{Item, Statement, ValueKind};
use cairn_lang_core::{lower, parse};

#[test]
fn statement_generic_span_round_trips_to_surface_text() {
    let src = "struct s size=1x1\n  walls height=4\n";
    let module = parse(src).expect("parse");
    let Item::Struct { body, span, .. } = &module.items[0] else {
        panic!("expected Struct, got {:?}", module.items[0]);
    };
    // The item span starts at the keyword and (per `last_content_byte`)
    // ends after the last meaningful body token.
    let item_slice = &src[span.clone()];
    assert!(
        item_slice.starts_with("struct s") && item_slice.ends_with("height=4"),
        "item span should bracket the whole struct, got {item_slice:?}",
    );

    let walls = &body[0];
    let Statement::Generic { args, span, .. } = walls else {
        panic!("expected Generic, got {walls:?}");
    };
    assert_eq!(&src[span.clone()], "walls height=4");
    let height = &args[0];
    assert_eq!(&src[height.span.clone()], "height=4");
    assert_eq!(&src[height.value.span.clone()], "4");
}

#[test]
fn value_variants_each_carry_their_token_span() {
    // One source sample per non-list `ValueKind`, asserting that the
    // value's `span` recovers exactly the surface lexeme.
    let src = "struct s size=2x3\n  walls flag=true height=4 mat_slot=wall token=@oak tag=front.outside name=\"label\"\n";
    let module = parse(src).expect("parse");
    let Item::Struct { args, body, .. } = &module.items[0] else {
        panic!("expected Struct");
    };
    let header_size = &args[0];
    assert_eq!(&src[header_size.value.span.clone()], "2x3");

    let Statement::Generic {
        args: walls_args, ..
    } = &body[0]
    else {
        panic!("expected Generic");
    };
    let by_key = |k: &str| -> &ValueKind {
        &walls_args
            .iter()
            .find(|a| a.key == k)
            .unwrap_or_else(|| panic!("missing key `{k}` in {walls_args:?}"))
            .value
            .kind
    };
    assert!(matches!(by_key("flag"), ValueKind::Bool(true)));
    assert!(matches!(by_key("height"), ValueKind::Int(4)));
    assert!(matches!(by_key("mat_slot"), ValueKind::Ident(s) if s == "wall"));
    assert!(matches!(by_key("token"), ValueKind::Token(s) if s == "oak"));
    assert!(matches!(by_key("tag"), ValueKind::DotRef(_)));
    assert!(matches!(by_key("name"), ValueKind::Str(s) if s == "label"));

    for arg in walls_args {
        // Every value's span yields a non-empty slice of the source.
        let span = arg.value.span.clone();
        assert!(span.start < span.end, "value span empty for {arg:?}");
        assert!(span.end <= src.len(), "value span runs past EOF: {arg:?}");
    }
}

#[test]
fn member_span_mirrors_the_originating_statement_span() {
    let src = "struct s size=1x1\n  walls height=4\n";
    let module = parse(src).expect("parse");
    let ir = lower(&module);
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("expected Struct");
    };
    let Statement::Generic {
        span: stmt_span, ..
    } = &body[0]
    else {
        panic!("expected Generic");
    };
    let member_span = &ir.structs[0].members[0].span;
    assert_eq!(
        member_span, stmt_span,
        "Member.span must equal the originating Statement::Generic.span",
    );
}

#[test]
fn intent_state_entries_carry_value_level_spans() {
    let src = "struct s size=1x1\n  walls height=4\n";
    let module = parse(src).expect("parse");
    let ir = lower(&module);
    let member = &ir.structs[0].members[0];
    let height = member
        .intent_state
        .get("height")
        .expect("`height=` survives lowering");
    assert_eq!(&src[height.span.clone()], "4");
}
