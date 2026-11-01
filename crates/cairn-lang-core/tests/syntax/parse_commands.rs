//! What each header, block, command and expression parses to, and which
//! malformed ones the parser refuses.

use std::num::{IntErrorKind, NonZeroU32};

use cairn_lang_core::ast::{
    DottedRef, Expr, Header, Item, RawRequirement, RawVersion, Statement, ThemeRule, Value,
    ValueKind,
};
use cairn_lang_core::error::IntContext;
use cairn_lang_core::logic_library::{LogicPrimitive, Notation};
use cairn_lang_core::{ParseError, parse};

use crate::pos;

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

#[test]
fn parses_cairn_header() {
    let module = parse("@cairn 2026.06\n").expect("parse");
    assert_eq!(module.headers.len(), 1);
    let Header::Cairn { version, .. } = &module.headers[0] else {
        panic!("expected Cairn header, got {:?}", module.headers[0]);
    };
    assert_eq!(version, &RawVersion::new("2026.06"));
}

#[test]
fn parses_requires_header() {
    let module = parse("@requires version>=1.20\n").expect("parse");
    assert_eq!(module.headers.len(), 1);
    let Header::Requires { requirement, .. } = &module.headers[0] else {
        panic!("expected Requires header, got {:?}", module.headers[0]);
    };
    assert_eq!(requirement, &RawRequirement::new("version>=1.20"));
}

#[test]
fn parses_intended_targets_header() {
    let module = parse("@intended_targets [\"1.20.4\",\"1.21.4\"]\n").expect("parse");
    assert_eq!(module.headers.len(), 1);
    let Header::IntendedTargets { targets, .. } = &module.headers[0] else {
        panic!("expected IntendedTargets, got {:?}", module.headers[0]);
    };
    assert_eq!(targets, &vec!["1.20.4".to_owned(), "1.21.4".to_owned()]);
}

#[test]
fn parses_empty_theme_block() {
    let module = parse("theme medieval:\n").expect("parse");
    assert_eq!(module.items.len(), 1);
    let Item::Theme { name, body, .. } = &module.items[0] else {
        panic!("expected Theme item, got {:?}", module.items[0]);
    };
    assert_eq!(name, "medieval");
    assert_eq!(body.as_slice(), []);
}

#[test]
fn parses_theme_with_slot() {
    let module = parse("theme medieval:\n  slot floor -> @oak_planks\n").expect("parse");
    let Item::Theme { body, .. } = &module.items[0] else {
        panic!("not a theme");
    };
    let ThemeRule::Slot { slot, value, .. } = &body[0] else {
        panic!("expected Slot rule, got {:?}", body[0]);
    };
    assert_eq!(slot, "floor");
    assert!(matches!(value, Value { kind: ValueKind::Token(s), .. } if s == "oak_planks"));
}

#[test]
fn parses_theme_with_selector() {
    let module =
        parse("theme medieval:\n  window[class=small] -> frame=@spruce_wood\n").expect("parse");
    let Item::Theme { body, .. } = &module.items[0] else {
        panic!("not a theme");
    };
    let ThemeRule::Selector {
        keyword,
        attrs,
        bindings,
        ..
    } = &body[0]
    else {
        panic!("expected Selector rule, got {:?}", body[0]);
    };
    assert_eq!(keyword, "window");
    assert_eq!(attrs.len(), 1);
    assert_eq!(attrs[0].key, "class");
    assert!(matches!(&attrs[0].value, Value { kind: ValueKind::Ident(s), .. } if s == "small"));
    assert_eq!(bindings.len(), 1);
    assert_eq!(bindings[0].key, "frame");
    assert!(
        matches!(&bindings[0].value, Value { kind: ValueKind::Token(s), .. } if s == "spruce_wood")
    );
}

#[test]
fn parses_struct_with_size_and_one_child() {
    let module = parse("struct cottage size=9x7\n  floor mat_slot=floor\n").expect("parse");
    let Item::Struct {
        name, args, body, ..
    } = &module.items[0]
    else {
        panic!("not a struct");
    };
    assert_eq!(name, "cottage");
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].key, "size");
    assert!(matches!(args[0].value.kind, ValueKind::Size { w, h } if w == nz(9) && h == nz(7)));
    assert_eq!(body.len(), 1);
    let Statement::Generic { keyword, args, .. } = &body[0] else {
        panic!("expected Generic, got {:?}", body[0]);
    };
    assert_eq!(keyword, "floor");
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].key, "mat_slot");
    assert!(matches!(&args[0].value, Value { kind: ValueKind::Ident(s), .. } if s == "floor"));
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
    let attrs = selector.as_ref().expect("selector must be present");
    assert_eq!(attrs.len(), 1);
    assert_eq!(attrs[0].key, "id");
    assert!(matches!(&attrs[0].value, Value { kind: ValueKind::Ident(s), .. } if s == "front"));
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].key, "opened_by");
    assert!(
        matches!(&args[0].value, Value { kind: ValueKind::DotRef(r), .. } if r == &dr(&["sig", "open"]))
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
    let binding = binding.as_ref().expect("binding must be present");
    assert!(
        matches!(binding, Value { kind: ValueKind::DotRef(r), .. } if r == &dr(&["sig", "step"]))
    );
}

/// The right-hand side of `logic sig.x = {expression}`.
fn logic_rhs(expression: &str) -> Expr {
    let module = parse(&format!("struct s\n  logic sig.x = {expression}\n")).expect("parse");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("not a struct");
    };
    let Statement::Logic { rhs, .. } = &body[0] else {
        panic!("expected Logic, got {:?}", body[0]);
    };
    rhs.clone()
}

fn name(segments: &[&str]) -> Expr {
    Expr::Ref(dr(segments))
}

fn infix(primitive: LogicPrimitive, lhs: Expr, rhs: Expr) -> Expr {
    Expr::Infix {
        primitive,
        lhs: Box::new(lhs),
        rhs: Box::new(rhs),
    }
}

fn not(operand: Expr) -> Expr {
    Expr::Prefix {
        primitive: LogicPrimitive::Not,
        operand: Box::new(operand),
    }
}

#[test]
fn parses_logic_expression() {
    let module = parse("struct s\n  logic sig.open = sig.step or sig.exit\n").expect("parse");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("not a struct");
    };
    let Statement::Logic { lhs, rhs, .. } = &body[0] else {
        panic!("expected Logic, got {:?}", body[0]);
    };
    assert_eq!(lhs, &dr(&["sig", "open"]));
    assert_eq!(
        rhs,
        &infix(
            LogicPrimitive::Or,
            name(&["sig", "step"]),
            name(&["sig", "exit"]),
        ),
    );
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
        ..
    } = &body[0]
    else {
        panic!("expected AssertTruth, got {:?}", body[0]);
    };
    assert_eq!(inputs.len(), 2);
    assert_eq!(output, &dr(&["sig", "open"]));
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].inputs, "00");
    assert_eq!(rows[0].output, Some(false));
    assert_eq!(rows[3].inputs, "11");
    assert_eq!(rows[3].output, Some(true));
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
        ..
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
    assert!(
        matches!(&positional[0], Value { kind: ValueKind::DotRef(r), .. } if r == &dr(&["home1", "entry"]))
    );
    assert!(matches!(&positional[1], Value { kind: ValueKind::Ident(s), .. } if s == "to"));
    assert!(
        matches!(&positional[2], Value { kind: ValueKind::DotRef(r), .. } if r == &dr(&["home2", "entry"]))
    );
    assert_eq!(args.len(), 1);
    assert_eq!(args[0].key, "path");
    assert!(matches!(&args[0].value, Value { kind: ValueKind::Token(s), .. } if s == "gravel"));
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
    let (a, b, c) = (name(&["a"]), name(&["b"]), name(&["c"]));
    assert_eq!(
        logic_rhs("a and b or c"),
        infix(
            LogicPrimitive::Or,
            infix(LogicPrimitive::And, a.clone(), b.clone()),
            c.clone(),
        ),
    );
    assert_eq!(
        logic_rhs("a or b and c"),
        infix(LogicPrimitive::Or, a, infix(LogicPrimitive::And, b, c)),
    );
}

#[test]
fn logic_not_higher_than_and() {
    assert_eq!(
        logic_rhs("not a and b"),
        infix(LogicPrimitive::And, not(name(&["a"])), name(&["b"])),
    );
}

#[test]
fn one_rank_groups_left_to_right() {
    let (a, b, c) = (name(&["a"]), name(&["b"]), name(&["c"]));
    for primitive in [LogicPrimitive::And, LogicPrimitive::Or] {
        let word = primitive.name();
        assert_eq!(
            logic_rhs(&format!("a {word} b {word} c")),
            infix(primitive, infix(primitive, a.clone(), b.clone()), c.clone()),
        );
    }
}

#[test]
fn ranks_group_first_and_parentheses_group_as_written() {
    let (a, b, c) = (
        name(&["sig", "a"]),
        name(&["sig", "b"]),
        name(&["sig", "c"]),
    );
    assert_eq!(
        logic_rhs("sig.a or sig.b and not sig.c"),
        infix(
            LogicPrimitive::Or,
            a.clone(),
            infix(LogicPrimitive::And, b.clone(), not(c.clone())),
        ),
    );
    assert_eq!(
        logic_rhs("(sig.a or sig.b) and not (sig.c)"),
        infix(LogicPrimitive::And, infix(LogicPrimitive::Or, a, b), not(c)),
    );
}

#[test]
fn every_library_primitive_parses_in_its_notation() {
    let (a, b) = (name(&["sig", "a"]), name(&["sig", "b"]));
    for primitive in LogicPrimitive::ALL {
        let word = primitive.name();
        let (expression, expected) = match primitive.notation() {
            Notation::Prefix => (
                format!("{word} sig.a"),
                Expr::Prefix {
                    primitive,
                    operand: Box::new(a.clone()),
                },
            ),
            Notation::Infix { .. } => (
                format!("sig.a {word} sig.b"),
                infix(primitive, a.clone(), b.clone()),
            ),
        };
        assert_eq!(logic_rhs(&expression), expected, "{expression:?}");
    }
}

#[test]
fn an_expression_serialises_with_its_primitive_by_name() {
    for (expression, expected) in [
        (
            "not sig.a",
            r#"{"kind":"Prefix","value":{"primitive":"not","operand":{"kind":"Ref","value":["sig","a"]}}}"#,
        ),
        (
            "sig.a and sig.b",
            r#"{"kind":"Infix","value":{"primitive":"and","lhs":{"kind":"Ref","value":["sig","a"]},"rhs":{"kind":"Ref","value":["sig","b"]}}}"#,
        ),
    ] {
        assert_eq!(
            serde_json::to_string(&logic_rhs(expression)).expect("serialise"),
            expected,
            "{expression:?}",
        );
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
    assert!(err.user_message().contains("expects a string such as"));
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
    // 2^32 is one past `u32::MAX`.
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
fn a_within_bound_past_i64_is_a_within_bound_error_not_a_literal_one() {
    let err = parse("struct s\n  assert always(a -> eventually b within 99999999999999999999)\n")
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
    assert_eq!(lexeme, "99999999999999999999");
    assert_eq!(kind, IntErrorKind::PosOverflow);
}

/// The directive re-lexes its value as a detached slice, so every position out of it is rebased.
#[test]
fn an_error_inside_intended_targets_points_at_the_directive() {
    let source = "@cairn 2026.06\n@intended_targets [\"a\", 99999999999999999999]\n";
    let err = parse(source).expect_err("overflow");
    // `@intended_targets ` is 18 characters and the list opens at 19; the `9` is 6 further in.
    assert_eq!(err.position(), pos(2, 25));
    assert_eq!(
        source.lines().nth(1).and_then(|l| l.chars().nth(24)),
        Some('9'),
        "the asserted column should land on the offending literal",
    );
}

#[test]
fn a_rebased_position_tracks_the_directives_own_line() {
    let err = parse(
        "@cairn 2026.06\n@requires version>=1.20\n@intended_targets [\"a\", 99999999999999999999]\n",
    )
    .expect_err("overflow");
    assert_eq!(err.position(), pos(3, 25));
}

#[test]
fn a_zero_extent_inside_intended_targets_is_located_too() {
    let err = parse("@cairn 2026.06\n@intended_targets [\"a\", 0x3]\n").expect_err("zero extent");
    assert_eq!(err.position(), pos(2, 25));
}

#[test]
fn invalid_int_carries_position() {
    let err = parse("struct s\n  assert always(a -> eventually b within 4294967296)\n")
        .expect_err("overflow");
    // The bound `4294967296` starts at column 42.
    assert_eq!(err.position(), pos(2, 42));
}

#[test]
fn invalid_int_user_message_is_string_compatible() {
    // Downstream tooling matches on this phrasing.
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
    let single = DottedRef::new("sig".into(), Vec::new());
    assert_eq!(single.head(), "sig");
    assert_eq!(single.tail(), [] as [String; 0]);
    assert_eq!(single.len().get(), 1);

    let multi = DottedRef::new("sig".into(), vec!["step".into(), "edge".into()]);
    assert_eq!(multi.head(), "sig");
    assert_eq!(multi.tail(), &["step".to_owned(), "edge".to_owned()]);
    assert_eq!(multi.len().get(), 3);

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
