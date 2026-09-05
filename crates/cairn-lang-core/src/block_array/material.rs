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
use crate::suggest::{nearest_match, nearest_namespaced_id};

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

    /// The ids **the pinned target declares** that name the same block as
    /// `id`, in the registry's own order.
    ///
    /// This is the rename half of a refused id, and the one a distance
    /// search cannot reach: `minecraft:light` and
    /// `minecraft:light_block_0` are eight edits apart and the same block,
    /// so no threshold that keeps `oak_plank` → `oak_planks` honest will
    /// ever connect them. A registry answers from a table that says so
    /// outright.
    ///
    /// Returning several is normal rather than exceptional — one old
    /// spelling routinely splits into a family — and the order is the
    /// registry's, since it is the one an author reads.
    ///
    /// Called only on the miss path, beside [`Self::known_tokens`] and on
    /// the same terms: allocating per miss is intentional, because misses
    /// are rare and the caller needs owned candidates anyway.
    ///
    /// The default is "this registry has no alias table", which is what a
    /// pack shipping no `aliases` component amounts to and what every
    /// registry did before the component existed. An implementation that
    /// has one overrides it; one that does not is not obliged to say so.
    fn aliases_for(&self, id: &str) -> Vec<String> {
        // A registry with no alias table has the same answer for every id.
        let _ = id;
        Vec::new()
    }
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
    /// The ids this target declares for the same block, from the registry's
    /// alias table. Empty when the pack has no alias table, no group names
    /// the refused id, or the group's other spellings are all absent from
    /// this target too.
    ///
    /// Kept apart from [`Self::suggestion`] rather than folded into it
    /// because the two are different claims about the same span. A
    /// suggestion is a guess from a string distance and can be wrong about
    /// which block was meant; an alias is a statement out of the pack's
    /// table that these names are one block. A consumer that offered them
    /// as one list would be offering a quick-fix it cannot stand behind.
    pub aliases: Vec<String>,
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
    /// Nobody chose it: the member carries no `mat_slot=`, the pack
    /// declares no catalog token for its default, and lowering fell back
    /// to an id compiled into `cairn-lang-core`.
    ///
    /// Distinct from [`Self::Catalog`] because the fix is the opposite
    /// one. A catalog mapping exists and is wrong for this target; a
    /// built-in fallback fires precisely because the pack has nothing to
    /// say, and the pack is what has to grow the row.
    Builtin {
        /// The catalog token that would have redirected it, so the
        /// message can name the row the pack is missing.
        token: &'static str,
    },
}

impl IdOrigin {
    /// Stable wire name for the diagnostic payload.
    ///
    /// `token` alone cannot separate [`Self::Catalog`] from
    /// [`Self::Builtin`] — both carry one — and the two need opposite
    /// fixes, so a consumer that can only see `token` would offer the
    /// wrong one half the time.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Authored => "authored",
            Self::Catalog { .. } => "catalog",
            Self::Builtin { .. } => "builtin",
        }
    }

    /// The catalog token involved, when the origin names one.
    #[must_use]
    pub fn token(&self) -> Option<&str> {
        match self {
            Self::Authored => None,
            Self::Catalog { token } => Some(token),
            Self::Builtin { token } => Some(token),
        }
    }
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
/// pack still see the shape they did before the pack carried a catalog.
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
///
/// Visible to the lowering pass because `mat_slot=` is not the only way an
/// id reaches a palette: a member whose default material comes from the
/// pack (`pressure_plate`) resolves outside [`resolve_block_state`] and
/// would otherwise be the one id in the build that nothing checks.
pub(crate) fn check_id(
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
    // Both answers are collected, and neither stands in for the other: the
    // alias table says what this target calls the block, the distance
    // search guesses which block was meant. A rename has the first and not
    // the second, a typo the second and not the first, and a typo *of* a
    // renamed id can have both — `stone_brick` on Bedrock 1.21.0 is one
    // edit from `stone_bricks`, which that version renamed to
    // `stonebrick`.
    let aliases = registry.map_or_else(Vec::new, |registry| registry.aliases_for(&state.id));
    Err(MaterialDeferred::UnknownId(UnknownId {
        suggestion: nearest_namespaced_id(&state.id, ids.iter()),
        aliases,
        registry: ids.label().to_owned(),
        origin: origin.clone(),
        id: state.id,
    }))
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
        /// One alias group: every spelling of one block, as a pack's
        /// `aliases` component declares it. Empty means "no alias table",
        /// which is what a pack without the component amounts to.
        group: Vec<String>,
    }

    impl FakeResolver {
        fn new(entries: Vec<(&'static str, &'static str)>) -> Self {
            Self {
                entries,
                ids: Vec::new(),
                group: Vec::new(),
            }
        }

        /// Pin a target declaring exactly `ids`.
        fn pinned(mut self, ids: &[&str]) -> Self {
            self.ids = ids.iter().map(|id| (*id).to_owned()).collect();
            self.ids.sort();
            self
        }

        /// Declare one alias group, in the order a pack wrote it.
        fn aliasing(mut self, group: &[&str]) -> Self {
            self.group = group.iter().map(|id| (*id).to_owned()).collect();
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

        fn aliases_for(&self, id: &str) -> Vec<String> {
            if !self.group.iter().any(|spelling| spelling == id) {
                return Vec::new();
            }
            self.group
                .iter()
                .filter(|spelling| self.ids.binary_search(spelling).is_ok())
                .cloned()
                .collect()
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
                aliases: Vec::new(),
            }),
        );
    }

    #[test]
    fn a_rename_is_answered_from_the_alias_table_and_not_by_distance() {
        // Bedrock 1.21.0 spells the Java `light` block `light_block`, six
        // edits away — past `nearest_match`'s cap. (From 1.21.40 it is
        // `light_block_0` … `_15`, which is further still.) No threshold
        // that keeps `oak_plank` → `oak_planks` honest reaches it, so the
        // answer has to come from a table that states the two are one
        // block.
        let registry = FakeResolver::new(vec![])
            .pinned(&["minecraft:light_block"])
            .aliasing(&["minecraft:light", "minecraft:light_block"]);
        let err = resolve_block_state(&token("light"), Some(&registry)).unwrap_err();
        match err {
            MaterialDeferred::UnknownId(unknown) => {
                assert_eq!(unknown.id, "minecraft:light");
                assert_eq!(unknown.aliases, ["minecraft:light_block".to_owned()]);
                assert!(
                    unknown.suggestion.is_none(),
                    "the distance search still has nothing to say about a rename",
                );
            }
            other => panic!("expected UnknownId, got {other:?}"),
        }
    }

    /// A group answers with every spelling the target declares, not with
    /// one of them.
    ///
    /// Bedrock 1.21.40 split the single `light_block` into sixteen ids by
    /// light level. Picking one on the author's behalf would be a silent
    /// substitution of a light level they never chose; the closed set is
    /// the honest answer, and choosing from it is theirs.
    #[test]
    fn a_split_answers_with_every_spelling_the_target_has() {
        let registry = FakeResolver::new(vec![])
            .pinned(&[
                "minecraft:light_block_0",
                "minecraft:light_block_1",
                "minecraft:light_block_2",
            ])
            .aliasing(&[
                "minecraft:light",
                "minecraft:light_block_0",
                "minecraft:light_block_1",
                "minecraft:light_block_2",
            ]);
        let err = resolve_block_state(&token("light"), Some(&registry)).unwrap_err();
        match err {
            MaterialDeferred::UnknownId(unknown) => assert_eq!(
                unknown.aliases,
                [
                    "minecraft:light_block_0".to_owned(),
                    "minecraft:light_block_1".to_owned(),
                    "minecraft:light_block_2".to_owned(),
                ],
            ),
            other => panic!("expected UnknownId, got {other:?}"),
        }
    }

    /// An alias the target does not declare either is not an answer.
    ///
    /// The group is edition-wide and version-free by construction, so it
    /// holds spellings from versions this compile is not building for.
    /// Offering one of those would refuse an id and then suggest another
    /// the same target also refuses.
    #[test]
    fn an_alias_the_target_lacks_is_not_offered() {
        let registry = FakeResolver::new(vec![])
            .pinned(&["minecraft:cobblestone"])
            .aliasing(&["minecraft:light", "minecraft:light_block"]);
        let err = resolve_block_state(&token("light"), Some(&registry)).unwrap_err();
        match err {
            MaterialDeferred::UnknownId(unknown) => assert!(
                unknown.aliases.is_empty(),
                "`light_block` is no more declared here than `light` is",
            ),
            other => panic!("expected UnknownId, got {other:?}"),
        }
    }

    /// A typo *of* a renamed id carries both answers, and they are
    /// different claims about it.
    ///
    /// `stone_brick` on a target spelling the block `stonebrick` is one
    /// edit from the alias and one edit from nothing else, so the two
    /// happen to agree here — what matters is that each field is filled by
    /// its own rule rather than one being derived from the other.
    #[test]
    fn a_typo_of_a_renamed_id_carries_both_answers() {
        let registry = FakeResolver::new(vec![])
            .pinned(&["minecraft:stonebrick"])
            .aliasing(&["minecraft:stone_bricks", "minecraft:stonebrick"]);
        let err = resolve_block_state(&token("stone_bricks"), Some(&registry)).unwrap_err();
        match err {
            MaterialDeferred::UnknownId(unknown) => {
                assert_eq!(unknown.aliases, ["minecraft:stonebrick".to_owned()]);
                assert_eq!(unknown.suggestion.as_deref(), Some("minecraft:stonebrick"));
            }
            other => panic!("expected UnknownId, got {other:?}"),
        }
    }

    /// A registry with no alias table answers exactly as every registry
    /// did before the component existed.
    #[test]
    fn a_registry_with_no_alias_table_answers_no_alias() {
        let registry = FakeResolver::new(vec![]).pinned(&["minecraft:light_block"]);
        let err = resolve_block_state(&token("light"), Some(&registry)).unwrap_err();
        match err {
            MaterialDeferred::UnknownId(unknown) => assert!(unknown.aliases.is_empty()),
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
                aliases: Vec::new(),
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
