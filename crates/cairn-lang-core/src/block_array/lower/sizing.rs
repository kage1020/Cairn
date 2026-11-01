//! How the `walls` and `roof` members of a paint set size its volume.

use crate::diagnostic::Diagnostic;
use crate::intent::{Member, MemberRole};
use crate::resolve::ScopeResolution;

use crate::block_array::args::{diag_invalid_member, nonneg_int_or_ignore};
use crate::block_array::material::TargetRegistry;

use super::members::member_will_paint;
use super::roof_kind::{roof_draws, roof_rise};
use super::written::height_value;

/// Highest wall row across `(y_offset, height)` pairs, `0` with none.
pub(super) fn max_wall_top(walls: &[(u32, u32)]) -> u32 {
    walls
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

/// Largest `overhang=` across the roofs that draw, reporting an unreadable one on every roof.
///
/// The one reader that reports `overhang=`, so a value not checked here is reported nowhere.
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

/// Tallest rise of any roof that draws, measured on the overhang-inflated box.
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
        .filter_map(|m| roof_rise(m, roof_w, roof_h))
        .max()
        .unwrap_or(0)
}

/// The note on a `walls` refused for its `height=`, saying what one is.
const WALLS_HEIGHT_FORM: &str = "write `height=N`, where `N` is a positive integer: the walls fill \
                                 the `N` rows above the base row";

/// The walls member's positive `height=`, or `None` after the `E_INVALID_MEMBER` refusing it.
pub(super) fn wall_height(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<u32> {
    if let Some(h) = height_value(member) {
        return Some(h);
    }
    let primary = match member.intent_state.get("height") {
        Some(raw) => format!(
            "walls `height=` must be a positive integer that fits in u32, not {}",
            raw.value.describe(),
        ),
        None => "walls has no `height=`, so nothing says how many rows it rises".to_owned(),
    };
    diagnostics.push(diag_invalid_member(
        member,
        primary,
        vec![WALLS_HEIGHT_FORM.to_owned()],
    ));
    None
}
