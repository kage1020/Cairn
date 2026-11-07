//! Translate a resolved theme-slot value into a [`BlockState`] and check its id against the
//! pinned target.

use indexmap::IndexMap;

use crate::Edition;
use crate::ast::ValueKind;
use crate::intent::ValueWithSpan;
use crate::resolve::{TokenKind, classify_token};
use crate::suggest::{nearest_match, nearest_namespaced_id};

use super::BlockState;
use super::joins::Joins;

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

    /// Why the pinned target's state map has no form for `state`, as a clause, or `None` when it
    /// has one or the registry keeps no state map
    /// (`spec/versioning-editions` "The `edition portability` row").
    fn unmapped_state(&self, state: &BlockState) -> Option<String> {
        let _ = state;
        None
    }

    /// The nearest versions of the pinned target's edition on either side of it that declare
    /// `id`, asked only for an id the target does not declare; `None` when no other does.
    fn declared_elsewhere(&self, id: &str) -> Option<DeclaredElsewhere> {
        let _ = id;
        None
    }

    /// The other edition's block `id` names, when the pinned target's edition has no counterpart
    /// for it; asked only for an id the target does not declare.
    fn absent_concept(&self, id: &str) -> Option<AbsentConcept> {
        let _ = id;
        None
    }

    /// What the pinned target's tables say about `state`'s properties
    /// (`spec/versioning-editions` "A state is checked against its block's states at the target").
    fn state_domain(&self, state: &BlockState) -> StateDomain {
        let _ = state;
        StateDomain::Unrecorded
    }

    /// What `state` is to the rules joining panes, fences and walls to their neighbours at the
    /// pinned target (`spec/blockstate` "Connections"), or `None` when the registry cannot say.
    fn joins(&self, state: &BlockState) -> Option<Joins> {
        let _ = state;
        None
    }

    /// Whether `state`'s block takes `waterlogged=true` at the pinned target, so water overlapping
    /// it waterlogs it (`spec/blockstate` "`waterlogged`"), or `None` when the registry cannot say.
    fn holds_water(&self, state: &BlockState) -> Option<bool> {
        let _ = state;
        None
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

    fn state_domain(&self, _state: &BlockState) -> StateDomain {
        StateDomain::Unpinned
    }
}

/// What a registry's tables say about one block state's properties at the pinned target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateDomain {
    /// No version is pinned, so a build that pins one answers.
    Unpinned,
    /// The pinned target's pack records no states for the block.
    Unrecorded,
    /// Each property or value outside the block's states at the target, in the state's order;
    /// empty when there is none.
    Checked(Vec<OutOfDomain>),
}

/// One property or value of a block state that its block does not take at the pinned target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutOfDomain {
    /// The property.
    pub key: String,
    /// The value the state gives it.
    pub value: String,
    /// Whether the block lacks the property or only the value, with what it takes instead.
    pub fault: DomainFault,
    /// The edition's other versions whose tables take `key=value` on the block, oldest first.
    pub valid_at: Vec<String>,
}

/// How a pair falls outside its block's states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DomainFault {
    /// The block has no such property.
    UnknownProperty {
        /// The properties it has, in its table's order.
        properties: Vec<String>,
    },
    /// The block has the property, but it takes no such value.
    ValueOutside {
        /// The values it takes, in its table's order.
        values: Vec<String>,
    },
}

impl DomainFault {
    /// Stable wire name for the diagnostic payload.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::UnknownProperty { .. } => "unknown_property",
            Self::ValueOutside { .. } => "value_outside",
        }
    }

    /// The properties or values the target takes instead.
    #[must_use]
    pub fn valid(&self) -> &[String] {
        match self {
            Self::UnknownProperty { properties } => properties,
            Self::ValueOutside { values } => values,
        }
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
    UnknownId(Box<UnknownId>),
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
    /// The edition's versions nearest the target that do declare the id.
    pub elsewhere: Option<DeclaredElsewhere>,
    /// Set when the id names a block only the other edition has.
    pub absent: Option<AbsentConcept>,
}

/// A block of one edition that the other has no counterpart for
/// (`spec/versioning-editions` "A block only the other edition has").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AbsentConcept {
    /// The edition that has the block.
    pub declared_by: Edition,
    /// What the block is and why the other edition has none, as the registry pack words it.
    pub reason: String,
}

/// The versions of the pinned edition nearest the target that declare an id it does not
/// (`spec/versioning-editions` "Which registry an unknown ID is unknown to").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeclaredElsewhere {
    /// The edition, which a floor that needs the id names.
    pub edition: Edition,
    /// The last version before the target that declares the id, where a removed id was last.
    pub until: Option<String>,
    /// The first version after the target that declares the id, where a new id first appears.
    pub since: Option<String>,
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
    // Both can be filled: a rename has aliases, a typo a suggestion, and one id can be both. An id
    // another version or edition declares is no typo, so the nearest spelling would only mislead.
    let aliases = registry.aliases_for(&state.id);
    let elsewhere = registry.declared_elsewhere(&state.id);
    let absent = registry.absent_concept(&state.id);
    Err(MaterialDeferred::UnknownId(Box::new(UnknownId {
        suggestion: (elsewhere.is_none() && absent.is_none())
            .then(|| nearest_namespaced_id(&state.id, ids.iter()))
            .flatten(),
        aliases,
        elsewhere,
        absent,
        registry: ids.label().to_owned(),
        origin: origin.clone(),
        id: state.id,
    })))
}

/// The state a block id names as written, `minecraft:light[level=15]` with its literal's
/// properties (`spec/materials-themes` "Raw blocks").
pub(crate) fn block_id_state(id: &str) -> BlockState {
    canonical_to_block_state(id)
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
