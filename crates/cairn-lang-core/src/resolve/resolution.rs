//! What [`resolve`](super::resolve()) returns.

use indexmap::IndexMap;
use serde::Serialize;

use crate::diagnostic::Diagnostic;
use crate::error::Span;
use crate::ids::{PlaceId, PortId, SiteName};
use crate::intent::ValueWithSpan;

use super::binding::ThemeBinding;

/// Resolution of one whole [`IntentModule`](crate::intent::IntentModule).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolution {
    /// One entry per theme defined in the module, keyed by theme name.
    pub themes: IndexMap<String, ThemeBinding>,
    /// Per struct, def and placement, keyed `struct::NAME`, `def::NAME` or `site::SITE::ID`.
    pub scopes: IndexMap<String, ScopeResolution>,
    /// Each `connect` row that resolved, in source order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub connects: Vec<ValidatedConnect>,
    /// Diagnostics found while resolving, in emission order.
    #[serde(skip)]
    pub diagnostics: Vec<Diagnostic>,
}

/// One end of a `connect` row, resolved to a `(place, port)` pair.
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

/// One validated `connect` row: everything the walkway voxeliser needs.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidatedConnect {
    /// Bare site name (no `site::` IR-key prefix).
    pub site: SiteName,
    /// First port — `from.port`.
    pub from: PortRef,
    /// Second port — `to.port`.
    pub to: PortRef,
    /// The `path=@MATERIAL` value, lifted later as a `mat_slot=` value is.
    pub path: ValueWithSpan,
    /// Byte range of the originating `connect ...` line.
    #[serde(skip)]
    pub span: Span,
}

impl ValidatedConnect {
    /// The walkway this row asks for, ends sorted so `a to b` and `b to a` are one pair.
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
    /// The theme applied to this scope: the module's only one, or a placement's `theme=`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound_theme: Option<String>,
    /// Per-member resolved binding, keyed by `member.span.start`.
    pub members: IndexMap<usize, ResolvedMemberBinding>,
}

/// Resolved data attached to a single [`Member`](crate::intent::Member).
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct ResolvedMemberBinding {
    /// The value the applied theme binds to this member's `mat_slot=`.
    ///
    /// `None` covers several cases, each already reported elsewhere.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_value: Option<ValueWithSpan>,
    /// Bindings from matching theme selectors, the later row winning on a shared key.
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub selector_extras: IndexMap<String, ValueWithSpan>,
}
