//! Port resolution and walkway voxelisation for site `connect` rows.
//!
//! Each `connect from.port to to.port path=@MAT` row lays a 1-block-wide
//! gravel-like strip between the two named ports. The work splits into
//! three pieces, each unit-tested in isolation so a coordinate sign error
//! or off-by-one fails loudly here rather than only at the full-village
//! integration boundary:
//!
//! 1. [`port_world_position`] turns a `(place_origin, def, port_id)`
//!    triple into the world-voxel coordinate one block outside the
//!    member's `side=` face. Ports are exposed on `door` and `window`
//!    members; doors anchor at one of the wall-local
//!    `at=center | left | right` positions, windows at the rectangle's
//!    geometric centre (`offset + size.w / 2`). A port it cannot place
//!    — another role (stair, roof, …), an argument the opening cannot
//!    use, masonry the opening does not reach, an opening the openings
//!    phase did not cut, a coordinate past `i32` — comes back as the
//!    [`PortRejection`] naming that one reason, which the caller turns
//!    into the `W_DEFERRED_MEMBER` on the `connect` row. Both roles are
//!    openings, so the caller hands over the [`WallColumn`] the body
//!    was lowered with: a port is somewhere a wall was opened, and
//!    asking the `def` a second time is what let the strip and the cut
//!    disagree.
//! 2. [`l_path`] walks a Manhattan L (x-axis first, then z-axis) between
//!    two world voxels at a constant Y; every coordinate appears once,
//!    the corner included. When the L collides with an
//!    existing structure, [`route_path`] searches the ground plane for a
//!    deterministic shortest detour around the obstacle instead.
//! 3. [`build_walkway_array`] turns the path into a [`BlockArray`] whose
//!    voxel grid bounds the strip's bounding box, returning the world-
//!    space origin so the lockfile can pin where the array lives. Cells
//!    that overlap an existing structure (`blocked` in the signature)
//!    are skipped and counted so the caller can emit one
//!    `W_WALKWAY_BLOCKED` warning per row — with [`route_path`] in
//!    front, that only happens when no unobstructed route exists at all.
//!
//! The walkway always sits at the two ports' shared Y. 3D path search
//! (staircases, multi-level walkways) is intentionally out of scope so
//! the port surface lands in one piece; every shipping example lays its
//! walkways flat against `y = 0`.

use std::collections::HashSet;
use std::hash::BuildHasher;

use crate::ast::ValueKind;
use crate::check::DiagnosticNote;
use crate::error::Span;
use crate::ids::{PortId, WalkwayScopeKey};
use crate::intent::{DefIr, Member, MemberRole};

use super::lower::size_value;
use super::openings::{WallSide, wall_length, wall_local_to_grid};
use super::wall_column::WallColumn;
use super::{BlockArray, BlockState, Dims, Palette, PaletteIndex};

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

/// Output of [`build_walkway_array`].
///
/// Bundles the lowered [`BlockArray`], the world-space origin the array
/// pins to, and the number of cells the lay-pass skipped because they
/// collided with an existing structure. Named struct rather than a
/// bare `(BlockArray, (i32, i32, i32), usize)` so a future axis-order
/// shuffle or extra return value (e.g. per-cell skip mask) cannot
/// silently re-bind callers to the wrong slot.
#[derive(Debug, Clone, PartialEq)]
pub struct WalkwayLayout {
    /// The voxelised walkway.
    pub array: BlockArray,
    /// Absolute `(x, y, z)` origin the [`BlockArray`] lives at — the
    /// `(min_x, port_y, min_z)` corner of the bounding box.
    pub origin: (i32, i32, i32),
    /// Number of cells dropped because they overlapped a placement
    /// floor (one per `W_WALKWAY_BLOCKED` collision).
    pub blocked_count: usize,
}

/// How a member argument a port reads was written, when it was not written
/// in a shape the port can use.
///
/// Kept apart from the value itself so the note can say *which* of the
/// three it was — an author who left `side=` off and one who typed
/// `side=frnt` are fixing different things.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Written {
    /// The key is not on the line.
    Absent,
    /// The key carries an identifier the port does not accept.
    Ident(String),
    /// The key carries a value of some other shape (a number, a string, …).
    OtherShape,
}

impl Written {
    fn of_ident(member: &Member, key: &str) -> Self {
        match member.intent_state.get(key).map(|v| &v.value.kind) {
            None => Self::Absent,
            Some(ValueKind::Ident(s)) => Self::Ident(s.clone()),
            Some(_) => Self::OtherShape,
        }
    }
}

/// What is wrong with a door's `at=`, as the rest of a sentence whose
/// subject is the door.
///
/// One wording for both places that say it — the door's own
/// `W_DEFERRED_MEMBER` ([`door_at_deferral`]) and the note a `connect` row
/// naming the door carries ([`PortRejection::note`]) — because both read
/// `at=` through [`door_anchor_offset`], so they cannot classify it
/// differently, and with one sentence they cannot describe it differently
/// either.
fn door_at_clause(written: &Written) -> String {
    match written {
        Written::Absent => "has no `at=`".to_owned(),
        Written::Ident(s) => format!("has `at={s}`, which is not one of center, left, right"),
        Written::OtherShape => "has an `at=` that is not one of center, left, right (numeric \
                                offsets are reserved)"
            .to_owned(),
    }
}

/// The reason `super::lower::carve_door` defers a door whose `at=`
/// [`door_anchor_offset`] refused.
pub(super) fn door_at_deferral(written: &Written) -> String {
    format!(
        "door {} — use `at=center | left | right`",
        door_at_clause(written)
    )
}

/// The one window argument [`read_window_args`] could not use, and how it
/// was written. One variant per case rather than a key and a flag, so an
/// `offset=` that is absent — which is not a fault, it reads as `0` — has
/// no variant to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowArgFault {
    /// `offset=` is present and not a non-negative integer.
    OffsetMalformed,
    /// No `y=`.
    YAbsent,
    /// `y=` is present and not a non-negative integer.
    YMalformed,
    /// No `size=`.
    SizeAbsent,
    /// `size=` is present and not a `WxH`.
    SizeMalformed,
}

impl WindowArgFault {
    /// What is wrong, as the rest of a sentence whose subject is the
    /// window — the same single wording source [`door_at_clause`] is.
    fn clause(self) -> &'static str {
        match self {
            Self::OffsetMalformed => {
                "has an `offset=` that is not a non-negative integer that fits in u32"
            }
            Self::YAbsent => "has no `y=`",
            Self::YMalformed => "has a `y=` that is not a non-negative integer that fits in u32",
            Self::SizeAbsent => "has no `size=WxH`",
            Self::SizeMalformed => "has a `size=` that is not a `WxH` of two positive integers",
        }
    }

    /// The reason `super::lower::fill_window` defers the window.
    pub(super) fn deferral(self) -> String {
        format!("window {}", self.clause())
    }
}

/// The window arguments a port and a cut both need, read once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WindowArgs {
    /// `offset=`, `0` when absent.
    pub(super) offset: u32,
    /// `y=`, relative to the member's own level.
    pub(super) y: u32,
    /// `size=`'s `W`.
    pub(super) width: u32,
    /// `size=`'s `H`.
    pub(super) height: u32,
}

/// Read a window's `offset=`, `y=` and `size=`, in that order, refusing
/// at the first one that cannot be used.
///
/// `super::lower::fill_window` and [`port_world_position`] both call this,
/// so the cut and the port accept one set and name one first fault.
/// `repeat=`, `step=` and `sym=` are not read here: only the cut uses them,
/// and a window they defer is refused as a port by
/// [`PortRejection::NotCut`] rather than by a reason of its own.
///
/// # Errors
///
/// The [`WindowArgFault`] for the first argument that cannot be used.
pub(super) fn read_window_args(member: &Member) -> Result<WindowArgs, WindowArgFault> {
    let offset = if member.intent_state.contains_key("offset") {
        member
            .nonneg_u32("offset")
            .ok_or(WindowArgFault::OffsetMalformed)?
    } else {
        0
    };
    let y = match (
        member.intent_state.contains_key("y"),
        member.nonneg_u32("y"),
    ) {
        (_, Some(y)) => y,
        (false, None) => return Err(WindowArgFault::YAbsent),
        (true, None) => return Err(WindowArgFault::YMalformed),
    };
    let (width, height) = match (
        member.intent_state.contains_key("size"),
        size_value(member, "size"),
    ) {
        (_, Some(size)) => size,
        (false, None) => return Err(WindowArgFault::SizeAbsent),
        (true, None) => return Err(WindowArgFault::SizeMalformed),
    };
    Ok(WindowArgs {
        offset,
        y,
        width,
        height,
    })
}

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

/// Walk a Manhattan L between two world voxels at a fixed Y, x-axis
/// first then z-axis. Every coordinate, the corner included, appears in
/// the returned `Vec` exactly once.
///
/// The two endpoints are included in the output. Caller is expected to
/// have already validated that `from.1 == to.1`; mismatched Y values
/// would still produce a connected path, just landing at the `from`
/// Y for the whole strip.
#[must_use]
pub fn l_path(from: (i32, i32, i32), to: (i32, i32, i32)) -> Vec<(i32, i32, i32)> {
    debug_assert!(
        l_path_area(from, to) <= ROUTE_AREA_CAP,
        "l_path called past the cap; callers must ask `l_path_area` first",
    );
    let y = from.1;
    // The path is exactly `|dx| + |dz| + 1` cells long. Summed in u64 so
    // two i32 spans cannot overflow; the size is only a capacity hint, so
    // a length that does not fit `usize` falls back to growing.
    let len = u64::from(from.0.abs_diff(to.0)) + u64::from(from.2.abs_diff(to.2)) + 1;
    let mut voxels: Vec<(i32, i32, i32)> = Vec::with_capacity(usize::try_from(len).unwrap_or(0));
    let (x0, z0) = (from.0, from.2);
    let (x1, z1) = (to.0, to.2);

    // x-axis leg: walk from (x0, z0) to (x1, z0), inclusive.
    let mut x = x0;
    voxels.push((x, y, z0));
    let step_x: i32 = match x1.cmp(&x0) {
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Less => -1,
    };
    while x != x1 {
        x += step_x;
        voxels.push((x, y, z0));
    }

    // z-axis leg: walk from (x1, z0) toward (x1, z1). The cell at
    // (x1, z0) is the corner already laid down at the end of the
    // x-leg, so the loop steps z BEFORE pushing. That order is the
    // whole corner dedup: every x-leg cell has `z == z0` and every
    // z-leg cell has `z != z0`, so no cell can appear twice and no
    // lookup into `voxels` is needed. A lookup would be a linear scan
    // per step, which makes a long north–south strip quadratic.
    let mut z = z0;
    let step_z: i32 = match z1.cmp(&z0) {
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
        std::cmp::Ordering::Less => -1,
    };
    while z != z1 {
        z += step_z;
        voxels.push((x1, y, z));
    }
    voxels
}

/// Upper bound on the search rectangle, in cells, that [`route_path`]
/// is willing to explore. The rectangle is the bounding box of the
/// blocked cells on the walk plane plus the two endpoints, inflated by
/// one cell — for every shipping example that is a few hundred cells.
/// The cap only exists so a pathological source (two ports megametres
/// apart with a pebble between them) degrades to the skip-and-warn
/// fallback instead of allocating the world.
pub const ROUTE_AREA_CAP: u64 = 4_000_000;

/// Ground-plane cells the straight L between these two ports would span,
/// as a bounding-box area.
///
/// Area, not path length, because area is what gets allocated:
/// `build_walkway_array` sizes its voxel buffer from the bounding box, and
/// `route_path` measures the same quantity against the same cap. A pair
/// `2_000_000` cells apart on each axis has a path length of 4M — inside a
/// length-based bound — and a bounding box of 4x10^12.
///
/// Saturates at `u64::MAX` if the product overflows, which is the sentinel
/// [`RoutePathError::AreaCapExceeded`] already documents for that field.
#[must_use]
pub fn l_path_area(from: (i32, i32, i32), to: (i32, i32, i32)) -> u64 {
    let dx = u128::from(from.0.abs_diff(to.0)) + 1;
    let dz = u128::from(from.2.abs_diff(to.2)) + 1;
    u64::try_from(dx * dz).unwrap_or(u64::MAX)
}

/// Direction of travel between two 4-neighbour ground-plane cells.
/// Carried in the search state so the cost function can count turns:
/// among equal-length routes the fewest-turns one wins, which keeps
/// the laid strip looking like a hand-drawn path (long straight runs)
/// instead of a staircase zigzag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum StepDir {
    PosX,
    NegX,
    PosZ,
    NegZ,
}

impl StepDir {
    /// The `(x, z)` cell one step from `cell` in this direction, or
    /// `None` when that step leaves `i32`. [`search_rect`] only
    /// guarantees that the inflated rectangle itself fits in `i32`, so
    /// a cell on its edge can sit at `i32::MIN` or `i32::MAX`; the
    /// neighbour past that edge is outside the rectangle anyway, and
    /// the caller skips it like any other out-of-bounds cell.
    fn step(self, (x, z): (i32, i32)) -> Option<(i32, i32)> {
        match self {
            Self::PosX => Some((x.checked_add(1)?, z)),
            Self::NegX => Some((x.checked_sub(1)?, z)),
            Self::PosZ => Some((x, z.checked_add(1)?)),
            Self::NegZ => Some((x, z.checked_sub(1)?)),
        }
    }
}

/// Fixed neighbour expansion order. Part of the determinism contract:
/// together with the monotonic queue sequence number it fully orders
/// equal-cost candidates, so the same source always lowers to the same
/// strip and the lockfile stays reproducible. Reordering the variants
/// changes which of two equal-cost detours wins — `+x` first is why
/// `village.crn`'s home1↔home3 walkway rounds home1's *east* face —
/// so a shuffle here breaks the village integration pins and every
/// lockfile that recorded a tie-broken detour.
const STEP_DIRS: [StepDir; 4] = [StepDir::PosX, StepDir::NegX, StepDir::PosZ, StepDir::NegZ];

/// Why [`route_path`] could not produce a detour. Each variant maps to
/// a different author-facing remedy, so the caller can write a
/// `W_WALKWAY_BLOCKED` note that names the actual problem instead of
/// suggesting a gap widening that may not help.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoutePathError {
    /// One or both port cells are themselves inside the blocked set —
    /// a port buried under another placement's floor. No search can
    /// leave (or reach) a buried cell, so the flags say which side to
    /// fix.
    EndpointBlocked {
        /// `from` is a blocked cell.
        from_blocked: bool,
        /// `to` is a blocked cell.
        to_blocked: bool,
    },
    /// The search exhausted the rectangle without reaching `to` — the
    /// target is fully enclosed by blocked cells.
    TargetUnreachable,
    /// The search rectangle exceeds `ROUTE_AREA_CAP`; the site is too
    /// spread out to route. Carries the offending area so the caller
    /// can surface both numbers.
    AreaCapExceeded {
        /// Cells the rectangle would cover (`u64::MAX` when the
        /// span product itself overflowed).
        area: u64,
        /// The cap it exceeded, i.e. `ROUTE_AREA_CAP`.
        cap: u64,
    },
    /// Inflating the search rectangle stepped past the `i32` coordinate
    /// space (an endpoint or obstacle at `i32::MIN` / `i32::MAX`).
    CoordinateOverflow,
}

/// Pre-indexed view of the blocked-cell set, built **once** per
/// lowering with [`BlockedIndex::new`] and shared by every `connect`
/// row. [`route_path`] needs the bounding rectangle of the blocked
/// cells on its walk plane; deriving that inside the router would
/// re-scan the whole set per row, letting a large site with many
/// colliding rows multiply one linear scan into billions of iterations
/// — the pre-computed per-plane bounds keep the router's per-row cost
/// bounded by `ROUTE_AREA_CAP` alone.
pub struct BlockedIndex<'a, S: BuildHasher> {
    cells: &'a HashSet<(i32, i32, i32), S>,
    /// `y → (min_x, max_x, min_z, max_z)` over the blocked cells on
    /// that plane. Planes with no blocked cells are absent.
    plane_bounds: std::collections::HashMap<i32, (i32, i32, i32, i32)>,
}

impl<'a, S: BuildHasher> BlockedIndex<'a, S> {
    /// Index `cells` with a single linear scan.
    #[must_use]
    pub fn new(cells: &'a HashSet<(i32, i32, i32), S>) -> Self {
        let mut plane_bounds: std::collections::HashMap<i32, (i32, i32, i32, i32)> =
            std::collections::HashMap::new();
        for &(x, y, z) in cells {
            plane_bounds
                .entry(y)
                .and_modify(|(min_x, max_x, min_z, max_z)| {
                    *min_x = (*min_x).min(x);
                    *max_x = (*max_x).max(x);
                    *min_z = (*min_z).min(z);
                    *max_z = (*max_z).max(z);
                })
                .or_insert((x, x, z, z));
        }
        Self {
            cells,
            plane_bounds,
        }
    }

    fn contains(&self, cell: (i32, i32, i32)) -> bool {
        self.cells.contains(&cell)
    }

    fn plane_bounds(&self, y: i32) -> Option<(i32, i32, i32, i32)> {
        self.plane_bounds.get(&y).copied()
    }
}

/// Inflated search rectangle for [`route_path`]: bbox(blocked cells on
/// the walk plane ∪ both endpoints) + 1 cell on every side, so a route
/// can always hug the outside of the outermost obstacle. Rejects a
/// rectangle that steps past the `i32` coordinate space or covers more
/// than [`ROUTE_AREA_CAP`] cells.
///
/// # Errors
///
/// [`RoutePathError::CoordinateOverflow`] when the one-cell inflation
/// leaves `i32`; [`RoutePathError::AreaCapExceeded`] when the
/// rectangle covers more than [`ROUTE_AREA_CAP`] cells.
fn search_rect(
    from: (i32, i32, i32),
    to: (i32, i32, i32),
    plane_bounds: Option<(i32, i32, i32, i32)>,
) -> Result<(i32, i32, i32, i32), RoutePathError> {
    let mut min_x = from.0.min(to.0);
    let mut max_x = from.0.max(to.0);
    let mut min_z = from.2.min(to.2);
    let mut max_z = from.2.max(to.2);
    if let Some((blocked_min_x, blocked_max_x, blocked_min_z, blocked_max_z)) = plane_bounds {
        min_x = min_x.min(blocked_min_x);
        max_x = max_x.max(blocked_max_x);
        min_z = min_z.min(blocked_min_z);
        max_z = max_z.max(blocked_max_z);
    }
    let (Some(min_x), Some(max_x), Some(min_z), Some(max_z)) = (
        min_x.checked_sub(1),
        max_x.checked_add(1),
        min_z.checked_sub(1),
        max_z.checked_add(1),
    ) else {
        return Err(RoutePathError::CoordinateOverflow);
    };
    // Spans fit u64 by construction (an i32 range is at most 2^32
    // wide); only the *product* can overflow, and an overflowing
    // product is by definition far past the cap — saturating to
    // u64::MAX keeps the reported area honest about "too big".
    let span_x = u64::try_from(i64::from(max_x) - i64::from(min_x) + 1)
        .expect("i32 bbox span is non-negative and fits u64");
    let span_z = u64::try_from(i64::from(max_z) - i64::from(min_z) + 1)
        .expect("i32 bbox span is non-negative and fits u64");
    let area = span_x.saturating_mul(span_z);
    if area > ROUTE_AREA_CAP {
        return Err(RoutePathError::AreaCapExceeded {
            area,
            cap: ROUTE_AREA_CAP,
        });
    }
    Ok((min_x, max_x, min_z, max_z))
}

/// Deterministic shortest detour between two world voxels at a shared
/// Y, avoiding blocked cells. The fallback [`l_path`] cannot route
/// around obstacles; this search can, so `connect` rows whose straight
/// L would cut through a placement floor still lay an unbroken strip.
///
/// The search is Dijkstra over `(cell, incoming direction)` states with
/// the lexicographic cost `(path length, turn count)` — shortest first,
/// and among equal-length routes the one with the fewest direction
/// changes. Ties beyond that are broken by the fixed `STEP_DIRS`
/// expansion order and a monotonic queue sequence number, never by hash
/// iteration order, so the result is fully deterministic (a lockfile
/// requirement).
///
/// The searchable area is the `search_rect` rectangle: blocked cells
/// on other Y planes neither obstruct nor inflate the search. The two
/// endpoints are expected to share a Y (ports are pinned to their
/// placements' shared ground row); a mismatch is a caller bug and
/// trips a `debug_assert`.
///
/// Returns the cell sequence from `from` to `to` inclusive.
///
/// # Errors
///
/// A [`RoutePathError`] naming why no detour exists — a buried
/// endpoint, an enclosed target, the area cap, or coordinate
/// overflow; see the variant docs for the author-facing remedy each
/// one maps to. The caller is expected to fall back to [`l_path`] with
/// skipped cells and a `W_WALKWAY_BLOCKED` warning whose note reflects
/// the variant.
///
/// # Panics
///
/// Panics when an internal search invariant breaks (state count or
/// path length exceeding the `4 * ROUTE_AREA_CAP` bound, or a
/// parent-chain cycle). These are algorithm bugs, not input
/// conditions — degrading them to an `Err` would silently swap the
/// deterministic shortest detour for the skip-and-warn fallback.
pub fn route_path<S: BuildHasher>(
    from: (i32, i32, i32),
    to: (i32, i32, i32),
    blocked: &BlockedIndex<'_, S>,
) -> Result<Vec<(i32, i32, i32)>, RoutePathError> {
    use std::cmp::Reverse;
    use std::collections::{BinaryHeap, HashMap};

    type Cell = (i32, i32);

    let y = from.1;
    debug_assert_eq!(
        from.1, to.1,
        "walkway ports must share a Y; the router searches a single ground plane",
    );
    let from_blocked = blocked.contains(from);
    let to_blocked = blocked.contains(to);
    if from_blocked || to_blocked {
        return Err(RoutePathError::EndpointBlocked {
            from_blocked,
            to_blocked,
        });
    }
    if from == to {
        return Ok(vec![from]);
    }

    // The per-plane bounds come pre-computed from the index so the
    // rectangle stays O(1) per row regardless of how many cells the
    // site blocks.
    let (min_x, max_x, min_z, max_z) = search_rect(from, to, blocked.plane_bounds(y))?;
    let in_bounds = |(x, z): (i32, i32)| x >= min_x && x <= max_x && z >= min_z && z <= max_z;

    // Dijkstra over (cell, dir). `best` keeps the smallest (len, turns)
    // seen per state; on an exact cost tie the first-queued candidate
    // wins (the relaxation below never replaces on equality), which
    // pins the tie-break to the deterministic queue order.
    let mut best: HashMap<(Cell, StepDir), (u32, u32)> = HashMap::new();
    let mut parent: HashMap<(Cell, StepDir), (Cell, StepDir)> = HashMap::new();
    // The heap orders by (len, turns, seq); `states[seq]` carries the
    // matching (cell, dir) payload so the heap entries stay `Copy` and
    // totally ordered without a custom `Ord` impl. Every count below —
    // states, seq, path length — is bounded by 4 directions ×
    // ROUTE_AREA_CAP cells = 16M, comfortably inside u32, so the
    // `expect`s are unreachable unless the cap or the dedup in `best`
    // regresses; that is an algorithm bug and must fail loud (see the
    // `# Panics` section).
    let mut heap: BinaryHeap<Reverse<(u32, u32, u32)>> = BinaryHeap::new();
    let mut states: Vec<(Cell, StepDir)> = Vec::new();

    let start = (from.0, from.2);
    let goal = (to.0, to.2);
    for dir in STEP_DIRS {
        let Some(cell) = dir.step(start) else {
            continue;
        };
        if !in_bounds(cell) || blocked.contains((cell.0, y, cell.1)) {
            continue;
        }
        // First step off the port costs no turn regardless of heading.
        let cost = (1, 0);
        best.insert((cell, dir), cost);
        let seq = u32::try_from(states.len()).expect("state count bounded by 4 * ROUTE_AREA_CAP");
        states.push((cell, dir));
        heap.push(Reverse((cost.0, cost.1, seq)));
    }

    let mut goal_state: Option<(Cell, StepDir)> = None;
    while let Some(Reverse((len, turns, seq))) = heap.pop() {
        let (cell, dir) = states[usize::try_from(seq).expect("u32 fits usize")];
        // Stale heap entry: a cheaper cost for this state was queued
        // after this one was pushed.
        if best.get(&(cell, dir)) != Some(&(len, turns)) {
            continue;
        }
        if cell == goal {
            goal_state = Some((cell, dir));
            break;
        }
        for next_dir in STEP_DIRS {
            let Some(next) = next_dir.step(cell) else {
                continue;
            };
            if !in_bounds(next) || blocked.contains((next.0, y, next.1)) {
                continue;
            }
            // `len` (and therefore `turns`) is bounded by the state
            // count, so plain `+` cannot overflow u32 — see the bound
            // note above the heap declaration.
            let next_cost = (len + 1, turns + u32::from(next_dir != dir));
            let key = (next, next_dir);
            // Strict `<` is load-bearing for determinism: relaxing on
            // equality (`<=`) would let a later-queued candidate steal
            // an equal-cost state and re-parent the path by heap
            // timing instead of the fixed queue order (see the
            // tie-break note above `best`).
            if best.get(&key).is_none_or(|&c| next_cost < c) {
                best.insert(key, next_cost);
                parent.insert(key, (cell, dir));
                let next_seq =
                    u32::try_from(states.len()).expect("state count bounded by 4 * ROUTE_AREA_CAP");
                states.push(key);
                heap.push(Reverse((next_cost.0, next_cost.1, next_seq)));
            }
        }
    }

    let Some(mut state) = goal_state else {
        return Err(RoutePathError::TargetUnreachable);
    };
    let mut cells = vec![(state.0.0, y, state.0.1)];
    while let Some(&prev) = parent.get(&state) {
        assert!(
            cells.len() <= states.len(),
            "walkway route reconstruction exceeded the state count — parent chain has a cycle",
        );
        cells.push((prev.0.0, y, prev.0.1));
        state = prev;
    }
    cells.push(from);
    cells.reverse();
    Ok(cells)
}

/// Build a [`BlockArray`] from a path of world voxels and a palette
/// material, returning the world-space origin and the count of cells
/// skipped because they collided with `blocked`.
///
/// `voxel_world` is a flat list of `(x, y, z)` cells in world
/// coordinates; all cells are assumed to share a Y. `blocked` is the
/// world-space set of cells already occupied by other structures (the
/// walkway should not overwrite a wall or floor it crosses). The
/// returned `BlockArray`'s `voxels` grid is dimensioned to the bounding
/// box of `voxel_world`; collided cells stay air so the lockfile sees a
/// truthful palette.
///
/// # Panics
///
/// Panics when `voxel_world` is empty: a zero-cell walkway has no
/// meaningful bounding box, and silently producing a 1×1 placeholder at
/// `(0, 0, 0)` would let an upstream bug pin walkway IR at the wrong
/// origin. Also panics if the bounding-box span on either axis exceeds
/// `u32::MAX` (i.e. an `i32` subtraction that overflows the cast); paths
/// produced by [`l_path`] cannot exercise either condition.
#[must_use]
pub fn build_walkway_array<S: BuildHasher>(
    voxel_world: &[(i32, i32, i32)],
    material: BlockState,
    blocked: &HashSet<(i32, i32, i32), S>,
    scope_key: &WalkwayScopeKey,
) -> WalkwayLayout {
    let first = voxel_world
        .first()
        .copied()
        .unwrap_or_else(|| panic!("walkway voxel_world is empty for scope `{scope_key}`"));
    let mut min_x = first.0;
    let mut max_x = first.0;
    let mut min_z = first.2;
    let mut max_z = first.2;
    for &(x, _, z) in voxel_world {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_z = min_z.min(z);
        max_z = max_z.max(z);
    }
    // Both spans are positive by construction: the min/max sweep above
    // gives `min_x ≤ max_x` and `min_z ≤ max_z`. The `u32::try_from` can
    // only fail if `max - min + 1` overflows `i32` (a span wider than
    // `i32::MAX`), which is unreachable from [`l_path`] for any realistic
    // world; surface that as a panic with the scope so a future caller
    // gets a locatable failure rather than a silent 1×1 strip.
    let dx = u32::try_from(max_x - min_x + 1)
        .unwrap_or_else(|_| panic!("walkway `{scope_key}` x span exceeds u32 ({min_x}..={max_x})"));
    let dz = u32::try_from(max_z - min_z + 1)
        .unwrap_or_else(|_| panic!("walkway `{scope_key}` z span exceeds u32 ({min_z}..={max_z})"));
    let dims = Dims { x: dx, y: 1, z: dz };
    let origin = (min_x, first.1, min_z);

    let mut palette = Palette::new_with_air();
    let mat_idx = palette.intern(material);
    let mut voxels = vec![PaletteIndex::AIR; dims.volume()];
    let mut blocked_count: usize = 0;
    for &(wx, wy, wz) in voxel_world {
        if blocked.contains(&(wx, wy, wz)) {
            blocked_count += 1;
            continue;
        }
        // `wx`/`wz` are members of the same min/max sweep above, so
        // `wx ≥ min_x` and `wz ≥ min_z` by construction. The same
        // overflow story as `dx`/`dz` applies — surface the cast with
        // the scope so an unreachable failure stays locatable.
        let lx = u32::try_from(wx - min_x)
            .unwrap_or_else(|_| panic!("walkway `{scope_key}` cell x={wx} below min={min_x}"));
        let lz = u32::try_from(wz - min_z)
            .unwrap_or_else(|_| panic!("walkway `{scope_key}` cell z={wz} below min={min_z}"));
        if let Some(i) = dims.index(lx, 0, lz) {
            voxels[i] = mat_idx;
        }
    }
    let mut array = BlockArray {
        dims,
        palette,
        voxels,
        block_entities: Vec::new(),
        entities: Vec::new(),
        source_scope: scope_key.as_str().to_owned(),
    };
    // A one-material walkway is already in canonical order, so this is a
    // no-op today. It is here so the order `BlockArray::palette` claims
    // holds of every array the pass produces by construction rather than
    // by that argument, and so a walkway that grows a second material
    // (kerbs, railings) does not have to remember to ask.
    array.canonicalize_palette();
    WalkwayLayout {
        array,
        origin,
        blocked_count,
    }
}

/// Wall-local `u` anchor for a door port. Accepts the three named
/// anchors the spec defines for `at=`:
///
/// * `center` — `len / 2` (integer division, so even widths land at
///   the column one cell `+u` of the midpoint, matching the
///   convention `super::lower::carve_door` uses when cutting the
///   opening; `spec/syntax` "Selectors" calls this "round-half-up").
/// * `left`   — `0`, the wall-local axis origin.
/// * `right`  — `len - 1`, the far corner. The `len.saturating_sub(1)`
///   guard returns `0` for a hypothetical `len == 0` rather than
///   underflowing `u32`, but `len == 0` is unreachable in practice:
///   `DefIr.size.w` / `.h` are `NonZeroU32`, and `wall_length` is one
///   of them — so every shipping caller has `len ≥ 1` and the
///   `right` anchor lands on a valid column.
///
/// Numeric offsets (`at=N`) are reserved for a future extension and
/// are refused with how `at=` was [`Written`] rather than silently
/// rounded to centre, as is a missing `at=` or any other value.
///
/// `super::lower::carve_door` cuts the doorway at this column and defers
/// with [`door_at_deferral`] of this refusal, so the cut, the port and
/// both of their messages read `at=` once.
pub(super) fn door_anchor_offset(member: &Member, len: u32) -> Result<u32, Written> {
    match member.ident_value("at") {
        Some("center") => Ok(len / 2),
        Some("left") => Ok(0),
        Some("right") => Ok(len.saturating_sub(1)),
        _ => Err(Written::of_ident(member, "at")),
    }
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
/// function `super::lower::fill_window` reads them with, so the two
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
/// Horizontal before vertical, the order `fill_window` asks in, so a
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

    /// Manhattan distance between two ground-plane cells — the minimal
    /// possible number of steps, so `route_path` output length can be
    /// asserted against `manhattan + 1` cells when no detour is needed.
    fn manhattan(a: (i32, i32, i32), b: (i32, i32, i32)) -> usize {
        usize::try_from((a.0 - b.0).abs() + (a.2 - b.2).abs()).expect("non-negative")
    }

    /// Structural invariants every successful route must satisfy: the
    /// endpoints are the requested ports, consecutive cells are
    /// 4-neighbour adjacent at a constant Y, no cell repeats, and no
    /// cell collides with `blocked`.
    fn assert_route_shape(
        path: &[(i32, i32, i32)],
        from: (i32, i32, i32),
        to: (i32, i32, i32),
        blocked: &HashSet<(i32, i32, i32)>,
    ) {
        assert_eq!(path.first(), Some(&from), "route must start at `from`");
        assert_eq!(path.last(), Some(&to), "route must end at `to`");
        for pair in path.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert_eq!(a.1, b.1, "route must stay at a constant Y: {a:?} -> {b:?}");
            assert_eq!(
                (a.0 - b.0).abs() + (a.2 - b.2).abs(),
                1,
                "route cells must be 4-neighbour adjacent: {a:?} -> {b:?}",
            );
        }
        let mut seen = HashSet::new();
        for cell in path {
            assert!(seen.insert(*cell), "route revisits cell {cell:?}");
            assert!(
                !blocked.contains(cell),
                "route crosses blocked cell {cell:?}"
            );
        }
    }

    /// Index-and-route shorthand so each test reads as `(from, to,
    /// blocked)` without repeating the [`BlockedIndex`] construction.
    fn route(
        from: (i32, i32, i32),
        to: (i32, i32, i32),
        blocked: &HashSet<(i32, i32, i32)>,
    ) -> Result<Vec<(i32, i32, i32)>, RoutePathError> {
        route_path(from, to, &BlockedIndex::new(blocked))
    }

    /// Number of direction changes along a path — the second component
    /// of the router's cost, re-derived so a test can pin it.
    fn turn_count(path: &[(i32, i32, i32)]) -> usize {
        path.windows(3)
            .filter(|w| {
                let d0 = (w[1].0 - w[0].0, w[1].2 - w[0].2);
                let d1 = (w[2].0 - w[1].0, w[2].2 - w[1].2);
                d0 != d1
            })
            .count()
    }

    #[test]
    fn route_path_unobstructed_is_shortest() {
        // With nothing in the way the route must not detour: the cell
        // count is exactly the Manhattan distance plus the start cell.
        let blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        let (from, to) = ((0, 0, 0), (3, 0, 2));
        let path = route(from, to, &blocked).expect("open plane routes");
        assert_route_shape(&path, from, to, &blocked);
        assert_eq!(path.len(), manhattan(from, to) + 1);
    }

    #[test]
    fn route_path_detours_around_a_wall() {
        // A solid wall of blocked cells across the straight line forces
        // the route around one end. Wall at x=2, z∈[-2, 2]; endpoints on
        // either side at z=0. Shortest detour: up/down to z=±3 and back
        // → 4 + manhattan extra steps. z=±3 lies *outside* the raw bbox
        // of blocked ∪ endpoints (z∈[-2, 2]) — the detour is only
        // reachable through the one-cell inflation margin, so this test
        // also pins the +1 inflation directly (an off-by-one there
        // leaves the router with no way around and fails the expect).
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for z in -2..=2 {
            blocked.insert((2, 0, z));
        }
        let (from, to) = ((0, 0, 0), (4, 0, 0));
        let path = route(from, to, &blocked).expect("detour exists");
        assert_route_shape(&path, from, to, &blocked);
        // Manhattan is 4; rounding the wall costs 3 extra cells each way
        // (to z=3 or z=-3 and back) → 4 + 6 steps, 11 cells.
        assert_eq!(path.len(), manhattan(from, to) + 6 + 1);
        assert!(
            path.iter().any(|c| c.2.abs() == 3),
            "the only shortest detours run through the inflated margin row, got {path:?}",
        );
    }

    #[test]
    fn route_path_prefers_fewest_turns_among_shortest_routes() {
        // Every shortest detour around the wall is 11 cells, but they
        // differ in turn count: a staircase zigzag has up to 8 turns,
        // the U along the margin row has 2. The cost's second component
        // must pick 2 — dropping `turns` from the cost (len-only
        // Dijkstra) would let heap timing pick a zigzag and the laid
        // gravel would look hand-broken.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for z in -2..=2 {
            blocked.insert((2, 0, z));
        }
        let path = route((0, 0, 0), (4, 0, 0), &blocked).expect("detour exists");
        assert_eq!(
            turn_count(&path),
            2,
            "shortest fewest-turns detour is a single U, got {path:?}",
        );
    }

    #[test]
    fn route_path_breaks_symmetric_ties_toward_positive_x() {
        // A wall across the z axis leaves two mirror-image shortest
        // detours: around the east end (+x) or the west end (-x), equal
        // in both length and turns. The fixed STEP_DIRS order expands
        // `PosX` first, so the east side must win — this is the same
        // tie-break that routes village.crn's home1↔home3 walkway
        // around home1's east face, pinned here in isolation so a
        // STEP_DIRS reorder fails a unit test and not just the village
        // integration pins.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for x in -2..=2 {
            blocked.insert((x, 0, 3));
        }
        let (from, to) = ((0, 0, 0), (0, 0, 6));
        let path = route(from, to, &blocked).expect("detour exists");
        assert_route_shape(&path, from, to, &blocked);
        assert!(
            path.contains(&(3, 0, 3)),
            "the +x-first expansion order must round the east end of the wall, got {path:?}",
        );
    }

    #[test]
    fn route_path_is_deterministic() {
        // Two runs over the same input must produce the identical cell
        // sequence — the lockfile pins walkway origin/dims, so a
        // hash-order-dependent tie-break would break reproducible builds.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for z in -2..=2 {
            blocked.insert((2, 0, z));
        }
        let a = route((0, 0, 0), (4, 0, 0), &blocked).expect("routes");
        let b = route((0, 0, 0), (4, 0, 0), &blocked).expect("routes");
        assert_eq!(a, b);
    }

    #[test]
    fn route_path_is_independent_of_blocked_insertion_order() {
        // The determinism contract must not lean on hash-map iteration
        // order: two sets with the same cells but different insertion
        // orders (and therefore different `RandomState` seeds and
        // bucket layouts) must route identically. This is the guard
        // that keeps a future refactor from sneaking an iteration-order
        // dependency into the search.
        let cells: Vec<(i32, i32, i32)> = (-2..=2).map(|z| (2, 0, z)).collect();
        let forward: HashSet<(i32, i32, i32)> = cells.iter().copied().collect();
        let reverse: HashSet<(i32, i32, i32)> = cells.iter().rev().copied().collect();
        let a = route((0, 0, 0), (4, 0, 0), &forward).expect("routes");
        let b = route((0, 0, 0), (4, 0, 0), &reverse).expect("routes");
        assert_eq!(a, b);
    }

    #[test]
    fn route_path_reports_which_endpoint_is_buried() {
        // A port buried under another placement's floor cannot anchor
        // a route; the error must say which side so the caller's
        // W_WALKWAY_BLOCKED note points at the right port.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        blocked.insert((0, 0, 0));
        blocked.insert((9, 0, 9));
        assert_eq!(
            route((0, 0, 0), (5, 0, 5), &blocked),
            Err(RoutePathError::EndpointBlocked {
                from_blocked: true,
                to_blocked: false,
            }),
        );
        assert_eq!(
            route((5, 0, 5), (9, 0, 9), &blocked),
            Err(RoutePathError::EndpointBlocked {
                from_blocked: false,
                to_blocked: true,
            }),
        );
        assert_eq!(
            route((0, 0, 0), (9, 0, 9), &blocked),
            Err(RoutePathError::EndpointBlocked {
                from_blocked: true,
                to_blocked: true,
            }),
        );
    }

    #[test]
    fn route_path_reports_enclosed_target_as_unreachable() {
        // A full ring of blocked cells around `to` leaves no route at
        // all — the search must terminate with `TargetUnreachable`
        // rather than spin.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for d in -1..=1 {
            blocked.insert((5 + d, 0, 4));
            blocked.insert((5 + d, 0, 6));
            blocked.insert((4, 0, 5 + d));
            blocked.insert((6, 0, 5 + d));
        }
        assert_eq!(
            route((0, 0, 0), (5, 0, 5), &blocked),
            Err(RoutePathError::TargetUnreachable),
        );
    }

    #[test]
    fn route_path_same_endpoints_yields_single_cell() {
        // Kept graceful rather than asserted away: `route_path` is a
        // public API, and a single-cell "route" is the honest answer
        // for coincident ports even though `lower_connects` never asks
        // (a collision-free 1-cell L never reaches the router).
        let blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        assert_eq!(route((5, 0, 5), (5, 0, 5), &blocked), Ok(vec![(5, 0, 5)]));
    }

    #[test]
    fn route_path_ignores_blocked_cells_on_other_y_planes() {
        // `blocked` is a world-space 3D set; cells at a different Y must
        // neither obstruct the route nor inflate the search bounds.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for z in -2..=2 {
            blocked.insert((2, 7, z));
        }
        let (from, to) = ((0, 0, 0), (4, 0, 0));
        let path = route(from, to, &blocked).expect("open at y=0");
        assert_eq!(path.len(), manhattan(from, to) + 1);
    }

    #[test]
    fn route_path_mixed_y_planes_only_walk_plane_obstructs() {
        // Obstacles on the walk plane and a *longer* copy of the same
        // wall on another plane, in one set: the route must detour
        // around the y=0 wall exactly as if the y=7 cells were absent.
        // A regression that inverts (or drops) the `y` filter would see
        // the taller y=7 wall, block the z=±3 margin crossing, and
        // return a longer path.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for z in -2..=2 {
            blocked.insert((2, 0, z));
        }
        for z in -4..=4 {
            blocked.insert((2, 7, z));
        }
        let (from, to) = ((0, 0, 0), (4, 0, 0));
        let path = route(from, to, &blocked).expect("detour exists at y=0");
        assert_route_shape(&path, from, to, &blocked);
        assert_eq!(path.len(), manhattan(from, to) + 6 + 1);
    }

    #[test]
    fn route_path_gives_up_past_the_area_cap() {
        // Endpoints so far apart that the bounding rectangle exceeds the
        // search cap must fail fast instead of allocating the world.
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        blocked.insert((1, 0, 0));
        assert!(matches!(
            route((0, 0, 0), (10_000_000, 0, 10_000_000), &blocked),
            Err(RoutePathError::AreaCapExceeded { area: _, cap }) if cap == ROUTE_AREA_CAP,
        ));
    }

    #[test]
    fn route_path_area_cap_boundary_is_exclusive() {
        // Pin the `>` in the cap check from both sides. The endpoints
        // are adjacent, so the allowed case resolves in a handful of
        // heap pops even though the rectangle is at the cap — the cap
        // bounds the worst case, not every search. With `from=(0,0,0)`,
        // `to=(1,0,0)` and one far blocked cell at `(a, 0, 1997)`, the
        // inflated rectangle spans `(a+3) × 2000`: `a=1997` lands
        // exactly on the 4-million cap (allowed), `a=1998` is one
        // column past it (refused with both numbers reported).
        let at_cap: HashSet<(i32, i32, i32)> = std::iter::once((1997, 0, 1997)).collect();
        let path = route((0, 0, 0), (1, 0, 0), &at_cap).expect("area == cap is allowed");
        assert_eq!(path, vec![(0, 0, 0), (1, 0, 0)]);

        let past_cap: HashSet<(i32, i32, i32)> = std::iter::once((1998, 0, 1997)).collect();
        assert_eq!(
            route((0, 0, 0), (1, 0, 0), &past_cap),
            Err(RoutePathError::AreaCapExceeded {
                area: 4_002_000,
                cap: ROUTE_AREA_CAP,
            }),
        );
    }

    #[test]
    fn route_path_pins_the_exact_tie_broken_detour() {
        // The full cell sequence for the wall fixture, pinned once so
        // any change to the cost function, the STEP_DIRS order, or the
        // equal-cost keep-first rule shows up as a concrete
        // before/after diff instead of a distant integration failure.
        // (The bounding-box pins in `village_lower.rs` survive a
        // tie-break flip because both sides share a bbox; this pin does
        // not.)
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        for z in -2..=2 {
            blocked.insert((2, 0, z));
        }
        let path = route((0, 0, 0), (4, 0, 0), &blocked).expect("detour exists");
        assert_eq!(
            path,
            vec![
                (0, 0, 0),
                (0, 0, 1),
                (0, 0, 2),
                (0, 0, 3),
                (1, 0, 3),
                (2, 0, 3),
                (3, 0, 3),
                (4, 0, 3),
                (4, 0, 2),
                (4, 0, 1),
                (4, 0, 0),
            ],
        );
    }

    #[test]
    fn route_path_detours_along_each_edge_of_the_coordinate_space() {
        // `search_rect` only refuses a rectangle whose one-cell margin
        // leaves `i32`, so the margin itself may lie on `i32::MIN` or
        // `i32::MAX`, and the router expands the cells there. Each of
        // those cells has a neighbour past the edge, which the router
        // has to skip rather than compute.
        //
        // The fixture is laid out in local coordinates: `a` is the
        // distance from the edge and `b` runs along it. The endpoints
        // are at `a = 1`, and a wall at `b = 2` covers `a ∈ 1..=3`.
        // The only short way round is through `a = 0`, the edge itself
        // (7 cells); the far way round, through `a = 4`, takes 11.
        type Local = fn(i32, i32) -> (i32, i32, i32);
        let edges: [(&str, Local); 4] = [
            ("+x", |a, b| (i32::MAX - a, 0, b)),
            ("-x", |a, b| (i32::MIN + a, 0, b)),
            ("+z", |a, b| (b, 0, i32::MAX - a)),
            ("-z", |a, b| (b, 0, i32::MIN + a)),
        ];
        for (edge, at) in edges {
            let blocked: HashSet<(i32, i32, i32)> = (1..=3).map(|a| at(a, 2)).collect();
            let (from, to) = (at(1, 0), at(1, 4));
            let path = route(from, to, &blocked)
                .unwrap_or_else(|e| panic!("{edge}: a detour along the edge exists, got {e:?}"));
            assert_route_shape(&path, from, to, &blocked);
            assert_eq!(
                path,
                [(1, 0), (0, 0), (0, 1), (0, 2), (0, 3), (0, 4), (1, 4)]
                    .map(|(a, b)| at(a, b))
                    .to_vec(),
                "{edge}: the route must run along the edge of the coordinate space",
            );
        }
    }

    #[test]
    fn l_path_x_then_z_dedupes_corner() {
        let path = l_path((0, 0, 0), (3, 0, 2));
        // Expected order: (0,0,0) (1,0,0) (2,0,0) (3,0,0) — x leg
        //                 (3,0,1) (3,0,2)                  — z leg
        assert_eq!(
            path,
            vec![
                (0, 0, 0),
                (1, 0, 0),
                (2, 0, 0),
                (3, 0, 0),
                (3, 0, 1),
                (3, 0, 2),
            ],
        );
    }

    #[test]
    fn l_path_negative_axes_step_backwards() {
        let path = l_path((2, 0, 1), (0, 0, -2));
        assert_eq!(
            path,
            vec![
                (2, 0, 1),
                (1, 0, 1),
                (0, 0, 1),
                (0, 0, 0),
                (0, 0, -1),
                (0, 0, -2),
            ],
        );
    }

    #[test]
    fn l_path_same_endpoints_yields_single_cell() {
        let path = l_path((5, 0, 5), (5, 0, 5));
        assert_eq!(path, vec![(5, 0, 5)]);
    }

    /// A path with no x leg is all z leg, and its first cell is still the
    /// one the x leg pushed: the stepping order alone keeps it single.
    #[test]
    fn l_path_along_z_alone_lays_each_cell_once() {
        let path = l_path((4, 0, 0), (4, 0, 3));
        assert_eq!(path, vec![(4, 0, 0), (4, 0, 1), (4, 0, 2), (4, 0, 3)]);
    }

    /// A north–south strip must not cost time quadratic in its length. A
    /// lookup into the laid cells on every z step would make it quadratic:
    /// a million-cell strip would then take hours in a debug build, where
    /// the walk without one takes milliseconds. The deadline sits orders of
    /// magnitude from both, so a slow runner cannot trip it and the
    /// quadratic walk cannot meet it; the walk runs on its own thread so a
    /// regression fails here instead of hanging the suite.
    #[test]
    fn l_path_lays_a_long_z_strip_in_less_than_quadratic_time() {
        use std::sync::mpsc::RecvTimeoutError;
        const LEN: i32 = 1_000_000;
        let (tx, rx) = std::sync::mpsc::channel();
        let walk = std::thread::spawn(move || {
            let _ = tx.send(l_path((0, 0, 0), (0, 0, LEN - 1)));
        });
        let path = match rx.recv_timeout(std::time::Duration::from_secs(60)) {
            Ok(path) => path,
            Err(RecvTimeoutError::Timeout) => {
                panic!("a million-cell z strip was not laid within a minute")
            }
            Err(RecvTimeoutError::Disconnected) => match walk.join() {
                Err(payload) => std::panic::resume_unwind(payload),
                Ok(()) => panic!("the walk thread ended without sending a path"),
            },
        };
        assert_eq!(path.len(), usize::try_from(LEN).expect("fits"));
        assert_eq!(path.first(), Some(&(0, 0, 0)));
        assert_eq!(path.last(), Some(&(0, 0, LEN - 1)));
    }

    fn sample_key() -> WalkwayScopeKey {
        use crate::ids::{PlaceId, PortId, SiteName, WalkwayEndpoint};
        let site = SiteName::new("s").expect("site");
        let a = WalkwayEndpoint {
            place: PlaceId::new("a").expect("place"),
            port: PortId::new("entry").expect("port"),
        };
        let b = WalkwayEndpoint {
            place: PlaceId::new("b").expect("place"),
            port: PortId::new("entry").expect("port"),
        };
        WalkwayScopeKey::from_parts(&site, &a, &b).expect("from_parts")
    }

    #[test]
    fn build_walkway_array_fills_unblocked_cells() {
        let path = vec![(0, 0, 0), (1, 0, 0), (1, 0, 1)];
        let blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        let layout = build_walkway_array(
            &path,
            BlockState::bare("minecraft:gravel"),
            &blocked,
            &sample_key(),
        );
        assert_eq!(layout.blocked_count, 0);
        assert_eq!(layout.origin, (0, 0, 0));
        assert_eq!(layout.array.dims, Dims { x: 2, y: 1, z: 2 });
        // Three of the four cells should hold gravel; (0,0,1) was never
        // in the path, so it stays air.
        let palette_id_at = |x: u32, z: u32| -> &str {
            let i = layout.array.dims.index(x, 0, z).expect("in-range");
            let pi = layout.array.voxels[i];
            layout.array.palette.entries[usize::from(pi.0)].id.as_str()
        };
        assert_eq!(palette_id_at(0, 0), "minecraft:gravel");
        assert_eq!(palette_id_at(1, 0), "minecraft:gravel");
        assert_eq!(palette_id_at(1, 1), "minecraft:gravel");
        assert_eq!(palette_id_at(0, 1), "minecraft:air");
    }

    #[test]
    fn build_walkway_array_skips_blocked_cells() {
        let path = vec![(0, 0, 0), (1, 0, 0), (2, 0, 0)];
        let mut blocked: HashSet<(i32, i32, i32)> = HashSet::new();
        blocked.insert((1, 0, 0));
        let layout = build_walkway_array(
            &path,
            BlockState::bare("minecraft:gravel"),
            &blocked,
            &sample_key(),
        );
        assert_eq!(layout.blocked_count, 1);
        // Middle cell stays air despite being on the path.
        let mid = layout.array.dims.index(1, 0, 0).unwrap();
        assert_eq!(layout.array.voxels[mid], PaletteIndex::AIR);
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
        // `fill_window` reads an absent `offset=` as `0` and cuts the
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
        // 3-high course: both wrong. `fill_window` asks the horizontal
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
