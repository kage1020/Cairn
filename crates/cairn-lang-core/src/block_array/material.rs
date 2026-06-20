//! Translate a resolved theme-slot value into a [`BlockState`].
//!
//! The semantic resolver hands lowering a `ValueWithSpan` for every member's
//! `mat_slot=` (when both ends bound). This module turns that surface value
//! into the canonical Minecraft id + property bag the voxel grid stores. The
//! split is here, not inside the resolver, because the canonical/abstract
//! decision is what the block-array lowering pivots on: abstract tokens
//! cannot be lowered without the registry pack, so they degrade explicitly
//! rather than silently substituting.

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::intent::ValueWithSpan;
use crate::resolve::{TokenKind, classify_token};

use super::BlockState;

/// Default Minecraft id namespace for bare `@name` tokens. A future mod-aware
/// registry pack will let theme slots opt into other namespaces; M2 hardcodes
/// vanilla so the cottage example lowers without registry data.
const VANILLA_NAMESPACE: &str = "minecraft";

/// Reason a slot value could not be lowered to a [`BlockState`].
///
/// The lowering pass turns each variant into a distinct diagnostic instead
/// of one catch-all so a downstream consumer (LSP quick-fix, future
/// registry-pack tooling) can tell "we know we need to upgrade this later"
/// (`Abstract`) from "the resolver already complained about this"
/// (`AlreadyDiagnosed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterialDeferred {
    /// The slot bound to an abstract token (`@floor.wood.broadleaf`).
    /// Carries the inner token text so the lowering can name it in the
    /// emitted warning.
    Abstract(String),
    /// The slot value was not a `@TOKEN` shape at all. The resolver already
    /// emitted `E_UNKNOWN_SLOT_TARGET` for this case during `resolve()`, so
    /// lowering stays silent to avoid double-diagnosing the same span.
    AlreadyDiagnosed,
}

/// Convert a resolved slot value into a canonical [`BlockState`].
///
/// Returns:
/// - `Ok(state)` for a canonical token (`@oak_planks`, `@oak_log[axis=x]`).
///   A bracketed state literal expands into [`BlockState::properties`].
/// - `Err(MaterialDeferred::Abstract)` for `@a.b.c` shapes — the registry
///   pack (later this milestone) translates these into canonical ids.
/// - `Err(MaterialDeferred::AlreadyDiagnosed)` for non-token values; the
///   `E_UNKNOWN_SLOT_TARGET` warning has already fired during resolve.
///
/// # Errors
///
/// Returns [`MaterialDeferred`] when the value cannot be lowered as
/// described above. The lowering pass converts each variant into a
/// `W_*` diagnostic and falls the cell back to air.
pub fn resolve_block_state(slot: &ValueWithSpan) -> Result<BlockState, MaterialDeferred> {
    match classify_token(&slot.value) {
        TokenKind::Canonical => {
            let ValueKind::Token(inner) = &slot.value.kind else {
                unreachable!("classify_token reports Canonical only for ValueKind::Token");
            };
            Ok(canonical_to_block_state(inner))
        }
        TokenKind::Abstract => {
            let ValueKind::Token(inner) = &slot.value.kind else {
                unreachable!("classify_token reports Abstract only for ValueKind::Token");
            };
            Err(MaterialDeferred::Abstract(inner.clone()))
        }
        TokenKind::NotAToken => Err(MaterialDeferred::AlreadyDiagnosed),
    }
}

/// Turn a canonical token body (`oak_planks`, `oak_log[axis=x]`,
/// `minecraft:cobblestone`) into a [`BlockState`].
///
/// Recognises an optional `namespace:` prefix and a trailing `[k=v,...]`
/// state literal. The state literal is parsed defensively because the M2
/// parser does not currently mint bracketed tokens directly — but
/// [`crate::resolve::classify_token`] documents the shape and other code
/// paths (registry-pack lookups, future schematic ingestion) may.
fn canonical_to_block_state(inner: &str) -> BlockState {
    let (head, properties_src) = match inner.find('[') {
        Some(i) => {
            let head = &inner[..i];
            let tail = &inner[i + 1..];
            // Trim the matching `]` so a well-formed `oak_log[axis=x]`
            // parses cleanly; an unterminated literal silently falls
            // through with whatever the tail contained (still better than
            // erroring at this layer — the surface lexer is the right
            // place to reject malformed brackets).
            let trimmed = tail.strip_suffix(']').unwrap_or(tail);
            (head, trimmed)
        }
        None => (inner, ""),
    };
    let id = if head.contains(':') {
        head.to_owned()
    } else {
        format!("{VANILLA_NAMESPACE}:{head}")
    };
    let properties = parse_state_literal(properties_src);
    BlockState { id, properties }
}

/// Parse a comma-separated `k=v,k=v` body into an ordered property map.
///
/// Whitespace around keys and values is trimmed; empty segments (from a
/// trailing comma or a stray `,,`) are skipped silently. The block-array IR
/// is below the lint layer, so noisy parsing here would surface as
/// diagnostics in the wrong place — the resolver-side
/// `E_UNKNOWN_SLOT_TARGET` is the right gate for badly-shaped values.
fn parse_state_literal(body: &str) -> IndexMap<String, String> {
    let mut out = IndexMap::new();
    if body.is_empty() {
        return out;
    }
    for pair in body.split(',') {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        if let Some((k, v)) = pair.split_once('=') {
            out.insert(k.trim().to_owned(), v.trim().to_owned());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Value;

    fn token(inner: &str) -> ValueWithSpan {
        let span = 0..inner.len() + 1; // account for the leading `@`
        ValueWithSpan::from_value(Value::new(ValueKind::Token(inner.to_owned()), span))
    }

    fn ident(name: &str) -> ValueWithSpan {
        let span = 0..name.len();
        ValueWithSpan::from_value(Value::new(ValueKind::Ident(name.to_owned()), span))
    }

    #[test]
    fn canonical_bare_token_becomes_minecraft_namespaced() {
        let bs = resolve_block_state(&token("cobblestone")).unwrap();
        assert_eq!(bs.id, "minecraft:cobblestone");
        assert!(bs.properties.is_empty());
    }

    #[test]
    fn canonical_state_literal_populates_properties() {
        let bs = resolve_block_state(&token("oak_log[axis=x]")).unwrap();
        assert_eq!(bs.id, "minecraft:oak_log");
        assert_eq!(bs.properties.get("axis").map(String::as_str), Some("x"));
    }

    #[test]
    fn canonical_state_literal_with_multiple_pairs() {
        let bs = resolve_block_state(&token("stairs[facing=north,half=top]")).unwrap();
        assert_eq!(bs.id, "minecraft:stairs");
        assert_eq!(bs.properties.len(), 2);
        assert_eq!(bs.properties.get("facing").unwrap(), "north");
        assert_eq!(bs.properties.get("half").unwrap(), "top");
    }

    #[test]
    fn canonical_explicit_namespace_is_preserved() {
        let bs = resolve_block_state(&token("create:cogwheel")).unwrap();
        assert_eq!(bs.id, "create:cogwheel");
    }

    #[test]
    fn abstract_token_defers() {
        let err = resolve_block_state(&token("floor.wood.broadleaf")).unwrap_err();
        assert_eq!(
            err,
            MaterialDeferred::Abstract("floor.wood.broadleaf".into())
        );
    }

    #[test]
    fn non_token_value_is_already_diagnosed() {
        let err = resolve_block_state(&ident("plain")).unwrap_err();
        assert_eq!(err, MaterialDeferred::AlreadyDiagnosed);
    }
}
