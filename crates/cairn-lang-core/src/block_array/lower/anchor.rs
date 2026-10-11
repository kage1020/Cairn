//! Where a `place` row lands relative to the rows placed before it.

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;
use crate::resolve::place_scope_key;

use crate::block_array::Dims;
use crate::block_array::args::{UnreadArgument, read_or_ignore};

use super::site_floor::PlacedBody;

/// Where one `place` line lands relative to prior placements, finished by [`PlaceAnchor::origin`]
/// once the new body's dims are known.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlaceAnchor {
    /// `at=origin`.
    WorldOrigin,
    /// `east_of=ID gap=N`: the prior placement's origin and inflated `dims.x`.
    EastOf {
        prior_origin: (i32, i32, i32),
        prior_dims_x: u32,
        gap: i64,
    },
    /// `north_of=ID gap=N`: the prior placement's origin; the step back is the new body's depth.
    NorthOf {
        prior_origin: (i32, i32, i32),
        gap: i64,
    },
}

/// A placement [`PlaceAnchor::origin`] works out lies partly outside the `i32` range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PlacementOutOfRange {
    pub(super) corner: PlacementCorner,
    pub(super) axis: char,
    pub(super) coord: i128,
}

/// Which end of a placement's box left the range, and so what `coord` holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlacementCorner {
    /// The low-`x`, low-`z` origin the placement records.
    Origin,
    /// The body's far edge, `origin + dims − 1`, on the first of `x` and `z` that leaves the range.
    FarEdge,
}

impl PlacementOutOfRange {
    /// `coord` as the `i32` it has to fit in, or the refusal.
    fn fit(corner: PlacementCorner, axis: char, coord: i128) -> Result<i32, Self> {
        i32::try_from(coord).map_err(|_| Self {
            corner,
            axis,
            coord,
        })
    }

    /// The `W_DEFERRED_MEMBER` reason for the row this refuses.
    pub(super) fn deferral(self) -> String {
        let Self {
            corner,
            axis,
            coord,
        } = self;
        let (what, recorded, repair) = match corner {
            PlacementCorner::Origin => (
                "origin works out to",
                "a placement's origin is recorded in",
                "shorten the `gap=` on this row or on a row it is placed relative to",
            ),
            PlacementCorner::FarEdge => (
                "body reaches",
                "a placement's cells are addressed in",
                "shrink the body with its `def`'s `size=` or a roof's `overhang=`, or shorten \
                 the `gap=` on this row or on a row it is placed relative to",
            ),
        };
        format!(
            "this placement's {what} {axis}={coord}, past the {} to {} range {recorded}; {repair}",
            i32::MIN,
            i32::MAX,
        )
    }
}

impl PlaceAnchor {
    /// The world origin of a placement whose body has `dims` (`spec/components-editing-sites`
    /// "Origin selectors"), or which end of it leaves `i32`.
    ///
    /// `y` is unchecked: no selector moves it, and one that does has to join the check.
    pub(super) fn origin(self, dims: Dims) -> Result<(i32, i32, i32), PlacementOutOfRange> {
        use PlacementCorner::{FarEdge, Origin};
        let fit = PlacementOutOfRange::fit;
        // The low corner first: the far edge is measured from it.
        let (x, y, z) = match self {
            Self::WorldOrigin => (0, 0, 0),
            Self::EastOf {
                prior_origin: (x, y, z),
                prior_dims_x,
                gap,
            } => {
                let next_x = i128::from(x) + i128::from(prior_dims_x) + i128::from(gap);
                (fit(Origin, 'x', next_x)?, y, z)
            }
            Self::NorthOf {
                prior_origin: (x, y, z),
                gap,
            } => {
                let next_z = i128::from(z) - i128::from(dims.z) - i128::from(gap);
                (x, y, fit(Origin, 'z', next_z)?)
            }
        };
        // INVARIANT: `size=` is non-zero, so `origin + dims − 1` is the last cell.
        debug_assert!(
            dims.x > 0 && dims.z > 0,
            "a placed body has a zero extent: {dims:?}",
        );
        for (axis, low, extent) in [('x', x, dims.x), ('z', z, dims.z)] {
            fit(FarEdge, axis, i128::from(low) + i128::from(extent) - 1)?;
        }
        Ok((x, y, z))
    }
}

/// Read a `place` line's `at=origin` / `east_of=` / `north_of=` selector into a [`PlaceAnchor`].
///
/// The anchor is `None` when the prior place did not lower. An unreadable `gap=` reads as `0`,
/// and its [`UnreadArgument`] comes back for the caller to report.
pub(super) fn resolve_place_anchor(
    member: &Member,
    placed: &IndexMap<String, PlacedBody>,
    site_name: &str,
) -> (Option<PlaceAnchor>, Option<UnreadArgument>) {
    if let Some(value) = member.intent_state.get("at")
        && matches!(&value.value.kind, ValueKind::Ident(s) if s == "origin")
    {
        return (Some(PlaceAnchor::WorldOrigin), None);
    }
    let (gap, gap_unread) = match read_or_ignore(
        member,
        "gap",
        |kind| match kind {
            ValueKind::Int(n) => Some(*n),
            _ => None,
        },
        "an integer",
    ) {
        Ok(gap) => (gap.unwrap_or(0), None),
        Err(unread) => (0, Some(unread)),
    };
    let prior = |key: &str| {
        member
            .intent_state
            .get(key)
            .and_then(|v| v.value.as_label_str())
            .and_then(|target| placed.get(&place_scope_key(site_name, target)))
            .map(|body| (body.placement.origin, body.placement.dims.x))
    };
    let anchor = if let Some((prior_origin, prior_dims_x)) = prior("east_of") {
        Some(PlaceAnchor::EastOf {
            prior_origin,
            prior_dims_x,
            gap,
        })
    } else {
        prior("north_of").map(|(prior_origin, _)| PlaceAnchor::NorthOf { prior_origin, gap })
    };
    (anchor, gap_unread)
}

/// Where a `place` row with an unreadable `gap=` ended up, which picks its finding's note.
#[derive(Clone, Copy)]
pub(super) enum GapOutcome {
    /// Placed at the `gap=0` the unreadable value falls back to.
    Placed,
    /// Refused for a reason no `gap=` reaches.
    NotPlaced,
    /// Refused because the placement at `gap=0` leaves the `i32` range.
    OutOfRange,
}

/// Report a `place` row's unreadable `gap=`, if any, with the note for where the row ended up.
pub(super) fn report_unread_gap(
    unread: Option<UnreadArgument>,
    outcome: GapOutcome,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.extend(unread.map(|unread| {
        unread.report(match outcome {
            GapOutcome::Placed => {
                "the row is placed as `gap=0` places it, edge to edge with the place it is \
                 relative to"
            }
            GapOutcome::NotPlaced => {
                "this row is not placed either way — see the finding on the same line"
            }
            GapOutcome::OutOfRange => {
                "this row is not placed at `gap=0`, the value its origin was worked out \
                 with — see the finding on the same line"
            }
        })
    }));
}
