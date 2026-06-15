//! Acceptance tests for individual parser building blocks.

use cairn_core::ast::{Arg, Header, Item, ThemeRule, Value};
use cairn_core::parse;

#[test]
fn parses_cairn_header() {
    let module = parse("@cairn 2026.06\n").expect("parse");
    assert_eq!(
        module.headers,
        vec![Header::Cairn {
            version: "2026.06".into()
        }]
    );
}

#[test]
fn parses_requires_header() {
    let module = parse("@requires version>=1.20\n").expect("parse");
    assert_eq!(
        module.headers,
        vec![Header::Requires {
            requirement: "version>=1.20".into()
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
            value: Value::Size { w: 9, h: 7 }
        }]
    );
    assert_eq!(body.len(), 1);
    assert_eq!(body[0].keyword, "floor");
    assert_eq!(
        body[0].args,
        vec![Arg {
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
    assert_eq!(body[0].keyword, "door");
    assert_eq!(
        body[0].selector,
        Some(vec![Arg {
            key: "id".into(),
            value: Value::Ident("front".into())
        }])
    );
    assert_eq!(
        body[0].args,
        vec![Arg {
            key: "opened_by".into(),
            value: Value::DotRef(vec!["sig".into(), "open".into()])
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
    assert_eq!(body[0].keyword, "pressure_plate");
    assert_eq!(
        body[0].binding,
        Some(Value::DotRef(vec!["sig".into(), "step".into()]))
    );
}

#[test]
fn parses_logic_expression() {
    use cairn_core::ast::Expr;
    let module = parse("struct s\n  logic sig.open = sig.step or sig.exit\n").expect("parse");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("not a struct");
    };
    let extra = body[0].extra.as_ref().expect("logic extra");
    if let cairn_core::ast::Extra::LogicEq { lhs, rhs } = extra {
        assert_eq!(lhs, &vec!["sig".to_string(), "open".to_string()]);
        assert!(matches!(rhs, Expr::Or(_, _)));
    } else {
        panic!("expected LogicEq");
    }
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
    let cairn_core::ast::Extra::AssertTruth {
        inputs,
        output,
        rows,
    } = body[0].extra.as_ref().expect("extra")
    else {
        panic!("expected AssertTruth");
    };
    assert_eq!(inputs.len(), 2);
    assert_eq!(output, &vec!["sig".to_string(), "open".to_string()]);
    assert_eq!(rows.len(), 4);
    assert_eq!(rows[0].inputs, "00");
    assert_eq!(rows[0].output, 0);
    assert_eq!(rows[3].inputs, "11");
    assert_eq!(rows[3].output, 1);
}

#[test]
fn parses_assert_always() {
    let module = parse("struct s\n  assert always(sig.step -> eventually sig.open within 2)\n")
        .expect("parse");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("not a struct");
    };
    let cairn_core::ast::Extra::AssertAlways {
        antecedent,
        consequent,
        within,
    } = body[0].extra.as_ref().expect("extra")
    else {
        panic!("expected AssertAlways");
    };
    assert_eq!(antecedent, &vec!["sig".to_string(), "step".to_string()]);
    assert_eq!(consequent, &vec!["sig".to_string(), "open".to_string()]);
    assert_eq!(*within, 2);
}

#[test]
fn parses_connect_with_positional_to() {
    let module =
        parse("site hamlet:\n  connect home1.entry to home2.entry path=@gravel\n").expect("parse");
    let Item::Site { body, .. } = &module.items[0] else {
        panic!("not a site");
    };
    let cmd = &body[0];
    assert_eq!(cmd.keyword, "connect");
    assert_eq!(cmd.positional.len(), 3);
    assert_eq!(
        cmd.positional[0],
        Value::DotRef(vec!["home1".into(), "entry".into()])
    );
    assert_eq!(cmd.positional[1], Value::Ident("to".into()));
    assert_eq!(
        cmd.positional[2],
        Value::DotRef(vec!["home2".into(), "entry".into()])
    );
    assert_eq!(
        cmd.args,
        vec![Arg {
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
    assert_eq!(body[0].keyword, "level");
    assert_eq!(body[0].children.len(), 1);
    assert_eq!(body[0].children[0].keyword, "door");
}
