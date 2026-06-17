//! Acceptance tests for individual parser building blocks.

use std::num::{IntErrorKind, NonZeroU32};

use cairn_lang_core::ast::{
    Arg, DottedRef, Header, Item, RawRequirement, RawVersion, Statement, ThemeRule, Value,
};
use cairn_lang_core::error::{IntContext, Position};
use cairn_lang_core::{ParseError, parse};

fn dr(segments: &[&str]) -> DottedRef {
    let (head, tail) = segments
        .split_first()
        .expect("DottedRef must be non-empty in tests");
    DottedRef::new(
        (*head).to_owned(),
        tail.iter().map(|s| (*s).to_owned()).collect(),
    )
}

fn nz(n: u32) -> NonZeroU32 {
    NonZeroU32::new(n).expect("test value must be non-zero")
}

fn pos(line: u32, col: u32) -> Position {
    Position {
        line: nz(line),
        col: nz(col),
    }
}

#[test]
fn parses_cairn_header() {
    let module = parse("@cairn 2026.06\n").expect("parse");
    assert_eq!(
        module.headers,
        vec![Header::Cairn {
            version: RawVersion::new("2026.06")
        }]
    );
}

#[test]
fn parses_requires_header() {
    let module = parse("@requires version>=1.20\n").expect("parse");
    assert_eq!(
        module.headers,
        vec![Header::Requires {
            requirement: RawRequirement::new("version>=1.20")
        }]
    );
}

#[test]
fn parses_intended_targets_header() {
    let module = parse("@intended_targets [\"1.20.4\",\"1.21.4\"]\n").expect("parse");
    assert_eq!(
        module.headers,
        vec![Header::IntendedTargets {
            targets: vec!["1.20.4".into(), "1.21.4".into()]
        }]
    );
}

#[test]
fn parses_empty_theme_block() {
    let module = parse("theme medieval:\n").expect("parse");
    assert_eq!(
        module.items,
        vec![Item::Theme {
            name: "medieval".into(),
            body: Vec::new()
        }]
    );
}

#[test]
fn parses_theme_with_slot() {
    let module = parse("theme medieval:\n  slot floor -> @oak_planks\n").expect("parse");
    let Item::Theme { body, .. } = &module.items[0] else {
        panic!("not a theme");
    };
    assert_eq!(
        body[0],
        ThemeRule::Slot {
            slot: "floor".into(),
            value: Value::Token("oak_planks".into())
        }
    );
}

#[test]
fn parses_theme_with_selector() {
    let module =
        parse("theme medieval:\n  window[class=small] -> frame=@spruce_wood\n").expect("parse");
    let Item::Theme { body, .. } = &module.items[0] else {
        panic!("not a theme");
    };
    assert_eq!(
        body[0],
        ThemeRule::Selector {
            keyword: "window".into(),
            attrs: vec![Arg {
                key: "class".into(),
                value: Value::Ident("small".into())
            }],
            bindings: vec![Arg {
                key: "frame".into(),
                value: Value::Token("spruce_wood".into())
            }],
        }
    );
}

#[test]
fn parses_struct_with_size_and_one_child() {
    let module = parse("struct cottage size=9x7\n  floor mat_slot=floor\n").expect("parse");
    let Item::Struct { name, args, body } = &module.items[0] else {
        panic!("not a struct");
    };
    assert_eq!(name, "cottage");
    assert_eq!(
        args,
        &vec![Arg {
            key: "size".into(),
            value: Value::Size { w: nz(9), h: nz(7) }
        }]
    );
    assert_eq!(body.len(), 1);
    let Statement::Generic { keyword, args, .. } = &body[0] else {
        panic!("expected Generic, got {:?}", body[0]);
    };
    assert_eq!(keyword, "floor");
    assert_eq!(
        args,
        &vec![Arg {
            key: "mat_slot".into(),
            value: Value::Ident("floor".into())
        }]
    );
}

#[test]
fn parses_command_with_selector_head() {
    let module = parse("struct s\n  door[id=front] opened_by=sig.open\n").expect("parse");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("not a struct");
    };
    let Statement::Generic {
        keyword,
        selector,
        args,
        ..
    } = &body[0]
    else {
        panic!("expected Generic, got {:?}", body[0]);
    };
    assert_eq!(keyword, "door");
    assert_eq!(
        selector,
        &Some(vec![Arg {
            key: "id".into(),
            value: Value::Ident("front".into())
        }])
    );
    assert_eq!(
        args,
        &vec![Arg {
            key: "opened_by".into(),
            value: Value::DotRef(dr(&["sig", "open"]))
        }]
    );
}

#[test]
fn parses_sensor_binding() {
    let module =
        parse("struct s\n  pressure_plate id=plate at=front.outside -> sig.step\n").expect("parse");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("not a struct");
    };
    let Statement::Generic {
        keyword, binding, ..
    } = &body[0]
    else {
        panic!("expected Generic, got {:?}", body[0]);
    };
    assert_eq!(keyword, "pressure_plate");
    assert_eq!(binding, &Some(Value::DotRef(dr(&["sig", "step"]))));
}

#[test]
fn parses_logic_expression() {
    use cairn_lang_core::ast::Expr;
    let module = parse("struct s\n  logic sig.open = sig.step or sig.exit\n").expect("parse");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("not a struct");
    };
    let Statement::Logic { lhs, rhs } = &body[0] else {
        panic!("expected Logic, got {:?}", body[0]);
    };
    assert_eq!(lhs, &dr(&["sig", "open"]));
    assert!(matches!(rhs, Expr::Or(_, _)));
}

#[test]
fn parses_assert_truth() {
    let module = parse(
        "struct s\n  assert truth(sig.step, sig.exit -> sig.open) { 00->0; 01->1; 10->1; 11->1 }\n",
    )
    .expect("parse");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("not a struct");
    };
    let Statement::AssertTruth {
        inputs,
        output,
        rows,
    } = &body[0]
    else {
        panic!("expected AssertTruth, got {:?}", body[0]);
    };
    assert_eq!(inputs.len(), 2);
    assert_eq!(output, &dr(&["sig", "open"]));
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].inputs, "00");
    assert!(!rows[0].output);
    assert_eq!(rows[3].inputs, "11");
    assert!(rows[3].output);
}

#[test]
fn parses_assert_always() {
    let module = parse("struct s\n  assert always(sig.step -> eventually sig.open within 2)\n")
        .expect("parse");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("not a struct");
    };
    let Statement::AssertAlways {
        antecedent,
        consequent,
        within,
    } = &body[0]
    else {
        panic!("expected AssertAlways, got {:?}", body[0]);
    };
    assert_eq!(antecedent, &dr(&["sig", "step"]));
    assert_eq!(consequent, &dr(&["sig", "open"]));
    assert_eq!(*within, 2);
}

#[test]
fn parses_connect_with_positional_to() {
    let module =
        parse("site hamlet:\n  connect home1.entry to home2.entry path=@gravel\n").expect("parse");
    let Item::Site { body, .. } = &module.items[0] else {
        panic!("not a site");
    };
    let Statement::Generic {
        keyword,
        positional,
        args,
        ..
    } = &body[0]
    else {
        panic!("expected Generic, got {:?}", body[0]);
    };
    assert_eq!(keyword, "connect");
    assert_eq!(positional.len(), 3);
    assert_eq!(positional[0], Value::DotRef(dr(&["home1", "entry"])));
    assert_eq!(positional[1], Value::Ident("to".into()));
    assert_eq!(positional[2], Value::DotRef(dr(&["home2", "entry"])));
    assert_eq!(
        args,
        &vec![Arg {
            key: "path".into(),
            value: Value::Token("gravel".into())
        }]
    );
}

#[test]
fn parses_nested_level() {
    let src = "\
struct keep size=11x9
  level id=floor1 y=0
    door id=entry side=front at=center
";
    let module = parse(src).expect("parse");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("not a struct");
    };
    let Statement::Generic {
        keyword, children, ..
    } = &body[0]
    else {
        panic!("expected Generic, got {:?}", body[0]);
    };
    assert_eq!(keyword, "level");
    assert_eq!(children.len(), 1);
    let Statement::Generic {
        keyword: child_kw, ..
    } = &children[0]
    else {
        panic!("expected Generic child, got {:?}", children[0]);
    };
    assert_eq!(child_kw, "door");
}

#[test]
fn logic_precedence_and_binds_tighter_than_or() {
    use cairn_lang_core::ast::Expr;
    // `a and b or c` must parse as `Or(And(a, b), c)`, not `And(a, Or(b, c))`.
    let module = parse("struct s\n  logic sig.x = a and b or c\n").expect("parse");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("not a struct");
    };
    let Statement::Logic { rhs, .. } = &body[0] else {
        panic!("expected Logic, got {:?}", body[0]);
    };
    match rhs {
        Expr::Or(lhs, rhs_c) => {
            assert!(
                matches!(**lhs, Expr::And(_, _)),
                "lhs should be And, got {lhs:?}"
            );
            assert!(matches!(**rhs_c, Expr::Ref(_)), "rhs should be Ref(c)");
        }
        other => panic!("expected Or at the root, got {other:?}"),
    }
}

#[test]
fn logic_not_higher_than_and() {
    use cairn_lang_core::ast::Expr;
    // `not a and b` must parse as `And(Not(a), b)`.
    let module = parse("struct s\n  logic sig.x = not a and b\n").expect("parse");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("not a struct");
    };
    let Statement::Logic { rhs, .. } = &body[0] else {
        panic!("expected Logic, got {:?}", body[0]);
    };
    match rhs {
        Expr::And(lhs, _) => assert!(matches!(**lhs, Expr::Not(_))),
        other => panic!("expected And, got {other:?}"),
    }
}

// --- Negative tests for ParseError::Syntax / LexError ------------------

#[test]
fn rejects_double_arrow_binding() {
    let err = parse("struct s\n  door at=center -> sig.a -> sig.b\n")
        .expect_err("expected double-arrow error");
    let msg = err.user_message();
    assert!(msg.contains("only have one"), "got: {msg}");
}

#[test]
fn rejects_unknown_top_level_directive() {
    let err = parse("@nope foo\n").expect_err("should fail");
    assert!(err.user_message().contains("unknown directive"));
}

#[test]
fn rejects_empty_header_value() {
    let err = parse("@cairn\n").expect_err("should fail");
    assert!(err.user_message().contains("requires a value"));
}

#[test]
fn rejects_intended_targets_with_trailing_tokens() {
    let err =
        parse("@intended_targets [\"1.20\"] garbage\n").expect_err("trailing tokens should fail");
    assert!(err.user_message().contains("trailing"));
}

#[test]
fn rejects_intended_targets_with_non_string_element() {
    let err = parse("@intended_targets [42]\n").expect_err("non-string element");
    assert!(err.user_message().contains("strings"));
}

#[test]
fn rejects_top_level_command_outside_block() {
    let err = parse("walls class=outer\n").expect_err("commands must live in a block");
    let msg = err.user_message();
    assert!(msg.contains("theme"), "got: {msg}");
}

#[test]
fn rejects_theme_rule_other_than_slot_or_selector() {
    let err = parse("theme t:\n  garbage foo=bar\n").expect_err("invalid theme rule");
    assert!(err.user_message().contains("slot"));
}

#[test]
fn rejects_assert_with_unknown_head() {
    let err = parse("struct s\n  assert sometimes(a -> b)\n").expect_err("bad assert head");
    assert!(err.user_message().contains("truth"));
}

#[test]
fn rejects_assert_always_without_within() {
    let err = parse("struct s\n  assert always(a -> eventually b)\n").expect_err("missing within");
    assert!(err.user_message().contains("within"));
}

#[test]
fn assert_always_overflowing_within_reports_out_of_range() {
    // 2^32 exceeds u32::MAX, so `within` parsing should fail with the
    // overflow-specific message rather than a generic "invalid" one.
    let err = parse("struct s\n  assert always(a -> eventually b within 4294967296)\n")
        .expect_err("overflow");
    let msg = err.user_message();
    assert!(msg.contains("out of range"), "got: {msg}");
}

#[test]
fn rejects_logic_without_eq() {
    let err = parse("struct s\n  logic sig.a sig.b\n").expect_err("missing =");
    let msg = err.user_message();
    assert!(msg.contains('='), "got: {msg}");
}

#[test]
fn carries_position_through_to_parse_error() {
    let err = parse("\n\n@unknown foo\n").expect_err("should fail");
    // The `@unknown` token sits on line 3, column 1.
    assert_eq!(err.position(), pos(3, 1));
}

#[test]
fn rejects_zero_size_width() {
    let err = parse("struct s size=0x4\n").expect_err("0x4 should fail");
    let msg = err.user_message();
    assert!(msg.contains("non-zero"), "got: {msg}");
}

#[test]
fn rejects_zero_size_height() {
    let err = parse("struct s size=4x0\n").expect_err("4x0 should fail");
    let msg = err.user_message();
    assert!(msg.contains("non-zero"), "got: {msg}");
}

// ---------- structured InvalidInt diagnostics ----------

#[test]
fn within_overflow_surfaces_invalid_int_variant() {
    let err = parse("struct s\n  assert always(a -> eventually b within 4294967296)\n")
        .expect_err("overflow");
    let ParseError::InvalidInt {
        context,
        lexeme,
        kind,
        ..
    } = err
    else {
        panic!("expected ParseError::InvalidInt, got {err:?}");
    };
    assert_eq!(context, IntContext::WithinBound);
    assert_eq!(lexeme, "4294967296");
    assert_eq!(kind, IntErrorKind::PosOverflow);
}

#[test]
fn size_width_overflow_surfaces_invalid_int_variant() {
    let err = parse("struct s size=99999999999999999999x9\n").expect_err("overflow");
    let ParseError::InvalidInt { context, kind, .. } = err else {
        panic!("expected ParseError::InvalidInt, got {err:?}");
    };
    assert_eq!(context, IntContext::SizeWidth);
    assert_eq!(kind, IntErrorKind::PosOverflow);
}

#[test]
fn size_height_overflow_surfaces_invalid_int_variant() {
    let err = parse("struct s size=9x99999999999999999999\n").expect_err("overflow");
    let ParseError::InvalidInt { context, kind, .. } = err else {
        panic!("expected ParseError::InvalidInt, got {err:?}");
    };
    assert_eq!(context, IntContext::SizeHeight);
    assert_eq!(kind, IntErrorKind::PosOverflow);
}

#[test]
fn int_literal_overflow_surfaces_invalid_int_variant() {
    let err = parse("struct s\n  walls height=99999999999999999999\n").expect_err("overflow");
    let ParseError::InvalidInt { context, kind, .. } = err else {
        panic!("expected ParseError::InvalidInt, got {err:?}");
    };
    assert_eq!(context, IntContext::IntLiteral);
    assert_eq!(kind, IntErrorKind::PosOverflow);
}

#[test]
fn invalid_int_carries_position() {
    let err = parse("struct s\n  assert always(a -> eventually b within 4294967296)\n")
        .expect_err("overflow");
    // The bound `4294967296` starts at column 42; the structured error
    // points at the offending lexeme itself, not at the next token.
    assert_eq!(err.position(), pos(2, 42));
}

#[test]
fn invalid_int_user_message_is_string_compatible() {
    // The `cairn parse` CLI renders `user_message()`; legacy substring
    // matchers in downstream tooling must still see the same phrasing.
    let err = parse("struct s\n  assert always(a -> eventually b within 4294967296)\n")
        .expect_err("overflow");
    let msg = err.user_message();
    assert!(
        msg.contains("invalid `within` bound `4294967296`"),
        "got: {msg}"
    );
    assert!(msg.contains("out of range"), "got: {msg}");

    let err = parse("struct s size=99999999999999999999x9\n").expect_err("overflow");
    let msg = err.user_message();
    assert!(
        msg.contains("invalid integer literal `99999999999999999999`"),
        "got: {msg}"
    );
    assert!(msg.contains("out of range"), "got: {msg}");
}

#[test]
fn dotted_ref_constructor_pins_non_empty_shape() {
    // `DottedRef::new(head, tail)` makes the non-empty invariant a structural
    // property of the call: the head is mandatory, the tail may be empty.
    let single = DottedRef::new("sig".into(), Vec::new());
    assert_eq!(single.head(), "sig");
    assert!(single.tail().is_empty());
    assert_eq!(single.len().get(), 1);

    let multi = DottedRef::new("sig".into(), vec!["step".into(), "edge".into()]);
    assert_eq!(multi.head(), "sig");
    assert_eq!(multi.tail(), &["step".to_owned(), "edge".to_owned()]);
    assert_eq!(multi.len().get(), 3);

    // The fallback `try_from_segments` still rejects empty input.
    assert!(DottedRef::try_from_segments(Vec::new()).is_none());
    assert!(DottedRef::try_from_segments(vec!["a".into()]).is_some());
}

#[test]
fn dotted_ref_display_joins_with_dots() {
    let dr = DottedRef::new("home1".into(), vec!["entry".into()]);
    assert_eq!(dr.to_string(), "home1.entry");

    let single = DottedRef::new("outer".into(), Vec::new());
    assert_eq!(single.to_string(), "outer");
}

#[test]
fn dotted_ref_iterates_segments_in_order() {
    let dr = DottedRef::new("sig".into(), vec!["step".into(), "edge".into()]);
    let collected: Vec<&str> = (&dr).into_iter().map(String::as_str).collect();
    assert_eq!(collected, vec!["sig", "step", "edge"]);
}
