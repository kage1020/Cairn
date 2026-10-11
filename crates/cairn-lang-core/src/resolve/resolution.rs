//! What [`resolve`](super::resolve()) returns: the module's
//! [`Resolution`], and the per-scope, per-member and per-`connect` records
//! inside it.

use indexmap::IndexMap;
use serde::Serialize;

use crate::diagnostic::Diagnostic;
use crate::error::Span;
use crate::ids::{PlaceId, PortId, SiteName};
use crate::intent::ValueWithSpan;

use super::binding::ThemeBinding;

/// Resolution of one whole [`IntentModule`](crate::intent::IntentModule).
///
/// Ownership: [`Resolution`] does **not** mutate the IR; it holds an
/// independent map keyed by scope name. The `members` map inside each
/// [`ScopeResolution`] is keyed by `member.span.start` so callers can
/// correlate without threading an index back through the original `Vec`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolution {
    /// One entry per theme defined in the module, keyed by theme name.
    pub themes: IndexMap<String, ThemeBinding>,
    /// One entry per struct/def/site, keyed by `kind::name` (e.g.
    /// `struct::cottage`, `def::cottage`, `site::hamlet`) so a file with a
    /// struct and a def of the same name still produces two distinct keys.
    pub scopes: IndexMap<String, ScopeResolution>,
    /// One entry per successfully-resolved `connect` row, in source order.
    /// Rows that failed validation (`E_UNRESOLVED_PORT`,
    /// `E_AMBIGUOUS_PORT`, `E_MISSING_PATH_MATERIAL`,
    /// `E_UNRESOLVED_PLACE_REF` on the place half) are dropped here so the
    /// walkway voxeliser does not lay a strip against a broken reference.
    /// Empty for any file that declares no `connect` lines, matching the
    /// `placements` shape on [`crate::block_array::BlockArrayIr`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub connects: Vec<ValidatedConnect>,
    /// Diagnostics gathered during resolution. The `check` pipeline merges
    /// these with the rest of `cairn check`'s output.
    #[serde(skip)]
    pub diagnostics: Vec<Diagnostic>,
}

/// One end of a `connect a.PORT to b.PORT` row, resolved into a
/// `(place, port)` pair the block-array lowering can look up directly.
///
/// `place` is the `id=` of the named place; `port` is the `id=` of the
/// matching member inside that place's `def`. The span points at the
/// originating `DotRef` value in source so the resolver-side
/// diagnostics (`E_UNRESOLVED_PORT`, `E_AMBIGUOUS_PORT`,
/// `E_UNRESOLVED_PLACE_REF` from the connect path) can underline the
/// exact token the user wrote, not the whole `connect` line. Block-array
/// side diagnostics (`W_WALKWAY_BLOCKED`, `W_DUPLICATE_WALKWAY`, the
/// endpoint-cascade `W_DEFERRED_MEMBER`, and the `W_DEFERRED_MEMBER` for a
/// port that could not be placed) describe the whole walkway and
/// therefore anchor at `ValidatedConnect::span` instead. The last one's
/// notes underline the refused port's member line in the `def`, not this
/// span.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortRef {
    /// `place id=` value the port belongs to.
    pub place: PlaceId,
    /// Member `id=` exposed by the place's def.
    pub port: PortId,
    /// Byte range of the `place.port` token in source.
    #[serde(skip)]
    pub span: Span,
}

impl std::fmt::Display for PortRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.place, self.port)
    }
}

/// One `connect from.port to to.port path=@MATERIAL` row, validated.
///
/// Carries everything the walkway voxeliser needs (both port refs, the
/// path material as a `ValueWithSpan` so `resolve_block_state` can
/// process it uniformly with `mat_slot=` values, the originating site
/// name, and the row's span for follow-up diagnostics).
///
/// The `path` is intentionally still a [`ValueWithSpan`] (and not a
/// lifted `BlockState`) at this layer: per-edition material resolution
/// is the responsibility of the next maturity tier (see
/// [`mod@crate::resolve`] module docs), and lifting here would invert the
/// `core` → `formats` dependency edge that owns the registry pack.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidatedConnect {
    /// Bare site name (no `site::` IR-key prefix).
    pub site: SiteName,
    /// First port — `from.port`.
    pub from: PortRef,
    /// Second port — `to.port`.
    pub to: PortRef,
    /// `path=@MATERIAL` value. Carried as [`ValueWithSpan`] so the
    /// walkway lowering can run it through the same
    /// `resolve_block_state` pipeline `mat_slot=` uses, lifting both
    /// canonical and abstract tokens through the registry pack with one
    /// code path.
    pub path: ValueWithSpan,
    /// Byte range of the originating `connect ...` line.
    #[serde(skip)]
    pub span: Span,
}

impl ValidatedConnect {
    /// The walkway this row asks for, with its two ends in sorted order:
    /// `(site, place, port, place, port)`.
    ///
    /// `a.entry to b.entry` and `b.entry to a.entry` give the same pair, so
    /// they ask for one walkway. Block-array lowering lays a pair once and
    /// reports a later row for it as `W_DUPLICATE_WALKWAY`, and
    /// `E_OUTPUT_NAME_COLLISION` counts one walkway per pair for the same
    /// reason; both key on this.
    pub(crate) fn walkway_pair(&self) -> (SiteName, PlaceId, PortId, PlaceId, PortId) {
        let mut ends = [
            (self.from.place.clone(), self.from.port.clone()),
            (self.to.place.clone(), self.to.port.clone()),
        ];
        ends.sort_unstable();
        let [(a_place, a_port), (b_place, b_port)] = ends;
        (self.site.clone(), a_place, a_port, b_place, b_port)
    }
}

/// Resolution outcome for a single struct/def/site body.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScopeResolution {
    /// Which theme governed this scope, when the resolver could pick one.
    /// `None` when the file has 0 or >1 themes (see the module-level
    /// "theme selection rule").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound_theme: Option<String>,
    /// Per-member resolved binding, keyed by `member.span.start`.
    pub members: IndexMap<usize, ResolvedMemberBinding>,
}

/// Resolved data attached to a single [`Member`](crate::intent::Member).
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct ResolvedMemberBinding {
    /// The value bound to this member's `mat_slot=` via the applied theme,
    /// when both ends matched.
    ///
    /// `None` covers six different situations, each reported by someone
    /// else:
    ///
    /// - the member carries no `mat_slot=` at all: `E_MISSING_MATERIAL`,
    ///   from `check::material`, for the roles that paint nothing without
    ///   one;
    /// - it carries one on a role whose
    ///   [`crate::intent::MemberRole::unread_arguments`] lists `mat_slot`,
    ///   which is not looked up ([`crate::intent::Member::read_slot`]):
    ///   `W_IGNORED_ARGUMENT`, from `check::arguments`;
    /// - no theme was bound to the scope, and the member reads its slot and
    ///   survives `block_array`'s `level` flattening: `W_NO_THEME_BOUND`,
    ///   which `block_array` pushes once for the scope. A `def`'s own scope
    ///   is not one it lowers: each `place` of the def is lowered under the
    ///   placement's scope instead, so nothing paints from the def's own;
    /// - no theme was bound to the scope, and the member is one a `level`
    ///   drops: the `W_DEFERRED_MEMBER` `block_array` pushes as it drops it.
    ///   It paints nothing whatever the theme, so on its own it does not
    ///   earn its scope a `W_NO_THEME_BOUND`;
    /// - the slot was not declared in the theme: `E_UNRESOLVED_SLOT`;
    /// - only a sibling edition variant declares it, which is deferred
    ///   until a pin picks one.
    ///
    /// A reader that treats them as one case will report a member twice or
    /// not at all.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_value: Option<ValueWithSpan>,
    /// Extra `key=value` bindings injected by a matching theme selector,
    /// merged left-to-right in source order (later selector wins on key
    /// collision).
    ///
    /// Nothing reads these yet, lowering included, so their values are
    /// never resolved against a registry either. `check::arguments` reports
    /// each binding on the row it was written on as `W_IGNORED_ARGUMENT`.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub selector_extras: IndexMap<String, ValueWithSpan>,
}
