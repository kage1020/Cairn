//! Theme-binding data types and canonical/abstract token classification.
//!
//! [`ThemeBinding`] is the resolver's view of one `theme` block: it owns the
//! same `slot NAME -> VALUE` map the IR carries, plus a [`SelectorMatch`]
//! per selector rule that tracks which members the selector actually bound
//! to. The "matched member spans" list is what the `unmatched-selector` pass
//! consumes to emit `E_THEME_SELECTOR_UNMATCHED`.

use indexmap::IndexMap;
use serde::Serialize;

use crate::ast::{Value, ValueKind};
use crate::error::Span;
use crate::intent::ValueWithSpan;

/// Classification of a `@TOKEN` value used as a theme slot target.
///
/// Drawn from `spec/materials-themes.md` §7.2: a canonical token names a
/// concrete Minecraft meaning (`@oak_planks`, `@oak_log[axis=x]`), an
/// abstract token names an aesthetic choice that the theme policy may
/// downgrade (`@floor.wood.broadleaf`). Anything else — a bare identifier,
/// a string, a list — is `NotAToken` and surfaces as
/// `E_UNKNOWN_SLOT_TARGET`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TokenKind {
    /// Concrete canonical block token (`@oak_planks`, `@oak_log[axis=x]`).
    Canonical,
    /// Abstract material token (`@floor.wood.broadleaf`).
    Abstract,
    /// Not a `@TOKEN` value at all.
    NotAToken,
}

/// Classify the value behind a `slot NAME -> VALUE` binding (or any other
/// `@`-prefixed token reference).
///
/// Rule: a token whose inner string contains `[` is canonical regardless of
/// whether dots appear inside the brackets (state literals like
/// `@oak_log[axis=x]` are canonical). A token whose inner string contains a
/// `.` *outside* any bracket is abstract (`@floor.wood.broadleaf`).
/// Everything else (`@oak_planks`) is canonical.
#[must_use]
pub fn classify_token(value: &Value) -> TokenKind {
    let ValueKind::Token(inner) = &value.kind else {
        return TokenKind::NotAToken;
    };
    if inner.is_empty() {
        return TokenKind::NotAToken;
    }
    // Slice off any state-literal tail so `@oak_log[axis=x]` is judged on
    // `oak_log` alone — bracketed state is canonical metadata, never an
    // abstract-token marker.
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
///
/// Owns its slot map and selector rules verbatim from the IR — the resolver
/// only annotates each selector with the spans of the members it matched, so
/// downstream passes can locate unmatched selectors without re-walking the
/// module.
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
    /// Byte ranges of every member this selector matched. Empty after
    /// `resolve()` means the selector never bound — `E_THEME_SELECTOR_UNMATCHED`.
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
        // `oak_log[axis=x]` has a dot inside the bracket (`axis=x` carries no
        // dot here, but the rule must hold even when state values contain
        // dots) — the head before `[` is what decides.
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
        // Defensive: an empty Token cannot be a meaningful slot target. The
        // parser does not currently emit this shape, but the classifier owns
        // the policy.
        assert_eq!(classify_token(&token("")), TokenKind::NotAToken);
    }
}
