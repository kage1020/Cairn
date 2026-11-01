//! Translate a resolved theme-slot value into a [`BlockState`] and check its id against the
//! pinned target.

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::intent::ValueWithSpan;
use crate::resolve::{TokenKind, classify_token};
use crate::suggest::{nearest_match, nearest_namespaced_id};

use super::BlockState;

/// What the registry pack tells lowering, for one compile.
///
/// [`Self::lookup`] runs once per `mat_slot=` value, so it must be cheap.
pub trait TargetRegistry {
    /// Lift `token`, the `@TOKEN` literal without its `@`, into a canonical [`BlockState`].
    fn lookup(&self, token: &str) -> Option<BlockState>;

    /// Every token this registry knows, for the suggestion after a [`Self::lookup`] miss.
    fn known_tokens(&self) -> Vec<String>;

    /// The block ids valid in the pinned target, or `None` when no id table applies.
    fn block_ids(&self) -> Option<BlockIdSet<'_>>;

    /// The ids the pinned target declares that name the same block as `id`, in registry order.
    fn aliases_for(&self, id: &str) -> Vec<String> {
        let _ = id;
        Vec::new()
    }

    /// Whether a version this registry covers may declare `id`, asked only when
    /// [`Self::block_ids`] is `None`. Lowering paints an id this denies as a stand-in, which no
    /// structure file can hold, so a registry whose lowering is written out denies none.
    fn may_declare(&self, id: &str) -> bool {
        let _ = id;
        true
    }
}

/// The registry of a lowering with no target. It lifts every abstract token, and every id no
/// version of the edition declares, to a stand-in rather than a canonical state; that is safe
/// because nothing it lowers leaves [`findings_without_target`](super::findings_without_target).
pub(crate) struct NoTarget<'a> {
    /// Whether some version of the edition being lowered declares a block id.
    pub(crate) declared: &'a dyn Fn(&str) -> bool,
}

impl TargetRegistry for NoTarget<'_> {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        Some(stand_in(token))
    }

    fn known_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        None
    }

    fn may_declare(&self, id: &str) -> bool {
        id.starts_with(STAND_IN_MARK) || (self.declared)(id)
    }
}

/// What a stand-in's id starts with; no block id or abstract token can.
const STAND_IN_MARK: char = '@';

/// A stand-in for the block a target's pack names for `name`, an abstract token or a block id.
pub(crate) fn stand_in(name: &str) -> BlockState {
    BlockState::bare(format!("{STAND_IN_MARK}{name}"))
}

/// Whether `state` is a [`stand_in`], whose block only a target's pack can name.
pub(crate) fn is_stand_in(state: &BlockState) -> bool {
    state.id.starts_with(STAND_IN_MARK)
}

/// The block ids valid in one pinned `(edition, version)` target.
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
    /// In a debug build, when `ids` is not ascending.
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

/// Namespace of a bare `@name` token.
const VANILLA_NAMESPACE: &str = "minecraft";

/// Why a slot value could not be lowered to a [`BlockState`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaterialDeferred {
    /// An abstract token with no [`TargetRegistry`] offered; carries the token without its `@`.
    Abstract(String),
    /// An abstract token the offered registry does not declare.
    UnknownAbstract {
        /// Inner token text (no leading `@`).
        token: String,
        /// Closest declared token, when one is within the suggestion cap.
        suggestion: Option<String>,
    },
    /// Not a `@TOKEN` at all; the resolver already reported `E_UNKNOWN_SLOT_TARGET`.
    AlreadyDiagnosed,
    /// An undeclared id (`spec/versioning-editions` "Fail-loud and minimum-version inference").
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
    /// Closest id the target does declare, within `nearest_match`'s edit cap.
    pub suggestion: Option<String>,
    /// The ids this target declares for the same block, from the registry's alias table.
    pub aliases: Vec<String>,
}

/// Who chose the id that turned out not to exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdOrigin {
    /// The author wrote the id as a canonical token.
    Authored,
    /// The pack's materials catalog mapped an abstract token onto it.
    Catalog {
        /// The abstract token the author actually wrote, without the `@`.
        token: String,
    },
    /// Nobody chose it: lowering fell back to an id built into `cairn-lang-core`.
    Builtin {
        /// The catalog token the pack is missing a row for.
        token: &'static str,
    },
}

impl IdOrigin {
    /// Stable wire name for the diagnostic payload.
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
/// # Errors
///
/// The [`MaterialDeferred`] that says why the value has no usable block state.
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

/// Refuse `state` when the pinned target does not declare its id; pass it when none is pinned,
/// as a stand-in when no version the registry covers declares it.
pub(crate) fn validated_id(
    state: BlockState,
    registry: Option<&dyn TargetRegistry>,
    origin: &IdOrigin,
) -> Result<BlockState, MaterialDeferred> {
    let Some(registry) = registry else {
        return Ok(state);
    };
    let Some(ids) = registry.block_ids() else {
        if registry.may_declare(&state.id) {
            return Ok(state);
        }
        // Every target refuses the id itself, so nothing may be said about its state literal.
        return Ok(stand_in(&state.id));
    };
    if ids.contains(&state.id) {
        return Ok(state);
    }
    // Both can be filled: a rename has aliases, a typo a suggestion, and one id can be both.
    let aliases = registry.aliases_for(&state.id);
    Err(MaterialDeferred::UnknownId(UnknownId {
        suggestion: nearest_namespaced_id(&state.id, ids.iter()),
        aliases,
        registry: ids.label().to_owned(),
        origin: origin.clone(),
        id: state.id,
    }))
}

/// Turn a canonical token body (`oak_planks`, `minecraft:oak_log[axis=x]`) into a [`BlockState`].
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

/// Parse a `k=v,k=v` body into a property map, in written order, skipping malformed pairs.
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
