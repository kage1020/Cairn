//! A coordinate triple, `at=4,3,2`, is a `key=` value of its own (`spec/syntax` "Literals and
//! separators"), and only the parser refuses a malformed one.

use cairn_lang_core::ast::{Item, Statement, ValueKind};
use cairn_lang_core::parse;

/// The `key=` arguments of the one member line under `struct s`, as `(key, value kind)`.
fn member_args(member: &str) -> Vec<(String, ValueKind)> {
    let source = format!("struct s size=5x5\n  {member}\n");
    let module = parse(&source).unwrap_or_else(|err| panic!("{member:?}: {err}"));
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("expected a struct, got {:?}", module.items[0]);
    };
    let Statement::Generic { args, .. } = &body[0] else {
        panic!("expected a member line, got {:?}", body[0]);
    };
    args.iter()
        .map(|arg| (arg.key.clone(), arg.value.kind.clone()))
        .collect()
}

/// The message `parse` refuses the member line with, and its 1-based column.
fn refusal(member: &str) -> (String, u32) {
    let source = format!("struct s size=5x5\n  {member}\n");
    let err = parse(&source).expect_err("the member line is malformed");
    (err.user_message(), err.position().col.get())
}

fn coord(x: u64, y: u64, z: u64) -> ValueKind {
    ValueKind::Coord { x, y, z }
}

#[test]
fn three_integers_joined_by_commas_are_one_coordinate() {
    assert_eq!(
        member_args("fill from=1,1,1 to=3,20,0"),
        [
            ("from".to_owned(), coord(1, 1, 1)),
            ("to".to_owned(), coord(3, 20, 0)),
        ],
    );
}

#[test]
fn a_coordinate_spans_its_three_components() {
    let source = "struct s size=5x5\n  fill from=1,10,100 to=0,0,0\n";
    let module = parse(source).expect("parses");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("expected a struct");
    };
    let Statement::Generic { args, .. } = &body[0] else {
        panic!("expected a member line");
    };
    assert_eq!(&source[args[0].value.span.clone()], "1,10,100");
    assert_eq!(&source[args[0].span.clone()], "from=1,10,100");
}

#[test]
fn a_selector_reads_a_coordinate_beside_its_separators() {
    let source = "struct s size=5x5\n  door[y=2,side=front, at=1,2,3, id=d] opened_by=sig.a\n";
    let module = parse(source).expect("parses");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("expected a struct");
    };
    let Statement::Generic {
        selector: Some(attrs),
        ..
    } = &body[0]
    else {
        panic!("expected a selector line");
    };
    let read: Vec<(&str, &ValueKind)> = attrs
        .iter()
        .map(|arg| (arg.key.as_str(), &arg.value.kind))
        .collect();
    assert_eq!(
        read,
        [
            ("y", &ValueKind::Int(2)),
            ("side", &ValueKind::Ident("front".to_owned())),
            ("at", &coord(1, 2, 3)),
            ("id", &ValueKind::Ident("d".to_owned())),
        ],
    );
}

#[test]
fn a_list_of_integers_is_no_coordinate() {
    let args = member_args("floor tags=[1,2,3]");
    let [(key, ValueKind::List(items))] = args.as_slice() else {
        panic!("expected one list argument, got {args:?}");
    };
    assert_eq!(key, "tags");
    let kinds: Vec<&ValueKind> = items.iter().map(|item| &item.kind).collect();
    assert_eq!(
        kinds,
        [&ValueKind::Int(1), &ValueKind::Int(2), &ValueKind::Int(3)]
    );
}

#[test]
fn a_malformed_coordinate_is_refused_where_it_goes_wrong() {
    let spaced = "a coordinate triple is written with nothing between its components and the `,` \
                  joining them, as `4,3,2`";
    for (member, message, col) in [
        ("fill from=1, 1, 1", spaced.to_owned(), 14),
        ("fill from=1 ,1,1", spaced.to_owned(), 15),
        ("fill from=1,1 ,1", spaced.to_owned(), 17),
        (
            "fill from=1,1",
            "the coordinate `1,1` has two components; a coordinate triple is three, `x,y,z`, as \
             `4,3,2`"
                .to_owned(),
            13,
        ),
        (
            "fill from=1,1,1,1",
            "the coordinate `1,1,1,1` has more than three components; a coordinate triple is \
             three, `x,y,z`, as `4,3,2`"
                .to_owned(),
            13,
        ),
        (
            "fill from=1,-1,1",
            "a coordinate component is never negative: a coordinate counts from the low corner \
             of the footprint, as `0,0,0`"
                .to_owned(),
            15,
        ),
        (
            "fill from=99999999999999999999,1,1",
            "invalid integer literal `99999999999999999999`: value out of range".to_owned(),
            13,
        ),
        // The comma after a whole triple ends it, and a bare one is no value.
        (
            "fill from=1,1,1,",
            "unexpected `,` in value position".to_owned(),
            18,
        ),
        // Only a `key=` value is read as one: a bare value or a binding takes one integer.
        (
            "fill 1,1,1",
            "unexpected `,` in value position".to_owned(),
            9,
        ),
    ] {
        assert_eq!(refusal(member), (message, col), "{member:?}");
    }
}
