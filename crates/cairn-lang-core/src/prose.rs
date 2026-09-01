//! Sentence fragments shared by the diagnostic builders.
//!
//! Crate-level rather than inside `check` because two of the three callers
//! are in `resolve`. One copy per fragment: the arity branches are where a
//! joined list goes wrong, and the middle arity is the one nobody writes a
//! test for first — the unbranched `format!("{}, and {last}", head.join(", "))`
//! renders two items as `` `a`, and `b` ``, a serial comma with nothing to
//! serialise.

use indexmap::IndexMap;

use crate::ast::{Value, ValueKind};
use crate::intent::ValueWithSpan;

/// Render `items` as an English list — `a`, `a and b`, `a, b, and c`.
///
/// The serial comma is a three-or-more rule, so two items join bare.
/// `None` for an empty slice: every caller's sentence asserts that
/// something was listed, so there is no message to build from nothing, and
/// the empty string would put that claim in front of the user anyway.
pub(crate) fn and_list(items: &[String]) -> Option<String> {
    Some(match items.split_last()? {
        (last, []) => last.clone(),
        (last, [only]) => format!("{only} and {last}"),
        (last, head) => format!("{}, and {last}", head.join(", ")),
    })
}

/// Render a `KEYWORD[attrs]` selector the way the source spells it.
///
/// Attributes keep their source order, which is not the order the matcher
/// reads them in — the point is to hand back a string the author can find
/// on the line, not a normal form. Two rows this crate calls one selector
/// can therefore render differently, and that is wanted: the message names
/// this row and the note points at the other.
pub(crate) fn selector_text(keyword: &str, attrs: &IndexMap<String, ValueWithSpan>) -> String {
    let rendered: Vec<String> = attrs
        .iter()
        .map(|(key, value)| format!("{key}={}", value_text(&value.value)))
        .collect();
    format!("{keyword}[{}]", rendered.join(","))
}

/// Render a value the way the surface grammar spells it.
///
/// Faithful rather than pretty. `Str` keeps its quotes: dropping them
/// would make `class="small"` and `class=small` render identically, and
/// telling those two apart is what a selector-attribute comparison turns
/// on. Re-quoting is exact because the lexer's string literal runs to the
/// next `"` with no escape sequences, so the content cannot contain one.
fn value_text(value: &Value) -> String {
    match &value.kind {
        ValueKind::Ident(name) => name.clone(),
        ValueKind::Bool(flag) => flag.to_string(),
        ValueKind::Int(number) => number.to_string(),
        ValueKind::Size { w, h } => format!("{w}x{h}"),
        ValueKind::Token(token) => format!("@{token}"),
        ValueKind::DotRef(reference) => reference.to_string(),
        ValueKind::Str(text) => format!("\"{text}\""),
        ValueKind::List(items) => {
            let rendered: Vec<String> = items.iter().map(value_text).collect();
            format!("[{}]", rendered.join(","))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{and_list, selector_text, value_text};
    use crate::{lower, parse};

    fn of(items: &[&str]) -> Option<String> {
        let owned: Vec<String> = items.iter().map(|s| (*s).to_owned()).collect();
        and_list(&owned)
    }

    #[test]
    fn every_arity_reads() {
        assert_eq!(of(&[]), None);
        assert_eq!(of(&["a"]).as_deref(), Some("a"));
        assert_eq!(of(&["a", "b"]).as_deref(), Some("a and b"));
        assert_eq!(of(&["a", "b", "c"]).as_deref(), Some("a, b, and c"));
        assert_eq!(of(&["a", "b", "c", "d"]).as_deref(), Some("a, b, c, and d"));
    }

    /// Every `ValueKind`, through the parser so the rendered text is
    /// checked against the spelling that produced it rather than against a
    /// hand-built tree. Rendering is exhaustive on the enum, so a new
    /// variant stops this module compiling.
    #[test]
    fn every_value_kind_renders_as_written() {
        let written = "@cairn 2026.06\n\ntheme t:\n  \
             window[id=front,class=\"small\",y=2,sym=true,size=2x2,\
             mat_slot=@oak_planks,tags=[a,b],port=inside.front] -> frame=@spruce_wood\n";
        let module = parse(written).expect("parses");
        let ir = lower(&module);
        let rule = &ir.themes[0].selectors[0];
        let rendered: Vec<String> = rule
            .attrs
            .iter()
            .map(|(key, value)| format!("{key}={}", value_text(&value.value)))
            .collect();
        assert_eq!(
            rendered,
            [
                "id=front",
                "class=\"small\"",
                "y=2",
                "sym=true",
                "size=2x2",
                "mat_slot=@oak_planks",
                "tags=[a,b]",
                "port=inside.front",
            ],
        );
        assert_eq!(
            selector_text(&rule.keyword, &rule.attrs),
            "window[id=front,class=\"small\",y=2,sym=true,size=2x2,\
             mat_slot=@oak_planks,tags=[a,b],port=inside.front]",
        );
    }

    /// An attribute-less selector still renders its brackets: they are part
    /// of the grammar, and `window` alone is a parse error.
    #[test]
    fn an_empty_selector_keeps_its_brackets() {
        let module = parse("theme t:\n  window[] -> frame=@a\n").expect("parses");
        let ir = lower(&module);
        let rule = &ir.themes[0].selectors[0];
        assert_eq!(selector_text(&rule.keyword, &rule.attrs), "window[]");
    }
}
