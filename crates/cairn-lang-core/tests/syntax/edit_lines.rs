//! A top-level `edit` line parses to its target and the keys it sets, and a malformed one is
//! refused where it goes wrong (`spec/components-editing-sites` "Editing model").

use cairn_lang_core::ast::{EditTarget, ThemeRule, ValueKind};
use cairn_lang_core::parse;

/// The text of `span` in `source`.
fn text<'a>(source: &'a str, span: &std::ops::Range<usize>) -> &'a str {
    &source[span.clone()]
}

/// The refusal for `source`, as `line:col: message`.
fn refusal(source: &str) -> String {
    let error = parse(source).expect_err("the source should be refused");
    format!("{}: {}", error.position(), error.user_message())
}

#[test]
fn a_selector_edit_joins_its_brackets_and_keeps_its_keys_in_order() {
    let source = "edit window[class=vent][level=floor2] set shape=arch y=2\n";
    let module = parse(source).expect("parses");
    let [edit] = module.edits.as_slice() else {
        panic!("expected one edit, got {:?}", module.edits);
    };
    let EditTarget::Selector { keyword, attrs, .. } = &edit.target else {
        panic!("expected a selector, got {:?}", edit.target);
    };
    assert_eq!(keyword, "window");
    let attrs: Vec<(&str, &ValueKind)> = attrs
        .iter()
        .map(|arg| (arg.key.as_str(), &arg.value.kind))
        .collect();
    assert_eq!(
        attrs,
        [
            ("class", &ValueKind::Ident("vent".into())),
            ("level", &ValueKind::Ident("floor2".into())),
        ],
    );
    let set: Vec<(&str, &ValueKind)> = edit
        .set
        .iter()
        .map(|arg| (arg.key.as_str(), &arg.value.kind))
        .collect();
    assert_eq!(
        set,
        [
            ("shape", &ValueKind::Ident("arch".into())),
            ("y", &ValueKind::Int(2)),
        ],
    );
    assert_eq!(edit.target.keyword(), "window");
    assert_eq!(text(source, edit.target.keyword_span()), "window");
    assert_eq!(
        text(source, edit.target.span()),
        "window[class=vent][level=floor2]"
    );
    assert_eq!(text(source, &edit.span), source.trim_end());
}

#[test]
fn an_address_edit_reads_each_step_and_a_dotted_side() {
    let source = "edit level[1].door@inside.back[0] set side=front at=center\n";
    let module = parse(source).expect("parses");
    let EditTarget::Address { address, .. } = &module.edits[0].target else {
        panic!("expected an address, got {:?}", module.edits[0].target);
    };
    let steps: Vec<(&str, Option<&str>, u32)> = address
        .segments()
        .iter()
        .map(|step| (step.role.as_str(), step.side.as_deref(), step.index))
        .collect();
    assert_eq!(
        steps,
        [("level", None, 1), ("door", Some("inside.back"), 0)]
    );
    assert_eq!(address.to_string(), "level[1].door@inside.back[0]");
    assert_eq!(module.edits[0].target.keyword(), "door");
    assert_eq!(text(source, module.edits[0].target.keyword_span()), "door");
    assert_eq!(
        text(source, module.edits[0].target.span()),
        "level[1].door@inside.back[0]"
    );
}

#[test]
fn edits_stand_anywhere_among_the_items() {
    let source = "edit window@front[0] set y=2\n\n\
                  struct s size=3x3\n  window side=front\n\n\
                  edit roof[0] set overhang=1\n\
                  theme t:\n  slot a -> @oak_planks\n";
    let module = parse(source).expect("parses");
    assert_eq!(module.items.len(), 2);
    let targets: Vec<&str> = module
        .edits
        .iter()
        .map(|edit| text(source, edit.target.span()))
        .collect();
    assert_eq!(targets, ["window@front[0]", "roof[0]"]);
}

#[test]
fn a_theme_selector_joins_its_brackets_too() {
    let source = "theme t:\n  window[class=vent][level=floor2] -> frame=@oak\n";
    let module = parse(source).expect("parses");
    let cairn_lang_core::ast::Item::Theme { body, .. } = &module.items[0] else {
        panic!("expected a theme, got {:?}", module.items[0]);
    };
    let ThemeRule::Selector { attrs, .. } = &body[0] else {
        panic!("expected a selector row, got {:?}", body[0]);
    };
    let keys: Vec<&str> = attrs.iter().map(|arg| arg.key.as_str()).collect();
    assert_eq!(keys, ["class", "level"]);
}

#[test]
fn a_malformed_edit_is_refused_where_it_goes_wrong() {
    for (source, expected) in [
        (
            "edit window[0] shape=arch\n",
            "1:16: expected `set` after the edit's selector or address, then the keys it sets: \
             `edit window[0] set key=value`",
        ),
        (
            "edit window[0] set\n",
            "1:19: an edit sets at least one `key=value` after `set`",
        ),
        (
            "edit window set shape=arch\n",
            "1:13: expected `[` or `@` after `window`: an edit selects by a selector, \
             `window[class=NAME]`, or by an address, `window@front[0]`",
        ),
        (
            "edit window@front set shape=arch\n",
            "1:19: an address step ends in its count among the `window` members on its wall, a \
             whole number in brackets: `window[0]`",
        ),
        (
            "edit level[0].window[class=a] set y=1\n",
            "1:21: an address step ends in its count among the `window` members on its wall, a \
             whole number in brackets: `window[0]`",
        ),
        (
            "edit window[4294967296] set y=1\n",
            "1:13: `window[4294967296]` counts past 4294967295, the most an address step counts",
        ),
    ] {
        assert_eq!(refusal(source), expected, "{source:?}");
    }
}

#[test]
fn a_misplaced_top_level_word_names_edit_among_the_lines() {
    assert_eq!(
        refusal("stuct s size=3x3\n"),
        "1:1: top-level line must be `theme`, `def`, `site`, `struct` or `edit`, got `stuct`",
    );
}
