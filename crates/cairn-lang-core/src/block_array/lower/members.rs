//! The paint set of a body: which members it holds, which phase paints
//! each, and the material each one reads.

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

/// Bundle of per-struct context shared by every member-lowering helper.
///
/// Carried as a struct (rather than threaded as 7 positional args) so a new
/// per-struct field (e.g. theme name for selector-binding lookups) lands as
/// one field change instead of touching every helper signature.
pub(super) struct StructCtx<'a> {
    pub(super) scope: Option<&'a ScopeResolution>,
    pub(super) registry: Option<&'a dyn TargetRegistry>,
    pub(super) theme_missing: bool,
    pub(super) dims: Dims,
    pub(super) overhang: u32,
    pub(super) interior_w: u32,
    pub(super) interior_h: u32,
    /// Highest wall voxel coordinate: the largest `y_offset + height=`
    /// across the walls members the pass will paint, so a `walls` inside a
    /// `level y=N` raises it by that `N`. `0` when no walls are present.
    ///
    /// This is where the roof starts and how tall the volume has to be.
    /// It is *not* what a member cut into the wall may check itself
    /// against — see [`StructCtx::wall_column`].
    pub(super) wall_top: u32,
    /// Every row the walls members actually paint, gaps and all.
    ///
    /// A `window` has to land inside masonry and a `door` has to open
    /// into it, neither of which `wall_top` can decide: it says nothing
    /// about the rows below the first course and nothing about the air
    /// between two of them. `ports::port_world_position` is handed
    /// this same column, so a port and the cut it anchors to read one
    /// answer rather than two that agree.
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

    /// This phase's position in the order `spec/compilation` "Phase
    /// evaluation" runs the phases in, counting from `0`. A list indexed by
    /// it and walked front to back visits the phases in that order.
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
///
/// Three answers rather than `Option<Phase>`, because "no phase" covered
/// three different facts and the caller had to take them apart again
/// behind a catch-all arm — which is the arm a role added later falls
/// into by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MemberDisposition {
    /// Evaluated in this phase, and paints voxels there.
    Paints(Phase),
    /// Reserves something a later pass reads, and paints nothing. Its
    /// surface shape is still checked, so a malformed one is reported.
    Reserves,
    /// Nothing in this pass lowers it, and it earns a
    /// `W_DEFERRED_MEMBER` saying so.
    NotLowered,
}

/// Where a member's role puts it, per `spec/compilation` "Phase evaluation".
///
/// The spec's order is `massing (shell: floor/walls/volume) → envelope
/// (roof/exterior) → openings (door/window) → fixtures (furnishings:
/// sign/painting/frame/bed/sensors & actuators)`, and each role below is
/// placed against that sentence rather than against what happens to lower
/// alike:
///
/// - `floor` / `walls` are the shell the sentence names.
/// - `roof` is the envelope; `stair` joins it because `spec/compilation`
///   "Gable roof voxel rules" describes it as an *eave* stair, and an eave is
///   exterior.
/// - `door` / `window` are the openings the sentence names.
/// - `pressure_plate` is a sensor, so it is a fixture. Sharing the openings
///   bucket with `window` meant a contested cell went to whichever line came
///   last, which is the order accident the phase order promises away.
/// - `circuit` reserves a routing region for the redstone phases and
///   writes no voxel.
/// - `place` and `connect` belong to a site body, which this pass does not
///   lower; `Other` is a keyword the role table does not know, which
///   includes the `raw` that same phase order names — not yet a keyword at
///   all, so it is reported as unknown by the allowlist pass on top of the
///   deferral here.
///
/// [`paint_member`]'s match spells every role out with no wildcard so a
/// role added here fails the compile there rather than reaching a user's
/// source and panicking. `Level` never arrives: flattening runs first, so
/// one that does means a caller skipped it.
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
        // Spelled out rather than folded into the arm above so that a
        // `Level` reaching here is a loud bug and not a deferral: it means
        // a caller skipped `flatten_members`, and the members it was
        // grouping would go missing without a word.
        MemberRole::Level => unreachable!(
            "`Level` members must be flattened before phase-bucketing (see `flatten_members`)"
        ),
    }
}

/// Flatten level blocks so their children participate in phase-bucketing.
///
/// Returns each contributing member paired with the `y_offset` derived from
/// its enclosing `level y=N` (`0` for members that sit directly under the
/// struct/def/place body).
///
/// The result is the **paint set**, not the source members with the `level`
/// wrappers unwrapped. Dimension derivation reads the same list, and the two
/// have to agree in both directions: a member the dims cannot see paints
/// into a volume too small to hold it, and a member the paint pass drops
/// inflates a volume nothing fills. Deciding participation here, once, is
/// what keeps them from drifting apart — the alternative is the same rule
/// spelled out twice, one copy per reader.
///
/// Dropped here, each with its own `W_DEFERRED_MEMBER`:
///
/// - a `level` whose `y=` is missing or is not a non-negative integer,
///   along with its whole body: there is no offset to place the children at;
/// - a `level` nested inside another, one diagnostic per direct child, so an
///   author who wrote deep nesting sees each dropped subtree rather than one
///   defer that hides how many members went with it;
/// - a member whose role has no lowering above the ground plane, per
///   [`level_offset_role_defer`].
///
/// `level` blocks themselves are consumed and never reach the returned list.
/// `logic` and `assert` items inside a level body are intentionally absent
/// too — they live on the intent IR for the resolver and redstone passes to
/// consume, unaffected by block-array lowering.
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
                    // Nested `level` blocks are not yet supported. Emit
                    // one warning per direct grandchild-defer so an
                    // author who wrote deep nesting sees each dropped
                    // subtree instead of a single top-level defer that
                    // hides how many members were skipped.
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

/// Why a role has no lowering at a non-zero `level y=`, or `None` when the
/// role means the same thing at any offset.
///
/// `walls`, `door`, `window`, `stair`, and `pressure_plate` read the offset
/// as the base their own geometry is measured from, which is exactly what
/// `level y=N` asks for — `themed-tower.crn` builds its second storey from
/// the first three. A `floor` and a `roof` are single planes the struct has
/// one of: a second floor slab would land in mid-air, and a second roof
/// would cap the building below its own roof plane. Neither generator has a
/// story for that yet, so the member is dropped rather than painted
/// somewhere unexpected — and, being dropped, it contributes nothing to the
/// volume.
///
/// Every variant is spelled out for the reason [`member_disposition`] spells its
/// own out: a role added later must not fall into "lowers the same at any
/// offset" because that is the arm a wildcard happens to reach. The three
/// that lower to nothing at any offset are listed with the rest — they
/// reach the phase buckets and defer there, with a message that names the
/// role rather than the level.
fn level_offset_role_defer(role: &MemberRole, y_offset: u32) -> Option<&'static str> {
    if y_offset == 0 {
        return None;
    }
    match role {
        MemberRole::Floor => Some("level-scoped `floor` is not yet supported"),
        MemberRole::Roof => Some("level-scoped `roof` is not yet supported"),
        // The first five measure their geometry from the offset. The rest
        // paint no voxels at any offset and are listed rather than left to
        // a wildcard: `place` and `connect` belong to a site body and are
        // reported as misplaced, `Other` is an unknown keyword, and
        // `circuit` marks a region for redstone lowering. `circuit` is the
        // one with something left owing — the region recogniser takes no
        // `y_offset`, so a level-scoped region is not shifted. That is a
        // gap on the redstone side rather than something dropping the
        // member would close: a lost region is not closer to right than an
        // unshifted one.
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
///
/// Which phase runs it is [`member_disposition`]'s answer, not this
/// function's: [`run_phase`] hands members over a phase at a time, and the
/// match here only picks the painter. The roles no phase paints are
/// spelled out with no wildcard, for the reason [`member_disposition`]
/// gives.
///
/// [`run_phase`]: super::body::run_phase
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
            // Always the struct's ground plane: `flatten_members` drops a
            // `floor` at a non-zero offset before it reaches a phase.
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
        // Always the struct's own roof plane: `flatten_members` drops a
        // `roof` at a non-zero offset before it reaches a phase.
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
///
/// A property of the role's painter, so it is derived from the role by
/// [`Self::for_role`] rather than handed in by each caller of
/// [`resolve_member_state`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MemberFallback {
    /// A `floor`: [`fill_floor`] is not reached, so its cells stay air.
    Air,
    /// A `walls`: it paints no row, and [`painting_walls`] drops it, so it
    /// claims none in the volume either ([`max_wall_top`]) and a `door` or
    /// `window` cut into it is refused.
    ///
    /// [`painting_walls`]: super::sizing::painting_walls
    /// [`max_wall_top`]: super::sizing::max_wall_top
    Absent,
    /// A `window`: [`fill_window`] refuses it, so the wall it would have cut
    /// stays, and a port anchored on it is refused with it.
    ///
    /// [`fill_window`]: super::window::fill_window
    WallStays,
    /// A `roof`, an eave `stair` or a `pressure_plate`: built from its
    /// default block.
    ///
    /// A `roof` or eave `stair` also gets [`geometry_material_id`]'s
    /// `W_DEFERRED_MEMBER`, which names the concrete id: only the geometry
    /// side knows it, since a roof's depends on its `kind`.
    /// [`plate_id_for_member`] deliberately does not echo; its doc says why.
    ///
    /// [`geometry_material_id`]: super::roof_fill::geometry_material_id
    /// [`plate_id_for_member`]: super::plate::plate_id_for_member
    DefaultBlock,
}

impl MemberFallback {
    /// The consequence for a member of this role.
    ///
    /// Only the six roles whose painters call [`resolve_member_state`] reach
    /// here. The rest are spelled out with no wildcard, as in
    /// [`paint_member`]'s match, so a role added later fails the compile
    /// here rather than borrowing another role's sentence: a `door` is
    /// carved and reads no material, a `circuit` reserves a region, a
    /// `place` or `connect` is a site row, `Other` is a keyword the role
    /// table does not know, and a `level` is flattened before any painter
    /// runs.
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

/// Resolve a member's `mat_slot=` binding into a concrete [`BlockState`]
/// without touching the palette.
///
/// Returns `None` (and emits at most one diagnostic) when:
/// - the scope had no theme bound (`theme_missing` short-circuits
///   silently). Every caller passes a member of the list
///   `flatten_members` returned for the body, so a member that reads its
///   slot ([`Member::read_slot`]) sits in a scope
///   `lower_body_to_block_array` pushed `W_NO_THEME_BOUND` for. One that
///   reads none has a `None` `slot_value` whether or not a theme is bound,
///   and is reported as the third item says,
/// - there is no resolution for the scope, or none for the member in it
///   (silently, and not always reported elsewhere; see the comment above
///   the `slot_value` read),
/// - the binding's `slot_value` is `None` (silently). That has the six
///   causes [`crate::resolve::ResolvedMemberBinding::slot_value`] lists,
///   with who reports each; among them, the member never carried a
///   `mat_slot=`, carries one on a role that reads none
///   (`W_IGNORED_ARGUMENT`), or names a slot the resolver flagged via
///   `E_UNRESOLVED_SLOT`,
/// - the value lowered as an abstract token and no `registry` resolver was
///   offered (a `W_ABSTRACT_TOKEN_DEFERRED` warning is emitted),
/// - the value lowered as an abstract token the offered `registry` resolver
///   does not declare (an `E_UNKNOWN_ABSTRACT_TOKEN` error is emitted with
///   the nearest declared candidate, when one exists),
/// - the value lowered as a canonical id the offered `registry` does not
///   declare (an `E_UNKNOWN_ID` error is emitted),
/// - the value was not a token at all (`E_UNKNOWN_SLOT_TARGET` already
///   fired during resolve, so no second diagnostic here).
///
/// Split out from [`palette_index_for`] so members that hard-code their
/// material (gable roof → `spruce_stairs`) can still resolve the user's
/// `mat_slot=` to check whether it agrees with the hard-coded id and emit
/// a warning when it does not — without polluting the palette with an
/// unreferenced entry.
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
    // INVARIANT(upstream-diagnosed): every reason `slot_value` is `None`
    // has a reporter on the same diagnostic stream, and that stream is
    // gated before anything is emitted — which is the guarantee, rather
    // than any ordering between the two passes. `W_NO_THEME_BOUND` is
    // pushed from *inside* this pass, and the CLI runs lowering before it
    // merges `check`'s findings in, so "reported first" would be false in
    // two different ways. `ResolvedMemberBinding::slot_value` names the
    // six causes and who owns each; the one that used to have no owner is
    // a member carrying no `mat_slot=` at all, now `check::material`'s for
    // the roles that paint nothing without one.
    //
    // Scoped to this `?` alone. The two above it are different questions —
    // a scope that was never built, and a member missing from one that
    // was — and `lower.rs`'s own comment on the `place` path admits the
    // first is reported by neither layer.
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
            // INVARIANT(upstream-diagnosed): `AlreadyDiagnosed` is returned
            // only when `slot_value.value.kind` is not `Token` (see
            // `material::resolve_block_state` /
            // `TokenKind::NotAToken`). For theme slot values the
            // `check_slot_targets` pass in
            // `resolve::resolver` (`resolver.rs` around the
            // `DiagnosticCode::UnknownSlotTarget` push) emits
            // `E_UNKNOWN_SLOT_TARGET` for exactly that shape during the
            // `resolve()` invocation that produced the `scope` we read
            // above; staying silent here avoids a duplicate diagnostic.
            // A local `debug_assert` would require threading the
            // resolver's `Resolution` into every caller of
            // `resolve_member_state` (palette helpers, opening carvers,
            // …). That blast radius is intentionally avoided here, so
            // the invariant is enforced by the resolver-pass unit
            // tests around `DiagnosticCode::UnknownSlotTarget` rather
            // than a local assert.
            None
        }
    }
}

/// Resolve a member's `mat_slot=` binding and intern the resulting state.
///
/// Thin shim over [`resolve_member_state`] for callers that always want to
/// store the material in the palette (floors, walls, windows).
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

/// Will this member put a block anywhere?
///
/// The same question [`palette_index_for`] asks when the massing phase
/// paints, through the same function, so the volume and the paint cannot
/// answer differently. The diagnostics are ignored because the massing
/// phase's own call pushes them for real; keeping them here would say
/// each twice.
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
    // Walls fill y = y_offset+1 .. y_offset+height. Struct-scoped walls run
    // at y_offset=0 (the historical `1..=height` range). A `walls` inside a
    // `level y=N` block starts one voxel above the level's base plane, so
    // the range shifts up by N. The upper bound is capped at the volume's
    // Y extent so a stray out-of-range `height=` cannot panic — `dims.y`
    // already covers `max_wall_top + roof_extra + 1`, so under normal
    // lowering this never trims; the min is defensive against a hand-built
    // `BlockArray`.
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
        // Distinguish "missing entirely" (no `side=` key) from "wrong
        // type" (`side=` present but its value is not an identifier). A
        // silent return on the missing case would let a `door at=center`
        // line lower to nothing without telling the author, which breaks
        // the module-level promise that every dropped member surfaces a
        // diagnostic.
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
