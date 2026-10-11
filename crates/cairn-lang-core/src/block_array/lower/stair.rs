//! Painting an eave `stair` member.

use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::PaletteIndex;
use crate::block_array::args::{
    NonNegRead, diag_deferred_member_reason, ident_or_ignore, nonneg_int_or_defer,
};
use crate::block_array::openings::{WallSide, wall_length, wall_local_to_grid};
use crate::block_array::roof::{Cardinal, STAIR_BASE_ID, StairShape, shed_high_side, stair_state};

use super::canvas::{MemberCanvas, ScopePalette};
use super::members::{StructCtx, resolve_member_state, side_name, side_of};
use super::roof_fill::{GeometryMemberDescription, geometry_material_id};

/// Voxelise a `stair` member as a horizontal band of stair blocks along one
/// wall — the eave pattern the `themed-tower` example uses to trim the
/// transition between floors.
///
/// Only the subset the example exercises is implemented: `kind=stairs`, a
/// `side=` naming one of the four cardinal walls, an optional
/// `half=top|bottom` (defaults to `top` so a plain `stair` reads as an
/// inverted eave), an optional `facing=out|in` (defaults to `out`) that
/// rotates the stair so its riser points away from the interior, and an
/// optional `shape=straight|outer_left|outer_right` (defaults to
/// `straight`). Any other `kind=` / `facing=` / `shape=` fires
/// `W_DEFERRED_MEMBER`. The row lands at `y = y_offset + (local y | 0)`
/// in the overhang column one voxel outside the wall (so it does not
/// overwrite the wall itself). Overhang has to be at least 1 for the
/// eave to sit outside the wall; without one the stair collapses onto
/// the wall row and a `W_DEFERRED_MEMBER` fires instead.
pub(super) fn fill_stair(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // A value that is not an identifier at all (`half="bottom"`,
    // `facing=1`) is unreadable rather than unsupported: the band is drawn
    // with the default state. Read before anything can refuse the stair,
    // so every unreadable state is reported in the same compile as the
    // refusal, with a note that says which of the two happened.
    let mut unread = Vec::new();
    let mut ident = |key: &'static str, expected: &'static str, default: &'static str| {
        ident_or_ignore(member, key, expected).unwrap_or_else(|argument| {
            unread.push((argument, default));
            None
        })
    };
    let states = EaveStates {
        facing: ident("facing", "`out` or `in`", "out"),
        half: ident("half", "`top` or `bottom`", "top"),
        shape: ident(
            "shape",
            "`straight`, `outer_left`, or `outer_right`",
            "straight",
        ),
    };
    let built = draw_eave_band(member, states, y_offset, ctx, palette, canvas, diagnostics);
    for (argument, default) in unread {
        let consequence = if built {
            format!(
                "the stair is built with the default `{}={default}`",
                argument.key
            )
        } else {
            "this stair is not built either way — see the finding on the same line".to_owned()
        };
        diagnostics.push(argument.report(&consequence));
    }
}

/// The three state arguments of an eave `stair`, as identifiers: `None`
/// is a key not written, or written with a value [`fill_stair`] could not
/// read, and either way takes the default.
#[derive(Debug, Clone, Copy)]
struct EaveStates<'m> {
    facing: Option<&'m str>,
    half: Option<&'m str>,
    shape: Option<&'m str>,
}

/// [`fill_stair`]'s refusals and paint. Returns whether the band was drawn,
/// which is what the note on an unreadable state has to say.
fn draw_eave_band(
    member: &Member,
    states: EaveStates<'_>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    if !eave_kind_is_stairs(member, diagnostics) {
        return false;
    }
    let Some(side) = side_of(member, diagnostics) else {
        return false;
    };
    let Some((facing, half, shape)) = states.block_states(member, side, diagnostics) else {
        return false;
    };
    let Some(y_world) = eave_row(member, y_offset, ctx, diagnostics) else {
        return false;
    };
    // An eave band is a row of stairs whose `facing` / `half` / `shape`
    // come from the member's own arguments rather than from a slope, but
    // they are states this pass attaches to whatever it paints just the
    // same — so the material has to be able to carry them.
    let resolved = resolve_member_state(
        member,
        ctx.scope,
        ctx.registry,
        diagnostics,
        ctx.theme_missing,
    );
    let stair_id = geometry_material_id(
        member,
        ctx.scope,
        resolved.as_ref(),
        STAIR_BASE_ID,
        &GeometryMemberDescription {
            subject: "eave `stair`".to_owned(),
            states_from: "its own arguments",
            // An eave is a row of stairs by construction — `fill_stair`
            // refuses any `kind=` but `stairs` well above here.
            requires_stair: true,
        },
        diagnostics,
    );
    let idx = palette.intern(stair_state(stair_id, facing, half, shape));
    paint_eave_band(side, y_world, idx, ctx, canvas);
    true
}

/// Whether the stair's `kind=` is `stairs`, the one kind an eave band
/// draws. Any other, or none, is refused with a `W_DEFERRED_MEMBER`.
fn eave_kind_is_stairs(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> bool {
    let Some(raw_kind) = member.ident_value("kind") else {
        let reason = if member.intent_state.contains_key("kind") {
            "stair `kind=` must be `stairs`"
        } else {
            "stair without `kind=` is not yet supported (currently only `kind=stairs`)"
        };
        diagnostics.push(diag_deferred_member_reason(member, reason));
        return false;
    };
    if raw_kind != "stairs" {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!("stair `kind={raw_kind}` is not yet supported (currently only `kind=stairs`)"),
        ));
        return false;
    }
    true
}

impl EaveStates<'_> {
    /// The `facing`, `half` and `shape` the band's stairs carry along the
    /// `side` wall, a state not given taking its default. `None` after a
    /// `W_DEFERRED_MEMBER` for a value the band does not support.
    fn block_states(
        self,
        member: &Member,
        side: WallSide,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<(Cardinal, &'static str, StairShape)> {
        let half = match self.half {
            Some("top") | None => "top",
            Some("bottom") => "bottom",
            Some(other) => {
                diagnostics.push(diag_deferred_member_reason(
                    member,
                    &format!("stair `half={other}` is not yet supported (use `top` or `bottom`)"),
                ));
                return None;
            }
        };
        let facing = match self.facing {
            Some("out") | None => shed_high_side(side),
            Some("in") => inward_cardinal(side),
            Some(other) => {
                diagnostics.push(diag_deferred_member_reason(
                    member,
                    &format!("stair `facing={other}` is not yet supported (use `out` or `in`)"),
                ));
                return None;
            }
        };
        let shape = match self.shape {
            Some("straight") | None => StairShape::Straight,
            Some("outer_left") => StairShape::OuterLeft,
            Some("outer_right") => StairShape::OuterRight,
            Some(other) => {
                diagnostics.push(diag_deferred_member_reason(
                    member,
                    &format!(
                        "stair `shape={other}` is not yet supported (use `straight`, `outer_left`, or `outer_right`)",
                    ),
                ));
                return None;
            }
        };
        Some((facing, half, shape))
    }
}

/// The row an eave band lands on: its `y=`, `0` when not given, above the
/// level's `y_offset`. `None` after a `W_DEFERRED_MEMBER` when no roof
/// draws an overhang for the band to sit in, when `y=` is unreadable, or
/// when the row is past the top of the volume.
fn eave_row(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<u32> {
    // The gate is the overhang the roof *draws*, not the `overhang=` the
    // author wrote: a roof that will not draw contributes none, so a
    // struct with `overhang=2` and no `kind=` arrives here at zero.
    // Naming only the key would send that author to the wrong line.
    if ctx.overhang == 0 {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "eave `stair` needs a roof that draws an overhang of at least 1 so the band can sit outside the wall — no roof on this struct contributes one",
        ));
        return None;
    }
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
                "stair y={y_world} does not fit in the struct (dims.y={})",
                ctx.dims.y,
            ),
        ));
        return None;
    }
    Some(y_world)
}

/// Paint `idx` along the whole `side` wall at row `y_world`, each cell
/// shifted one voxel outward into the overhang column.
fn paint_eave_band(
    side: WallSide,
    y_world: u32,
    idx: PaletteIndex,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
) {
    let length = wall_length(side, ctx.interior_w, ctx.interior_h);
    for u in 0..length {
        let Some((wx, _wy, wz)) = wall_local_to_grid(
            side,
            u,
            y_world,
            ctx.overhang,
            ctx.interior_w,
            ctx.interior_h,
            ctx.dims,
        ) else {
            // INVARIANT(wall-grid-validated): `u` walks `0..length`, the
            // same length the helper checks, and any `y_world >= dims.y`
            // was refused by `eave_row` with a diagnostic — so the helper
            // has nothing to reject. Reaching here means one of those
            // stopped agreeing with `wall_local_to_grid`. Loud in debug
            // builds; release builds skip the cell rather than panic.
            debug_assert!(
                false,
                "eave stair cell u={u} y={y_world} on the {} wall is outside the wall grid; \
                 the band's `u < wall_length` / `y_world < dims.y` checks stopped agreeing \
                 with `wall_local_to_grid`",
                side_name(side),
            );
            continue;
        };
        let (x, z) = shift_outward(side, wx, wz);
        canvas.paint((x, y_world, z), || idx);
    }
}

/// Opposite of the wall's outward normal — used for `facing=in`.
///
/// The `facing=out` case reuses [`shed_high_side`] because a shed roof's
/// high edge points in the same cardinal as a wall's outward normal
/// (both are "the direction the wall or slope faces the sky").
/// Duplicating the mapping in a second helper would let the two drift.
fn inward_cardinal(side: WallSide) -> Cardinal {
    match side {
        WallSide::Front => Cardinal::North,
        WallSide::Back => Cardinal::South,
        WallSide::Left => Cardinal::East,
        WallSide::Right => Cardinal::West,
    }
}

/// Shift a wall voxel's `(x, z)` by one voxel toward the wall's outward
/// normal so an eave lands in the overhang row instead of overwriting the
/// wall itself.
pub(super) fn shift_outward(side: WallSide, x: u32, z: u32) -> (u32, u32) {
    let (dx, dz) = side.outward_normal();
    (x.saturating_add_signed(dx), z.saturating_add_signed(dz))
}
