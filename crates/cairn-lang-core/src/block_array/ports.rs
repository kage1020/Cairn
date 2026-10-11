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
/// shifted it and the row is `super::lower::carve_door`'s `y_offset + 1`
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
        // `at=` before the masonry, the order `super::lower::carve_door`
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
/// function `super::lower::cut_window` reads them with, so the two
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
mod tests {
    use super::*;
    use crate::block_array::openings::door_at_deferral;

    fn pid(name: &str) -> PortId {
        PortId::new(name).expect("valid port id")
    }

    /// [`port_world_position`] on a body whose every door and window was
    /// cut — the cases here are about the port's own rules, so the
    /// openings phase's answer is taken as yes. The cases about that
    /// answer call the function directly.
    fn port_at(
        origin: (i32, i32, i32),
        dims: Dims,
        def: &DefIr,
        id: &PortId,
        walls: &WallColumn,
    ) -> Result<(i32, i32, i32), PortRejection> {
        let cut: HashSet<Span> = def
            .members
            .iter()
            .filter(|m| matches!(m.role, MemberRole::Door | MemberRole::Window))
            .map(|m| m.span.clone())
            .collect();
        port_world_position(origin, dims, def, id, walls, &cut)
    }

    /// The column the fixtures below declare — a single span reaching
    /// the tallest declared `height=`, since every top-level `walls`
    /// starts at row 1 and `WallColumn::from_walls` merges runs that
    /// touch.
    ///
    /// The cases here are about coordinates, so they hand the port the
    /// wall their own source spells. What a body actually paints — which
    /// materials resolved, which walls a `level` block carries — is
    /// [`super::super::lower`]'s answer, and the tests that pin the port
    /// against *that* drive the whole pass
    /// (`tests/port_reads_the_wall_that_paints.rs`).
    fn declared_column(def: &DefIr) -> WallColumn {
        WallColumn::from_walls(
            def.members
                .iter()
                .filter(|m| matches!(m.role, MemberRole::Walls))
                .filter_map(|m| m.nonneg_u32("height").map(|h| (0, h))),
        )
    }

    #[test]
    fn port_world_position_offsets_one_block_outside_front_door() {
        // size=3x3 interior, no overhang inflation (place dims match
        // interior). center_u = wall_length / 2 = 3 / 2 = 1; door wall
        // world at (10 + 1, 0, 20 + 3 - 1) = (11, 0, 22); +1 in +z
        // direction → (11, 0, 23).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  door id=entry side=front at=center\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (11, 0, 23));
    }

    #[test]
    fn port_world_position_shifts_outward_past_roof_overhang() {
        // size=3x3 with a `+1` overhang on every horizontal side → place
        // dims (5, _, 5). Front wall world: (origin.x + overhang + u,
        // origin.z + overhang + interior_h - 1) = (10 + 1 + 1,
        // 20 + 1 + 3 - 1) = (12, 23); +1 in the +z direction puts the
        // port one block beyond the eave → (12, 0, 24).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  door id=entry side=front at=center\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 5, y: 1, z: 5 };
        let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (12, 0, 24));
    }

    #[test]
    fn port_world_position_back_side_steps_into_negative_z() {
        // size=3x3, overhang=0, center u=1. Back wall world:
        // x = origin.x + (w-1-u) = 10 + (3-1-1) = 11, z = origin.z = 20.
        // Normal step is (0, -1) → port (11, 0, 19).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  door id=entry side=back at=center\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (11, 0, 19));
    }

    #[test]
    fn port_world_position_left_side_steps_into_negative_x() {
        // size=3x3, overhang=0, center u=1. Left wall world:
        // x = origin.x = 10, z = origin.z + u = 20 + 1 = 21.
        // Normal step is (-1, 0) → port (9, 0, 21).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  door id=entry side=left at=center\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (9, 0, 21));
    }

    #[test]
    fn port_world_position_right_side_steps_into_positive_x() {
        // size=3x3, overhang=0, center u=1. Right wall world:
        // x = origin.x + (w-1) = 12, z = origin.z + (h-1-u) = 20 + 1 = 21.
        // Normal step is (+1, 0) → port (13, 0, 21).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  door id=entry side=right at=center\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (13, 0, 21));
    }

    #[test]
    fn port_world_position_window_front_resolves_to_offset_center() {
        // size=3x3, no overhang, window offset=0 size=1x1 on front wall.
        // wall_length(Front, 3, 3) = 3; u = 0 + 1/2 = 0. Wall world via
        // wall_local_to_grid: (origin.x + 0, origin.z + 3 - 1) = (10, 22);
        // +1 in the +z normal step → port (10, 0, 23).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=front y=1 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (10, 0, 23));
    }

    #[test]
    fn port_world_position_window_back_resolves_to_mirrored_center() {
        // size=3x3, window offset=1 size=1x1 on back wall. wall_length = 3;
        // u = 1 + 0 = 1. Back wall world: mirrored = 3 - 1 - 1 = 1,
        // (origin.x + 1, origin.z + 0) = (11, 20); -1 in the -z normal
        // step → port (11, 0, 19).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=back y=1 offset=1 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (11, 0, 19));
    }

    #[test]
    fn port_world_position_window_left_resolves_to_offset_center() {
        // size=3x3, window offset=1 size=1x1 on left wall. wall_length
        // (Left, 3, 3) = 3; u = 1. Left wall world: (origin.x + 0,
        // origin.z + 1) = (10, 21); -1 in the -x normal step → port
        // (9, 0, 21).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=left y=1 offset=1 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (9, 0, 21));
    }

    #[test]
    fn port_world_position_window_right_resolves_to_mirrored_center() {
        // size=3x3, window offset=1 size=1x1 on right wall. wall_length
        // (Right, 3, 3) = 3; u = 1. Right wall world: mirrored = 1,
        // x = origin.x + 3 - 1 = 12, z = origin.z + 1 = 21; +1 in the +x
        // normal step → port (13, 0, 21).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=right y=1 offset=1 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (13, 0, 21));
    }

    #[test]
    fn port_world_position_window_shifts_outward_past_roof_overhang() {
        // size=3x3 interior, place_dims=(5,_,5) for overhang=1. Window
        // offset=0 size=1x1 on front. u = 0. Wall world via
        // wall_local_to_grid with overhang=1: (origin.x + 1, origin.z +
        // 1 + 3 - 1) = (11, 23); +1 in the +z normal step → port
        // (11, 0, 24).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=front y=1 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 5, y: 1, z: 5 };
        let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (11, 0, 24));
    }

    #[test]
    fn port_world_position_window_centres_on_2x2_offset() {
        // village.crn shape: size=9x7, place_dims (11,_,9) for
        // overhang=1, window id=front side=front y=2 offset=2 size=2x2.
        // wall_length(Front, 9, 7) = 9; u = 2 + 2/2 = 3. Wall world:
        // (origin.x + 1 + 3, origin.z + 1 + 7 - 1) = (origin.x + 4,
        // origin.z + 7); +1 in the +z normal step → port shifts to z+8.
        // With origin (0,0,0): port (4, 0, 8).
        let src = concat!(
            "def cottage size=9x7:\n",
            "  walls mat_slot=w height=4\n",
            "  window id=front side=front y=2 offset=2 size=2x2 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 11, y: 1, z: 9 };
        let pos = port_at((0, 0, 0), dims, def, &pid("front"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (4, 0, 8));
    }

    #[test]
    fn port_world_position_window_refuses_when_offset_size_overflows_wall() {
        // size=3x3 → wall_length(Front) = 3. offset=2 + size.w=2 = 4 > 3.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=5\n",
            "  window id=light side=front y=1 offset=2 size=2x2 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        assert!(matches!(
            port_at((0, 0, 0), dims, def, &pid("light"), &declared_column(def)),
            Err(PortRejection::WindowPastWall {
                offset: 2,
                width: 2,
                wall_length: 3,
                ..
            }),
        ));
    }

    #[test]
    fn port_world_position_window_sym_true_uses_primary_offset() {
        // `sym=true` mirrors the cut at lowering time but the port is
        // taken from the primary `offset` side only (the rule the spec
        // calls out so a single `id=` always maps to one coordinate).
        // Same geometry as the front-resolves test, just with `sym=true`
        // tacked on; the world position must not move.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=front y=1 offset=0 size=1x1 sym=true mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (10, 0, 23));
    }

    #[test]
    fn port_world_position_window_pins_y_to_ground_row_regardless_of_authored_y() {
        // `y=4` on the window must not lift the port off the ground row
        // — walkways are flat 1-voxel strips and the port Y must agree
        // with the other endpoint (door y=0). The port stays at
        // `place_origin.1`, here = 7. Walls `height=10` so the window
        // still fits vertically (`y + size.h = 5 ≤ 10`) and the
        // resolve / pin separation is the only thing under test.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=10\n",
            "  window id=light side=front y=4 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        // Geometry is identical to the front-resolves test apart from
        // the `place_origin.1` lift, so the full `(x, y, z)` triple is
        // pinned: a regression that honours `window.y` would land the
        // port at `(10, 11, 23)` instead of `(10, 7, 23)`.
        let pos = port_at((10, 7, 20), dims, def, &pid("light"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (10, 7, 23));
    }

    #[test]
    fn port_world_position_refuses_for_roof_role() {
        // Roof ports are reserved; the role guard must short-circuit
        // even when `id=` matches.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  roof id=top kind=gable mat_slot=r\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        assert!(matches!(
            port_at((0, 0, 0), dims, def, &pid("top"), &declared_column(def)),
            Err(PortRejection::ReservedRole { ref role, .. }) if role == "roof",
        ));
    }

    #[test]
    fn port_world_position_refuses_for_stair_role() {
        // Stair ports are reserved; same short-circuit as roof.
        let src = concat!("def cottage size=3x3:\n", "  stair id=up at=corner\n");
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        assert!(matches!(
            port_at((0, 0, 0), dims, def, &pid("up"), &declared_column(def)),
            Err(PortRejection::ReservedRole { ref role, .. }) if role == "stair",
        ));
    }

    #[test]
    fn port_world_position_window_accepts_boundary_offset_plus_size_equal_wall_length() {
        // Pin the *acceptance* edge of the horizontal bound — a window
        // whose right edge touches the wall's right corner
        // (`offset + size.w == wall_length`) must resolve. A regression
        // that tightens the check from `>` to `>=` would only fail
        // this test, not the existing overflow case.
        // size=3x3 → wall_length(Front) = 3. offset=1 + size.w=2 = 3
        // (== wall_length, so accepted). u = 1 + 2/2 = 2. Wall world:
        // (origin.x + 2, origin.z + 3 - 1) = (12, 22); +1 +z → port
        // (12, 0, 23).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=5\n",
            "  window id=light side=front y=1 offset=1 size=2x2 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (12, 0, 23));
    }

    #[test]
    fn port_world_position_window_accepts_a_rectangle_inside_the_wall() {
        // Pin the acceptance side of the *vertical* bound with a rectangle
        // strictly inside the wall (rows 2..=2 of 1..=3), so the
        // flush-with-the-top case is pinned by a test of its own and the
        // two edges cannot be re-pinned together by accident.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=front y=2 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (10, 0, 23));
    }

    #[test]
    fn port_world_position_window_refuses_when_the_top_edge_pierces_the_wall() {
        // The window cut itself would be deferred when its top row is
        // above the wall. Anchoring a walkway to a non-existent cut would
        // leave the user with a strip running into a solid wall, so the
        // port must defer too.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=2\n",
            "  window id=light side=front y=2 offset=0 size=1x2 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        // `walls height=2` paints rows 1..=2; the rectangle wants 2..=3.
        assert!(matches!(
            port_at((0, 0, 0), dims, def, &pid("light"), &declared_column(def)),
            Err(PortRejection::WindowOutsideCourse { y: 2, height: 2, ref walls, .. }) if !walls.is_empty(),
        ));
    }

    #[test]
    fn port_world_position_window_refuses_when_def_has_no_walls() {
        // A `def` without a `walls` member cannot voxelise any window
        // (the openings pass has nothing to carve into). The port must
        // defer for the same reason: anchoring a walkway to a
        // never-voxelised cut would leave the strip running into
        // emptiness.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  window id=light side=front y=0 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        assert!(matches!(
            port_at((0, 0, 0), dims, def, &pid("light"), &declared_column(def)),
            Err(PortRejection::WindowNoWalls { .. }),
        ));
    }

    #[test]
    fn port_world_position_window_back_shifts_outward_past_roof_overhang() {
        // Back wall's `wall_local_to_grid` differs from Front's (it
        // mirrors `u` along `x` and pins `z = overhang`), so an overhang
        // regression on the back side would slip past the Front-only
        // overhang test. size=3x3 interior, place_dims=(5,_,5) for
        // overhang=1, window offset=0 size=1x1 → u = 0; mirrored = 3 -
        // 1 - 0 = 2; wall world (origin.x + 1 + 2, origin.z + 1) =
        // (13, 21); -1 -z normal → port (13, 0, 20).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=back y=1 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 5, y: 1, z: 5 };
        let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (13, 0, 20));
    }

    #[test]
    fn port_world_position_window_right_shifts_outward_past_roof_overhang() {
        // Right wall mirrors `u` along `z` and pins `x = overhang +
        // interior_w - 1`. size=3x3 interior, place_dims=(5,_,5) for
        // overhang=1, window offset=0 size=1x1 → u = 0; mirrored = 3 -
        // 1 - 0 = 2; wall world (origin.x + 1 + 3 - 1, origin.z + 1 +
        // 2) = (13, 23); +1 +x normal → port (14, 0, 23).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  window id=light side=right y=1 offset=0 size=1x1 mat_slot=g\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 5, y: 1, z: 5 };
        let pos = port_at((10, 0, 20), dims, def, &pid("light"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (14, 0, 23));
    }

    #[test]
    fn port_world_position_refuses_for_unknown_port_id() {
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  door id=entry side=front at=center\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        assert!(matches!(
            port_at((0, 0, 0), dims, def, &pid("nope"), &declared_column(def)),
            Err(PortRejection::UnknownMember),
        ));
    }

    #[test]
    fn port_world_position_door_at_left_resolves_to_origin_corner_on_front() {
        // size=3x3, no overhang. `at=left` pins u = 0 (the wall-local axis
        // origin). Front wall world: (origin.x + 0, _, origin.z + 3 - 1)
        // = (10, _, 22); +1 in the +z normal step → port (10, 0, 23).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  door id=entry side=front at=left\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (10, 0, 23));
    }

    #[test]
    fn port_world_position_door_at_right_resolves_to_far_corner_on_front() {
        // size=3x3, no overhang. `at=right` pins u = wall_length - 1 = 2.
        // Front wall world: (origin.x + 2, _, origin.z + 3 - 1) = (12, _,
        // 22); +1 in the +z normal step → port (12, 0, 23).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  door id=entry side=front at=right\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (12, 0, 23));
    }

    #[test]
    fn port_world_position_door_back_at_left_uses_mirrored_axis() {
        // Back wall mirrors u along x (`x = w - 1 - u`), so `at=left`
        // (u = 0) lands at the far x corner: (origin.x + (3 - 1 - 0),
        // origin.z + 0) = (12, _, 20); -1 in the -z normal step → port
        // (12, 0, 19). A regression that forgets the mirror would land at
        // (10, 0, 19).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  door id=entry side=back at=left\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (12, 0, 19));
    }

    #[test]
    fn port_world_position_door_left_at_right_uses_far_z_corner() {
        // Left wall maps u to z without mirroring, so `at=right`
        // (u = interior_h - 1 = 2) lands at z = origin.z + 2 = 22.
        // x = origin.x; -1 in the -x normal step → port (9, 0, 22).
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  door id=entry side=left at=right\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (9, 0, 22));
    }

    #[test]
    fn port_world_position_door_right_at_left_mirrors_along_z() {
        // Right wall mirrors u along z (`z = h - 1 - u`), so `at=left`
        // (u = 0) lands at the far z corner: z = origin.z + 2 = 22.
        // x = origin.x + interior_w - 1 = 12; +1 in the +x normal step →
        // port (13, 0, 22). A regression that forgets the mirror would
        // land at z = 20 instead.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  door id=entry side=right at=left\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (13, 0, 22));
    }

    #[test]
    fn port_world_position_door_at_left_shifts_outward_past_roof_overhang() {
        // size=3x3 interior, place_dims=(5,_,5) for overhang=1.
        // `at=left` pins u = 0. Front wall world: (origin.x + overhang +
        // 0, _, origin.z + overhang + 3 - 1) = (11, _, 23); +1 in the +z
        // normal step → port (11, 0, 24). A regression that drops the
        // overhang shift would land inside the eave at z = 23.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  door id=entry side=front at=left\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 5, y: 1, z: 5 };
        let pos = port_at((10, 0, 20), dims, def, &pid("entry"), &declared_column(def))
            .expect("port resolves");
        assert_eq!(pos, (11, 0, 24));
    }

    #[test]
    fn port_world_position_door_refuses_when_the_body_paints_no_wall() {
        // A doorway is a hole in a wall. `super::super::lower::carve_door`
        // defers on the same question, so a port that did not ask it laid
        // a strip to a doorway that was never carved.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  door id=entry side=front at=center\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        assert!(matches!(
            port_at((0, 0, 0), dims, def, &pid("entry"), &WallColumn::default()),
            Err(PortRejection::DoorNoWalls { .. }),
        ));
    }

    #[test]
    fn port_world_position_door_refuses_when_the_walls_start_above_the_doorway() {
        // The column holds rows 7..=10 — a `level y=6 walls height=4` —
        // and the doorway opens at row 1, which is open air. A port that
        // asked only whether the column held anything answered "yes" and
        // anchored a strip to a doorway `carve_door` refuses to cut.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  door id=entry side=front at=center\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        let upper_storey = WallColumn::from_walls([(6, 4)]);
        assert!(matches!(
            port_at((0, 0, 0), dims, def, &pid("entry"), &upper_storey),
            Err(PortRejection::DoorOutsideCourse { ref walls, .. }) if !walls.is_empty(),
        ));
        // …and the same column with a ground course under it anchors the
        // port, so the refusal is about the row and not about the level.
        let both_storeys = WallColumn::from_walls([(0, 3), (6, 4)]);
        assert!(port_at((0, 0, 0), dims, def, &pid("entry"), &both_storeys).is_ok());
        // A course of exactly that one row is enough: the port asks
        // where the doorway opens, not where it ends, so a column of
        // `1..=1` anchors it. Asked one row higher — which is what a
        // constant off by one would do — this is refused.
        let one_row = WallColumn::from_walls([(0, 1)]);
        assert!(port_at((0, 0, 0), dims, def, &pid("entry"), &one_row).is_ok());
    }

    #[test]
    fn port_world_position_door_refuses_for_unknown_at_value() {
        // `at=middle` is not one of `center | left | right` and must be
        // refused with the value the author wrote rather than being
        // silently rounded to a centre value.
        let src = concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  door id=entry side=front at=middle\n",
        );
        let module = crate::parse(src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 1, z: 3 };
        assert!(matches!(
            port_at((0, 0, 0), dims, def, &pid("entry"), &declared_column(def)),
            Err(PortRejection::DoorAt { written: Written::Ident(ref s), .. }) if s == "middle",
        ));
    }

    /// Lower `body` as the members of `def cottage size=3x3` and ask for
    /// the port `id` against the column its own `walls` declare.
    fn port_in(body: &str, id: &str) -> Result<(i32, i32, i32), PortRejection> {
        let src = format!("def cottage size=3x3:\n{body}");
        let module = crate::parse(&src).expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 5, z: 3 };
        port_at((0, 0, 0), dims, def, &pid(id), &declared_column(def))
    }

    #[test]
    fn port_world_position_asks_the_role_before_the_side() {
        // A stair with no `side=` is refused for being a stair: fixing
        // the side would still leave a member no port can anchor on.
        assert!(matches!(
            port_in("  stair id=up kind=stairs mat_slot=s\n", "up"),
            Err(PortRejection::ReservedRole { ref role, .. }) if role == "stair",
        ));
    }

    #[test]
    fn port_world_position_refuses_a_missing_side_as_absent() {
        assert!(matches!(
            port_in("  walls mat_slot=w height=3\n  door id=e at=center\n", "e"),
            Err(PortRejection::Side {
                written: Written::Absent,
                ..
            }),
        ));
    }

    #[test]
    fn port_world_position_refuses_a_non_cardinal_side_with_what_was_written() {
        assert!(matches!(
            port_in("  walls mat_slot=w height=3\n  door id=e side=frnt at=center\n", "e"),
            Err(PortRejection::Side { written: Written::Ident(ref s), .. }) if s == "frnt",
        ));
        assert!(matches!(
            port_in(
                "  walls mat_slot=w height=3\n  door id=e side=3 at=center\n",
                "e"
            ),
            Err(PortRejection::Side {
                written: Written::OtherShape,
                ..
            }),
        ));
    }

    #[test]
    fn port_world_position_refuses_a_missing_door_at_as_absent() {
        assert!(matches!(
            port_in("  walls mat_slot=w height=3\n  door id=e side=front\n", "e"),
            Err(PortRejection::DoorAt {
                written: Written::Absent,
                ..
            }),
        ));
    }

    #[test]
    fn port_world_position_asks_the_door_at_before_the_masonry() {
        // `carve_door`'s order, so the reason on the `connect` row is the
        // one on the door's own line when both are wrong.
        assert!(matches!(
            port_in("  door id=e side=front at=middle\n", "e"),
            Err(PortRejection::DoorAt { .. }),
        ));
    }

    #[test]
    fn port_world_position_refuses_a_window_argument_by_name() {
        for (args, fault) in [
            ("side=front offset=0 size=1x1", WindowArgFault::YAbsent),
            (
                "side=front offset=0 y=abc size=1x1",
                WindowArgFault::YMalformed,
            ),
            ("side=front offset=0 y=1", WindowArgFault::SizeAbsent),
            (
                "side=front offset=0 y=1 size=3",
                WindowArgFault::SizeMalformed,
            ),
            (
                "side=front offset=x y=1 size=1x1",
                WindowArgFault::OffsetMalformed,
            ),
        ] {
            let body = format!("  walls mat_slot=w height=3\n  window id=l {args} mat_slot=g\n");
            assert!(
                matches!(
                    port_in(&body, "l"),
                    Err(PortRejection::WindowArgument { fault: f, .. }) if f == fault,
                ),
                "`{args}` should be refused as {fault:?}",
            );
        }
    }

    #[test]
    fn port_world_position_window_without_offset_anchors_at_the_origin_like_the_cut() {
        // `cut_window` reads an absent `offset=` as `0` and cuts the
        // window; the port used to refuse the same member, so the strip
        // was dropped beside a window that was there.
        assert_eq!(
            port_in(
                "  walls mat_slot=w height=3\n  window id=l side=front y=1 size=1x1 mat_slot=g\n",
                "l"
            ),
            port_in(
                "  walls mat_slot=w height=3\n  window id=l side=front offset=0 y=1 size=1x1 mat_slot=g\n",
                "l"
            ),
        );
        assert!(
            port_in(
                "  walls mat_slot=w height=3\n  window id=l side=front y=1 size=1x1 mat_slot=g\n",
                "l"
            )
            .is_ok()
        );
    }

    #[test]
    fn port_world_position_refuses_a_sizeless_def() {
        let module =
            crate::parse("def cottage:\n  door id=e side=front at=center\n").expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 5, z: 3 };
        assert_eq!(
            port_at(
                (0, 0, 0),
                dims,
                def,
                &pid("e"),
                &WallColumn::from_walls([(0, 3)])
            ),
            Err(PortRejection::Sizeless),
        );
    }

    #[test]
    fn port_world_position_refuses_a_coordinate_past_i32_as_out_of_range() {
        let module = crate::parse(
            "def cottage size=3x3:\n  walls mat_slot=w height=3\n  door id=e side=right at=center\n",
        )
        .expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 5, z: 3 };
        // The right wall sits at `origin.x + 2`, so `i32::MAX - 2` puts
        // it one column past the last addressable one: the refusal comes
        // from `door_world_xz`'s own sum, before the step out of the wall.
        // `..._at_the_last_step_out_of_the_wall` below reaches that step.
        assert_eq!(
            port_at(
                (i32::MAX - 2, 0, 0),
                dims,
                def,
                &pid("e"),
                &declared_column(def)
            ),
            Err(PortRejection::OutOfRange),
        );
    }

    #[test]
    fn port_world_position_refuses_a_coordinate_past_i32_at_the_last_step_out_of_the_wall() {
        // A back door on a body at `z = i32::MIN`: the wall itself sits on
        // the last addressable row, which `door_world_xz` computes
        // cleanly, and only the one step outward (`-z`) leaves the range.
        // Every other refusal in this file overflows earlier, so this is
        // the case that holds the final `checked_add`.
        let module = crate::parse(
            "def cottage size=3x3:\n  walls mat_slot=w height=3\n  door id=e side=back at=center\n",
        )
        .expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 5, z: 3 };
        assert_eq!(
            port_at(
                (0, 0, i32::MIN),
                dims,
                def,
                &pid("e"),
                &declared_column(def)
            ),
            Err(PortRejection::OutOfRange),
        );
        // One row further in, the same door is placed, so the refusal
        // above is the last step and not the wall.
        assert_eq!(
            port_at(
                (0, 0, i32::MIN + 1),
                dims,
                def,
                &pid("e"),
                &declared_column(def)
            ),
            Ok((1, 0, i32::MIN)),
        );
    }

    #[test]
    fn port_world_position_asks_the_window_horizontal_fit_before_the_masonry() {
        // `offset=4 size=3x2` runs past a 5-long wall and `y=9` is above a
        // 3-high course: both wrong. `cut_window` asks the horizontal
        // fit first, so the port does too, and the two lines give one
        // reason.
        assert!(matches!(
            port_in(
                "  walls mat_slot=w height=3\n  window id=l side=front offset=4 y=9 size=3x2 mat_slot=g\n",
                "l"
            ),
            Err(PortRejection::WindowPastWall { .. }),
        ));
    }

    #[test]
    fn port_world_position_refuses_an_opening_the_openings_pass_did_not_cut() {
        // Every rule the port reads holds, and the body's `cut` set does
        // not hold the member — `repeat=0`, or a `mat_slot=` that
        // resolved to nothing. The port is refused rather than laid to
        // a wall that is still standing.
        let module = crate::parse(concat!(
            "def cottage size=3x3:\n",
            "  walls mat_slot=w height=3\n",
            "  door id=e side=front at=center\n",
            "  window id=l side=back offset=0 y=1 size=1x1 mat_slot=g\n",
        ))
        .expect("parse");
        let ir = crate::lower(&module);
        let def = ir.defs.first().expect("def lowered");
        let dims = Dims { x: 3, y: 5, z: 3 };
        let walls = declared_column(def);
        let none: HashSet<Span> = HashSet::new();
        for (id, role) in [("e", "door"), ("l", "window")] {
            assert!(
                matches!(
                    port_world_position((0, 0, 0), dims, def, &pid(id), &walls, &none),
                    Err(PortRejection::NotCut { role: r, .. }) if r == role,
                ),
                "`{id}` was not cut",
            );
            assert!(port_at((0, 0, 0), dims, def, &pid(id), &walls).is_ok());
        }
    }

    /// One rejection per variant and, where a variant carries how an
    /// argument was [`Written`], per way of writing it. Built from an
    /// exhaustive `match` so a new variant is a compile error here until
    /// it has a row.
    fn every_rejection() -> Vec<PortRejection> {
        // One seed per variant; `samples` expands each. The seeds' fields
        // are placeholders — only the variant is read.
        let seeds = [
            PortRejection::UnknownMember,
            PortRejection::ReservedRole {
                role: String::new(),
                member: 0..0,
            },
            PortRejection::Side {
                written: Written::Absent,
                member: 0..0,
            },
            PortRejection::Sizeless,
            PortRejection::DoorAt {
                written: Written::Absent,
                member: 0..0,
            },
            PortRejection::DoorNoWalls { member: 0..0 },
            PortRejection::DoorOutsideCourse {
                walls: WallColumn::default(),
                member: 0..0,
            },
            PortRejection::WindowArgument {
                fault: WindowArgFault::YAbsent,
                member: 0..0,
            },
            PortRejection::WindowPastWall {
                offset: 0,
                width: 0,
                wall_length: 0,
                member: 0..0,
            },
            PortRejection::WindowNoWalls { member: 0..0 },
            PortRejection::WindowOutsideCourse {
                y: 0,
                height: 0,
                walls: WallColumn::default(),
                member: 0..0,
            },
            PortRejection::NotCut {
                role: "door",
                member: 0..0,
            },
            PortRejection::OutOfRange,
        ];
        seeds.iter().flat_map(samples).collect()
    }

    /// Every rendering [`every_rejection`] checks for the variant of `r`.
    /// An exhaustive `match`, so a new variant is a compile error here
    /// until it has a row.
    fn samples(r: &PortRejection) -> Vec<PortRejection> {
        let span: Span = 3..9;
        match r {
            PortRejection::UnknownMember => vec![PortRejection::UnknownMember],
            PortRejection::ReservedRole { .. } => ["stair", "roof", "floor"]
                .into_iter()
                .map(|role| PortRejection::ReservedRole {
                    role: role.to_owned(),
                    member: span.clone(),
                })
                .collect(),
            PortRejection::Side { .. } => written_samples("frnt")
                .into_iter()
                .map(|written| PortRejection::Side {
                    written,
                    member: span.clone(),
                })
                .collect(),
            PortRejection::Sizeless => vec![PortRejection::Sizeless],
            PortRejection::DoorAt { .. } => written_samples("middle")
                .into_iter()
                .map(|written| PortRejection::DoorAt {
                    written,
                    member: span.clone(),
                })
                .collect(),
            PortRejection::DoorNoWalls { .. } => vec![PortRejection::DoorNoWalls {
                member: span.clone(),
            }],
            PortRejection::DoorOutsideCourse { .. } => {
                vec![PortRejection::DoorOutsideCourse {
                    walls: WallColumn::from_walls([(6, 4)]),
                    member: span.clone(),
                }]
            }
            PortRejection::WindowArgument { .. } => [
                WindowArgFault::OffsetMalformed,
                WindowArgFault::YAbsent,
                WindowArgFault::YMalformed,
                WindowArgFault::SizeAbsent,
                WindowArgFault::SizeMalformed,
            ]
            .into_iter()
            .map(|fault| PortRejection::WindowArgument {
                fault,
                member: span.clone(),
            })
            .collect(),
            PortRejection::WindowPastWall { .. } => vec![PortRejection::WindowPastWall {
                offset: 2,
                width: 2,
                wall_length: 3,
                member: span.clone(),
            }],
            PortRejection::WindowNoWalls { .. } => vec![PortRejection::WindowNoWalls {
                member: span.clone(),
            }],
            PortRejection::WindowOutsideCourse { .. } => [(0, 1), (u32::MAX, 2)]
                .into_iter()
                .map(|(y, height)| PortRejection::WindowOutsideCourse {
                    y,
                    height,
                    walls: WallColumn::from_walls([(0, 3)]),
                    member: span.clone(),
                })
                .collect(),
            PortRejection::NotCut { .. } => ["door", "window"]
                .into_iter()
                .map(|role| PortRejection::NotCut {
                    role,
                    member: span.clone(),
                })
                .collect(),
            PortRejection::OutOfRange => vec![PortRejection::OutOfRange],
        }
    }

    fn written_samples(ident: &str) -> [Written; 3] {
        [
            Written::Absent,
            Written::Ident(ident.to_owned()),
            Written::OtherShape,
        ]
    }

    #[test]
    fn every_rejection_note_renders_in_full() {
        const OWN: &str = "; the member says so on its own line too";
        const NO_WALLS: &str = "its `def` has no `walls` that paints — a positive `height=` and a `mat_slot=` that resolves";
        let expected: Vec<(String, bool)> = vec![
            ("`a.p` names no member of its `def` body".to_owned(), false),
            ("`a.p` is a `stair`, and only a `door` or a `window` can anchor a port (`stair` ports are reserved) — declare the port on a door or window instead".to_owned(), true),
            ("`a.p` is a `roof`, and only a `door` or a `window` can anchor a port (`roof` ports are reserved) — declare the port on a door or window instead".to_owned(), true),
            ("`a.p` is a `floor`, and only a `door` or a `window` can anchor a port — declare the port on a door or window instead".to_owned(), true),
            (format!("`a.p` has no `side=` (expected one of front, back, left, right){OWN}"), true),
            (format!("`a.p` has `side=frnt`, which is not one of front, back, left, right{OWN}"), true),
            (format!("`a.p` has a `side=` that is not one of front, back, left, right{OWN}"), true),
            ("`a.p` belongs to a `def` with no `size=`, so its walls have no length".to_owned(), false),
            (format!("`a.p` is a door that has no `at=`{OWN}"), true),
            (format!("`a.p` is a door that has `at=middle`, which is not one of center, left, right{OWN}"), true),
            (format!("`a.p` is a door that has an `at=` that is not one of center, left, right (numeric offsets are reserved){OWN}"), true),
            (format!("`a.p` is a door with no wall to open: {NO_WALLS}{OWN}"), true),
            (format!("`a.p` is a door that opens at y=1, the row above the floor slab, which is not inside any wall course (the walls occupy y=7..=10){OWN}"), true),
            (format!("`a.p` is a window that has an `offset=` that is not a non-negative integer that fits in u32{OWN}"), true),
            (format!("`a.p` is a window that has no `y=`{OWN}"), true),
            (format!("`a.p` is a window that has a `y=` that is not a non-negative integer that fits in u32{OWN}"), true),
            (format!("`a.p` is a window that has no `size=WxH`{OWN}"), true),
            (format!("`a.p` is a window that has a `size=` that is not a `WxH` of two positive integers{OWN}"), true),
            (format!("`a.p` is a window that runs past the end of its wall (`offset + size.w` = 2 + 2, wall length 3){OWN}"), true),
            (format!("`a.p` is a window with no wall to cut into: {NO_WALLS}{OWN}"), true),
            (format!("`a.p` is a window whose rows y=0..=0 are not all inside one wall course (the walls occupy y=1..=3){OWN}"), true),
            (format!("`a.p` is a window whose 2 rows from y=4294967295 run past the highest row a build can address (the walls occupy y=1..=3){OWN}"), true),
            ("`a.p` is a door the openings pass did not cut, so a strip would end against the wall — the finding on its own line says why".to_owned(), true),
            ("`a.p` is a window the openings pass did not cut, so a strip would end against the wall — the finding on its own line, or on the material its `mat_slot=` names, says why".to_owned(), true),
            ("`a.p` would sit outside the coordinate range a build can address — bring its `place` closer to the origin".to_owned(), false),
        ];
        let got: Vec<(String, bool)> = every_rejection()
            .iter()
            .map(|r| {
                let note = r.note("a.p");
                (note.message, note.span == Some(3..9))
            })
            .collect();
        assert_eq!(got.len(), expected.len(), "{got:#?}");
        for (got, expected) in got.iter().zip(&expected) {
            assert_eq!(got, expected);
        }
    }

    #[test]
    fn the_member_line_and_the_note_word_a_door_at_fault_alike() {
        // `carve_door`'s deferral and the port's note are built from one
        // clause, so what the note says `at=` is, the door's line says too.
        for written in written_samples("middle") {
            let deferral = door_at_deferral(&written);
            let note = PortRejection::DoorAt {
                written: written.clone(),
                member: 0..0,
            }
            .note("a.p")
            .message;
            let clause = door_at_clause(&written);
            assert!(
                deferral.starts_with(&format!("door {clause}")),
                "{deferral}"
            );
            assert!(note.contains(&clause), "{note}");
        }
    }
}
