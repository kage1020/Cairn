//! Where a `place` row lands relative to the rows placed before it.

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;
use crate::resolve::place_scope_key;

use crate::block_array::Dims;
use crate::block_array::args::{UnreadArgument, read_or_ignore};

use super::site_floor::PlacedBody;

/// Where one `place` line lands relative to what has already been placed:
/// the prior placement's origin (and, for `east_of`, its width) and the
/// `gap`, read before this placement's body is lowered.
///
/// Finished by [`PlaceAnchor::origin`] once the new body's dims are known,
/// because `north_of` steps back by the *new* placement's depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlaceAnchor {
    /// `at=origin`.
    WorldOrigin,
    /// `east_of=ID gap=N`: the prior placement's origin and inflated
    /// `dims.x`.
    EastOf {
        prior_origin: (i32, i32, i32),
        prior_dims_x: u32,
        gap: i64,
    },
    /// `north_of=ID gap=N`: the prior placement's origin. Its depth plays
    /// no part: the step back is the new body's own depth.
    NorthOf {
        prior_origin: (i32, i32, i32),
        gap: i64,
    },
}

/// A placement [`PlaceAnchor::origin`] works out lies partly outside the
/// `i32` range world coordinates are recorded and addressed in. `axis` is
/// the one that left the range and `coord` the coordinate on it that did,
/// for the message; `corner` says which coordinate that is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PlacementOutOfRange {
    pub(super) corner: PlacementCorner,
    pub(super) axis: char,
    pub(super) coord: i128,
}

/// Which end of a placement's box [`PlacementOutOfRange`] found outside
/// the range, and so what its `coord` holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PlacementCorner {
    /// The low-`x`, low-`z` origin the placement records. `coord` is the
    /// origin on the axis its selector moves along: a sum for `east_of`, a
    /// difference for `north_of`.
    Origin,
    /// The body's far edge, `origin + dims − 1`, where its last cell sits.
    /// `coord` is that edge on the first of `x` and `z` that leaves the
    /// range, not the whole high corner.
    FarEdge,
}

impl PlacementOutOfRange {
    /// `coord` as the `i32` it has to fit in, or the refusal that says it
    /// does not.
    fn fit(corner: PlacementCorner, axis: char, coord: i128) -> Result<i32, Self> {
        i32::try_from(coord).map_err(|_| Self {
            corner,
            axis,
            coord,
        })
    }

    /// The `W_DEFERRED_MEMBER` reason for the row this refuses.
    ///
    /// The repair depends on which end left the range. The far edge also
    /// moves with the body's own extent, and on a `north_of` row no `gap=`
    /// on the row itself moves `x`, so its advice names the body's `size=`
    /// and `overhang=` beside the `gap=`.
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
    /// The world-space origin (low-`x`, low-`z` corner) of a placement
    /// whose lowered body has `dims`, per `spec/components-editing-sites`
    /// "Origin selectors": `east_of` is `prior.x + prior.dims.x + gap`,
    /// `north_of` is `prior.z − new.dims.z − gap`. Either way `gap` counts
    /// the empty blocks between the two facing bounding-box faces (each the
    /// wall plus its `overhang=` columns), so `gap=0` makes the boxes touch
    /// whichever of the two is wider or deeper.
    ///
    /// The sum is taken in `i128`, where no `i32` origin, `u32` extent and
    /// `i64` gap can overflow, and refused when it leaves `i32` rather than
    /// saturated: a saturated origin would silently stack two placements on
    /// one coordinate. The body's
    /// far edge, `origin + dims − 1` on `x` and `z`, is refused the same
    /// way, since a cell past it has no world coordinate either. `y` is not
    /// checked, because every placement's `y` is the `0` of the `at=origin`
    /// row its chain starts from (`east_of` and `north_of` carry the
    /// prior's `y` through) and [`MAX_STRUCTURE_VOLUME`] keeps `dims.y` far
    /// below `i32::MAX`; a selector that moves `y` has to join the check.
    ///
    /// [`MAX_STRUCTURE_VOLUME`]: crate::block_array::MAX_STRUCTURE_VOLUME
    pub(super) fn origin(self, dims: Dims) -> Result<(i32, i32, i32), PlacementOutOfRange> {
        use PlacementCorner::{FarEdge, Origin};
        let fit = PlacementOutOfRange::fit;
        // The low corner first: the far edge is measured from it, so it
        // has to be in range before the far edge means anything.
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
        // INVARIANT: a body's `x` and `z` extents are its `size=` (a
        // `NonZeroU32`) plus twice its overhang, so neither is 0, and
        // `origin + dims − 1` is the last cell rather than one before the
        // origin.
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

/// Read the `at=origin` / `east_of=ID gap=N` / `north_of=ID gap=N`
/// selector of one `place` line into a [`PlaceAnchor`].
///
/// On a relative (`east_of` / `north_of`) row the anchor is `None` when the
/// prior place it names did not lower, which the caller reports before
/// skipping the row. A row with no usable selector never gets here: the
/// resolver refuses it (`E_INVALID_PLACE_ORIGIN`, or
/// `E_UNRESOLVED_PLACE_REF` when it names no prior place) and binds no
/// scope for it. The front-is-`+z` convention of
/// `spec/components-editing-sites` "Multi-building with `site`" is why
/// `north_of` retreats along `-z`.
///
/// On a relative row, a `gap=` that is not an integer is an unreadable
/// value: the anchor carries `gap=0`, and the [`UnreadArgument`] is handed
/// back beside it — also when the anchor is `None` — for the caller to
/// report with the note that matches whether the row was placed. An
/// `at=origin` row returns before `gap=` is read.
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

/// Where a `place` row with an unreadable `gap=` ended up, which picks the
/// note its finding carries.
#[derive(Clone, Copy)]
pub(super) enum GapOutcome {
    /// Placed at the `gap=0` the unreadable value falls back to.
    Placed,
    /// Refused for a reason no `gap=` reaches: its anchor did not lower,
    /// or its body was refused.
    NotPlaced,
    /// Refused because the placement worked out at `gap=0` leaves the
    /// `i32` range: its origin, or its body's far edge. Only that value was
    /// tried, so the note claims nothing about any other `gap=`.
    OutOfRange,
}

/// Report a `place` row's unreadable `gap=`, if it had one, with the note
/// for where the row ended up.
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
