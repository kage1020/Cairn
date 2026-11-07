//! A block id, `mat=minecraft:stone`, is a `key=` value of its own (`spec/syntax` "Literals and
//! separators"), and only the parser refuses a malformed one.

use cairn_lang_core::ast::{Item, Statement, ValueKind};
use cairn_lang_core::parse;

use crate::{member_args, member_refusal};

fn block_id(id: &str) -> ValueKind {
    ValueKind::BlockId(id.to_owned())
}

#[test]
fn a_namespace_and_a_path_touching_a_colon_are_one_block_id() {
    assert_eq!(
        member_args("raw_block mat=minecraft:stone at=1,0,1"),
        [
            ("mat".to_owned(), block_id("minecraft:stone")),
            ("at".to_owned(), ValueKind::Coord { x: 1, y: 0, z: 1 }),
        ],
    );
    assert_eq!(
        member_args("raw_block mat=my_mod:copper_lamp"),
        [("mat".to_owned(), block_id("my_mod:copper_lamp"))],
    );
}

#[test]
fn a_touching_state_literal_folds_into_the_block_id() {
    for (written, id) in [
        ("minecraft:light[level=15]", "minecraft:light[level=15]"),
        (
            "minecraft:oak_stairs[half=top, facing=north]",
            "minecraft:oak_stairs[half=top,facing=north]",
        ),
    ] {
        assert_eq!(
            member_args(&format!("raw_block mat={written}")),
            [("mat".to_owned(), block_id(id))],
            "{written:?}",
        );
    }
}

#[test]
fn a_block_id_spans_its_namespace_path_and_literal() {
    let source = "struct s size=5x5\n  raw_block mat=minecraft:light[level=15] at=1,0,1\n";
    let module = parse(source).expect("parses");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("expected a struct");
    };
    let Statement::Generic { args, .. } = &body[0] else {
        panic!("expected a member line");
    };
    assert_eq!(
        &source[args[0].value.span.clone()],
        "minecraft:light[level=15]"
    );
    assert_eq!(
        &source[args[0].span.clone()],
        "mat=minecraft:light[level=15]"
    );
}

#[test]
fn a_colon_ending_a_header_line_is_no_block_id() {
    let source = "def hut size=3x3 tag=small:\n  floor mat_slot=f\n";
    let module = parse(source).unwrap_or_else(|err| panic!("{err}"));
    let Item::Def { args, .. } = &module.items[0] else {
        panic!("expected a def, got {:?}", module.items[0]);
    };
    assert_eq!(args[1].value.kind, ValueKind::Ident("small".to_owned()));
}

#[test]
fn a_malformed_block_id_is_refused_where_it_goes_wrong() {
    let spaced = "a space parts `minecraft` and `stone` from the `:` between them; a block id is \
                  `namespace:path` with nothing around the `:`, as `minecraft:stone`";
    let no_path = |got: &str| {
        format!(
            "a block id expected a path after `minecraft:`, got {got}; a block id is \
             `namespace:path`, as `minecraft:stone`"
        )
    };
    for (member, message, col) in [
        ("raw_block mat=minecraft: stone", spaced.to_owned(), 26),
        ("raw_block mat=minecraft :stone", spaced.to_owned(), 27),
        ("raw_block mat=minecraft : stone", spaced.to_owned(), 27),
        ("raw_block mat=minecraft:1", no_path("integer `1`"), 27),
        ("raw_block mat=minecraft:@stone", no_path("`@`"), 27),
        (
            "raw_block mat=minecraft:light[level=]",
            "the state literal on `minecraft:light` expected a value for `level`, got `]`; a \
             state literal is `[property=value]` pairs separated by `,`, as in `@oak_log[axis=x]`"
                .to_owned(),
            39,
        ),
        // The path is one identifier, so a second `:` or a `.` is whatever comes next.
        (
            "raw_block mat=minecraft:stone:x",
            "unexpected `:` in value position".to_owned(),
            32,
        ),
        (
            "raw_block mat=minecraft:stone.x",
            "unexpected `.` in value position".to_owned(),
            32,
        ),
    ] {
        assert_eq!(member_refusal(member), (message, col), "{member:?}");
    }
}

#[test]
fn a_slot_row_binding_a_block_id_is_told_to_write_a_canonical_token() {
    for (target, message) in [
        (
            "minecraft:stone",
            "a slot row binds a canonical token, not the block id `minecraft:stone`; write its \
             canonical token, `@stone`, or place the block as written with a `raw_block` or \
             `raw_fill`",
        ),
        (
            "minecraft:oak_log[axis=x]",
            "a slot row binds a canonical token, not the block id `minecraft:oak_log[axis=x]`; \
             write its canonical token, `@oak_log[axis=x]`, or place the block as written with a \
             `raw_block` or `raw_fill`",
        ),
        (
            "my_mod:copper_lamp",
            "a slot row binds a canonical token, not the block id `my_mod:copper_lamp`; write a \
             canonical token, as `@stone`, or place the block as written with a `raw_block` or \
             `raw_fill`",
        ),
    ] {
        let source = format!("theme t:\n  slot wall -> {target}\n");
        let err = parse(&source).expect_err("a slot row binds no block id");
        assert_eq!(
            (err.user_message(), err.position().col.get()),
            (message.to_owned(), 16),
            "{target:?}",
        );
    }
}
