//! A `raw_repeat` line takes `count=` and a `step=` triple, with the raw members it repeats
//! indented under it (`spec/syntax` "Nesting").

use cairn_lang_core::ast::{Item, Statement, ValueKind};
use cairn_lang_core::parse;

use crate::member_refusal;

#[test]
fn a_raw_repeat_holds_the_lines_indented_under_it() {
    let source = "struct s size=9x3\n  raw_repeat count=3 step=3,0,0\n    \
                  raw_fill mat=minecraft:glass_pane from=0,1,0 to=1,2,0\n    \
                  raw_block mat=minecraft:torch at=2,1,0 origin=imported\n";
    let module = parse(source).expect("parses");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("expected a struct, got {:?}", module.items[0]);
    };
    let [
        Statement::Generic {
            keyword,
            args,
            children,
            ..
        },
    ] = body.as_slice()
    else {
        panic!("expected one member line, got {body:?}");
    };
    assert_eq!(keyword, "raw_repeat");
    let read: Vec<(&str, &ValueKind)> = args
        .iter()
        .map(|arg| (arg.key.as_str(), &arg.value.kind))
        .collect();
    assert_eq!(
        read,
        [
            ("count", &ValueKind::Int(3)),
            ("step", &ValueKind::Coord { x: 3, y: 0, z: 0 }),
        ],
    );
    let lines: Vec<&str> = children
        .iter()
        .map(|child| match child {
            Statement::Generic { keyword, .. } => keyword.as_str(),
            other => panic!("expected a member line, got {other:?}"),
        })
        .collect();
    assert_eq!(lines, ["raw_fill", "raw_block"]);
}

#[test]
fn a_raw_repeat_does_not_take_its_member_on_its_own_line() {
    assert_eq!(
        member_refusal(
            "raw_repeat count=3 step=3,0,0: raw_fill mat=minecraft:glass from=0,1,0 to=1,2,0"
        ),
        ("unexpected `:` in value position".to_owned(), 32),
    );
}
