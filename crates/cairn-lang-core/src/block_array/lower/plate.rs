//! Placing a `pressure_plate` member.

use crate::ast::ValueKind;
use crate::check::Diagnostic;
use crate::intent::Member;

use crate::block_array::BlockState;
use crate::block_array::args::{NonNegRead, diag_deferred_member_reason, nonneg_int_or_defer};
use crate::block_array::material::{IdOrigin, MaterialDeferred, TargetRegistry, validated_id};
use crate::block_array::openings::{WallSide, wall_length, wall_local_to_grid};

use super::canvas::{MemberCanvas, ScopePalette};
use super::members::{StructCtx, resolve_member_state, side_name};
use super::stair::shift_outward;
use super::token_diag::diag_unknown_id;

/// Catalog token a `pressure_plate` member's default material comes from.
///
/// The id itself is edition-specific — Java spells it
/// `oak_pressure_plate`, Bedrock `wooden_pressure_plate` — and lowering
/// has no edition of its own to decide with. Reading it out of the pack
/// keeps the knowledge in the one place that is already per-edition, at
/// the cost of the token also being writable by an author as
/// `@pressure_plate.default`. That is accepted: the alternative is a
/// second pack component whose only member is this one row.
pub(super) const PRESSURE_PLATE_TOKEN: &str = "pressure_plate.default";

/// Pressure plate id used when the pack cannot supply one — no registry
/// at all, or a registry with no [`PRESSURE_PLATE_TOKEN`] row.
///
/// Neither case carries an edition, and `spec/versioning-editions` "Backend =
/// data tables" makes Java the base, so the Java spelling is the honest
/// default. It is still checked against the pinned target before it reaches a
/// palette.
/// Species-specific plates (spruce, dark oak, ...) still come from a
/// `mat_slot=` binding, which is honoured verbatim — mirroring
/// [`FLAT_BASE_ID`]'s contract, not [`STAIR_BASE_ID`]'s. A plate attaches
/// no blockstates, so it has nothing to require of the block it names.
///
/// [`FLAT_BASE_ID`]: crate::block_array::roof::FLAT_BASE_ID
/// [`STAIR_BASE_ID`]: crate::block_array::roof::STAIR_BASE_ID
pub(super) const PRESSURE_PLATE_BASE_ID: &str = "minecraft:oak_pressure_plate";

/// Shift a wall voxel's `(x, z)` by one voxel toward the interior, the
/// cell an `at=inside.<side>` fixture sits on at whatever row it asks for.
///
/// The step saturates at 0 and never checks the result: whether the cell
/// is really inside the building, rather than a block of another wall, is
/// [`inside_plate_refusal`]'s question, asked by the caller.
fn shift_inward(side: WallSide, x: u32, z: u32) -> (u32, u32) {
    let (dx, dz) = side.outward_normal();
    (x.saturating_add_signed(-dx), z.saturating_add_signed(-dz))
}

/// Which side of the wall a `pressure_plate at=…` anchor sits on.
///
/// The DSL spells this as a two-segment `DotRef`:
/// - `at=<side>.outside` — plate sits on the exterior overhang column
///   adjacent to `<side>`. When the struct has no overhang the plate
///   falls back to the wall's own foundation cell (the floor voxel
///   directly beneath the wall column) so authors can still write
///   `at=front.outside` on a plain flat-roof gatehouse.
/// - `at=inside.<side>` — plate sits one voxel toward the interior from
///   the wall's own column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlateAnchor {
    Outside(WallSide),
    Inside(WallSide),
}

impl PlateAnchor {
    fn side(self) -> WallSide {
        match self {
            Self::Outside(s) | Self::Inside(s) => s,
        }
    }
}

/// Parse a `pressure_plate at=…` value into a [`PlateAnchor`].
///
/// Returns `Err(reason)` when the `at=` argument is not a two-segment
/// dotted reference of the shape the DSL accepts. The caller turns the
/// reason into a `W_DEFERRED_MEMBER` diagnostic anchored on the member.
fn plate_anchor_of(member: &Member) -> Result<PlateAnchor, String> {
    let raw = member
        .intent_state
        .get("at")
        .ok_or_else(|| "pressure_plate without `at=` is not supported (use `at=<side>.outside` or `at=inside.<side>`)".to_owned())?;
    let ValueKind::DotRef(dotref) = &raw.value.kind else {
        return Err(
            "pressure_plate `at=` must be `<side>.outside` or `inside.<side>` (two-segment dotted reference)"
                .to_owned(),
        );
    };
    let segments = dotref.segments();
    if segments.len() != 2 {
        return Err(format!(
            "pressure_plate `at={dotref}` must have exactly two segments (`<side>.outside` or `inside.<side>`)",
        ));
    }
    let (head, tail) = (segments[0].as_str(), segments[1].as_str());
    // `<side>.outside` — the head names the wall side, the tail is the
    // literal `outside`. Everything else fails through to the paired
    // `inside.<side>` shape below.
    if tail == "outside" {
        return WallSide::from_ident(head)
            .map(PlateAnchor::Outside)
            .ok_or_else(|| {
                format!(
                    "pressure_plate `at={head}.outside`: `{head}` is not one of front, back, left, right",
                )
            });
    }
    if head == "inside" {
        return WallSide::from_ident(tail)
            .map(PlateAnchor::Inside)
            .ok_or_else(|| {
                format!(
                    "pressure_plate `at=inside.{tail}`: `{tail}` is not one of front, back, left, right",
                )
            });
    }
    Err(format!(
        "pressure_plate `at={dotref}` is not a recognised anchor (use `<side>.outside` or `inside.<side>`)",
    ))
}

/// Paint a `pressure_plate` fixture onto the block array as a single
/// vanilla plate voxel.
///
/// Only the compound-anchor subset the intent IR currently exposes is
/// honoured:
/// - `at=<side>.outside` / `at=inside.<side>` compound anchors,
/// - `offset=N` and `y=N` non-negative integer offsets along the wall's
///   axis and the vertical axis (both default to 0 when absent),
/// - an optional `mat_slot=` that resolves to a bare block id (no
///   `[property=…]` state literal). Anything else emits a
///   `W_DEFERRED_MEMBER` warning so the fixture is loud about what it
///   dropped.
///
/// The `-> sig.<name>` signal binding on `member.binding` is parsed but
/// not consumed here — sensor/actuator wiring belongs to the redstone
/// lowering pass that voxelises `logic` / `assert` / `circuit` items.
/// The physical block is placed regardless, mirroring `carve_door`'s
/// handling of `mat_slot=`.
pub(super) fn fill_pressure_plate(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let anchor = match plate_anchor_of(member) {
        Ok(a) => a,
        Err(reason) => {
            diagnostics.push(diag_deferred_member_reason(member, &reason));
            return;
        }
    };
    let Some((x, y_world, z)) = plate_voxel_position(member, y_offset, anchor, ctx, diagnostics)
    else {
        return;
    };
    let Some(base_id) = plate_id_for_member(member, ctx, diagnostics) else {
        return;
    };
    // Interned only once the cell is known to exist. `plate_voxel_position`
    // already rejects every out-of-range anchor it can name, so this arm is
    // the guard against a future one it cannot — and a guard that leaves a
    // palette entry behind reports a block the structure does not contain.
    if !canvas.try_paint((x, y_world, z), || {
        palette.intern(BlockState::bare(base_id))
    }) {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "pressure_plate resolved to a voxel outside the struct's block array",
        ));
    }
}

/// Resolve a `pressure_plate` anchor + `offset=` + `y=` into the world
/// voxel `(x, y, z)` the plate should paint onto, or `None` (with a
/// diagnostic already pushed) when any of the inputs is missing / out
/// of range / lands outside the block array. The one `None` without a
/// diagnostic is the `INVARIANT(wall-grid-validated)` arm, which the
/// checks before it make unreachable and which asserts in debug builds.
///
/// `<side>.outside` shifts one voxel toward the exterior. When the
/// shift lands outside the struct's dims *and* `y_world == 0`, it falls
/// back to the wall's own foundation cell (the floor voxel directly
/// under the wall column) so authors can still write `at=front.outside`
/// on a plain flat-roof gatehouse with no overhang. Above the floor row
/// the same fallback would overwrite the wall block that the massing
/// phase painted, so `y_world >= 1` without a usable exterior cell
/// defers instead.
///
/// `inside.<side>` always shifts one voxel inward and defers unless the
/// shifted cell is strictly inside the wall ring on both horizontal axes
/// (see [`inside_plate_refusal`]), at every row including the floor row:
/// otherwise the plate would sit on the ring, where the walls paint their
/// courses, or outside the building altogether.
fn plate_voxel_position(
    member: &Member,
    y_offset: u32,
    anchor: PlateAnchor,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(u32, u32, u32)> {
    let side = anchor.side();
    let offset = match nonneg_int_or_defer(member, "offset", diagnostics) {
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 0,
        NonNegRead::Deferred => return None,
    };
    let y_local = match nonneg_int_or_defer(member, "y", diagnostics) {
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 0,
        NonNegRead::Deferred => return None,
    };
    let y_world = y_local.saturating_add(y_offset);
    if y_world >= ctx.dims.y {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "pressure_plate y={y_world} does not fit in the struct (dims.y={})",
                ctx.dims.y,
            ),
        ));
        return None;
    }
    let length = wall_length(side, ctx.interior_w, ctx.interior_h);
    if offset >= length {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "pressure_plate `offset={offset}` runs past the {} wall (length {length})",
                side_name(side),
            ),
        ));
        return None;
    }
    let Some((wx, _wy, wz)) = wall_local_to_grid(
        side,
        offset,
        y_world,
        ctx.overhang,
        ctx.interior_w,
        ctx.interior_h,
        ctx.dims,
    ) else {
        // INVARIANT(wall-grid-validated): the two refusals above
        // (`y_world >= dims.y`, `offset >= length`) each pushed their own
        // diagnostic and returned, and they are every rejection
        // `wall_local_to_grid` performs — so it has nothing to reject.
        // Reaching here means one of them stopped agreeing with the
        // helper, which is a compiler bug, not something the author can
        // act on, so it is an assertion rather than a warning. Loud in
        // debug builds; release builds drop the plate.
        debug_assert!(
            false,
            "pressure_plate cell u={offset} y={y_world} on the {} wall is outside the wall grid; \
             the `offset < wall_length` / `y_world < dims.y` checks stopped agreeing with \
             `wall_local_to_grid`",
            side_name(side),
        );
        return None;
    };
    match anchor {
        PlateAnchor::Outside(_) => {
            let (sx, sz) = shift_outward(side, wx, wz);
            // The shift succeeds only when it produces a genuinely new
            // voxel that lives in dims. A saturating shift that lands
            // back on the wall column (Left/Back walls at coordinate 0
            // when overhang=0) is not a real exterior cell, so treat it
            // as "no exterior available" just like an out-of-dims shift.
            let shift_reached_exterior =
                (sx, sz) != (wx, wz) && ctx.dims.index(sx, y_world, sz).is_some();
            if shift_reached_exterior {
                Some((sx, y_world, sz))
            } else if y_world == 0 {
                // Foundation fallback: the wall column's y=0 cell is
                // still floor material (walls start at y=1), so
                // replacing it with a plate is honest to the anchor
                // name.
                Some((wx, y_world, wz))
            } else {
                diagnostics.push(diag_deferred_member_reason(
                    member,
                    &format!(
                        "pressure_plate `at={}.outside` at y={y_world} has no exterior voxel to sit on (no roof on this struct draws an overhang; the foundation fallback only applies at y=0 so a higher plate would overwrite the wall)",
                        side_name(side),
                    ),
                ));
                None
            }
        }
        PlateAnchor::Inside(_) => {
            let (sx, sz) = shift_inward(side, wx, wz);
            if let Some(mut reason) = inside_plate_refusal(side, offset, y_world, (sx, sz), ctx) {
                if member.binding.is_some() {
                    // The redstone pass reads the binding, not the block
                    // array, so the signal outlives the refused block.
                    reason.push_str(
                        ". Its signal binding still reaches the netlist, with no plate placed to drive it",
                    );
                }
                diagnostics.push(diag_deferred_member_reason(member, &reason));
                return None;
            }
            Some((sx, y_world, sz))
        }
    }
}

/// Why an `at=inside.<side>` plate whose inward step landed on `(sx, sz)`
/// at row `y` has no interior cell to sit on, or `None` when it has one.
///
/// The interior is what the wall ring encloses. The ring is the
/// footprint's outermost row and column, offset by the roof overhang, and
/// it is where `walls` paint their courses, so only a cell strictly inside
/// it can take a plate without replacing a wall block. The ring is decided
/// from the footprint whether or not a `walls` member paints it, so a cell
/// on or outside it is refused either way, and the reason claims a wall
/// only at a row one is painted on.
fn inside_plate_refusal(
    side: WallSide,
    offset: u32,
    y: u32,
    (sx, sz): (u32, u32),
    ctx: &StructCtx<'_>,
) -> Option<String> {
    // Saturation cannot turn an outside cell into an inside one: that
    // needs `overhang + extent > u32::MAX`, where `dims` saturates too and
    // `fits_volume_budget` has already dropped the struct.
    let strictly_inside = |c: u32, extent: u32| {
        c > ctx.overhang && c < ctx.overhang.saturating_add(extent).saturating_sub(1)
    };
    if strictly_inside(sx, ctx.interior_w) && strictly_inside(sz, ctx.interior_h) {
        return None;
    }
    let name = side_name(side);
    let length = wall_length(side, ctx.interior_w, ctx.interior_h);
    // An interior needs one voxel between two walls on each axis, so a
    // size below 3 on either axis has none for any side or offset.
    let thin: Vec<(&str, u32)> = [("w", ctx.interior_w), ("h", ctx.interior_h)]
        .into_iter()
        .filter(|&(_, extent)| extent < 3)
        .collect();
    // With both axes at least 3, every offset but the two ends steps into
    // the interior, so a refusal with no thin axis is always a corner.
    let corner = thin.is_empty() || offset == 0 || offset == length - 1;
    let valid_offsets = if length >= 3 {
        format!("from 1 to {}", length - 2)
    } else {
        "away from both ends of the wall".to_owned()
    };
    if thin.is_empty() {
        let inside_it = if ctx.wall_column.course_top_at(y).is_some() {
            "belongs to the neighbouring wall"
        } else {
            "is not an interior cell of this struct"
        };
        return Some(format!(
            "pressure_plate `at=inside.{name} offset={offset}` is at a corner of the {name} wall, \
             so the voxel inside it {inside_it}; use an `offset=` {valid_offsets} to reach an \
             interior voxel",
        ));
    }
    let also_corner = if corner {
        format!(", and `offset={offset}` is at a corner of the {name} wall besides")
    } else {
        String::new()
    };
    let sizes = thin
        .iter()
        .map(|(axis, extent)| format!("size.{axis} is {extent}"))
        .collect::<Vec<_>>()
        .join(" and ");
    let grow = thin
        .iter()
        .map(|(axis, _)| format!("`size.{axis}` to at least 3"))
        .collect::<Vec<_>>()
        .join(" and ");
    let use_offset = if corner {
        format!(" and use an `offset=` {valid_offsets}")
    } else {
        String::new()
    };
    // Without an overhang `<side>.outside` finds no exterior cell above
    // the floor row and, at it, falls back to the cell under the wall, so
    // it is a remedy only when a roof draws an overhang ring to sit in.
    let outside = if ctx.overhang > 0 {
        format!(", or anchor the plate with `at={name}.outside`")
    } else {
        String::new()
    };
    Some(format!(
        "pressure_plate `at=inside.{name} offset={offset}`: the struct's {sizes}, so it has no \
         interior voxel{also_corner}; an interior needs a size of at least 3 on both axes. \
         Grow {grow}{use_offset}{outside}",
    ))
}

/// Resolve a `pressure_plate` `mat_slot=` binding into the concrete
/// block id the palette entry should carry, defaulting to
/// [`PRESSURE_PLATE_BASE_ID`] when no binding is present or the resolver
/// returned no state.
///
/// `resolve_member_state` already emits `W_ABSTRACT_TOKEN_DEFERRED` /
/// `E_UNKNOWN_ABSTRACT_TOKEN` for abstract-token failures, and a missing
/// theme is the scope's `W_NO_THEME_BOUND`: a `pressure_plate` reads its
/// `mat_slot=`, so `lower_body_to_block_array` pushed one for the body a
/// plate carrying one sits in. A `None` return here is therefore already
/// diagnosed upstream — echoing the failure with another
/// `W_DEFERRED_MEMBER` would just double up on the same root cause.
/// Falling back to `PRESSURE_PLATE_BASE_ID` keeps the fixture visible
/// in-game so authors can still read the artefact.
///
/// A resolved state with non-empty `properties` still defers *and*
/// skips the paint: the block-array IR has no handling for bracketed
/// state literals on plates, so silently reducing a `plate[...]`
/// binding to a plain plate would drop the author's intent. This is
/// stricter than `fill_stair`'s current behaviour (which defers but
/// keeps painting) — `pressure_plate` has no geometry-derived state
/// axis of its own, so the plain-plate fallback carries less signal
/// than the stair band's does.
pub(super) fn plate_id_for_member(
    member: &Member,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    let resolved = resolve_member_state(
        member,
        ctx.scope,
        ctx.registry,
        diagnostics,
        ctx.theme_missing,
    );
    if let Some(state) = &resolved
        && !state.properties.is_empty()
    {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "pressure_plate does not honour bracketed state literals; the `mat_slot=` binding to `{}[...]` was not applied",
                state.id,
            ),
        ));
        return None;
    }
    match resolved {
        Some(state) => Some(state.id),
        None => pack_plate_default(member, ctx.registry, diagnostics),
    }
}

/// The pressure plate id for this compile's edition, checked against the
/// target the way an authored id is.
///
/// Asks the pack for [`PRESSURE_PLATE_TOKEN`] and falls back to
/// [`PRESSURE_PLATE_BASE_ID`] when there is no pack to ask, or when the
/// pack declares no such row. A pack that declares it wins, which is how
/// `--edition bedrock` stops emitting the Java-only `oak_pressure_plate`.
///
/// The result goes through [`validated_id`] because it reaches the palette
/// without passing [`resolve_block_state`]. Skipping it would leave one
/// path in the build — the one whose default is edition-specific and
/// whose fallback is a Java spelling — writing an id nothing verified,
/// which is the failure this whole pass exists to remove. Returns `None`
/// after diagnosing, so the plate is dropped rather than painted with an
/// id the target has no block for.
///
/// [`resolve_block_state`]: crate::block_array::material::resolve_block_state
fn pack_plate_default(
    member: &Member,
    registry: Option<&dyn TargetRegistry>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    let (state, origin) = match registry.and_then(|r| r.lookup(PRESSURE_PLATE_TOKEN)) {
        Some(state) => (
            state,
            IdOrigin::Catalog {
                token: PRESSURE_PLATE_TOKEN.to_owned(),
            },
        ),
        None => (
            BlockState::bare(PRESSURE_PLATE_BASE_ID),
            IdOrigin::Builtin {
                token: PRESSURE_PLATE_TOKEN,
            },
        ),
    };
    match validated_id(state, registry, &origin) {
        Ok(state) => Some(state.id),
        Err(MaterialDeferred::UnknownId(unknown)) => {
            diagnostics.push(diag_unknown_id(member.span.clone(), &unknown));
            None
        }
        // INVARIANT(validated-id-refuses-one-way): `validated_id` returns either
        // the state or `UnknownId`; it has no path to the three deferral
        // variants, which describe how a `mat_slot=` value failed to
        // resolve and there is no such value here.
        Err(other) => {
            debug_assert!(
                false,
                "validated_id returned {other:?} for the pressure plate default, \
                 which resolves no mat_slot= value",
            );
            None
        }
    }
}
