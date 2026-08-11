//! Translate a resolved theme-slot value into a [`BlockState`].
//!
//! The semantic resolver hands lowering a `ValueWithSpan` for every member's
//! `mat_slot=` (when both ends bound). This module turns that surface value
//! into the canonical Minecraft id + property bag the voxel grid stores. The
//! split is here, not inside the resolver, because the canonical/abstract
//! decision is what the block-array lowering pivots on: abstract tokens are
//! lifted through a registry-pack-backed [`TargetRegistry`]; when no
//! registry is available, or the catalog does not declare the token, the
//! caller is told which mode the failure took so the resulting diagnostic
//! can be precise.
//!
//! The last step is the same either way: whatever id the value resolved to
//! is checked against the target's block-id table. That is the only place
//! that catches a *canonical* token nobody declared — `@totally_not_a_block`
//! has no dot, so it never reaches the materials catalog at all.

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::intent::ValueWithSpan;
use crate::resolve::{TokenKind, classify_token};
use crate::suggest::nearest_match;

use super::BlockState;

/// What the registry pack tells lowering, for one compile.
///
/// The block-array lowering pass keeps no direct dependency on
/// `cairn-lang-formats` (where the on-disk registry pack lives). Instead the
/// pack side implements this trait, and lowering takes it as
/// `Option<&dyn TargetRegistry>`: `None` means "no pack was offered for this
/// run" (LSP highlight, `cairn check` without a pack), `Some` means "lift
/// abstract tokens through this catalog, fail-loud on misses".
///
/// Implementations must be cheap to call: lowering invokes [`Self::lookup`]
/// once per `mat_slot=` value, and [`Self::known_tokens`] only on the miss
/// path to feed `nearest_match`. Returning an unordered iterator is fine —
/// the suggestion pass is order-insensitive past the tie-break rule.
pub trait TargetRegistry {
    /// Lift `token` into a canonical [`BlockState`]. The token text is the
    /// inner body of the `@TOKEN` literal (no leading `@`). Implementations
    /// resolve the namespace and any state literal themselves so the trait
    /// shape stays format-agnostic.
    fn lookup(&self, token: &str) -> Option<BlockState>;

    /// Every token this registry knows about. Used only when [`Self::lookup`]
    /// returns `None`, to feed the `nearest_match` suggestion. Allocating a
    /// fresh `Vec` per miss is intentional — misses are rare on a well-formed
    /// pack and `nearest_match` needs to consume the candidates anyway.
    fn known_tokens(&self) -> Vec<String>;

    /// The block ids valid in the target this compile is pinned to.
    ///
    /// `None` means no id table applies — either the pack ships no `blocks`
    /// component, or the run has no `--target` to pin one (`cairn check`,
    /// `cairn info`, `cairn lower`). Id validation is then skipped, because
    /// the honest answer to "does this id exist" is "in which version?" and
    /// picking one on the caller's behalf would refuse ids that are fine on
    /// the version they actually compile against.
    fn block_ids(&self) -> Option<BlockIdSet<'_>>;
}

/// The block ids valid in one pinned `(edition, version)` target.
///
/// A borrowed, sorted slice rather than a set type so the pack can hand out
/// the table it already folded at load time without rebuilding an index per
/// lookup, and so `cairn-lang-core` needs no hashing dependency for it.
#[derive(Debug, Clone, Copy)]
pub struct BlockIdSet<'a> {
    /// How the target reads in a diagnostic — `"bedrock 1.21.60"`.
    label: &'a str,
    /// Fully namespaced ids in ascending order.
    ids: &'a [String],
}

impl<'a> BlockIdSet<'a> {
    /// Wrap a sorted, fully namespaced id list.
    ///
    /// # Panics
    ///
    /// Panics in debug builds when `ids` is not sorted ascending — the
    /// binary search below would otherwise miss real ids for reasons no
    /// caller could see from the outside.
    #[must_use]
    pub fn new(label: &'a str, ids: &'a [String]) -> Self {
        debug_assert!(
            ids.is_sorted(),
            "BlockIdSet expects a sorted id list; `{label}` is not sorted",
        );
        Self { label, ids }
    }

    /// How the target reads in a diagnostic.
    #[must_use]
    pub fn label(&self) -> &'a str {
        self.label
    }

    /// Whether the target declares `id`.
    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.ids
            .binary_search_by(|known| known.as_str().cmp(id))
            .is_ok()
    }

    /// Every declared id, ascending.
    pub fn iter(&self) -> impl Iterator<Item = &'a str> {
        self.ids.iter().map(String::as_str)
    }

    /// Number of declared ids.
    #[must_use]
    pub fn len(&self) -> usize {
        self.ids.len()
    }

    /// `true` when the target declares no ids at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }
}

/// Default Minecraft id namespace for bare `@name` tokens. A future mod-aware
/// registry pack will let theme slots opt into other namespaces; this module
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
    /// [`TargetRegistry`] was offered. Carries the inner token
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
    /// The value resolved to a block id the pinned target does not declare.
    /// Spec versioning-editions §10.4 makes this a hard error: writing the
    /// id anyway produces a structure file the game loads as air, with no
    /// diagnostic to explain the hole.
    UnknownId(UnknownId),
}

/// An id that resolved cleanly but does not exist in the pinned target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownId {
    /// The fully namespaced id that was refused.
    pub id: String,
    /// How the target reads in the message — `"bedrock 1.21.60"`.
    pub registry: String,
    /// Whether the author named the id or the pack's catalog did.
    pub origin: IdOrigin,
    /// Closest id the target does declare, when one is within
    /// `nearest_match`'s edit cap.
    pub suggestion: Option<String>,
}

/// Who chose the id that turned out not to exist.
///
/// The two cases need different prose and different fixes: an author can
/// correct what they typed, but a pack mapping is not theirs to edit, and a
/// message that blames their token for it sends them looking in the wrong
/// file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdOrigin {
    /// The author wrote the id, as a canonical `@minecraft:foo` /`@foo`
    /// token.
    Authored,
    /// The pack's materials catalog mapped an abstract token onto it.
    Catalog {
        /// The abstract token the author actually wrote, without the `@`.
        token: String,
    },
}

/// Convert a resolved slot value into a canonical [`BlockState`].
///
/// `registry` is the offered registry view — usually backed by the built-in
/// registry pack. When `Some`, abstract tokens (`@floor.wood.broadleaf`) are
/// lifted through its catalog and every resolved id is checked against the
/// pinned target's id table; misses become
/// [`MaterialDeferred::UnknownAbstract`] / [`MaterialDeferred::UnknownId`]
/// with a suggestion. When `None`, abstract tokens stay deferred
/// ([`MaterialDeferred::Abstract`]) so library callers that never load a
/// pack still see the same shape they did pre-PR2.
///
/// Returns:
/// - `Ok(state)` for a canonical token (`@oak_planks`, `@oak_log[axis=x]`)
///   or an abstract token the registry knew about, whose id the target
///   declares. A bracketed state literal on canonical tokens expands into
///   [`BlockState::properties`].
/// - `Err(MaterialDeferred::Abstract)` for `@a.b.c` shapes when `registry`
///   is `None`.
/// - `Err(MaterialDeferred::UnknownAbstract)` when `registry` is `Some` but
///   the catalog does not declare the token. The `suggestion` field carries
///   the nearest declared token when one exists.
/// - `Err(MaterialDeferred::UnknownId)` when the value resolved to an id the
///   pinned target does not declare — including one the catalog itself
///   produced, which is a pack bug rather than an authoring one.
/// - `Err(MaterialDeferred::AlreadyDiagnosed)` for non-token values; the
///   `E_UNKNOWN_SLOT_TARGET` error has already fired during resolve.
///
/// # Errors
///
/// Returns [`MaterialDeferred`] when the value cannot be lowered as
/// described above. The lowering pass converts each variant into a
/// `W_*` or `E_*` diagnostic.
pub fn resolve_block_state(
    slot: &ValueWithSpan,
    registry: Option<&dyn TargetRegistry>,
) -> Result<BlockState, MaterialDeferred> {
    match classify_token(&slot.value) {
        TokenKind::Canonical => {
            let ValueKind::Token(inner) = &slot.value.kind else {
                unreachable!("classify_token reports Canonical only for ValueKind::Token");
            };
            let state = canonical_to_block_state(inner);
            check_id(state, registry, &IdOrigin::Authored)
        }
        TokenKind::Abstract => {
            let ValueKind::Token(inner) = &slot.value.kind else {
                unreachable!("classify_token reports Abstract only for ValueKind::Token");
            };
            let Some(registry) = registry else {
                return Err(MaterialDeferred::Abstract(inner.clone()));
            };
            if let Some(state) = registry.lookup(inner) {
                return check_id(
                    state,
                    Some(registry),
                    &IdOrigin::Catalog {
                        token: inner.clone(),
                    },
                );
            }
            let pool = registry.known_tokens();
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

/// Refuse `state` when the pinned target does not declare its id.
///
/// Passes the state through untouched when no target is pinned — that is
/// the documented "cannot refute" mode of [`TargetRegistry::block_ids`],
/// not an acceptance.
fn check_id(
    state: BlockState,
    registry: Option<&dyn TargetRegistry>,
    origin: &IdOrigin,
) -> Result<BlockState, MaterialDeferred> {
    let Some(ids) = registry.and_then(TargetRegistry::block_ids) else {
        return Ok(state);
    };
    if ids.contains(&state.id) {
        return Ok(state);
    }
    Err(MaterialDeferred::UnknownId(UnknownId {
        suggestion: nearest_id(&state.id, ids),
        registry: ids.label().to_owned(),
        origin: origin.clone(),
        id: state.id,
    }))
}

/// The declared id closest to `id`, compared on the path alone.
///
/// `nearest_match`'s edit cap scales with input length, and every vanilla
/// id shares the ten-character `minecraft:` prefix — long enough to buy a
/// third edit that the meaningful part never earned. Over a table of a
/// thousand ids that is the difference between suggesting `oak_planks` for
/// `oak_plank` and suggesting `dirt` for `light`. Comparing paths within
/// one namespace puts the cap back on the part the author actually typed.
fn nearest_id(id: &str, ids: BlockIdSet<'_>) -> Option<String> {
    let (namespace, path) = id.split_once(':')?;
    let candidates = ids
        .iter()
        .filter_map(|known| known.split_once(':'))
        .filter(|(ns, _)| *ns == namespace)
        .map(|(_, path)| path);
    nearest_match(path, candidates).map(|best| format!("{namespace}:{best}"))
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

    /// In-memory registry used by the material tests. Real callers go
    /// through `cairn-lang-formats::registry::RegistryPack::view`; the
    /// inline fake here keeps this test file free of a circular crate
    /// dependency.
    struct FakeResolver {
        entries: Vec<(&'static str, &'static str)>,
        /// Sorted, fully namespaced ids the pinned target declares. Empty
        /// means "no target pinned", matching a `blocks`-less pack.
        ids: Vec<String>,
    }

    impl FakeResolver {
        fn new(entries: Vec<(&'static str, &'static str)>) -> Self {
            Self {
                entries,
                ids: Vec::new(),
            }
        }

        /// Pin a target declaring exactly `ids`.
        fn pinned(mut self, ids: &[&str]) -> Self {
            self.ids = ids.iter().map(|id| (*id).to_owned()).collect();
            self.ids.sort();
            self
        }
    }

    impl TargetRegistry for FakeResolver {
        fn lookup(&self, token: &str) -> Option<BlockState> {
            self.entries
                .iter()
                .find(|(t, _)| *t == token)
                .map(|(_, id)| BlockState::bare(format!("minecraft:{id}")))
        }

        fn known_tokens(&self) -> Vec<String> {
            self.entries.iter().map(|(t, _)| (*t).to_owned()).collect()
        }

        fn block_ids(&self) -> Option<BlockIdSet<'_>> {
            (!self.ids.is_empty()).then(|| BlockIdSet::new("test 1.0", &self.ids))
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
        let resolver = FakeResolver::new(vec![
            ("floor.wood.broadleaf", "oak_planks"),
            ("wall.stone.cobble", "cobblestone"),
        ]);
        let bs = resolve_block_state(&token("floor.wood.broadleaf"), Some(&resolver)).unwrap();
        assert_eq!(bs.id, "minecraft:oak_planks");
    }

    #[test]
    fn abstract_token_unknown_returns_suggestion() {
        let resolver = FakeResolver::new(vec![
            ("floor.wood.broadleaf", "oak_planks"),
            ("floor.wood.conifer", "spruce_planks"),
        ]);
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
        let resolver = FakeResolver::new(vec![("floor.wood.broadleaf", "oak_planks")]);
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

    #[test]
    fn an_authored_id_the_target_lacks_is_refused_with_its_registry_named() {
        let registry = FakeResolver::new(vec![]).pinned(&["minecraft:oak_planks"]);
        let err = resolve_block_state(&token("oak_plank"), Some(&registry)).unwrap_err();
        assert_eq!(
            err,
            MaterialDeferred::UnknownId(UnknownId {
                id: "minecraft:oak_plank".into(),
                registry: "test 1.0".into(),
                origin: IdOrigin::Authored,
                suggestion: Some("minecraft:oak_planks".into()),
            }),
        );
    }

    #[test]
    fn a_rename_is_refused_without_a_suggestion_because_it_is_not_a_typo() {
        // Bedrock spells the Java `light` block `light_block`, six edits
        // away — past `nearest_match`'s cap. The suggestion is a typo
        // finder, not a rename map, and claiming otherwise here would need
        // per-edition id aliases the pack does not carry.
        let registry = FakeResolver::new(vec![]).pinned(&["minecraft:light_block"]);
        let err = resolve_block_state(&token("light"), Some(&registry)).unwrap_err();
        match err {
            MaterialDeferred::UnknownId(unknown) => {
                assert_eq!(unknown.id, "minecraft:light");
                assert!(unknown.suggestion.is_none());
            }
            other => panic!("expected UnknownId, got {other:?}"),
        }
    }

    /// A suggestion is only useful if it is an id the target actually
    /// declares, and an id is namespace plus path.
    ///
    /// Searching paths across namespaces finds `cogwheels` one edit from
    /// `cogwheel` and hands back `minecraft:cogwheels` — a string that
    /// exists in no registry anywhere, offered as the fix for an id that
    /// does not exist either. Both built-in packs are entirely
    /// `minecraft:`, so only a modded-shaped table reaches this.
    #[test]
    fn a_suggestion_never_borrows_a_path_from_another_namespace() {
        let registry = FakeResolver::new(vec![]).pinned(&["create:cogwheels"]);
        let err = resolve_block_state(&token("cogwheel"), Some(&registry)).unwrap_err();
        match err {
            MaterialDeferred::UnknownId(unknown) => {
                assert_eq!(unknown.id, "minecraft:cogwheel");
                assert_eq!(
                    unknown.suggestion, None,
                    "`create:cogwheels` is not a candidate for a `minecraft:` id, and \
                     `minecraft:cogwheels` is not an id at all",
                );
            }
            other => panic!("expected UnknownId, got {other:?}"),
        }
    }

    /// The same table does suggest within its own namespace, so the test
    /// above is not passing because the search found nothing to look at.
    #[test]
    fn a_suggestion_is_still_found_inside_the_namespace_that_has_one() {
        let registry = FakeResolver::new(vec![]).pinned(&["create:cogwheels"]);
        let err = resolve_block_state(&token("create:cogwheel"), Some(&registry)).unwrap_err();
        match err {
            MaterialDeferred::UnknownId(unknown) => {
                assert_eq!(unknown.suggestion.as_deref(), Some("create:cogwheels"));
            }
            other => panic!("expected UnknownId, got {other:?}"),
        }
    }

    #[test]
    fn an_authored_id_the_target_declares_passes() {
        let registry = FakeResolver::new(vec![]).pinned(&["minecraft:light"]);
        let bs = resolve_block_state(&token("light"), Some(&registry)).unwrap();
        assert_eq!(bs.id, "minecraft:light");
    }

    #[test]
    fn a_state_literal_is_checked_on_the_id_alone() {
        // `oak_log[axis=x]` is one id plus a property bag; the id table
        // keys on the id, so the brackets must not defeat the lookup.
        let registry = FakeResolver::new(vec![]).pinned(&["minecraft:oak_log"]);
        let bs = resolve_block_state(&token("oak_log[axis=x]"), Some(&registry)).unwrap();
        assert_eq!(bs.properties.get("axis").map(String::as_str), Some("x"));
    }

    #[test]
    fn a_catalog_mapping_onto_a_missing_id_names_the_token_that_reached_it() {
        let registry = FakeResolver::new(vec![("floor.stone.smooth", "stone_bricks")])
            .pinned(&["minecraft:stonebrick"]);
        let err = resolve_block_state(&token("floor.stone.smooth"), Some(&registry)).unwrap_err();
        assert_eq!(
            err,
            MaterialDeferred::UnknownId(UnknownId {
                id: "minecraft:stone_bricks".into(),
                registry: "test 1.0".into(),
                origin: IdOrigin::Catalog {
                    token: "floor.stone.smooth".into(),
                },
                suggestion: Some("minecraft:stonebrick".into()),
            }),
        );
    }

    #[test]
    fn an_unpinned_registry_refutes_nothing() {
        // No target means no id table; the value passes untouched rather
        // than being checked against a version nobody asked for.
        let registry = FakeResolver::new(vec![("floor.stone.smooth", "stone_bricks")]);
        assert!(registry.block_ids().is_none());
        let bs = resolve_block_state(&token("totally_not_a_block"), Some(&registry)).unwrap();
        assert_eq!(bs.id, "minecraft:totally_not_a_block");
    }

    #[test]
    fn an_unknown_id_with_no_close_candidate_carries_no_suggestion() {
        let registry = FakeResolver::new(vec![]).pinned(&["minecraft:stone"]);
        let err = resolve_block_state(&token("totally_not_a_block"), Some(&registry)).unwrap_err();
        match err {
            MaterialDeferred::UnknownId(unknown) => assert!(unknown.suggestion.is_none()),
            other => panic!("expected UnknownId, got {other:?}"),
        }
    }
}
