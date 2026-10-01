//! A canonical token's block-state literal, `@oak_log[axis=x]`.
//!
//! `spec/materials-themes` "Canonical vocabulary" gives `@oak_log[axis=x]`
//! as a canonical block token, and everything past the parser already read
//! that shape: `resolve::classify_token` judges it on the part before the
//! `[`, and `block_array::material` turns the tail into the block's
//! properties. The parser stopped at the `[`, so none of it could be
//! written. It now reads the literal into the token's text, and it is the
//! one place a malformed literal is refused — the material layer reads the
//! tail leniently on that understanding.

mod common;

use cairn_lang_core::ast::{Item, ThemeRule, ValueKind};
use cairn_lang_core::parse;

use common::{lowered, only_structure};

/// The text and source slice of the value bound by the one slot of a
/// `theme t:` whose only row is `slot s -> {target}`.
fn slot_target(target: &str) -> (String, String) {
    let source = format!("theme t:\n  slot s -> {target}\n");
    let module = parse(&source).unwrap_or_else(|err| panic!("{target:?}: {err}"));
    let Item::Theme { body: rules, .. } = &module.items[0] else {
        panic!("expected a theme, got {:?}", module.items[0]);
    };
    let ThemeRule::Slot { value, .. } = &rules[0] else {
        panic!("expected a slot row, got {:?}", rules[0]);
    };
    let ValueKind::Token(text) = &value.kind else {
        panic!("{target:?} should be a token, got {:?}", value.kind);
    };
    (text.clone(), source[value.span.clone()].to_owned())
}

/// The message `parse` refuses a slot target with, and its 1-based column.
fn refusal(target: &str) -> (String, u32) {
    let source = format!("theme t:\n  slot s -> {target}\n");
    let err = parse(&source).expect_err("the target is malformed");
    (err.user_message(), err.position().col.get())
}

#[test]
fn the_literal_folds_into_the_token_text() {
    for (target, text) in [
        ("@oak_log[axis=x]", "oak_log[axis=x]"),
        (
            "@oak_stairs[half=top,facing=north]",
            "oak_stairs[half=top,facing=north]",
        ),
        // Spaces inside the brackets are not part of the text, so the
        // material layer never has to trim them.
        (
            "@oak_stairs[ half = top , facing = north ]",
            "oak_stairs[half=top,facing=north]",
        ),
        // The three shapes a block state's value takes.
        ("@cake[bites=3,lit=true]", "cake[bites=3,lit=true]"),
    ] {
        let (found, slice) = slot_target(target);
        assert_eq!(found, text, "{target:?}");
        // The value's span runs to the `]`, so a diagnostic on it
        // underlines the whole token.
        assert_eq!(slice, target, "{target:?}");
    }
}

#[test]
fn the_issues_example_builds_a_log_along_x() {
    let source =
        "theme t:\n  slot floor -> @oak_log[axis=x]\n\nstruct s size=3x3\n  floor mat_slot=floor\n";
    let ir = lowered(source);
    let structure = only_structure(&ir);
    let log = structure
        .palette
        .entries
        .iter()
        .find(|state| state.id == "minecraft:oak_log")
        .unwrap_or_else(|| panic!("no oak_log in {:?}", structure.palette.entries));
    assert_eq!(
        log.properties.get("axis").map(String::as_str),
        Some("x"),
        "{log:?}",
    );
}

/// `spec/compilation` "Within-phase conflicts and the palette": the two
/// spellings name one block, so a build that binds both holds one entry.
#[test]
fn two_spellings_of_one_state_are_one_palette_entry() {
    let source = "theme t:\n  slot a -> @oak_stairs[half=top,facing=north]\n  slot b -> @oak_stairs[facing=north,half=top]\n\nstruct s size=3x3\n  floor mat_slot=a\n  walls mat_slot=b height=1\n";
    let ir = lowered(source);
    let stairs: Vec<_> = only_structure(&ir)
        .palette
        .entries
        .iter()
        .filter(|state| state.id == "minecraft:oak_stairs")
        .collect();
    assert_eq!(stairs.len(), 1, "{stairs:?}");
}

#[test]
fn a_malformed_literal_is_refused_where_it_goes_wrong() {
    // `theme t:\n  slot s -> @oak_log` puts the `[` at column 21.
    for (target, expected, col) in [
        ("@oak_log[]", "expected a property name, got `]`", 22),
        (
            "@oak_log[axis=x",
            "expected `,` or `]`, got end of line",
            28,
        ),
        (
            "@oak_log[axis=]",
            "expected a value for `axis`, got `]`",
            27,
        ),
        ("@oak_log[=x]", "expected a property name, got `=`", 22),
        ("@oak_log[axis=x,]", "expected a property name, got `]`", 29),
        (
            "@oak_log[axis=x,,a=b]",
            "expected a property name, got `,`",
            29,
        ),
        (
            "@oak_log[axis=x a=b]",
            "expected `,` or `]`, got identifier `a`",
            29,
        ),
        (
            "@oak_log[axis=\"x\"]",
            "expected a value for `axis`, got string literal",
            27,
        ),
        ("@oak_log[axis]", "expected `=` after `axis`, got `]`", 26),
    ] {
        let (message, at) = refusal(target);
        assert!(
            message.contains(expected) && message.contains("`@oak_log`"),
            "{target:?}: {message}",
        );
        assert_eq!(at, col, "{target:?}: {message}");
    }
}

#[test]
fn a_property_named_twice_is_refused() {
    let (message, at) = refusal("@oak_log[axis=x,axis=y]");
    assert!(message.contains("sets `axis` twice"), "{message}");
    // At the second `axis`, which is the one that would have won.
    assert_eq!(at, 29);
}

#[test]
fn an_abstract_token_takes_no_state_literal() {
    let (message, at) = refusal("@floor.wood[axis=x]");
    assert!(
        message.contains("`@floor.wood` is an abstract material"),
        "{message}",
    );
    assert_eq!(at, 24);
}

/// A `[` after a space is not the token's: a slot takes one value, so the
/// row ends at the token and the bracket is refused as it always was.
#[test]
fn a_bracket_after_a_space_is_not_a_state_literal() {
    let (message, at) = refusal("@oak_log [axis=x]");
    assert_eq!(message, "expected end of line, got `[`");
    assert_eq!(at, 22);
}

/// Commas are optional in a value list, so a token and a nested list used
/// to be two items whether or not a space stood between them. Apart they
/// still are; touching, the `[` is the token's and `b` is no property.
#[test]
fn a_value_list_reads_a_touching_bracket_as_the_tokens() {
    let apart = parse("struct s size=3x3\n  thing mat=[@a [b]]\n").expect("two items");
    let Item::Struct { body, .. } = &apart.items[0] else {
        panic!("expected a struct");
    };
    let cairn_lang_core::ast::Statement::Generic { args, .. } = &body[0] else {
        panic!("expected a command, got {:?}", body[0]);
    };
    let ValueKind::List(items) = &args[0].value.kind else {
        panic!("expected a list, got {:?}", args[0].value.kind);
    };
    assert_eq!(items.len(), 2, "{items:?}");
    assert_eq!(items[0].kind, ValueKind::Token("a".to_owned()));

    let err = parse("struct s size=3x3\n  thing mat=[@a[b]]\n").expect_err("`b` is no property");
    assert!(
        err.user_message()
            .contains("expected `=` after `b`, got `]`"),
        "{}",
        err.user_message(),
    );
}
