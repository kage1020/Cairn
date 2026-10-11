//! Where a `connect` row's ports sit in world space, or why one cannot be placed.
//!
//! Ports take the [`WallColumn`] the body was lowered against, so they cannot disagree with the
//! cut.

use std::collections::HashSet;
use std::hash::BuildHasher;

use crate::diagnostic::DiagnosticNote;
use crate::error::Span;
use crate::ids::PortId;
use crate::intent::{DefIr, Member, MemberRole};

use super::Dims;
use super::openings::{
    WallSide, WindowArgFault, WindowArgs, Written, door_anchor_offset, door_at_clause,
    read_window_args, wall_length, wall_local_to_grid,
};
use super::wall_column::WallColumn;

/// Wall-local `v` every port anchors at, whatever the member's `y=`.
const PORT_GROUND_V: u32 = 0;

/// World row a door port's doorway opens at: one above the floor slab.
///
/// `carve_door` asks about `y_offset + 1`; the two agree because no port reaches a door under a
/// `level`.
const DOOR_PORT_BASE_V: u32 = 1;

/// Why [`port_world_position`] placed no port, one variant per refusal in the order it asks.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PortRejection {
    /// No member of the `def` body carries the port's `id=`.
    UnknownMember,
    /// The member's role is not `door` or `window`.
    ReservedRole { role: String, member: Span },
    /// `side=` is missing or does not name a cardinal wall.
    Side { written: Written, member: Span },
    /// The `def` has no `size=`, so its walls have no length.
    Sizeless,
    /// A door's `at=` is missing or is not `center | left | right`.
    DoorAt { written: Written, member: Span },
    /// No `walls` member of the body paints.
    DoorNoWalls { member: Span },
    /// The walls paint, but not the row a door opens at.
    DoorOutsideCourse { walls: WallColumn, member: Span },
    /// A window's `offset=` / `y=` / `size=` cannot be used.
    WindowArgument { fault: WindowArgFault, member: Span },
    /// A window's rectangle runs past the far end of its wall.
    WindowPastWall {
        offset: u32,
        width: u32,
        wall_length: u32,
        member: Span,
    },
    /// No `walls` member of the body paints.
    WindowNoWalls { member: Span },
    /// The walls paint, but not every row of the window.
    WindowOutsideCourse {
        y: u32,
        height: u32,
        walls: WallColumn,
        member: Span,
    },
    /// Every rule above held and the openings pass still did not cut the opening.
    NotCut { role: &'static str, member: Span },
    /// The port's world coordinate does not fit an `i32`.
    OutOfRange,
}

impl PortRejection {
    /// The note the `connect` row carries for the endpoint `port` (`place.port`).
    pub(super) fn note(&self, port: &str) -> DiagnosticNote {
        let (member, own_line) = self.anchor();
        let own_line = if own_line {
            "; the member says so on its own line too"
        } else {
            ""
        };
        DiagnosticNote {
            span: member.cloned(),
            message: format!("`{port}` {}{own_line}", self.fault()),
        }
    }

    /// The member the refusal is about, and whether its own line reports the same fault.
    fn anchor(&self) -> (Option<&Span>, bool) {
        match self {
            Self::Side { member, .. }
            | Self::DoorAt { member, .. }
            | Self::DoorNoWalls { member }
            | Self::DoorOutsideCourse { member, .. }
            | Self::WindowArgument { member, .. }
            | Self::WindowPastWall { member, .. }
            | Self::WindowNoWalls { member }
            | Self::WindowOutsideCourse { member, .. } => (Some(member), true),
            Self::ReservedRole { member, .. } | Self::NotCut { member, .. } => {
                (Some(member), false)
            }
            Self::UnknownMember | Self::Sizeless | Self::OutOfRange => (None, false),
        }
    }

    /// What is wrong, as the predicate of a sentence whose subject is the port.
    fn fault(&self) -> String {
        const NO_WALLS: &str = "its `def` has no `walls` that paints — a positive `height=` and a `mat_slot=` that resolves";
        match self {
            Self::UnknownMember => "names no member of its `def` body".to_owned(),
            Self::ReservedRole { role, .. } => {
                // After `floor` the parenthetical would read as though floor ports were reserved.
                let reserved = if role == "stair" || role == "roof" {
                    format!(" (`{role}` ports are reserved)")
                } else {
                    String::new()
                };
                format!(
                    "is a `{role}`, and only a `door` or a `window` can anchor a port{reserved} \
                     — declare the port on a door or window instead",
                )
            }
            Self::Side { written, .. } => match written {
                Written::Absent => {
                    "has no `side=` (expected one of front, back, left, right)".to_owned()
                }
                Written::Ident(s) => {
                    format!("has `side={s}`, which is not one of front, back, left, right")
                }
                Written::OtherShape => {
                    "has a `side=` that is not one of front, back, left, right".to_owned()
                }
            },
            Self::Sizeless => {
                "belongs to a `def` with no `size=`, so its walls have no length".to_owned()
            }
            Self::DoorAt { written, .. } => format!("is a door that {}", door_at_clause(written)),
            Self::DoorNoWalls { .. } => format!("is a door with no wall to open: {NO_WALLS}"),
            Self::DoorOutsideCourse { walls, .. } => format!(
                "is a door that opens at y={DOOR_PORT_BASE_V}, the row above the floor slab, which \
                 is not inside any wall course (the walls occupy {walls})",
            ),
            Self::WindowArgument { fault, .. } => format!("is a window that {}", fault.clause()),
            Self::WindowPastWall {
                offset,
                width,
                wall_length,
                ..
            } => format!(
                "is a window that runs past the end of its wall (`offset + size.w` = {offset} + \
                 {width}, wall length {wall_length})",
            ),
            Self::WindowNoWalls { .. } => {
                format!("is a window with no wall to cut into: {NO_WALLS}")
            }
            Self::WindowOutsideCourse {
                y, height, walls, ..
            } => match y.checked_add(height.saturating_sub(1)) {
                Some(last) => format!(
                    "is a window whose rows y={y}..={last} are not all inside one wall course \
                     (the walls occupy {walls})",
                ),
                None => format!(
                    "is a window whose {height} rows from y={y} run past the highest row a build \
                     can address (the walls occupy {walls})",
                ),
            },
            Self::NotCut { role, .. } => {
                // A door is carved to air, so only a window has a material to blame.
                let why = if *role == "window" {
                    "the finding on its own line, or on the material its `mat_slot=` names, says why"
                } else {
                    "the finding on its own line says why"
                };
                format!(
                    "is a {role} the openings pass did not cut, so a strip would end against \
                     the wall — {why}",
                )
            }
            Self::OutOfRange => "would sit outside the coordinate range a build can address — \
                                 bring its `place` closer to the origin"
                .to_owned(),
        }
    }
}

/// The world cell one block outside `port_id`'s wall, on the placement's ground row.
///
/// # Errors
///
/// The first [`PortRejection`], in the order the member's own line reports its faults.
pub(super) fn port_world_position<S: BuildHasher>(
    place_origin: (i32, i32, i32),
    place_dims: Dims,
    def: &DefIr,
    port_id: &PortId,
    walls: &WallColumn,
    cut: &HashSet<Span, S>,
) -> Result<(i32, i32, i32), PortRejection> {
    let member = def
        .members
        .iter()
        .find(|m| m.id.as_deref() == Some(port_id.as_str()))
        .ok_or(PortRejection::UnknownMember)?;
    match member.role {
        MemberRole::Door | MemberRole::Window => {}
        // No `_` arm, so a new `MemberRole` has to be classified here.
        MemberRole::Floor
        | MemberRole::Walls
        | MemberRole::Roof
        | MemberRole::Stair
        | MemberRole::Level
        | MemberRole::PressurePlate
        | MemberRole::Circuit
        | MemberRole::Place
        | MemberRole::Connect
        | MemberRole::Other(_) => {
            return Err(PortRejection::ReservedRole {
                role: member.role.keyword().to_owned(),
                member: member.span.clone(),
            });
        }
    }
    let side = member
        .ident_value("side")
        .and_then(WallSide::from_ident)
        .ok_or_else(|| PortRejection::Side {
            written: Written::of_ident(member, "side"),
            member: member.span.clone(),
        })?;
    let def_size = def.size.as_ref().ok_or(PortRejection::Sizeless)?;
    let interior_w = def_size.w.get();
    let interior_h = def_size.h.get();
    let overhang_x = place_dims.x.saturating_sub(interior_w) / 2;
    let overhang_z = place_dims.z.saturating_sub(interior_h) / 2;
    let overhang = overhang_x.max(overhang_z);
    let len = wall_length(side, interior_w, interior_h);
    let (wall_x, wall_z) = if matches!(member.role, MemberRole::Door) {
        let u = door_anchor_offset(member, len).map_err(|written| PortRejection::DoorAt {
            written,
            member: member.span.clone(),
        })?;
        if walls.is_empty() {
            return Err(PortRejection::DoorNoWalls {
                member: member.span.clone(),
            });
        }
        if walls.course_top_at(DOOR_PORT_BASE_V).is_none() {
            return Err(PortRejection::DoorOutsideCourse {
                walls: walls.clone(),
                member: member.span.clone(),
            });
        }
        opening_was_cut(member, "door", cut)?;
        wall_world_xz(
            side,
            u,
            overhang,
            interior_w,
            interior_h,
            place_dims,
            place_origin,
        )
        .ok_or(PortRejection::OutOfRange)?
    } else {
        let u = window_center_offset(member, len, walls)?;
        opening_was_cut(member, "window", cut)?;
        wall_world_xz(
            side,
            u,
            overhang,
            interior_w,
            interior_h,
            place_dims,
            place_origin,
        )
        .ok_or(PortRejection::OutOfRange)?
    };
    let (nx, nz) = side.outward_normal();
    let x = wall_x.checked_add(nx).ok_or(PortRejection::OutOfRange)?;
    let z = wall_z.checked_add(nz).ok_or(PortRejection::OutOfRange)?;
    Ok((x, place_origin.1, z))
}

/// Refuse a port on an opening the openings pass did not cut.
fn opening_was_cut<S: BuildHasher>(
    member: &Member,
    role: &'static str,
    cut: &HashSet<Span, S>,
) -> Result<(), PortRejection> {
    if cut.contains(&member.span) {
        Ok(())
    } else {
        Err(PortRejection::NotCut {
            role,
            member: member.span.clone(),
        })
    }
}

/// A window port's wall-local `u`, its rectangle's centre `offset + size.w / 2`.
///
/// Bounds are checked in the cut's order, so a window wrong both ways gets its own line's reason.
fn window_center_offset(
    member: &Member,
    len: u32,
    wall_column: &WallColumn,
) -> Result<u32, PortRejection> {
    let WindowArgs {
        offset,
        y,
        width: sw,
        height: sh,
    } = read_window_args(member).map_err(|fault| PortRejection::WindowArgument {
        fault,
        member: member.span.clone(),
    })?;
    match offset.checked_add(sw) {
        Some(end) if end <= len => {}
        _ => {
            return Err(PortRejection::WindowPastWall {
                offset,
                width: sw,
                wall_length: len,
                member: member.span.clone(),
            });
        }
    }
    if wall_column.is_empty() {
        return Err(PortRejection::WindowNoWalls {
            member: member.span.clone(),
        });
    }
    if !wall_column.contains_rows(y, sh) {
        return Err(PortRejection::WindowOutsideCourse {
            y,
            height: sh,
            walls: wall_column.clone(),
            member: member.span.clone(),
        });
    }
    Ok(offset + sw / 2)
}

/// The world `(x, z)` of wall-local `u` on the ground row, or `None` on overflow.
fn wall_world_xz(
    side: WallSide,
    u: u32,
    overhang: u32,
    interior_w: u32,
    interior_h: u32,
    place_dims: Dims,
    origin: (i32, i32, i32),
) -> Option<(i32, i32)> {
    let (grid_x, _, grid_z) = wall_local_to_grid(
        side,
        u,
        PORT_GROUND_V,
        overhang,
        interior_w,
        interior_h,
        place_dims,
    )?;
    let grid_x = i32::try_from(grid_x).ok()?;
    let grid_z = i32::try_from(grid_z).ok()?;
    Some((origin.0.checked_add(grid_x)?, origin.2.checked_add(grid_z)?))
}

#[cfg(test)]
mod tests;
