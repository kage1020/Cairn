//! Sentence fragments shared by the diagnostic builders in `check` and `resolve`.
//!
//! One copy of each, because a joined list goes wrong in its arity branches: the unbranched
//! `format!("{}, and {last}", head.join(", "))` renders two items as `` `a`, and `b` ``.

use indexmap::IndexMap;

use crate::ast::{Value, ValueKind};
use crate::intent::ValueWithSpan;

/// Render `items` as an English list — `a`, `a and b`, `a, b, and c`.
///
/// `None` for an empty slice, because every caller's sentence claims that something was listed.
pub(crate) fn and_list(items: &[String]) -> Option<String> {
    join_list(items, "and")
}

/// Render `items` as an English alternation — `a`, `a or b`, `a, b, or c`.
pub(crate) fn or_list(items: &[String]) -> Option<String> {
    join_list(items, "or")
}

fn join_list(items: &[String], conjunction: &str) -> Option<String> {
    Some(match items.split_last()? {
        (last, []) => last.clone(),
        (last, [only]) => format!("{only} {conjunction} {last}"),
        (last, head) => format!("{}, {conjunction} {last}", head.join(", ")),
    })
}

/// Render a `KEYWORD[attrs]` selector the way the source spells it.
///
/// Attributes keep their source order, not a normal form, so the author can find the string on
/// the line. Two rows that are one selector can therefore render differently, which is wanted:
/// the message names this row and the note points at the other.
pub(crate) fn selector_text(keyword: &str, attrs: &IndexMap<String, ValueWithSpan>) -> String {
    let rendered: Vec<String> = attrs
        .iter()
        .map(|(key, value)| format!("{key}={}", value_text(&value.value)))
        .collect();
    format!("{keyword}[{}]", rendered.join(","))
}

/// Render a value the way the surface grammar spells it.
///
/// `Str` keeps its quotes, because `class="small"` and `class=small` are different selector
/// attributes. Re-quoting is exact because a string literal has no escapes, so its content holds
/// no `"`.
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
    use super::{and_list, or_list, selector_text, value_text};
    use crate::{lower, parse};

    fn of(items: &[&str]) -> Option<String> {
        let owned: Vec<String> = items.iter().map(|s| (*s).to_owned()).collect();
        and_list(&owned)
    }

    fn either_of(items: &[&str]) -> Option<String> {
        let owned: Vec<String> = items.iter().map(|s| (*s).to_owned()).collect();
        or_list(&owned)
    }

    #[test]
    fn every_arity_reads() {
        assert_eq!(of(&[]), None);
        assert_eq!(of(&["a"]).as_deref(), Some("a"));
        assert_eq!(of(&["a", "b"]).as_deref(), Some("a and b"));
        assert_eq!(of(&["a", "b", "c"]).as_deref(), Some("a, b, and c"));
        assert_eq!(of(&["a", "b", "c", "d"]).as_deref(), Some("a, b, c, and d"));
    }

    #[test]
    fn every_arity_reads_with_the_other_conjunction() {
        assert_eq!(either_of(&[]), None);
        assert_eq!(either_of(&["a"]).as_deref(), Some("a"));
        assert_eq!(either_of(&["a", "b"]).as_deref(), Some("a or b"));
        assert_eq!(either_of(&["a", "b", "c"]).as_deref(), Some("a, b, or c"));
    }

    /// Goes through the parser, so each rendering is checked against the spelling that produced
    /// it.
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

    /// `window` without brackets is a parse error, so the brackets are part of the spelling.
    #[test]
    fn an_empty_selector_keeps_its_brackets() {
        let module = parse("theme t:\n  window[] -> frame=@a\n").expect("parses");
        let ir = lower(&module);
        let rule = &ir.themes[0].selectors[0];
        assert_eq!(selector_text(&rule.keyword, &rule.attrs), "window[]");
    }
}
