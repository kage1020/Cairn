//! The paint set of a body: its members, the phase that paints each, and the material each reads.

use crate::diagnostic::Diagnostic;
use crate::intent::{Member, MemberRole, ValueWithSpan};
use crate::resolve::ScopeResolution;

use crate::block_array::args::{
    NonNegRead, diag_deferred_member_reason, member_or_slot_span, nonneg_int_or_defer,
};
use crate::block_array::material::{MaterialDeferred, TargetRegistry, resolve_block_state};
use crate::block_array::openings::WallSide;
use crate::block_array::wall_column::WallColumn;
use crate::block_array::{BlockState, Dims, PaletteIndex};

use super::canvas::{MemberCanvas, ScopePalette};
use super::door::carve_door;
use super::plate::fill_pressure_plate;
use super::roof_fill::fill_roof;
use super::sizing::wall_height;
use super::stair::fill_stair;
use super::token_diag::{
    TokenSite, diag_abstract_token, diag_state_literal_unchecked, diag_unknown_abstract_token,
    diag_unknown_id,
};
use super::window::fill_window;

/// Per-body context every member painter reads.
pub(super) struct StructCtx<'a> {
    pub(super) scope: Option<&'a ScopeResolution>,
    pub(super) registry: Option<&'a dyn TargetRegistry>,
    pub(super) theme_missing: bool,
    pub(super) dims: Dims,
    pub(super) overhang: u32,
    pub(super) interior_w: u32,
    pub(super) interior_h: u32,
    /// Highest wall row; the roof starts above it. A cut checks [`StructCtx::wall_column`] instead.
    pub(super) wall_top: u32,
    /// Every row the walls members paint, gaps and all; ports read this same column.
    pub(super) wall_column: WallColumn,
}

/// A phase of `spec/compilation` "Phase evaluation" that paints voxels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Massing,
    Envelope,
    Openings,
    Fixtures,
}

impl Phase {
    /// How many phases paint: one past the highest [`Self::index`].
    pub(super) const COUNT: usize = 4;

    /// This phase's position in `spec/compilation` "Phase evaluation" order, from `0`.
    pub(super) const fn index(self) -> usize {
        match self {
            Self::Massing => 0,
            Self::Envelope => 1,
            Self::Openings => 2,
            Self::Fixtures => 3,
        }
    }
}

/// What the phase model does with a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MemberDisposition {
    /// Evaluated in this phase, and paints voxels there.
    Paints(Phase),
    /// Reserves something a later pass reads and paints nothing; its shape is still checked.
    Reserves,
    /// Nothing in this pass lowers it; it earns a `W_DEFERRED_MEMBER`.
    NotLowered,
}

/// Where a member's role puts it, per `spec/compilation` "Phase evaluation".
///
/// Every role is spelled out, so a new one fails the compile here instead of panicking later.
pub(super) fn member_disposition(role: &MemberRole) -> MemberDisposition {
    match role {
        MemberRole::Floor | MemberRole::Walls => MemberDisposition::Paints(Phase::Massing),
        MemberRole::Roof | MemberRole::Stair => MemberDisposition::Paints(Phase::Envelope),
        MemberRole::Door | MemberRole::Window => MemberDisposition::Paints(Phase::Openings),
        MemberRole::PressurePlate => MemberDisposition::Paints(Phase::Fixtures),
        MemberRole::Circuit => MemberDisposition::Reserves,
        MemberRole::Place | MemberRole::Connect | MemberRole::Other(_) => {
            MemberDisposition::NotLowered
        }
        MemberRole::Level => unreachable!(
            "`Level` members must be flattened before phase-bucketing (see `flatten_members`)"
        ),
    }
}

/// Flatten `level` blocks into the body's paint set, each member with its level's `y` offset.
///
/// Sizing reads the same list, so the volume and the paint agree on which members count.
pub(super) fn flatten_members<'a>(
    members: &'a [Member],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<(u32, &'a Member)> {
    let mut out: Vec<(u32, &'a Member)> = Vec::new();
    for member in members {
        if matches!(member.role, MemberRole::Level) {
            let y_offset = match nonneg_int_or_defer(member, "y", diagnostics) {
                NonNegRead::Valid(v) => v,
                NonNegRead::Absent => {
                    diagnostics.push(diag_deferred_member_reason(
                        member,
                        "level requires `y=N` (non-negative integer) to place its children",
                    ));
                    continue;
                }
                NonNegRead::Deferred => continue,
            };
            for child in &member.children.members {
                if matches!(child.role, MemberRole::Level) {
                    diagnostics.push(diag_deferred_member_reason(
                        child,
                        "nested `level` blocks are not yet supported; this level and every member declared under it were dropped",
                    ));
                    continue;
                }
                if let Some(reason) = level_offset_role_defer(&child.role, y_offset) {
                    diagnostics.push(diag_deferred_member_reason(child, reason));
                    continue;
                }
                out.push((y_offset, child));
            }
        } else {
            out.push((0, member));
        }
    }
    out
}

/// Why a role has no lowering at a non-zero `level y=`, or `None` when it lowers at any offset.
///
/// Every role is spelled out, as in [`member_disposition`].
fn level_offset_role_defer(role: &MemberRole, y_offset: u32) -> Option<&'static str> {
    if y_offset == 0 {
        return None;
    }
    match role {
        MemberRole::Floor => Some("level-scoped `floor` is not yet supported"),
        MemberRole::Roof => Some("level-scoped `roof` is not yet supported"),
        // A level-scoped `circuit` region is not shifted: its recogniser takes no `y_offset`.
        MemberRole::Walls
        | MemberRole::Door
        | MemberRole::Window
        | MemberRole::Stair
        | MemberRole::PressurePlate
        | MemberRole::Circuit
        | MemberRole::Place
        | MemberRole::Connect
        | MemberRole::Other(_) => None,
        MemberRole::Level => unreachable!(
            "a nested `level` is dropped by `flatten_members` before this function sees it"
        ),
    }
}

/// Paint one member of the paint set with the generator its role names.
pub(super) fn paint_member(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &member.role {
        MemberRole::Floor => {
            // `flatten_members` drops a `floor` at a non-zero offset, so this is the ground plane.
            let Some(idx) = palette_index_for(
                member,
                ctx.scope,
                ctx.registry,
                palette,
                diagnostics,
                ctx.theme_missing,
            ) else {
                return;
            };
            fill_floor(ctx, idx, canvas);
        }
        MemberRole::Walls => {
            let Some(height) = wall_height(member, diagnostics) else {
                return;
            };
            let Some(idx) = palette_index_for(
                member,
                ctx.scope,
                ctx.registry,
                palette,
                diagnostics,
                ctx.theme_missing,
            ) else {
                return;
            };
            fill_walls(ctx, height, y_offset, idx, canvas);
        }
        // `flatten_members` drops a `roof` at a non-zero offset, so this is the struct's plane.
        MemberRole::Roof => fill_roof(member, ctx, palette, canvas, diagnostics),
        MemberRole::Stair => fill_stair(member, y_offset, ctx, palette, canvas, diagnostics),
        MemberRole::Door => carve_door(member, y_offset, ctx, canvas, diagnostics),
        MemberRole::Window => fill_window(member, y_offset, ctx, palette, canvas, diagnostics),
        MemberRole::PressurePlate => {
            fill_pressure_plate(member, y_offset, ctx, palette, canvas, diagnostics);
        }
        MemberRole::Level
        | MemberRole::Circuit
        | MemberRole::Place
        | MemberRole::Connect
        | MemberRole::Other(_) => {
            unreachable!("no phase paints a `{}`", MemberRole::keyword(&member.role))
        }
    }
}

/// What becomes of a member whose `mat_slot=` resolves to no material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MemberFallback {
    /// A `floor`: [`fill_floor`] is not reached, so its cells stay air.
    Air,
    /// A `walls`: it paints no row and claims none in the volume, so a cut into it is refused.
    Absent,
    /// A `window`: [`fill_window`] refuses it, so the wall stays and a port on it is refused.
    ///
    /// [`fill_window`]: super::window::fill_window
    WallStays,
    /// A `roof`, an eave `stair` or a `pressure_plate`: built from its default block.
    DefaultBlock,
}

impl MemberFallback {
    /// The consequence for a member of this role, each spelled out as in [`paint_member`].
    fn for_role(role: &MemberRole) -> Self {
        match role {
            MemberRole::Floor => Self::Air,
            MemberRole::Walls => Self::Absent,
            MemberRole::Window => Self::WallStays,
            MemberRole::Roof | MemberRole::Stair | MemberRole::PressurePlate => Self::DefaultBlock,
            MemberRole::Door
            | MemberRole::Level
            | MemberRole::Circuit
            | MemberRole::Place
            | MemberRole::Connect
            | MemberRole::Other(_) => {
                unreachable!(
                    "no painter resolves the `mat_slot=` of a `{}`",
                    MemberRole::keyword(role)
                )
            }
        }
    }
}

/// Resolve a member's `mat_slot=` binding into a [`BlockState`] without touching the palette.
///
/// `None` when there is no material, with at most one diagnostic saying why.
pub(super) fn resolve_member_state(
    member: &Member,
    scope: Option<&ScopeResolution>,
    registry: Option<&dyn TargetRegistry>,
    diagnostics: &mut Vec<Diagnostic>,
    theme_missing: bool,
) -> Option<BlockState> {
    if theme_missing {
        return None;
    }
    let scope = scope?;
    let binding = scope.members.get(&member.span.start)?;
    // INVARIANT(upstream-diagnosed): every reason `slot_value` is `None` is reported on the same
    // diagnostic stream; `ResolvedMemberBinding::slot_value` names who reports each.
    let slot_value: &ValueWithSpan = binding.slot_value.as_ref()?;
    match resolve_block_state(slot_value, registry) {
        Ok(state) => {
            diagnostics.extend(diag_state_literal_unchecked(slot_value, &state));
            Some(state)
        }
        Err(MaterialDeferred::Abstract(token)) => {
            diagnostics.push(diag_abstract_token(
                member_or_slot_span(member, slot_value),
                &token,
                TokenSite::MemberSlot(MemberFallback::for_role(&member.role)),
            ));
            None
        }
        Err(MaterialDeferred::UnknownAbstract { token, suggestion }) => {
            diagnostics.push(diag_unknown_abstract_token(
                member_or_slot_span(member, slot_value),
                &token,
                suggestion.as_deref(),
                TokenSite::MemberSlot(MemberFallback::for_role(&member.role)),
            ));
            None
        }
        Err(MaterialDeferred::UnknownId(unknown)) => {
            diagnostics.push(diag_unknown_id(slot_value.span.clone(), &unknown));
            None
        }
        Err(MaterialDeferred::AlreadyDiagnosed) => {
            // INVARIANT(upstream-diagnosed): `check_slot_targets` reports `E_UNKNOWN_SLOT_TARGET`.
            None
        }
    }
}

/// [`resolve_member_state`], interned.
pub(super) fn palette_index_for(
    member: &Member,
    scope: Option<&ScopeResolution>,
    registry: Option<&dyn TargetRegistry>,
    palette: &mut ScopePalette,
    diagnostics: &mut Vec<Diagnostic>,
    theme_missing: bool,
) -> Option<PaletteIndex> {
    resolve_member_state(member, scope, registry, diagnostics, theme_missing)
        .map(|state| palette.intern(state))
}

/// Whether this member will put a block anywhere, asked as the massing phase paints it.
pub(super) fn member_will_paint(
    member: &Member,
    scope: Option<&ScopeResolution>,
    registry: Option<&dyn TargetRegistry>,
    theme_missing: bool,
) -> bool {
    let mut ignored_diagnostics = Vec::new();
    resolve_member_state(
        member,
        scope,
        registry,
        &mut ignored_diagnostics,
        theme_missing,
    )
    .is_some()
}

fn fill_floor(ctx: &StructCtx<'_>, idx: PaletteIndex, canvas: &mut MemberCanvas<'_>) {
    let y = 0;
    for z_local in 0..ctx.interior_h {
        for x_local in 0..ctx.interior_w {
            let x = ctx.overhang + x_local;
            let z = ctx.overhang + z_local;
            canvas.paint((x, y, z), || idx);
        }
    }
}

fn fill_walls(
    ctx: &StructCtx<'_>,
    height: u32,
    y_offset: u32,
    idx: PaletteIndex,
    canvas: &mut MemberCanvas<'_>,
) {
    // One above the base plane the floor owns; the cap only guards a stray `height=`.
    let start = 1u32.saturating_add(y_offset);
    let end = height
        .saturating_add(y_offset)
        .min(ctx.dims.y.saturating_sub(1));
    for y in start..=end {
        for z_local in 0..ctx.interior_h {
            for x_local in 0..ctx.interior_w {
                let on_edge = x_local == 0
                    || x_local + 1 == ctx.interior_w
                    || z_local == 0
                    || z_local + 1 == ctx.interior_h;
                if !on_edge {
                    continue;
                }
                let x = ctx.overhang + x_local;
                let z = ctx.overhang + z_local;
                canvas.paint((x, y, z), || idx);
            }
        }
    }
}

pub(super) fn side_of(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<WallSide> {
    let Some(raw) = member.ident_value("side") else {
        let reason = match member.intent_state.get("side") {
            Some(written) => format!(
                "`side=` must be one of front, back, left, right, not {}",
                written.value.describe(),
            ),
            None => "missing `side=` (expected one of front, back, left, right)".to_owned(),
        };
        diagnostics.push(diag_deferred_member_reason(member, &reason));
        return None;
    };
    if let Some(side) = WallSide::from_ident(raw) {
        return Some(side);
    }
    diagnostics.push(diag_deferred_member_reason(
        member,
        &format!("unknown `side={raw}` (expected one of front, back, left, right)"),
    ));
    None
}

pub(super) fn side_name(side: WallSide) -> &'static str {
    match side {
        WallSide::Front => "front",
        WallSide::Back => "back",
        WallSide::Left => "left",
        WallSide::Right => "right",
    }
}
