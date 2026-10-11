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
/// The IR is not mutated: each [`ScopeResolution`] keys its members by
/// `member.span.start`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolution {
    /// One entry per theme defined in the module, keyed by theme name.
    pub themes: IndexMap<String, ThemeBinding>,
    /// One entry per struct, def and placement, keyed `struct::NAME`,
    /// `def::NAME` or [`place_scope_key`](super::place_scope_key)'s
    /// `site::SITE::ID`, so a struct and a def of one name stay apart.
    pub scopes: IndexMap<String, ScopeResolution>,
    /// One entry per resolved `connect` row, in source order. A row that
    /// failed validation is left out, so the walkway voxeliser never lays
    /// a strip against a broken reference.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub connects: Vec<ValidatedConnect>,
    /// Diagnostics gathered during resolution, in emission order;
    /// [`crate::check::check`] merges and sorts them.
    #[serde(skip)]
    pub diagnostics: Vec<Diagnostic>,
}

/// One end of a `connect a.PORT to b.PORT` row, resolved into a
/// `(place, port)` pair the block-array lowering can look up directly.
///
/// The resolver's port diagnostics underline `span`, the token the author
/// wrote; block-array findings about the whole walkway anchor at
/// [`ValidatedConnect::span`] instead.
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

/// One `connect from.port to to.port path=@MATERIAL` row, validated:
/// everything the walkway voxeliser needs.
///
/// `path` stays a [`ValueWithSpan`] rather than a lifted `BlockState`,
/// because lifting needs the registry pack, and `cairn-lang-formats`, which
/// owns it, depends on this crate.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidatedConnect {
    /// Bare site name (no `site::` IR-key prefix).
    pub site: SiteName,
    /// First port — `from.port`.
    pub from: PortRef,
    /// Second port — `to.port`.
    pub to: PortRef,
    /// `path=@MATERIAL` value, lifted later by the same `resolve_block_state`
    /// path a `mat_slot=` value takes.
    pub path: ValueWithSpan,
    /// Byte range of the originating `connect ...` line.
    #[serde(skip)]
    pub span: Span,
}

impl ValidatedConnect {
    /// The walkway this row asks for, with its two ends in sorted order:
    /// `(site, place, port, place, port)`.
    ///
    /// `a.entry to b.entry` and `b.entry to a.entry` ask for one walkway:
    /// block-array lowering lays a pair once (`W_DUPLICATE_WALKWAY` for a
    /// later row), and `E_OUTPUT_NAME_COLLISION` counts one walkway per pair.
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
    /// The theme applied to this scope, when one was picked: the module's
    /// only logical theme, or a placement's `theme=`.
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
    /// `None` covers six cases, each reported elsewhere, so a reader that
    /// treats them as one reports a member twice or not at all:
    ///
    /// - no `mat_slot=`: `E_MISSING_MATERIAL` from `check::material`, for
    ///   the roles that paint nothing without one;
    /// - a `mat_slot=` its role does not read
    ///   ([`crate::intent::Member::read_slot`]): `W_IGNORED_ARGUMENT` from
    ///   `check::arguments`;
    /// - no theme bound, on a member that survives `block_array`'s `level`
    ///   flattening: `W_NO_THEME_BOUND`, once per lowered scope (a `def` is
    ///   lowered only under its placements' scopes);
    /// - no theme bound, on a member a `level` drops: that drop's
    ///   `W_DEFERRED_MEMBER`, without a `W_NO_THEME_BOUND` of its own;
    /// - the slot is not declared in the theme: `E_UNRESOLVED_SLOT`;
    /// - only a sibling edition variant declares it, deferred until a pin
    ///   picks one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_value: Option<ValueWithSpan>,
    /// Extra `key=value` bindings injected by a matching theme selector,
    /// merged left-to-right in source order (later selector wins on key
    /// collision).
    ///
    /// Nothing reads these yet, lowering included; `check::arguments`
    /// reports each binding on its row as `W_IGNORED_ARGUMENT`.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub selector_extras: IndexMap<String, ValueWithSpan>,
}
