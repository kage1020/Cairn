//! Translate a resolved theme-slot value into a [`BlockState`]: the
//! canonical id and properties the voxel grid stores.
//!
//! Abstract tokens are lifted through a [`TargetRegistry`]; when there is
//! none, or its catalog lacks the token, the caller learns which, so its
//! diagnostic can be precise. Either way the resolved id is then checked
//! against the target's block-id table, the only check that catches an
//! undeclared *canonical* token: `@totally_not_a_block` has no dot, so it
//! never reaches the materials catalog.

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::intent::ValueWithSpan;
use crate::resolve::{TokenKind, classify_token};
use crate::suggest::{nearest_match, nearest_namespaced_id};

use super::BlockState;

/// What the registry pack tells lowering, for one compile.
///
/// Lowering does not depend on `cairn-lang-formats`, where the registry
/// pack lives; the pack implements this trait instead. Lowering takes an
/// `Option`, `None` when no pack was offered (LSP highlight, `cairn check`
/// without a pack).
///
/// [`Self::lookup`] runs once per `mat_slot=` value, so it must be cheap;
/// [`Self::known_tokens`] runs only on a miss, and its order matters only
/// to `nearest_match`'s tie-break.
pub trait TargetRegistry {
    /// Lift `token`, the `@TOKEN` literal without its `@`, into a canonical
    /// [`BlockState`]. The implementation resolves the namespace and any
    /// state literal itself.
    fn lookup(&self, token: &str) -> Option<BlockState>;

    /// Every token this registry knows, for the suggestion after a
    /// [`Self::lookup`] miss. A fresh `Vec` per call is fine: misses are
    /// rare.
    fn known_tokens(&self) -> Vec<String>;

    /// The block ids valid in the target this compile is pinned to.
    ///
    /// `None` when no id table applies: the pack ships no `blocks`
    /// component, or the run pins no `--target` (`cairn check`,
    /// `cairn info`, `cairn lower`). Validation is then skipped rather than
    /// guessing a version, which would refuse ids that are fine on the one
    /// actually compiled against.
    fn block_ids(&self) -> Option<BlockIdSet<'_>>;

    /// The ids **the pinned target declares** that name the same block as
    /// `id`, in the registry's own order, which is the one an author reads.
    /// One old spelling often splits into several.
    ///
    /// A distance search cannot find a rename: `minecraft:light` and
    /// `minecraft:light_block_0` are eight edits apart, and no threshold
    /// that keeps `oak_plank` → `oak_planks` honest connects them. Called
    /// only on a miss, like [`Self::known_tokens`].
    ///
    /// The default answers no aliases, as for a pack without an `aliases`
    /// component.
    fn aliases_for(&self, id: &str) -> Vec<String> {
        let _ = id;
        Vec::new()
    }
}

/// The block ids valid in one pinned `(edition, version)` target.
///
/// A borrowed sorted slice rather than a set, so the pack lends the table
/// it built at load time and `cairn-lang-core` needs no hashing dependency.
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
    /// Panics in a debug build when `ids` is not ascending, which would
    /// make [`Self::contains`] miss real ids.
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

/// Namespace of a bare `@name` token, fixed so canonical tokens lower
/// without registry data.
const VANILLA_NAMESPACE: &str = "minecraft";

/// Why a slot value could not be lowered to a [`BlockState`].
///
/// Each variant becomes its own diagnostic, or none, so a consumer such as
/// an LSP quick-fix can tell them apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterialDeferred {
    /// The slot bound to an abstract token (`@floor.wood.broadleaf`) and no
    /// [`TargetRegistry`] was offered. Carries the token without its `@`.
    Abstract(String),
    /// The slot bound to an abstract token the offered registry does not
    /// declare.
    UnknownAbstract {
        /// Inner token text (no leading `@`).
        token: String,
        /// Closest declared token, when one is within the suggestion cap.
        suggestion: Option<String>,
    },
    /// The slot value was not a `@TOKEN` at all. The resolver has reported
    /// `E_UNKNOWN_SLOT_TARGET` for it, so lowering stays silent rather than
    /// report the span twice.
    AlreadyDiagnosed,
    /// The value resolved to a block id the pinned target does not declare.
    /// An error (`spec/versioning-editions` "Fail-loud and minimum-version
    /// inference"), because the game loads the id as air with nothing to
    /// explain the hole.
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
    /// Kept apart from [`Self::suggestion`]: a suggestion is a guess from
    /// string distance, an alias is the pack's statement that the names
    /// are one block, and a quick-fix offering both as one list would vouch
    /// for the guess.
    pub aliases: Vec<String>,
}

/// Who chose the id that turned out not to exist.
///
/// Each needs its own fix, and a message blaming the author's token for a
/// pack mapping sends them to the wrong file.
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
    /// Unlike [`Self::Catalog`], the fix is for the pack to add the missing
    /// row, not to correct an existing one.
    Builtin {
        /// The catalog token that would have redirected it, so the
        /// message can name the row the pack is missing.
        token: &'static str,
    },
}

impl IdOrigin {
    /// Stable wire name for the diagnostic payload, which needs it because
    /// [`Self::token`] alone cannot tell [`Self::Catalog`] from
    /// [`Self::Builtin`].
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
/// A canonical token (`@oak_planks`, `@oak_log[axis=x]`) maps directly, its
/// state literal into [`BlockState::properties`]. With `registry`, an
/// abstract token (`@floor.wood.broadleaf`) is lifted through its catalog
/// and every resolved id is checked against the pinned target.
///
/// # Errors
///
/// - [`MaterialDeferred::Abstract`] for an abstract token when `registry`
///   is `None`.
/// - [`MaterialDeferred::UnknownAbstract`] when the catalog does not
///   declare the token.
/// - [`MaterialDeferred::UnknownId`] when the target does not declare the
///   resolved id, including one the catalog produced, which is a pack bug
///   rather than an authoring one.
/// - [`MaterialDeferred::AlreadyDiagnosed`] for a value that is not a
///   token.
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
            validated_id(state, registry, &IdOrigin::Authored)
        }
        TokenKind::Abstract => {
            let ValueKind::Token(inner) = &slot.value.kind else {
                unreachable!("classify_token reports Abstract only for ValueKind::Token");
            };
            let Some(registry) = registry else {
                return Err(MaterialDeferred::Abstract(inner.clone()));
            };
            if let Some(state) = registry.lookup(inner) {
                return validated_id(
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

/// Refuse `state` when the pinned target does not declare its id; with no
/// target pinned, pass it through.
///
/// Crate-visible because a pack-supplied default material
/// (`pressure_plate`'s) reaches the palette outside
/// [`resolve_block_state`] and needs the same check.
pub(crate) fn validated_id(
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
    // Neither answer stands in for the other: a rename has aliases and a
    // typo a suggestion. Both are filled when a renamed id is also within
    // the edit cap of a declared spelling, as `stone_bricks` is of Bedrock
    // 1.21.0's `stonebrick`.
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
/// state literal, whose properties come back sorted by name so the order a
/// source spelled them reaches neither a backend's `Properties` nor the
/// lockfile's hash (`spec/compilation`). `roof::stair_state` builds its
/// states in that order too.
///
/// The parser refuses a malformed literal, and a debug build asserts none
/// arrives. A release build still reads the tail leniently, because a
/// library caller can hand [`resolve_block_state`] any text.
fn canonical_to_block_state(inner: &str) -> BlockState {
    let (head, properties_src) = match inner.find('[') {
        Some(i) => {
            let head = &inner[..i];
            let tail = &inner[i + 1..];
            debug_assert!(
                tail.ends_with(']')
                    && tail[..tail.len() - 1]
                        .split(',')
                        .all(|pair| !pair.is_empty() && pair.contains('=')),
                "a state literal reaches the material layer as the parser folds it, \
                 `[key=value,...]`, got `{inner}`"
            );
            // An unterminated literal keeps its whole tail.
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
    let mut properties = parse_state_literal(properties_src);
    properties.sort_unstable_keys();
    BlockState { id, properties }
}

/// Parse a `k=v,k=v` body into a property map, in written order.
///
/// Lenient: whitespace is trimmed, and empty segments and pairs without
/// `=` are skipped. The parser refuses those shapes in source; a
/// diagnostic from here would land in the wrong place.
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
mod tests;
