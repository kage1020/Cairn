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

/// Catalog token a `pressure_plate`'s default material comes from; the pack spells it per edition.
pub(super) const PRESSURE_PLATE_TOKEN: &str = "pressure_plate.default";

/// Pressure plate id used when the pack cannot supply one: the Java spelling, Java being the base
/// edition (`spec/versioning-editions` "Backend = data tables").
pub(super) const PRESSURE_PLATE_BASE_ID: &str = "minecraft:oak_pressure_plate";

/// Shift a wall voxel's `(x, z)` one voxel toward the interior, saturating at 0 and unchecked.
fn shift_inward(side: WallSide, x: u32, z: u32) -> (u32, u32) {
    let (dx, dz) = side.outward_normal();
    (x.saturating_add_signed(-dx), z.saturating_add_signed(-dz))
}

/// Which side of the wall a `pressure_plate at=<side>.outside | inside.<side>` sits on.
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

/// Parse a `pressure_plate at=` value, or the `W_DEFERRED_MEMBER` reason for any other shape.
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

/// Paint a `pressure_plate` fixture as a single plate voxel.
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
    // Interned only once the cell exists, so a refusal leaves no palette entry.
    if !canvas.try_paint((x, y_world, z), || {
        palette.intern(BlockState::bare(base_id))
    }) {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "pressure_plate resolved to a voxel outside the struct's block array",
        ));
    }
}

/// The world voxel a plate paints onto, or `None` after a diagnostic.
///
/// With no exterior cell, `<side>.outside` falls back to the wall's foundation, at `y = 0` only.
/// `inside.<side>` must land strictly inside the wall ring.
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
        // INVARIANT(wall-grid-validated): the two refusals above are every rejection
        // `wall_local_to_grid` makes; release builds drop the plate.
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
            // A saturating shift that stays on the wall column reaches no exterior cell either.
            let shift_reached_exterior =
                (sx, sz) != (wx, wz) && ctx.dims.index(sx, y_world, sz).is_some();
            if shift_reached_exterior {
                Some((sx, y_world, sz))
            } else if y_world == 0 {
                // The wall column's y=0 cell is floor (walls start at y=1).
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
                    // The redstone pass reads the binding, not the block array.
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

/// Why an `at=inside.<side>` plate stepped onto `(sx, sz)` has no interior cell, or `None`.
///
/// The interior is strictly inside the footprint's wall ring, whether or not walls paint it.
fn inside_plate_refusal(
    side: WallSide,
    offset: u32,
    y: u32,
    (sx, sz): (u32, u32),
    ctx: &StructCtx<'_>,
) -> Option<String> {
    let strictly_inside = |c: u32, extent: u32| {
        c > ctx.overhang && c < ctx.overhang.saturating_add(extent).saturating_sub(1)
    };
    if strictly_inside(sx, ctx.interior_w) && strictly_inside(sz, ctx.interior_h) {
        return None;
    }
    let name = side_name(side);
    let length = wall_length(side, ctx.interior_w, ctx.interior_h);
    let thin: Vec<(&str, u32)> = [("w", ctx.interior_w), ("h", ctx.interior_h)]
        .into_iter()
        .filter(|&(_, extent)| extent < 3)
        .collect();
    // With no thin axis, only the two ends of the wall miss the interior.
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
    // Without an overhang, `<side>.outside` has no ring to sit in, so it is no remedy.
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

/// The block id a `pressure_plate` paints: its `mat_slot=`, or the pack's default.
///
/// A binding with a state literal defers and paints nothing. An unresolved binding was already
/// reported, so the default raises no finding of its own.
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

/// The pack's pressure plate id, or [`PRESSURE_PLATE_BASE_ID`], checked against the target;
/// `None` after diagnosing an id the target lacks.
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
        // INVARIANT(validated-id-refuses-one-way): `validated_id` answers the state or `UnknownId`.
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
