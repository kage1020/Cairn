//! Translate a resolved theme-slot value into a [`BlockState`].
//!
//! The semantic resolver hands lowering a `ValueWithSpan` for every member's
//! `mat_slot=` (when both ends bound). This module turns that surface value
//! into the canonical Minecraft id + property bag the voxel grid stores. The
//! split is here, not inside the resolver, because the canonical/abstract
//! decision is what the block-array lowering pivots on: abstract tokens are
//! lifted through a registry-pack-backed [`AbstractMaterialResolver`]; when
//! no resolver is available, or the catalog does not declare the token, the
//! caller is told which mode the failure took so the resulting diagnostic
//! can be precise.

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::intent::ValueWithSpan;
use crate::resolve::{TokenKind, classify_token};
use crate::suggest::nearest_match;

use super::BlockState;

/// Lookup table for abstract material tokens.
///
/// The block-array lowering pass keeps no direct dependency on
/// `cairn-lang-formats` (where the on-disk registry pack lives). Instead the
/// pack-side `MaterialsIndex` implements this trait, and lowering takes it
/// as `Option<&dyn AbstractMaterialResolver>`: `None` means "no pack was
/// offered for this run" (LSP highlight, `cairn check` without a pack),
/// `Some` means "lift abstract tokens through this catalog, fail-loud on
/// misses".
///
/// Implementations must be cheap to call: lowering invokes [`Self::lookup`]
/// once per `mat_slot=` value, and [`Self::known_tokens`] only on the miss
/// path to feed `nearest_match`. Returning an unordered iterator is fine —
/// the suggestion pass is order-insensitive past the tie-break rule.
pub trait AbstractMaterialResolver {
    /// Lift `token` into a canonical [`BlockState`]. The token text is the
    /// inner body of the `@TOKEN` literal (no leading `@`). Implementations
    /// resolve the namespace and any state literal themselves so the trait
    /// shape stays format-agnostic.
    fn lookup(&self, token: &str) -> Option<BlockState>;

    /// Every token this resolver knows about. Used only when [`Self::lookup`]
    /// returns `None`, to feed the `nearest_match` suggestion. Allocating a
    /// fresh `Vec` per miss is intentional — misses are rare on a well-formed
    /// pack and `nearest_match` needs to consume the candidates anyway.
    fn known_tokens(&self) -> Vec<String>;
}

/// Default Minecraft id namespace for bare `@name` tokens. A future mod-aware
/// registry pack will let theme slots opt into other namespaces; the resolver
/// hardcodes vanilla so the cottage example lowers without registry data.
const VANILLA_NAMESPACE: &str = "minecraft";

/// Reason a slot value could not be lowered to a [`BlockState`].
///
/// The lowering pass turns each variant into a distinct diagnostic instead
/// of one catch-all so a downstream consumer (LSP quick-fix, future
/// registry-pack tooling) can tell "we know we need to upgrade this later"
/// (`Abstract`), "the pack rejected this token" (`UnknownAbstract`), or
/// "the resolver already complained about this" (`AlreadyDiagnosed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterialDeferred {
    /// The slot bound to an abstract token (`@floor.wood.broadleaf`) and no
    /// [`AbstractMaterialResolver`] was offered. Carries the inner token
    /// text so the caller can name it in the emitted warning.
    Abstract(String),
    /// The slot bound to an abstract token that the offered resolver does
    /// not declare. Carries the token plus the closest declared candidate
    /// (when one exists within `nearest_match`'s edit cap) so the diagnostic
    /// can suggest a fix instead of just listing valid options.
    UnknownAbstract {
        /// Inner token text (no leading `@`).
        token: String,
        /// Closest declared token, when one is within the suggestion cap.
        suggestion: Option<String>,
    },
    /// The slot value was not a `@TOKEN` shape at all. The resolver already
    /// emitted `E_UNKNOWN_SLOT_TARGET` for this case during `resolve()`, so
    /// lowering stays silent to avoid double-diagnosing the same span.
    AlreadyDiagnosed,
}

/// Convert a resolved slot value into a canonical [`BlockState`].
///
/// `materials` is the offered abstract-token resolver — usually backed by the
/// built-in registry pack. When `Some`, abstract tokens (`@floor.wood.broadleaf`)
/// are lifted through the catalog; misses become
/// [`MaterialDeferred::UnknownAbstract`] with a suggestion. When `None`,
/// abstract tokens stay deferred ([`MaterialDeferred::Abstract`]) so library
/// callers that never load a pack still see the same shape they did
/// pre-PR2.
///
/// Returns:
/// - `Ok(state)` for a canonical token (`@oak_planks`, `@oak_log[axis=x]`)
///   or an abstract token the resolver knew about. A bracketed state literal
///   on canonical tokens expands into [`BlockState::properties`].
/// - `Err(MaterialDeferred::Abstract)` for `@a.b.c` shapes when `materials`
///   is `None`.
/// - `Err(MaterialDeferred::UnknownAbstract)` when `materials` is `Some` but
///   the catalog does not declare the token. The `suggestion` field carries
///   the nearest declared token when one exists.
/// - `Err(MaterialDeferred::AlreadyDiagnosed)` for non-token values; the
///   `E_UNKNOWN_SLOT_TARGET` warning has already fired during resolve.
///
/// # Errors
///
/// Returns [`MaterialDeferred`] when the value cannot be lowered as
/// described above. The lowering pass converts each variant into a
/// `W_*` or `E_*` diagnostic.
pub fn resolve_block_state(
    slot: &ValueWithSpan,
    materials: Option<&dyn AbstractMaterialResolver>,
) -> Result<BlockState, MaterialDeferred> {
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
            let Some(resolver) = materials else {
                return Err(MaterialDeferred::Abstract(inner.clone()));
            };
            if let Some(state) = resolver.lookup(inner) {
                return Ok(state);
            }
            let pool = resolver.known_tokens();
            let suggestion =
                nearest_match(inner, pool.iter().map(String::as_str)).map(str::to_owned);
            Err(MaterialDeferred::UnknownAbstract {
                token: inner.clone(),
                suggestion,
            })
        }
        TokenKind::NotAToken => Err(MaterialDeferred::AlreadyDiagnosed),
    }
}

/// Turn a canonical token body (`oak_planks`, `oak_log[axis=x]`,
/// `minecraft:cobblestone`) into a [`BlockState`].
///
/// Recognises an optional `namespace:` prefix and a trailing `[k=v,...]`
/// state literal. The state literal is parsed defensively because the
/// surface parser does not currently mint bracketed tokens directly — but
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

    /// In-memory resolver used by the material tests. Real callers go
    /// through `cairn-lang-formats::registry::MaterialsIndex`; the inline
    /// fake here keeps this test file free of a circular crate dependency.
    struct FakeResolver {
        entries: Vec<(&'static str, &'static str)>,
    }

    impl AbstractMaterialResolver for FakeResolver {
        fn lookup(&self, token: &str) -> Option<BlockState> {
            self.entries
                .iter()
                .find(|(t, _)| *t == token)
                .map(|(_, id)| BlockState::bare(format!("minecraft:{id}")))
        }

        fn known_tokens(&self) -> Vec<String> {
            self.entries.iter().map(|(t, _)| (*t).to_owned()).collect()
        }
    }

    #[test]
    fn canonical_bare_token_becomes_minecraft_namespaced() {
        let bs = resolve_block_state(&token("cobblestone"), None).unwrap();
        assert_eq!(bs.id, "minecraft:cobblestone");
        assert!(bs.properties.is_empty());
    }

    #[test]
    fn canonical_state_literal_populates_properties() {
        let bs = resolve_block_state(&token("oak_log[axis=x]"), None).unwrap();
        assert_eq!(bs.id, "minecraft:oak_log");
        assert_eq!(bs.properties.get("axis").map(String::as_str), Some("x"));
    }

    #[test]
    fn canonical_state_literal_with_multiple_pairs() {
        let bs = resolve_block_state(&token("stairs[facing=north,half=top]"), None).unwrap();
        assert_eq!(bs.id, "minecraft:stairs");
        assert_eq!(bs.properties.len(), 2);
        assert_eq!(bs.properties.get("facing").unwrap(), "north");
        assert_eq!(bs.properties.get("half").unwrap(), "top");
    }

    #[test]
    fn canonical_explicit_namespace_is_preserved() {
        let bs = resolve_block_state(&token("create:cogwheel"), None).unwrap();
        assert_eq!(bs.id, "create:cogwheel");
    }

    #[test]
    fn abstract_token_without_resolver_defers() {
        let err = resolve_block_state(&token("floor.wood.broadleaf"), None).unwrap_err();
        assert_eq!(
            err,
            MaterialDeferred::Abstract("floor.wood.broadleaf".into())
        );
    }

    #[test]
    fn abstract_token_lifts_through_resolver() {
        let resolver = FakeResolver {
            entries: vec![
                ("floor.wood.broadleaf", "oak_planks"),
                ("wall.stone.cobble", "cobblestone"),
            ],
        };
        let bs = resolve_block_state(&token("floor.wood.broadleaf"), Some(&resolver)).unwrap();
        assert_eq!(bs.id, "minecraft:oak_planks");
    }

    #[test]
    fn abstract_token_unknown_returns_suggestion() {
        let resolver = FakeResolver {
            entries: vec![
                ("floor.wood.broadleaf", "oak_planks"),
                ("floor.wood.conifer", "spruce_planks"),
            ],
        };
        let err = resolve_block_state(&token("floor.wood.broadlef"), Some(&resolver)).unwrap_err();
        assert_eq!(
            err,
            MaterialDeferred::UnknownAbstract {
                token: "floor.wood.broadlef".into(),
                suggestion: Some("floor.wood.broadleaf".into()),
            },
        );
    }

    #[test]
    fn abstract_token_unknown_without_close_candidate_has_no_suggestion() {
        let resolver = FakeResolver {
            entries: vec![("floor.wood.broadleaf", "oak_planks")],
        };
        let err =
            resolve_block_state(&token("totally.different.tree"), Some(&resolver)).unwrap_err();
        match err {
            MaterialDeferred::UnknownAbstract { suggestion, .. } => assert!(suggestion.is_none()),
            other => panic!("expected UnknownAbstract, got {other:?}"),
        }
    }

    #[test]
    fn non_token_value_is_already_diagnosed() {
        let err = resolve_block_state(&ident("plain"), None).unwrap_err();
        assert_eq!(err, MaterialDeferred::AlreadyDiagnosed);
    }
}
