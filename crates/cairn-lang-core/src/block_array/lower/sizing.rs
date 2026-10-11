//! How the `walls` and `roof` members of a paint set size its volume.

use crate::ast::ValueKind;
use crate::check::Diagnostic;
use crate::intent::{Member, MemberRole};
use crate::resolve::ScopeResolution;

use crate::block_array::args::{diag_deferred_member_reason, nonneg_int_or_ignore};
use crate::block_array::material::TargetRegistry;
use crate::block_array::openings::WallSide;
use crate::block_array::roof::{
    RoofKind, flat_extra_height, gable_extra_height, hip_extra_height, shed_extra_height,
    shed_slope_span,
};

use super::members::member_will_paint;

/// Highest wall voxel Y across every walls member the flatten pass surfaced.
///
/// The struct's roof plane must sit above the tallest wall column, and a
/// level-scoped `walls id=upper height=H` inside a `level y=N` block extends
/// the wall column up to `y = N + H`. Returning the maximum over the
/// `(y_offset, height)` pairs from the flattened member list keeps the
/// dim math correct when a struct mixes struct-scoped and level-scoped
/// walls. Members without a positive `height=` contribute `0`; the
/// `W_DEFERRED_MEMBER` for that member fires later in the massing phase
/// so a hand-built sizeless `walls` still surfaces its own diagnostic.
///
/// Members whose material will not resolve are absent from the list for
/// the same reason a heightless one is: they paint no row. A themeless
/// struct used to reserve the full wall height for walls that lowered to
/// air, so the artifact was as tall as the building it did not contain.
pub(super) fn max_wall_top(painting: &[(u32, u32)]) -> u32 {
    painting
        .iter()
        .map(|(y_offset, h)| y_offset.saturating_add(*h))
        .max()
        .unwrap_or(0)
}

/// The `(y_offset, height)` pairs of every walls member that will both
/// stand and paint.
///
/// One list behind both the volume and [`WallColumn`], because
/// `spec/compilation` "Level grouping and volume derivation" makes them two
/// readings of a single list and rests the "no member paints past the end of
/// the array" invariant on their agreeing. Filtering one and not the other
/// would leave the window carve writing rows the array no longer has.
/// `max_wall_top` collapses the pairs to their maximum, which sizes the volume
/// and seats the roof; the column keeps the spans, which is what decides
/// whether a rectangle cut into a wall lands in masonry.
///
/// [`WallColumn`]: crate::block_array::wall_column::WallColumn
pub(super) fn painting_walls<'a>(
    flattened: &'a [(u32, &'a Member)],
    scope: Option<&'a ScopeResolution>,
    registry: Option<&'a dyn TargetRegistry>,
    theme_missing: bool,
) -> impl Iterator<Item = (u32, u32)> + 'a {
    flattened
        .iter()
        .filter(|(_, m)| matches!(m.role, MemberRole::Walls))
        .filter(move |(_, m)| member_will_paint(m, scope, registry, theme_missing))
        .filter_map(|(y_offset, m)| height_value(m).map(|h| (*y_offset, h)))
}

/// Largest `overhang=` across the roof members the pass will paint.
///
/// Two concerns over one walk, and they cover different members.
///
/// The **contribution** is filtered through [`roof_draws`], the same
/// question [`max_roof_extra_height`] asks: a roof that will not draw must
/// not widen the array. Without the filter it gave the footprint its eaves
/// and the height nothing, so the walls moved inward and a ring of air
/// surrounded them.
///
/// The **validation** is not filtered, because this is the only place
/// `overhang=` is read: a value nothing here looks at is a value nothing
/// anywhere reports. It walks the flattened list for the same reason — a
/// roof this function cannot see is a roof whose `overhang=` nothing
/// validates.
pub(super) fn max_roof_overhang(
    flattened: &[(u32, &Member)],
    diagnostics: &mut Vec<Diagnostic>,
) -> u32 {
    flattened
        .iter()
        .map(|(_, m)| *m)
        .filter(|m| matches!(m.role, MemberRole::Roof))
        .filter_map(|m| {
            // The consequence differs by member, so it is worked out here
            // rather than written once: a roof that draws is in the build
            // flush with the wall line, and a roof that does not is
            // already earning its own finding from the envelope phase.
            let draws = roof_draws(m);
            let consequence = if draws.is_some() {
                "the roof is drawn flush with the wall line"
            } else {
                "this roof draws nothing either way — see the finding on the same line"
            };
            let read = nonneg_int_or_ignore(m, "overhang", consequence, diagnostics);
            draws.and(read)
        })
        .max()
        .unwrap_or(0)
}

/// Maximum vertical contribution from any roof member the pass will draw
/// ([`roof_draws`]). A roof that draws nothing contributes `0` here; its
/// `W_DEFERRED_MEMBER` warning fires later, during the envelope phase,
/// against the actual member span. Computing the dim from the inflated
/// roof bounding box (interior + 2 * overhang on each axis) keeps the math
/// consistent with each per-kind generator.
pub(super) fn max_roof_extra_height(
    flattened: &[(u32, &Member)],
    interior_w: u32,
    interior_h: u32,
    overhang: u32,
) -> u32 {
    let roof_w = interior_w.saturating_add(overhang.saturating_mul(2));
    let roof_h = interior_h.saturating_add(overhang.saturating_mul(2));
    flattened
        .iter()
        .map(|(_, m)| *m)
        .filter(|m| matches!(m.role, MemberRole::Roof))
        .filter_map(|m| roof_draws(m).map(|k| roof_extra_height(k, m, roof_w, roof_h)))
        .max()
        .unwrap_or(0)
}

fn roof_extra_height(kind: RoofKind, member: &Member, roof_w: u32, roof_h: u32) -> u32 {
    match kind {
        RoofKind::Gable => gable_extra_height(roof_w.min(roof_h)),
        RoofKind::Shed => {
            // Shed's slope axis depends on `slope_to=`. We do not have
            // diagnostics here (the dim pass runs before envelope-phase
            // diagnostics), so an unrecognised or missing `slope_to=`
            // contributes `0`; the same member will surface a
            // `W_DEFERRED_MEMBER` in `fill_roof_shed` and lower to no
            // voxels, keeping the dim math conservative. The axis
            // choice goes through `shed_slope_span` — the same helper
            // `shed_voxels` uses — so the dim and the generator cannot
            // disagree on which axis the slope runs along.
            match member
                .ident_value("slope_to")
                .and_then(WallSide::from_ident)
            {
                Some(slope_to) => shed_extra_height(shed_slope_span(roof_w, roof_h, slope_to)),
                None => 0,
            }
        }
        RoofKind::Hip => hip_extra_height(roof_w, roof_h),
        RoofKind::Flat => flat_extra_height(),
    }
}

/// The kind this roof will actually draw with, or `None` when it will
/// draw nothing.
///
/// Not [`roof_kind_of`], which answers "does the kind table know this
/// name". A `shed` with no usable `slope_to=` is a name the table knows
/// and a member [`fill_roof_shed`] returns from without painting a voxel,
/// and [`roof_extra_height`]'s shed arm already gives it `0`. Both roof
/// walks go through here so the footprint and the height cannot disagree
/// about which roofs are real — the disagreement that let a roof widen an
/// array it put nothing in.
///
/// Silent, unlike [`shed_slope_to`]: this is the question, and the answer
/// is reported by the pass that tries to draw.
///
/// [`fill_roof_shed`]: super::roof_fill::fill_roof_shed
/// [`shed_slope_to`]: super::roof_fill::shed_slope_to
pub(super) fn roof_draws(member: &Member) -> Option<RoofKind> {
    let kind = roof_kind_of(member)?;
    if matches!(kind, RoofKind::Shed)
        && member
            .ident_value("slope_to")
            .and_then(WallSide::from_ident)
            .is_none()
    {
        return None;
    }
    Some(kind)
}

fn roof_kind_of(member: &Member) -> Option<RoofKind> {
    let raw = member.intent_state.get("kind")?;
    let ValueKind::Ident(name) = &raw.value.kind else {
        return None;
    };
    RoofKind::from_ident(name)
}

pub(super) fn wall_height(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<u32> {
    match height_value(member) {
        Some(h) if h >= 1 => Some(h),
        _ => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                "walls without a positive `height=` cannot voxelise",
            ));
            None
        }
    }
}

/// `None` covers "absent", "not a positive integer", and "past `u32`"
/// alike, which is what [`wall_height`] turns into one `W_DEFERRED_MEMBER`.
///
/// Saturating instead would put a wall top at `u32::MAX` because the author
/// asked for `2^33` — the outcome [`Member::nonneg_u32`] documents as the
/// reason it refuses rather than clamps.
fn height_value(member: &Member) -> Option<u32> {
    let raw = member.intent_state.get("height")?;
    match &raw.value.kind {
        ValueKind::Int(v) if *v > 0 => u32::try_from(*v).ok(),
        _ => None,
    }
}
