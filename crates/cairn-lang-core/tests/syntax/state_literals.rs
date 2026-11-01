//! A canonical token's block-state literal, `@oak_log[axis=x]`, folds into the token's text
//! (`spec/materials-themes` "Canonical vocabulary"), and only the parser refuses a malformed one.

use cairn_lang_core::ast::{Item, Statement, ThemeRule, Value, ValueKind};
use cairn_lang_core::block_array::{BlockState, resolve_block_state};
use cairn_lang_core::intent::ValueWithSpan;
use cairn_lang_core::lock::hash_resolved_ir;
use cairn_lang_core::{DiagnosticCode, parse};

use crate::common::{diagnose, lowered, only_structure};

/// The value a one-row `slot s -> {target}` theme binds, with the source it was parsed from.
fn slot_value(target: &str) -> (Value, String) {
    let source = format!("theme t:\n  slot s -> {target}\n");
    let module = parse(&source).unwrap_or_else(|err| panic!("{target:?}: {err}"));
    let Item::Theme { body: rules, .. } = &module.items[0] else {
        panic!("expected a theme, got {:?}", module.items[0]);
    };
    let ThemeRule::Slot { value, .. } = &rules[0] else {
        panic!("expected a slot row, got {:?}", rules[0]);
    };
    (value.clone(), source)
}

/// The text and source slice of the value [`slot_value`] parses.
fn slot_target(target: &str) -> (String, String) {
    let (value, source) = slot_value(target);
    let ValueKind::Token(text) = &value.kind else {
        panic!("{target:?} should be a token, got {:?}", value.kind);
    };
    (text.clone(), source[value.span.clone()].to_owned())
}

/// The block state the material layer reads out of `target`'s folded text.
fn state_of(target: &str) -> BlockState {
    let (value, _) = slot_value(target);
    resolve_block_state(&ValueWithSpan::from_value(value), None)
        .unwrap_or_else(|err| panic!("{target:?}: {err:?}"))
}

/// A one-theme, one-struct source binding `target` to a floor.
fn floored(target: &str) -> String {
    format!("theme t:\n  slot f -> {target}\n\nstruct s size=3x3\n  floor mat_slot=f\n")
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
        // Spaces inside the brackets are not part of the text.
        (
            "@oak_stairs[ half = top , facing = north ]",
            "oak_stairs[half=top,facing=north]",
        ),
        // The three shapes a block state's value takes.
        ("@cake[bites=3,lit=true]", "cake[bites=3,lit=true]"),
    ] {
        let (found, slice) = slot_target(target);
        assert_eq!(found, text, "{target:?}");
        assert_eq!(slice, target, "{target:?}");
    }
}

#[test]
fn a_log_along_x_builds_from_the_literal() {
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

/// `spec/compilation` "Within-phase conflicts and the palette".
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
fn the_order_a_literal_spells_its_properties_in_reaches_no_artifact() {
    let a = lowered(&floored("@oak_stairs[half=top,facing=north]"));
    let b = lowered(&floored("@oak_stairs[facing=north,half=top]"));
    let keys = |ir: &cairn_lang_core::block_array::BlockArrayIr| -> Vec<Vec<String>> {
        only_structure(ir)
            .palette
            .entries
            .iter()
            .map(|state| state.properties.keys().cloned().collect())
            .collect()
    };
    assert_eq!(keys(&a), keys(&b));
    assert_eq!(
        hash_resolved_ir(&a).expect("hash"),
        hash_resolved_ir(&b).expect("hash"),
    );
}

#[test]
fn a_folded_literal_reads_back_as_the_properties_it_was_written_with() {
    for (target, id, pairs) in [
        (
            "@oak_log[axis=x]",
            "minecraft:oak_log",
            &[("axis", "x")][..],
        ),
        (
            "@oak_stairs[ half = top , facing = north ]",
            "minecraft:oak_stairs",
            &[("facing", "north"), ("half", "top")],
        ),
        (
            "@cake[lit=true,bites=3]",
            "minecraft:cake",
            &[("bites", "3"), ("lit", "true")],
        ),
    ] {
        let state = state_of(target);
        assert_eq!(state.id, id, "{target:?}");
        let read: Vec<(&str, &str)> = state
            .properties
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect();
        assert_eq!(read, pairs, "{target:?}");
        let rendered = format!(
            "@{}[{}]",
            id.trim_start_matches("minecraft:"),
            state.properties_text(),
        );
        assert_eq!(
            state_of(&rendered),
            state,
            "{target:?} rendered as {rendered:?}"
        );
    }
}

/// `lowered` skips the check pass, which judges a slot's binding on its own.
#[test]
fn a_well_formed_literal_is_clean_through_check() {
    for target in ["@oak_log[axis=x]", "@oak_stairs[half=top, facing=north]"] {
        let source = floored(target);
        assert_eq!(diagnose(&source), [], "{target:?}");
        let codes: Vec<_> = lowered(&source)
            .diagnostics
            .iter()
            .map(|d| d.code)
            .collect();
        assert_eq!(codes, [DiagnosticCode::StateLiteralUnchecked], "{target:?}");
    }
}

#[test]
fn an_unchecked_literal_is_announced_on_its_value() {
    let source = floored("@oak_log[axis=q]");
    let ir = lowered(&source);
    let found: Vec<_> = ir
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::StateLiteralUnchecked)
        .collect();
    assert_eq!(found.len(), 1, "{:?}", ir.diagnostics);
    assert_eq!(&source[found[0].span.clone()], "@oak_log[axis=q]");
    assert!(
        found[0]
            .primary
            .contains("`minecraft:oak_log` is written with `axis=q`"),
        "{}",
        found[0].primary,
    );
    assert!(
        lowered(&floored("@oak_log"))
            .diagnostics
            .iter()
            .all(|d| d.code != DiagnosticCode::StateLiteralUnchecked),
    );
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

/// The lexer ends every file with a line break.
#[test]
fn a_literal_cut_short_by_the_end_of_the_file_reports_end_of_line() {
    for (target, expected) in [
        ("@oak_log[", "expected a property name, got end of line"),
        (
            "@oak_log[axis",
            "expected `=` after `axis`, got end of line",
        ),
        (
            "@oak_log[axis=",
            "expected a value for `axis`, got end of line",
        ),
        ("@oak_log[axis=x", "expected `,` or `]`, got end of line"),
    ] {
        let source = format!("theme t:\n  slot s -> {target}");
        let err = parse(&source).expect_err("the literal is cut short");
        assert!(
            err.user_message().contains(expected),
            "{target:?}: {}",
            err.user_message(),
        );
    }
}

/// A dotted token is abstract, so it has no literal.
#[test]
fn an_abstract_token_leaves_a_touching_bracket_alone() {
    let (message, at) = refusal("@floor.wood[axis=x]");
    assert_eq!(message, "expected end of line, got `[`");
    assert_eq!(at, 24);
}

#[test]
fn a_bracket_touching_a_dotted_token_opens_what_follows() {
    let module = parse("struct s size=3x3\n  thing mat=@a.b[1]\n").expect("a positional follows");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("expected a struct");
    };
    let Statement::Generic {
        args, positional, ..
    } = &body[0]
    else {
        panic!("expected a command, got {:?}", body[0]);
    };
    assert_eq!(args[0].value.kind, ValueKind::Token("a.b".to_owned()));
    assert!(
        matches!(&positional[..], [Value { kind: ValueKind::List(items), .. }] if items.len() == 1),
        "{positional:?}",
    );

    let module = parse("struct s size=3x3\n  thing mat=[@a.b[c]]\n").expect("two items");
    let Item::Struct { body, .. } = &module.items[0] else {
        panic!("expected a struct");
    };
    let Statement::Generic { args, .. } = &body[0] else {
        panic!("expected a command, got {:?}", body[0]);
    };
    let ValueKind::List(items) = &args[0].value.kind else {
        panic!("expected a list, got {:?}", args[0].value.kind);
    };
    assert_eq!(items.len(), 2, "{items:?}");
    assert_eq!(items[0].kind, ValueKind::Token("a.b".to_owned()));
}

#[test]
fn a_bracket_after_a_space_is_not_a_state_literal() {
    let (message, at) = refusal("@oak_log [axis=x]");
    assert_eq!(message, "expected end of line, got `[`");
    assert_eq!(at, 22);
}

/// Commas are optional in a value list, so apart, a token and a nested list are two items.
#[test]
fn a_value_list_reads_a_touching_bracket_as_the_tokens() {
    let apart = parse("struct s size=3x3\n  thing mat=[@a [b]]\n").expect("two items");
    let Item::Struct { body, .. } = &apart.items[0] else {
        panic!("expected a struct");
    };
    let Statement::Generic { args, .. } = &body[0] else {
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

#[test]
fn a_walkway_path_carries_its_literal_into_the_walkway_palette() {
    let source = crate::common::read_example("village.crn").replace(
        "connect home1.entry to home2.entry path=@gravel",
        "connect home1.entry to home2.entry path=@gravel[waterlogged=true]",
    );
    assert!(
        source.contains("@gravel[waterlogged=true]"),
        "the example moved"
    );
    assert_eq!(diagnose(&source), [], "the literal is clean through check");
    let ir = lowered(&source);
    let holding: Vec<&String> = ir
        .structures
        .iter()
        .filter(|(_, array)| {
            array.palette.entries.iter().any(|state| {
                state.id == "minecraft:gravel"
                    && state.properties.get("waterlogged").map(String::as_str) == Some("true")
            })
        })
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        holding.len(),
        1,
        "{:?}",
        ir.structures.keys().collect::<Vec<_>>()
    );
    assert!(holding[0].contains("walkway"), "{holding:?}");
    let unchecked: Vec<_> = ir
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::StateLiteralUnchecked)
        .collect();
    assert_eq!(unchecked.len(), 1, "{:?}", ir.diagnostics);
    assert_eq!(
        &source[unchecked[0].span.clone()],
        "@gravel[waterlogged=true]"
    );
}
