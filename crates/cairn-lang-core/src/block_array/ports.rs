//! Where a `connect` row's ports sit in world space.
//!
//! [`port_world_position`] turns a `(place_origin, def, port_id)` triple
//! into the world voxel one block outside the member's `side=` face. Only
//! `door` and `window` members carry ports: a door anchors at its
//! `at=center | left | right`, a window at its rectangle's centre
//! (`offset + size.w / 2`). A port it cannot place comes back as the one
//! [`PortRejection`] that says why, which the caller turns into the
//! `connect` row's `W_DEFERRED_MEMBER`.
//!
//! The caller hands over the [`WallColumn`] the body was lowered with
//! rather than this module deriving it again from the `def`: a port is
//! somewhere a wall was opened, and a second derivation can disagree with
//! the cut.

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

/// Wall-local `v` every port anchors at, whatever the member's `y=`:
/// walkways are flat one-voxel strips on the placement's ground row.
const PORT_GROUND_V: u32 = 0;

/// World row a door port's doorway opens at: one above the base plane,
/// which the floor slab owns. It is where the masonry has to be; the strip
/// lands on [`PORT_GROUND_V`], one row below.
///
/// `super::lower::door::carve_door` asks about `y_offset + 1`, which is
/// this only because port resolution refuses a door nested under a
/// `level` (pinned in `tests/lower/door_wall_fit.rs`). A port lookup over
/// the flattened members would make the two disagree.
const DOOR_PORT_BASE_V: u32 = 1;

/// Why [`port_world_position`] placed no port: one variant per refusal, in
/// the order it asks, so the `connect` row names the one it hit rather than
/// every contract a port has.
///
/// A variant naming a member carries its span for the note to point at.
/// For the side, argument and masonry variants that line also has its own
/// `W_DEFERRED_MEMBER`, since the opening was not cut either.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PortRejection {
    /// No member of the `def` body carries the port's `id=`. A `connect`
    /// never reaches this, because the resolver refuses it first
    /// (`E_UNRESOLVED_PORT`), but the function answers every input.
    UnknownMember,
    /// The member's role is not one a port can anchor on. Only `door` and
    /// `window` are openings; stair and roof ports are reserved.
    ReservedRole { role: String, member: Span },
    /// `side=` is missing or does not name a cardinal wall.
    Side { written: Written, member: Span },
    /// The `def` has no `size=`, so its walls have no length. Unreachable
    /// from a `connect` too: such a placement is refused before walkways
    /// are laid.
    Sizeless,
    /// A door's `at=` is missing or is not `center | left | right`.
    DoorAt { written: Written, member: Span },
    /// No `walls` member of the body paints, so a door has nothing to
    /// open.
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
    /// No `walls` member of the body paints, so a window has nothing to
    /// be cut into.
    WindowNoWalls { member: Span },
    /// The walls paint, but not every row of the window.
    WindowOutsideCourse {
        y: u32,
        height: u32,
        walls: WallColumn,
        member: Span,
    },
    /// Every rule above held and the openings pass still did not cut the
    /// opening: a window argument only the cut reads (`repeat=`, `step=`,
    /// `sym=`) deferred it, or its `mat_slot=` resolved to no block. A
    /// strip to it would end at a wall.
    NotCut { role: &'static str, member: Span },
    /// The port's world coordinate does not fit an `i32`: a `place` far
    /// enough from the origin that one more step overflows.
    OutOfRange,
}

impl PortRejection {
    /// The note the `connect` row carries for the endpoint `port`
    /// (`place.port`), pointing at the member when there is one.
    ///
    /// A door's `at=` and a window's arguments are worded by the same
    /// clause as the member's own line.
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

    /// The member the refusal is about, when it is about one, and whether
    /// that member's own line carries a deferral for the same fault.
    ///
    /// The two answers split differently: a reserved role has a line but
    /// no finding there about the port (a `stair` lowers clean), and
    /// [`Self::NotCut`] says where to look itself.
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

    /// What is wrong, as the predicate of a sentence whose subject is the
    /// port.
    fn fault(&self) -> String {
        const NO_WALLS: &str = "its `def` has no `walls` that paints — a positive `height=` and a `mat_slot=` that resolves";
        match self {
            Self::UnknownMember => "names no member of its `def` body".to_owned(),
            Self::ReservedRole { role, .. } => {
                // Only the reserved roles: after `floor` the parenthetical
                // would read as though floor ports were reserved.
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
                // Only a window paints a material; a door is carved to
                // air, so its line is the one place a reason can be.
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

/// World-space `(x, y, z)` one block outside the named port's wall, on
/// the placement's ground row (`place_origin.1`).
///
/// `place_dims` are the placement's extents inflated by the roof
/// overhang, so the walls sit at `origin + overhang`;
/// `(dims.x - interior_w) / 2` inverts the inflation [`super::lower`]
/// applies.
///
/// `walls` is the column [`super::lower`] painted this body against: the
/// openings phase reads the rows the `walls` members *painted*, which a
/// walk over the `def` cannot see.
///
/// A door's `u` comes from `at=` ([`door_anchor_offset`]); a window's is
/// its rectangle's centre, `offset + size.w / 2`, which `sym=` and `y=` do
/// not move.
///
/// `cut` holds the spans of the doors and windows the openings pass cut.
/// The checks here say *why* for every rule the port reads itself; `cut`
/// covers the rest (`repeat=`, `step=`, `sym=`, a `mat_slot=` that
/// resolves to no block), including any rule the cut gains later.
///
/// # Errors
///
/// A [`PortRejection`] naming the first refusal, asked in this order: the
/// role (a stair is refused for being a stair, whatever else is wrong),
/// `side=`, the `def`'s `size=`, then the openings pass's own questions in
/// its order, so the `connect` row gives the reason the member's line
/// gives: the door's `at=` and masonry, or the window's `offset=`, `y=`,
/// `size=`, horizontal fit and masonry. Then [`PortRejection::NotCut`],
/// and last whether the world coordinate fits.
///
/// The horizontal fit here is `offset + size.w`; the cut's spans every
/// `repeat=` stamp. A window the two disagree on was deferred by the cut,
/// and `cut` refuses it.
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
    // Overhang inflates both axes alike; `max` keeps the port outside the
    // larger eave should they ever differ.
    let overhang_x = place_dims.x.saturating_sub(interior_w) / 2;
    let overhang_z = place_dims.z.saturating_sub(interior_h) / 2;
    let overhang = overhang_x.max(overhang_z);
    let len = wall_length(side, interior_w, interior_h);
    let (wall_x, wall_z) = if matches!(member.role, MemberRole::Door) {
        let u = door_anchor_offset(member, len).map_err(|written| PortRejection::DoorAt {
            written,
            member: member.span.clone(),
        })?;
        // The question `carve_door` asks of this column: *where* the walls
        // are, not merely whether, since walls only above a `level` start
        // above the doorway's row.
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

/// A window port's wall-local `u`, its rectangle's centre
/// `offset + size.w / 2`, or the [`PortRejection`] saying how the window
/// misses its wall.
///
/// The arguments come from [`read_window_args`], as the cut's do
/// (`super::lower::window::cut_window`). The bounds follow in the cut's
/// order, so a window wrong both ways gets the reason its own line gives:
///
/// - Horizontal: `offset + size.w ≤ wall_length`, so a window may end
///   flush with the corner. The cut also bounds every `repeat=` stamp,
///   which this does not.
/// - Vertical: every row lies in one course of `wall_column`
///   ([`WallColumn::contains_rows`], the cut's predicate). A
///   `walls height=H` fills rows `1 ..= H`, so a window flush with its top
///   (`y + size.h == H + 1`) fits and one at `y=0`, on the floor slab,
///   does not.
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

/// The world `(x, z)` of wall-local `u` on the ground row, through
/// [`wall_local_to_grid`] so ports and the openings pass share one mapping.
///
/// `None` means the world coordinate overflowed: [`door_anchor_offset`] and
/// [`window_center_offset`] already put `u` inside the wall, so
/// [`wall_local_to_grid`]'s own `None` is unreachable.
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
