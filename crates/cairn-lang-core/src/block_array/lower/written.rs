//! The struct as written: every member at the size its line asks for, whether or not it builds
//! (`spec/lint` "Lowering").

use crate::ast::ValueKind;
use crate::intent::{Member, MemberRole};

use crate::block_array::wall_column::WallColumn;

use super::roof_kind::{roof_rise, tallest_rise};

/// Every member a body writes, each with its level's `y` offset, or `None` under a `level` that is
/// not placed: one with no readable `y=`, or one inside another `level`.
pub(super) fn written_members(members: &[Member]) -> Vec<(Option<u32>, &Member)> {
    let mut out = Vec::new();
    for member in members {
        if matches!(member.role, MemberRole::Level) {
            under_level(&member.children.members, member.nonneg_u32("y"), &mut out);
        } else {
            out.push((Some(0), member));
        }
    }
    out
}

/// The members under a level placed at `y`, a nested level placing none of its own.
fn under_level<'a>(
    members: &'a [Member],
    y: Option<u32>,
    out: &mut Vec<(Option<u32>, &'a Member)>,
) {
    for member in members {
        if matches!(member.role, MemberRole::Level) {
            under_level(&member.children.members, None, out);
        } else {
            out.push((y, member));
        }
    }
}

/// What a body's walls and roofs ask for as written, whether or not they paint or draw.
#[derive(Debug)]
pub(super) struct Written {
    /// Every row the placed `walls` members are written to.
    pub(super) walls: WallColumn,
    /// The highest of those rows, `0` with none.
    pub(super) wall_top: u32,
    /// The tallest rise of a roof whose line says its shape.
    pub(super) rise: u32,
    /// The `y` extent of the body with every placed `walls` member painted and every roof drawn.
    pub(super) height: u32,
    /// The `y` extent [`Self::height`] could grow to once a roof whose line does not say its shape
    /// draws.
    pub(super) reach: u32,
    /// The largest `overhang=` a roof is written with; an unreadable one counts as none, the
    /// default its `W_IGNORED_ARGUMENT` puts in its place.
    pub(super) overhang: u32,
    /// Whether a `walls` member is written with no positive `height=`.
    pub(super) unsized_walls: bool,
    /// Whether a `walls` member sits under a `level` that is not placed.
    pub(super) unplaced_walls: bool,
    /// Whether a `floor` is written on row 0.
    pub(super) floor: bool,
}

impl Written {
    /// The struct `members` write on an `interior_w` by `interior_h` footprint.
    pub(super) fn of(members: &[(Option<u32>, &Member)], interior_w: u32, interior_h: u32) -> Self {
        let walls = || {
            members
                .iter()
                .filter(|(_, m)| matches!(m.role, MemberRole::Walls))
        };
        let roofs = || {
            members
                .iter()
                .map(|(_, m)| *m)
                .filter(|m| matches!(m.role, MemberRole::Roof))
        };
        let placed: Vec<(u32, u32)> = walls()
            .filter_map(|(y_offset, m)| y_offset.zip(height_value(m)))
            .collect();
        let wall_top = placed
            .iter()
            .map(|(y_offset, h)| y_offset.saturating_add(*h))
            .max()
            .unwrap_or(0);
        let overhang = roofs()
            .filter_map(|m| m.nonneg_u32("overhang"))
            .max()
            .unwrap_or(0);
        let roof_w = interior_w.saturating_add(overhang.saturating_mul(2));
        let roof_h = interior_h.saturating_add(overhang.saturating_mul(2));
        let rises: Vec<Option<u32>> = roofs().map(|m| roof_rise(m, roof_w, roof_h)).collect();
        let rise = rises.iter().flatten().copied().max().unwrap_or(0);
        let reach_rise = if rises.contains(&None) {
            rise.max(tallest_rise(roof_w, roof_h))
        } else {
            rise
        };
        let height_with = |rise: u32| 1u32.saturating_add(wall_top).saturating_add(rise);
        Self {
            walls: WallColumn::from_walls(placed),
            wall_top,
            rise,
            height: height_with(rise),
            reach: height_with(reach_rise),
            overhang,
            unsized_walls: walls().any(|(_, m)| height_value(m).is_none()),
            unplaced_walls: walls()
                .any(|(y_offset, m)| y_offset.is_none() && height_value(m).is_some()),
            floor: members
                .iter()
                .any(|(y_offset, m)| *y_offset == Some(0) && matches!(m.role, MemberRole::Floor)),
        }
    }
}

/// A positive `height=` that fits `u32`, else `None` rather than a saturated height.
pub(super) fn height_value(member: &Member) -> Option<u32> {
    let raw = member.intent_state.get("height")?;
    match &raw.value.kind {
        ValueKind::Int(v) if *v > 0 => u32::try_from(*v).ok(),
        _ => None,
    }
}
