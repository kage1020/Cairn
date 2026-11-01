//! Theme-binding data types and canonical / abstract token classification.

use indexmap::IndexMap;
use serde::Serialize;

use crate::ast::{Value, ValueKind};
use crate::error::Span;
use crate::intent::ValueWithSpan;

/// The kind of value a theme slot binds (`spec/materials-themes` "Canonical vocabulary").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TokenKind {
    /// Concrete canonical block token (`@oak_planks`, `@oak_log[axis=x]`).
    Canonical,
    /// Abstract material token (`@floor.wood.broadleaf`).
    Abstract,
    /// Not a `@TOKEN` value at all.
    NotAToken,
}

/// Classifies a token value: a `.` before any `[` makes it abstract, anything else canonical.
#[must_use]
pub fn classify_token(value: &Value) -> TokenKind {
    let ValueKind::Token(inner) = &value.kind else {
        return TokenKind::NotAToken;
    };
    if inner.is_empty() {
        return TokenKind::NotAToken;
    }
    let head = match inner.find('[') {
        Some(i) => &inner[..i],
        None => inner.as_str(),
    };
    if head.contains('.') {
        TokenKind::Abstract
    } else {
        TokenKind::Canonical
    }
}

/// Resolver-side view of one `theme` block.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThemeBinding {
    /// Theme name.
    pub name: String,
    /// `slot NAME -> VALUE` map, in source order.
    pub slots: IndexMap<String, ValueWithSpan>,
    /// One entry per `KEYWORD[attrs] -> bindings` row, in source order.
    pub selectors: Vec<SelectorMatch>,
    /// Byte range of the originating `theme NAME ...` block.
    #[serde(skip)]
    pub span: Span,
}

/// A selector rule paired with the spans of the members it matched.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SelectorMatch {
    /// Member keyword on the LHS (`window`, `walls`, ...).
    pub keyword: String,
    /// `[attr=value]` selector attributes in source order.
    pub attrs: IndexMap<String, ValueWithSpan>,
    /// `key=value` bindings on the RHS of the arrow.
    pub bindings: IndexMap<String, ValueWithSpan>,
    /// Byte ranges of every member this selector matched.
    #[serde(skip)]
    pub matched_member_spans: Vec<Span>,
    /// Byte range of the originating `KEYWORD[...] -> ...` row in source.
    #[serde(skip)]
    pub source_span: Span,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(inner: &str) -> Value {
        Value::new(ValueKind::Token(inner.to_owned()), 0..inner.len())
    }

    #[test]
    fn classify_token_recognises_canonical_block_id() {
        assert_eq!(classify_token(&token("oak_planks")), TokenKind::Canonical);
    }

    #[test]
    fn classify_token_recognises_abstract_material() {
        assert_eq!(
            classify_token(&token("floor.wood.broadleaf")),
            TokenKind::Abstract,
        );
    }

    #[test]
    fn classify_token_treats_bracketed_state_as_canonical() {
        assert_eq!(
            classify_token(&token("oak_log[axis=x]")),
            TokenKind::Canonical
        );
    }

    #[test]
    fn classify_token_rejects_non_token_values() {
        let v = Value::new(ValueKind::Ident("plain".to_owned()), 0..5);
        assert_eq!(classify_token(&v), TokenKind::NotAToken);
    }

    #[test]
    fn classify_token_rejects_empty_token() {
        // The parser never emits this shape.
        assert_eq!(classify_token(&token("")), TokenKind::NotAToken);
    }
}
