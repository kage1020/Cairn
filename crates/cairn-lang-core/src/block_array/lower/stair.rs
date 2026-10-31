//! Painting an eave `stair` member.

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::PaletteIndex;
use crate::block_array::args::{
    NonNegRead, diag_deferred_member_noted, diag_invalid_member, ident_or_ignore,
    nonneg_int_or_refuse,
};
use crate::block_array::openings::{WallSide, wall_length, wall_local_to_grid};
use crate::block_array::roof::{Cardinal, STAIR_BASE_ID, StairShape, shed_high_side, stair_state};

use super::canvas::{MemberCanvas, ScopePalette};
use super::members::{StructCtx, resolve_member_state, side_name, side_of};
use super::roof_fill::{GeometryMemberDescription, geometry_material_id};

/// The note on a stair refused for its `kind=`, naming the one kind there is.
const STAIR_KINDS: &str = "write `kind=stairs`, the one stair kind: a band of stairs in the \
                           overhang outside a wall";

/// Paint a `stair kind=stairs` member as a band of stairs in the overhang outside a wall.
pub(super) fn fill_stair(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Read before anything can refuse the stair, so every unreadable state is reported now.
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

/// The eave's `facing`, `half` and `shape` identifiers; `None` takes the default.
#[derive(Debug, Clone, Copy)]
struct EaveStates<'m> {
    facing: Option<&'m str>,
    half: Option<&'m str>,
    shape: Option<&'m str>,
}

/// [`fill_stair`]'s refusals and paint; whether the band was drawn.
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
            // `eave_kind_is_stairs` has refused any other `kind=`.
            requires_stair: true,
        },
        diagnostics,
    );
    let idx = palette.intern(stair_state(stair_id, facing, half, shape));
    paint_eave_band(side, y_world, idx, ctx, canvas);
    true
}

/// Whether the stair's `kind=` is `stairs`, refusing any other with an `E_INVALID_MEMBER`.
fn eave_kind_is_stairs(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> bool {
    let primary = match member.intent_state.get("kind") {
        None => "stair has no `kind=`, so nothing says what it builds".to_owned(),
        Some(raw) => match &raw.value.kind {
            ValueKind::Ident(kind) if kind == "stairs" => return true,
            ValueKind::Ident(kind) => format!("unknown stair `kind={kind}`"),
            _ => format!(
                "stair `kind=` must be an identifier, not {}",
                raw.value.describe(),
            ),
        },
    };
    diagnostics.push(diag_invalid_member(
        member,
        primary,
        vec![STAIR_KINDS.to_owned()],
    ));
    false
}

impl EaveStates<'_> {
    /// The `facing`, `half` and `shape` the band carries on `side`, or `None` after a refusal.
    fn block_states(
        self,
        member: &Member,
        side: WallSide,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<(Cardinal, &'static str, StairShape)> {
        let mut refuse = |key: &str, other: &str, values: &[&str]| {
            diagnostics.push(diag_invalid_member(
                member,
                format!("unknown stair `{key}={other}`"),
                vec![format!("`{key}=` takes one of {}", values.join(", "))],
            ));
        };
        let half = match self.half {
            Some("top") | None => "top",
            Some("bottom") => "bottom",
            Some(other) => {
                refuse("half", other, &["top", "bottom"]);
                return None;
            }
        };
        let facing = match self.facing {
            Some("out") | None => shed_high_side(side),
            Some("in") => inward_cardinal(side),
            Some(other) => {
                refuse("facing", other, &["out", "in"]);
                return None;
            }
        };
        let shape = match self.shape {
            Some("straight") | None => StairShape::Straight,
            Some("outer_left") => StairShape::OuterLeft,
            Some("outer_right") => StairShape::OuterRight,
            Some(other) => {
                refuse("shape", other, &["straight", "outer_left", "outer_right"]);
                return None;
            }
        };
        Some((facing, half, shape))
    }
}

/// The row an eave band lands on, or `None` after the finding that refuses the stair.
///
/// The stair's own line is asked against the struct as written first, so its fault is an error
/// even when a `roof` or `walls` member that would have held it paints nothing too.
fn eave_row(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<u32> {
    let y_local = match nonneg_int_or_refuse(member, "y", diagnostics) {
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 0,
        NonNegRead::Refused => return None,
    };
    let y_world = y_local.saturating_add(y_offset);
    if ctx.written_overhang == 0 {
        diagnostics.push(diag_invalid_member(
            member,
            "eave `stair` has no overhang to sit in: no roof on this struct is written with an `overhang=` of at least 1".to_owned(),
            vec![
                "an eave band sits one voxel outside the wall, in the ring a roof's `overhang=` \
                 adds; give the struct's `roof` an `overhang=` of at least 1"
                    .to_owned(),
            ],
        ));
        return None;
    }
    let past_the_struct = || {
        format!(
            "stair y={y_world} does not fit in the struct (dims.y={})",
            ctx.dims.y
        )
    };
    if y_world >= ctx.written_height {
        diagnostics.push(diag_invalid_member(
            member,
            past_the_struct(),
            vec![format!(
                "the struct's top row is {}; lower the stair or its `level`, or raise the `walls` \
                 to reach its row",
                ctx.written_height.saturating_sub(1),
            )],
        ));
        return None;
    }
    // The overhang a roof draws, not the `overhang=` written: a roof that draws nothing gives none.
    if ctx.overhang == 0 {
        diagnostics.push(diag_deferred_member_noted(
            member,
            "eave `stair` has no overhang to sit in: the roof written with an `overhang=` draws nothing",
            "the stair is built once that roof draws, and the roof's own finding says why it does not",
        ));
        return None;
    }
    if y_world >= ctx.dims.y {
        diagnostics.push(diag_deferred_member_noted(
            member,
            &past_the_struct(),
            "a `walls` member that would reach this row paints nothing; the stair is built once \
             that member paints, and its own finding says why it does not",
        ));
        return None;
    }
    Some(y_world)
}

/// Paint `idx` along the whole `side` wall at `y_world`, one voxel out in the overhang column.
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
            // INVARIANT(wall-grid-validated): `u` is on the wall; `eave_row` checked `y_world`.
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

/// The cardinal opposite the wall's outward normal, for `facing=in`.
fn inward_cardinal(side: WallSide) -> Cardinal {
    match side {
        WallSide::Front => Cardinal::North,
        WallSide::Back => Cardinal::South,
        WallSide::Left => Cardinal::East,
        WallSide::Right => Cardinal::West,
    }
}

/// Shift a wall voxel's `(x, z)` one voxel along the wall's outward normal.
pub(super) fn shift_outward(side: WallSide, x: u32, z: u32) -> (u32, u32) {
    let (dx, dz) = side.outward_normal();
    (x.saturating_add_signed(dx), z.saturating_add_signed(dz))
}
