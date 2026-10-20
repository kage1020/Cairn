//! How the `walls` and `roof` members of a paint set size its volume.

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
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

/// Highest wall row across the [`painting_walls`] pairs, `0` with none.
pub(super) fn max_wall_top(painting: &[(u32, u32)]) -> u32 {
    painting
        .iter()
        .map(|(y_offset, h)| y_offset.saturating_add(*h))
        .max()
        .unwrap_or(0)
}

/// The `(y_offset, height)` pairs of every walls member that will stand and paint.
///
/// The volume and the [`WallColumn`] both read this one list (`spec/compilation` "Level grouping
/// and volume derivation").
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

/// Highest wall row the walls members of `flattened` ask for, whether or not they paint.
pub(super) fn written_wall_top(flattened: &[(u32, &Member)]) -> u32 {
    flattened
        .iter()
        .filter(|(_, m)| matches!(m.role, MemberRole::Walls))
        .filter_map(|(y_offset, m)| height_value(m).map(|h| y_offset.saturating_add(h)))
        .max()
        .unwrap_or(0)
}

/// Largest `overhang=` a roof among `members` or their levels is written with, drawn or not.
pub(super) fn written_roof_overhang(members: &[Member]) -> u32 {
    members
        .iter()
        .map(|m| match m.role {
            MemberRole::Roof => m.nonneg_u32("overhang").unwrap_or(0),
            MemberRole::Level => written_roof_overhang(&m.children.members),
            _ => 0,
        })
        .max()
        .unwrap_or(0)
}

/// Largest `overhang=` across the roofs that draw, reporting an unreadable one on every roof.
///
/// This is the only reader of `overhang=`, so a value not checked here is reported nowhere.
pub(super) fn max_roof_overhang(
    flattened: &[(u32, &Member)],
    diagnostics: &mut Vec<Diagnostic>,
) -> u32 {
    flattened
        .iter()
        .map(|(_, m)| *m)
        .filter(|m| matches!(m.role, MemberRole::Roof))
        .filter_map(|m| {
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

/// Tallest rise of any roof that draws ([`roof_draws`]), measured on the overhang-inflated box.
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
            // An unusable `slope_to=` contributes `0`; `fill_roof` reports it.
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

/// The kind this roof will draw with, or `None` when it draws nothing, such as a `shed` with no
/// usable `slope_to=`.
///
/// Both roof walks ask this, so the footprint and the height agree on which roofs are real.
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

/// A positive `height=` that fits `u32`, else `None` rather than a saturated height.
fn height_value(member: &Member) -> Option<u32> {
    let raw = member.intent_state.get("height")?;
    match &raw.value.kind {
        ValueKind::Int(v) if *v > 0 => u32::try_from(*v).ok(),
        _ => None,
    }
}
