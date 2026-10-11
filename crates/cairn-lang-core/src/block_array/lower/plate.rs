//! Placing a `pressure_plate` member.

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
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
/// The id is edition-specific (Java `oak_pressure_plate`, Bedrock
/// `wooden_pressure_plate`) and lowering has no edition to decide with, so
/// the pack, already per-edition, supplies it. An author can write the
/// token too, as `@pressure_plate.default`, which beats a second pack
/// component holding this one row.
pub(super) const PRESSURE_PLATE_TOKEN: &str = "pressure_plate.default";

/// Pressure plate id used when the pack cannot supply one — no registry
/// at all, or a registry with no [`PRESSURE_PLATE_TOKEN`] row.
///
/// Neither case carries an edition, and `spec/versioning-editions` "Backend =
/// data tables" makes Java the base, so the Java spelling is the honest
/// default. It is still checked against the pinned target before it reaches a
/// palette.
///
/// Species-specific plates (spruce, dark oak, ...) come from a `mat_slot=`
/// binding, honoured verbatim as [`FLAT_BASE_ID`]'s contract is, not
/// [`STAIR_BASE_ID`]'s: a plate attaches no blockstates, so it requires
/// nothing of the block it names.
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
/// Written as a two-segment `DotRef`:
/// - `at=<side>.outside`: the overhang cell just outside `<side>`, or, on
///   the floor row of a struct with no overhang, the wall's foundation
///   cell.
/// - `at=inside.<side>`: one voxel inward from the wall's column.
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
/// `Err` carries the `W_DEFERRED_MEMBER` reason for any other shape.
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
/// Honours:
/// - `at=<side>.outside` / `at=inside.<side>` compound anchors,
/// - `offset=N` and `y=N`, non-negative offsets along the wall and up
///   (both `0` when absent),
/// - an optional `mat_slot=` resolving to a bare block id (no
///   `[property=…]` state literal).
///
/// Anything else defers with `W_DEFERRED_MEMBER`. The `-> sig.<name>`
/// binding on `member.binding` is the redstone pass's to wire; the block
/// is placed regardless.
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
    // Interned only once the cell exists, so a refusal leaves no palette
    // entry. `plate_voxel_position` refuses every out-of-range anchor it
    // can name; this catches one it cannot.
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
        // INVARIANT(wall-grid-validated): the two refusals above are
        // every rejection `wall_local_to_grid` makes, so reaching here is
        // a compiler bug, not the author's: an assertion rather than a
        // warning. Release builds drop the plate.
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
            // A saturating shift that stays on the wall column (left or
            // back wall at coordinate 0 with no overhang) reaches no
            // exterior cell, like a shift out of dims.
            let shift_reached_exterior =
                (sx, sz) != (wx, wz) && ctx.dims.index(sx, y_world, sz).is_some();
            if shift_reached_exterior {
                Some((sx, y_world, sz))
            } else if y_world == 0 {
                // The wall column's y=0 cell is floor (walls start at
                // y=1), so a plate there is still at the wall's foot.
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
/// A `None` from `resolve_member_state` is already reported: a token
/// failure by it, a missing theme as the body's `W_NO_THEME_BOUND`, since
/// a plate reads its `mat_slot=`. Another `W_DEFERRED_MEMBER` would report
/// one cause twice. The default keeps the fixture visible in-game.
///
/// A resolved state with `properties` defers *and* skips the paint:
/// reducing a `plate[...]` binding to a plain plate would drop the
/// author's intent. That is stricter than `fill_stair`, which defers but
/// keeps painting, because a plate has no geometry-derived state, so a
/// plain one carries less of what was asked than the stair band does.
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
/// without passing [`resolve_block_state`]; skipping it would leave the
/// one path with an edition-specific default and a Java fallback writing
/// an unverified id. `None` after diagnosing, so the plate is dropped
/// rather than painted with an id the target has no block for.
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
        // INVARIANT(validated-id-refuses-one-way): `validated_id` answers
        // the state or `UnknownId`; the other variants describe a
        // `mat_slot=` value, and there is none here.
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
