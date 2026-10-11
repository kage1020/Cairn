//! Where a `connect` row's ports sit in world space.
//!
//! [`port_world_position`] turns a `(place_origin, def, port_id)`
//! triple into the world-voxel coordinate one block outside the
//! member's `side=` face. Ports are exposed on `door` and `window`
//! members; doors anchor at one of the wall-local
//! `at=center | left | right` positions, windows at the rectangle's
//! geometric centre (`offset + size.w / 2`). A port it cannot place
//! — another role (stair, roof, …), an argument the opening cannot
//! use, masonry the opening does not reach, an opening the openings
//! phase did not cut, a coordinate past `i32` — comes back as the
//! [`PortRejection`] naming that one reason, which the caller turns
//! into the `W_DEFERRED_MEMBER` on the `connect` row. Both roles are
//! openings, so the caller hands over the [`WallColumn`] the body
//! was lowered with: a port is somewhere a wall was opened, and
//! asking the `def` a second time is what let the strip and the cut
//! disagree.

use std::collections::HashSet;
use std::hash::BuildHasher;

use crate::check::DiagnosticNote;
use crate::error::Span;
use crate::ids::PortId;
use crate::intent::{DefIr, Member, MemberRole};

use super::Dims;
use super::openings::{
    WallSide, WindowArgFault, WindowArgs, Written, door_anchor_offset, door_at_clause,
    read_window_args, wall_length, wall_local_to_grid,
};
use super::wall_column::WallColumn;

/// Wall-local `v` coordinate where a port anchors. Walkways are flat
/// 1-voxel-thick strips at the placement's ground row, so every port —
/// door or window — pins to `v = 0` regardless of the member's
/// authored `y=`. Named so a future port surface (multi-level walkway,
/// raised window catwalk) shows up as an intentional re-bind here
/// rather than a stray `0` literal at the call site.
const PORT_GROUND_V: u32 = 0;

/// World row a door port's doorway opens at: one above the base plane,
/// which the floor slab owns.
///
/// A port names a member of the `def` body, so no `level y=N` has
/// shifted it and the row is `super::lower::door::carve_door`'s `y_offset + 1`
/// with `y_offset = 0`. That is an invariant of port *resolution*, not of
/// this module: a door nested under a `level` is refused as a port
/// outright, which `tests/door_wall_fit.rs` pins, because a lookup that
/// walked the flattened members would resolve one and leave this
/// constant disagreeing with the row `carve_door` asks about. It is
/// where the masonry has to be, not where the strip lands — that is
/// [`PORT_GROUND_V`], one row below.
const DOOR_PORT_BASE_V: u32 = 1;

/// Why [`port_world_position`] placed no port — one variant per refusal,
/// in the order the function asks, so the `connect` row can say which one
/// it hit instead of listing every contract a port has.
///
/// Every variant that names a member carries its span, so the note on the
/// `connect` row can point at the line the author has to change. For the
/// side, argument and masonry variants that line also carries the
/// member's own `W_DEFERRED_MEMBER` — the opening was not cut either — and
/// the note says so rather than restating it.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum PortRejection {
    /// No member of the `def` body carries the port's `id=`. The resolver
    /// refuses such a row with `E_UNRESOLVED_PORT` before lowering, so a
    /// `connect` does not reach this; the variant exists so the function
    /// answers every input.
    UnknownMember,
    /// The member's role is not one a port can anchor on. Only `door` and
    /// `window` are openings; stair and roof ports are reserved.
    ReservedRole { role: String, member: Span },
    /// `side=` is missing or does not name a cardinal wall.
    Side { written: Written, member: Span },
    /// The `def` has no `size=`, so its walls have no length. A placement
    /// of such a def is refused before walkways are laid, so a `connect`
    /// does not reach this either.
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
    /// `sym=`) deferred it, or its `mat_slot=` resolved to no block. The
    /// port is refused because a strip that ends at an opening nobody cut
    /// ends at a wall.
    NotCut { role: &'static str, member: Span },
    /// The port's world coordinate does not fit an `i32`: a `place` far
    /// enough from the origin that one more step overflows.
    OutOfRange,
}

impl PortRejection {
    /// The note the `connect` row carries for the endpoint `port`
    /// (`place.port`), pointing at the member when there is one.
    ///
    /// The wording lives here, next to the checks, and the two reasons a
    /// member's own line can also give — a door's `at=` and a window's
    /// arguments — come from the same clause that line is built from.
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
    /// One match for both so the two answers for a variant sit on one
    /// line: they are not the same split. A reserved role has a line but
    /// no finding on it that is about the port — a `stair` lowers clean,
    /// and what a `roof` or an unknown `kind=` gets there is about the
    /// member, not the port — and a refusal the openings pass made for a
    /// reason of its own ([`Self::NotCut`]) says where to look itself.
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
                // The parenthetical is about the two roles it names, so
                // only they get it: said after `floor` it reads as though
                // floor ports were what is reserved.
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

/// World-space `(x, y, z)` coordinate one block outside the named
/// port's wall, at the placement's ground row (`place_origin.1`).
///
/// `place_dims` carries the full inflated placement extents (interior
/// plus roof overhang on each side) so the helper can shift the
/// member's wall-local coordinate into the right world cell — the
/// building walls sit at `origin + overhang`, not at `origin`, when a
/// roof `overhang=` inflates the bounding box. The
/// `(dims.x - interior_w) / 2` derivation is the inverse of the
/// inflation [`super::lower`] does up front.
///
/// `walls` is the column [`super::lower`] painted this body against,
/// handed over rather than re-derived: a port is only where the openings
/// phase could cut one, and that phase reads the rows the `walls` members
/// *painted*, which a walk over the `def` cannot see.
///
/// Ports anchor on [`MemberRole::Door`] (wall-local `u` from `at=`, see
/// [`door_anchor_offset`]) and [`MemberRole::Window`] (`u` at the
/// rectangle's centre, `offset + size.w / 2`; `sym=` and `y=` do not
/// move it, since the strip is flat and one-voxel thick). Both sit on
/// the ground row.
///
/// `cut` holds the spans of the doors and windows the openings pass
/// actually cut into this body. The checks above answer *why* a port
/// cannot be placed for every rule the port reads itself; `cut` is what
/// makes "a port is somewhere a wall was opened" hold for the rules it
/// does not — `repeat=`, `step=` and `sym=`, and a `mat_slot=` that
/// resolves to no block — so a rule the cut grows later refuses the port
/// too without being restated here.
///
/// # Errors
///
/// A [`PortRejection`] naming the first refusal. The member's role is
/// asked first — a stair is refused for being a stair, whatever else is
/// wrong with it — then `side=`, then whether the `def` has a `size=`,
/// then the questions the openings pass asks of the same member in the
/// order it asks them: the door's `at=` and masonry, or the window's
/// `offset=` / `y=` / `size=`, horizontal fit and masonry. For those, the
/// reason the `connect` row gives is the one the member's own line gives.
/// Then whether the openings pass cut it at all ([`PortRejection::NotCut`]),
/// and last whether the world coordinate fits.
///
/// The horizontal fit here is `offset + size.w`; the cut's is the span of
/// every `repeat=` stamp. A window the two disagree on is one the cut
/// deferred, and `cut` refuses it.
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
    // The role first: a stair with no `side=` is refused for being a
    // stair, which is the finding, rather than for the argument.
    match member.role {
        MemberRole::Door | MemberRole::Window => {}
        // Stair / roof ports are reserved for a future extension.
        // Exhaustive match (no `_ =>`) so adding a new `MemberRole`
        // variant trips the non-exhaustive-patterns check instead of
        // silently being treated as "not a port".
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
    // Overhang inflates symmetrically on each horizontal axis, so x and
    // z agree; `.max()` is the conservative pick if a future divergence
    // sneaks in — it keeps the port outside the larger eave rather than
    // averaging into a half-inside coordinate.
    let overhang_x = place_dims.x.saturating_sub(interior_w) / 2;
    let overhang_z = place_dims.z.saturating_sub(interior_h) / 2;
    let overhang = overhang_x.max(overhang_z);
    let len = wall_length(side, interior_w, interior_h);
    let (wall_x, wall_z) = if matches!(member.role, MemberRole::Door) {
        // `at=` before the masonry, the order `super::lower::door::carve_door`
        // asks in, so an `at=` typo on a body whose walls are also wrong
        // is reported as the typo here as it is on the door's own line.
        let u = door_anchor_offset(member, len).map_err(|written| PortRejection::DoorAt {
            written,
            member: member.span.clone(),
        })?;
        // A doorway is a hole in a wall, so a row no `walls` member
        // paints has no doorway for a strip to arrive at:
        // `carve_door` asks this same column the same question before it
        // carves, and defers when the answer is `None`.
        //
        // *Where*, like the window below, rather than merely *whether*:
        // a def whose walls live only above a `level` has a column that
        // starts above the row a door opens at.
        //
        // The row is `DOOR_PORT_BASE_V` because a port names a member of
        // the def body, which no `level y=N` has shifted — `carve_door`
        // asks after adding its member's level offset.
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
        door_world_xz(side, u, overhang, interior_w, interior_h, place_origin)
            .ok_or(PortRejection::OutOfRange)?
    } else {
        // A window port also has to fit *vertically* inside the
        // masonry — otherwise the cut itself is deferred and the
        // strip leads into a solid wall.
        let u = window_center_offset(member, len, walls)?;
        opening_was_cut(member, "window", cut)?;
        window_world_xz(
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

fn door_world_xz(
    side: WallSide,
    u: u32,
    overhang: u32,
    interior_w: u32,
    interior_h: u32,
    origin: (i32, i32, i32),
) -> Option<(i32, i32)> {
    let u_i = i32::try_from(u).ok()?;
    let w_i = i32::try_from(interior_w).ok()?;
    let h_i = i32::try_from(interior_h).ok()?;
    let o = i32::try_from(overhang).ok()?;
    // Composed with `checked_*`, matching `window_world_xz`; the caller
    // reports a `None` from either as `PortRejection::OutOfRange`.
    // Guarding only the individual conversions left the sum unguarded, so
    // a `place` far enough out — `gap=2147483647` reaches it — panicked in
    // a debug build and wrapped in a release one, sending the router
    // billions of cells the other way.
    let (x, z) = match side {
        WallSide::Front => (
            origin.0.checked_add(o)?.checked_add(u_i)?,
            origin.2.checked_add(o)?.checked_add(h_i)?.checked_sub(1)?,
        ),
        WallSide::Back => (
            origin
                .0
                .checked_add(o)?
                .checked_add(w_i.checked_sub(1)?.checked_sub(u_i)?)?,
            origin.2.checked_add(o)?,
        ),
        WallSide::Left => (
            origin.0.checked_add(o)?,
            origin.2.checked_add(o)?.checked_add(u_i)?,
        ),
        WallSide::Right => (
            origin.0.checked_add(o)?.checked_add(w_i)?.checked_sub(1)?,
            origin
                .2
                .checked_add(o)?
                .checked_add(h_i.checked_sub(1)?.checked_sub(u_i)?)?,
        ),
    };
    Some((x, z))
}

/// Window port wall-local centre offset: `offset + size.w / 2`, with
/// two bounds checks so a window that does not fit the wall is refused
/// with the [`PortRejection`] that says how, rather than producing an
/// out-of-range world coordinate.
///
/// `offset=`, `y=` and `size=` are read by [`read_window_args`], the
/// function `super::lower::window::cut_window` reads them with, so the two
/// accept the same values and refuse the same first one. `repeat=`,
/// `step=` and `sym=` are not read: a window they defer is refused by the
/// caller's `cut` check instead.
///
/// Horizontal bound: `offset + size.w ≤ wall_length`. The equality
/// case (`==`) is intentionally accepted — a window whose right edge
/// touches the wall's right corner still fits. This is the cut's bound
/// for a single stamp; the cut also bounds every `repeat=` stamp, which
/// this does not.
///
/// Vertical bound: every row of the rectangle, `y ..= y + size.h - 1`,
/// lies inside one course of the def's [`WallColumn`]. A `walls
/// height=H` fills the world rows `1 ..= H` — the floor slab owns row
/// `0` — so a window flush with the top course (`y + size.h == H + 1`)
/// is inside the wall and one starting on the ground plane (`y == 0`)
/// is not. This is [`WallColumn::contains_rows`], the predicate
/// [`super::lower`] cuts the window with, called on the column that pass
/// builds.
///
/// Horizontal before vertical, the order `cut_window` asks in, so a
/// window wrong both ways is refused for the reason its own line gives.
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

/// Window-side variant of [`door_world_xz`]. Delegates to
/// [`wall_local_to_grid`] so the wall-local → grid mapping is shared
/// with the openings carved into the wall itself (`block_array::lower`
/// uses the same helper for the window cut). `v = PORT_GROUND_V` pins
/// the port to the ground row regardless of the window's authored
/// `y=`.
///
/// `None` means the world coordinate overflowed: [`window_center_offset`]
/// already put `u` inside the wall (`offset + size.w / 2 < offset + size.w
/// ≤ wall_length`, with `size.w ≥ 1`), so the helper's own refusal is not
/// one a caller can reach, and the caller reports it as
/// [`PortRejection::OutOfRange`].
fn window_world_xz(
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
