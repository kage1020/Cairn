//! Intent IR → block-array IR lowering.
//!
//! The pass is total: every struct ends up in
//! [`BlockArrayIr::structures`], every issue surfaces as a warning on
//! [`BlockArrayIr::diagnostics`]. That keeps `cairn lower` useful even on
//! a half-finished module — the operator can see what voxels did lower,
//! and the diagnostic stream tells them what was skipped and why.
//!
//! ## Phase ordering
//!
//! `spec/compilation.md` §4.1 evaluates members in a fixed phase order
//! independent of source order:
//!
//! ```text
//! massing  (floor, walls)
//!   → envelope (roof)
//!   → openings (door, window)
//!   → fixtures, logic_*, raw
//! ```
//!
//! The current pass implements the first three (massing, envelope,
//! openings). Members are bucketed by role and processed phase-by-phase;
//! within a phase source order wins (the last-wins rule for local
//! overrides). Roles outside the three implemented phases emit
//! `W_DEFERRED_MEMBER` and skip.
//!
//! Defs are skipped at this layer: they only concretise via a `site`
//! `place ... use=def_name` reference, and site lowering arrives with the
//! multi-building pass. Sites themselves are also skipped for the same
//! reason.

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::check::{Diagnostic, DiagnosticCode, DiagnosticNote, Severity};
use crate::error::Span;
use crate::intent::{IntentModule, Member, MemberRole, StructIr, ValueWithSpan};
use crate::resolve::{Resolution, ScopeResolution};

use super::material::{AbstractMaterialResolver, MaterialDeferred, resolve_block_state};
use super::openings::{WallSide, wall_length, wall_local_to_grid};
use super::roof::{
    GableVoxel, HipVoxel, RoofKind, ShedFace, ShedVoxel, StairFace, flat_block_state,
    flat_extra_height, flat_voxels, gable_extra_height, gable_ridge_axis, gable_stair_state,
    gable_voxels, hip_extra_height, hip_stair_state, hip_voxels, shed_extra_height,
    shed_slope_span, shed_stair_state, shed_voxels,
};
use super::{BlockArray, BlockArrayIr, BlockState, Dims, Palette, PaletteIndex};

/// Lower every `struct` in `intent` into a [`BlockArray`].
///
/// Pairs each struct with its [`ScopeResolution`] from `resolution` so the
/// material lookups go through the same theme bindings `cairn check` and
/// `cairn info` already used. Members are processed in phase order
/// (massing → envelope → openings), so a `door` written before `walls` in
/// the source still cuts an opening through the resulting wall. Roles
/// outside the three implemented phases are reported via
/// `W_DEFERRED_MEMBER` and skipped.
///
/// `materials` is the registry-pack-backed abstract-token lifter. `Some`
/// turns `@floor.wood.broadleaf`-style tokens into concrete Java ids and
/// fail-loud on misses; `None` keeps the pre-PR2 behaviour where every
/// abstract token degrades to a `W_ABSTRACT_TOKEN_DEFERRED` warning so
/// library callers without a pack still get a partial build.
#[must_use]
pub fn lower_to_block_array(
    intent: &IntentModule,
    resolution: &Resolution,
    materials: Option<&dyn AbstractMaterialResolver>,
) -> BlockArrayIr {
    let mut structures: IndexMap<String, BlockArray> = IndexMap::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for s in &intent.structs {
        let key = format!("struct::{}", s.name);
        let scope = resolution.scopes.get(&key);
        // `lower_struct` returns `None` only after it has already pushed a
        // diagnostic (no `size=`, etc.), so the skip here is silent on
        // purpose — diagnosing twice would teach a reader the struct had
        // two unrelated problems instead of one.
        if let Some(ba) = lower_struct(s, scope, materials, &mut diagnostics) {
            structures.insert(key, ba);
        }
    }

    BlockArrayIr {
        structures,
        diagnostics,
    }
}

fn lower_struct<'a>(
    s: &StructIr,
    scope: Option<&'a ScopeResolution>,
    materials: Option<&'a dyn AbstractMaterialResolver>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<BlockArray> {
    let Some(size) = s.size.as_ref() else {
        diagnostics.push(diag_struct_no_size(s));
        return None;
    };
    let interior_w = size.w.get();
    let interior_h = size.h.get();

    let theme_missing = scope.is_none_or(|sc| sc.bound_theme.is_none());
    if theme_missing {
        diagnostics.push(diag_no_theme_bound(s));
    }

    // Inflate the struct's footprint by the maximum `overhang=` across all
    // roof members so the roof's eaves and gable-end overhangs have voxel
    // room outside the wall ring. Floors, walls, doors, and windows are
    // authored against the *interior* size and shifted inward by this
    // amount in their respective fill helpers.
    let overhang = max_roof_overhang(&s.members);
    let max_wall_height = max_wall_height(&s.members);
    let roof_extra = max_roof_extra_height(&s.members, interior_w, interior_h, overhang);

    let dims = Dims {
        x: interior_w.saturating_add(overhang.saturating_mul(2)),
        y: 1u32
            .saturating_add(max_wall_height)
            .saturating_add(roof_extra),
        z: interior_h.saturating_add(overhang.saturating_mul(2)),
    };
    let mut palette = Palette::new_with_air();
    let mut voxels = vec![PaletteIndex::AIR; dims.volume()];

    let ctx = StructCtx {
        scope,
        materials,
        theme_missing,
        dims,
        overhang,
        interior_w,
        interior_h,
        wall_top: max_wall_height,
    };

    // Phase ordering: collect members per phase, then process the buckets
    // in massing → envelope → openings order. Within a phase source order
    // wins (the IndexMap is filled in source order via push).
    let mut massing: Vec<&Member> = Vec::new();
    let mut envelope: Vec<&Member> = Vec::new();
    let mut openings: Vec<&Member> = Vec::new();
    for member in &s.members {
        match member_phase(&member.role) {
            Some(Phase::Massing) => massing.push(member),
            Some(Phase::Envelope) => envelope.push(member),
            Some(Phase::Openings) => openings.push(member),
            None => diagnostics.push(diag_deferred_member(member)),
        }
    }

    for member in massing {
        lower_massing_member(member, &ctx, &mut palette, &mut voxels, diagnostics);
    }
    for member in envelope {
        lower_envelope_member(member, &ctx, &mut palette, &mut voxels, diagnostics);
    }
    for member in openings {
        lower_opening_member(member, &ctx, &mut palette, &mut voxels, diagnostics);
    }

    Some(BlockArray {
        dims,
        palette,
        voxels,
        block_entities: Vec::new(),
        entities: Vec::new(),
        source_scope: format!("struct::{}", s.name),
    })
}

/// Bundle of per-struct context shared by every member-lowering helper.
///
/// Carried as a struct (rather than threaded as 7 positional args) so a new
/// per-struct field (e.g. theme name for selector-binding lookups) lands as
/// one field change instead of touching every helper signature.
struct StructCtx<'a> {
    scope: Option<&'a ScopeResolution>,
    materials: Option<&'a dyn AbstractMaterialResolver>,
    theme_missing: bool,
    dims: Dims,
    overhang: u32,
    interior_w: u32,
    interior_h: u32,
    /// Highest wall voxel coordinate (= max `height=` across walls members).
    /// `0` when no walls are present.
    wall_top: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Massing,
    Envelope,
    Openings,
}

fn member_phase(role: &MemberRole) -> Option<Phase> {
    match role {
        MemberRole::Floor | MemberRole::Walls => Some(Phase::Massing),
        MemberRole::Roof => Some(Phase::Envelope),
        MemberRole::Door | MemberRole::Window => Some(Phase::Openings),
        MemberRole::Stair
        | MemberRole::Level
        | MemberRole::PressurePlate
        | MemberRole::Circuit
        | MemberRole::Place
        | MemberRole::Connect
        | MemberRole::Other(_) => None,
    }
}

fn lower_massing_member(
    member: &Member,
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &member.role {
        MemberRole::Floor => {
            let Some(idx) = palette_index_for(
                member,
                ctx.scope,
                ctx.materials,
                palette,
                diagnostics,
                ctx.theme_missing,
            ) else {
                return;
            };
            fill_floor(ctx, idx, voxels);
        }
        MemberRole::Walls => {
            let Some(height) = wall_height(member, diagnostics) else {
                return;
            };
            let Some(idx) = palette_index_for(
                member,
                ctx.scope,
                ctx.materials,
                palette,
                diagnostics,
                ctx.theme_missing,
            ) else {
                return;
            };
            fill_walls(ctx, height, idx, voxels);
        }
        _ => unreachable!("massing phase only contains floor/walls"),
    }
}

fn lower_envelope_member(
    member: &Member,
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &member.role {
        MemberRole::Roof => fill_roof(member, ctx, palette, voxels, diagnostics),
        _ => unreachable!("envelope phase only contains roof"),
    }
}

fn lower_opening_member(
    member: &Member,
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &member.role {
        MemberRole::Door => carve_door(member, ctx, voxels, diagnostics),
        MemberRole::Window => fill_window(member, ctx, palette, voxels, diagnostics),
        _ => unreachable!("openings phase only contains door/window"),
    }
}

/// Resolve a member's `mat_slot=` binding into a concrete [`BlockState`]
/// without touching the palette.
///
/// Returns `None` (and emits at most one diagnostic) when:
/// - the scope had no theme bound (`theme_missing` short-circuits silently;
///   the `W_NO_THEME_BOUND` warning was already emitted once per struct),
/// - the member never carried a `mat_slot=`,
/// - the resolver already flagged the slot via `E_UNRESOLVED_SLOT` (the
///   binding has `slot_value == None`),
/// - the value lowered as an abstract token and no `materials` resolver was
///   offered (a `W_ABSTRACT_TOKEN_DEFERRED` warning is emitted),
/// - the value lowered as an abstract token the offered `materials` resolver
///   does not declare (an `E_UNKNOWN_ABSTRACT_TOKEN` error is emitted with
///   the nearest declared candidate, when one exists),
/// - the value was not a token at all (`E_UNKNOWN_SLOT_TARGET` already
///   fired during resolve, so no second diagnostic here).
///
/// Split out from [`palette_index_for`] so members that hard-code their
/// material (gable roof → `spruce_stairs`) can still resolve the user's
/// `mat_slot=` to check whether it agrees with the hard-coded id and emit
/// a warning when it does not — without polluting the palette with an
/// unreferenced entry.
fn resolve_member_state(
    member: &Member,
    scope: Option<&ScopeResolution>,
    materials: Option<&dyn AbstractMaterialResolver>,
    diagnostics: &mut Vec<Diagnostic>,
    theme_missing: bool,
) -> Option<BlockState> {
    if theme_missing {
        return None;
    }
    let scope = scope?;
    let binding = scope.members.get(&member.span.start)?;
    let slot_value: &ValueWithSpan = binding.slot_value.as_ref()?;
    match resolve_block_state(slot_value, materials) {
        Ok(state) => Some(state),
        Err(MaterialDeferred::Abstract(token)) => {
            diagnostics.push(diag_abstract_token(member, &token, slot_value));
            None
        }
        Err(MaterialDeferred::UnknownAbstract { token, suggestion }) => {
            diagnostics.push(diag_unknown_abstract_token(
                member,
                &token,
                suggestion.as_deref(),
                slot_value,
            ));
            None
        }
        Err(MaterialDeferred::AlreadyDiagnosed) => None,
    }
}

/// Resolve a member's `mat_slot=` binding and intern the resulting state.
///
/// Thin shim over [`resolve_member_state`] for callers that always want to
/// store the material in the palette (floors, walls, windows).
fn palette_index_for(
    member: &Member,
    scope: Option<&ScopeResolution>,
    materials: Option<&dyn AbstractMaterialResolver>,
    palette: &mut Palette,
    diagnostics: &mut Vec<Diagnostic>,
    theme_missing: bool,
) -> Option<PaletteIndex> {
    resolve_member_state(member, scope, materials, diagnostics, theme_missing)
        .map(|state| palette.intern(state))
}

fn max_wall_height(members: &[Member]) -> u32 {
    members
        .iter()
        .filter(|m| matches!(m.role, MemberRole::Walls))
        .filter_map(height_value)
        .max()
        .unwrap_or(0)
}

fn max_roof_overhang(members: &[Member]) -> u32 {
    members
        .iter()
        .filter(|m| matches!(m.role, MemberRole::Roof))
        .filter_map(|m| nonneg_int(m, "overhang"))
        .max()
        .unwrap_or(0)
}

/// Maximum vertical contribution from any roof member with a
/// recognisable [`RoofKind`]. Roofs without a recognised kind (missing
/// `kind=` or a kind outside the supported set) contribute `0` here;
/// their `W_DEFERRED_MEMBER` warning fires later, during the envelope
/// phase, against the actual member span. Computing the dim from the
/// inflated roof bounding box (interior + 2 * overhang on each axis)
/// keeps the math consistent with each per-kind generator.
fn max_roof_extra_height(
    members: &[Member],
    interior_w: u32,
    interior_h: u32,
    overhang: u32,
) -> u32 {
    let roof_w = interior_w.saturating_add(overhang.saturating_mul(2));
    let roof_h = interior_h.saturating_add(overhang.saturating_mul(2));
    members
        .iter()
        .filter(|m| matches!(m.role, MemberRole::Roof))
        .filter_map(|m| roof_kind_of(m).map(|k| roof_extra_height(k, m, roof_w, roof_h)))
        .max()
        .unwrap_or(0)
}

fn roof_extra_height(kind: RoofKind, member: &Member, roof_w: u32, roof_h: u32) -> u32 {
    match kind {
        RoofKind::Gable => gable_extra_height(roof_w.min(roof_h)),
        RoofKind::Shed => {
            // Shed's slope axis depends on `slope_to=`. We do not have
            // diagnostics here (the dim pass runs before envelope-phase
            // diagnostics), so an unrecognised or missing `slope_to=`
            // contributes `0`; the same member will surface a
            // `W_DEFERRED_MEMBER` in `fill_roof_shed` and lower to no
            // voxels, keeping the dim math conservative. The axis
            // choice goes through `shed_slope_span` — the same helper
            // `shed_voxels` uses — so the dim and the generator cannot
            // disagree on which axis the slope runs along.
            match ident_value(member, "slope_to").and_then(WallSide::from_ident) {
                Some(slope_to) => shed_extra_height(shed_slope_span(roof_w, roof_h, slope_to)),
                None => 0,
            }
        }
        RoofKind::Hip => hip_extra_height(roof_w, roof_h),
        RoofKind::Flat => flat_extra_height(),
    }
}

fn roof_kind_of(member: &Member) -> Option<RoofKind> {
    let raw = member.intent_state.get("kind")?;
    let ValueKind::Ident(name) = &raw.value.kind else {
        return None;
    };
    RoofKind::from_ident(name)
}

fn wall_height(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<u32> {
    match height_value(member) {
        Some(h) if h >= 1 => Some(h),
        _ => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                "walls without a positive `height=` cannot voxelise",
            ));
            None
        }
    }
}

fn height_value(member: &Member) -> Option<u32> {
    let raw = member.intent_state.get("height")?;
    match &raw.value.kind {
        ValueKind::Int(v) if *v > 0 => Some(u32::try_from(*v).unwrap_or(u32::MAX)),
        _ => None,
    }
}

fn nonneg_int(member: &Member, key: &str) -> Option<u32> {
    let raw = member.intent_state.get(key)?;
    match &raw.value.kind {
        ValueKind::Int(v) if *v >= 0 => Some(u32::try_from(*v).unwrap_or(u32::MAX)),
        _ => None,
    }
}

fn ident_value<'a>(member: &'a Member, key: &str) -> Option<&'a str> {
    let raw = member.intent_state.get(key)?;
    match &raw.value.kind {
        ValueKind::Ident(name) => Some(name.as_str()),
        _ => None,
    }
}

fn bool_value(member: &Member, key: &str) -> Option<bool> {
    let raw = member.intent_state.get(key)?;
    match &raw.value.kind {
        ValueKind::Bool(b) => Some(*b),
        _ => None,
    }
}

fn size_value(member: &Member, key: &str) -> Option<(u32, u32)> {
    let raw = member.intent_state.get(key)?;
    match &raw.value.kind {
        ValueKind::Size { w, h } => Some((w.get(), h.get())),
        _ => None,
    }
}

fn fill_floor(ctx: &StructCtx<'_>, idx: PaletteIndex, voxels: &mut [PaletteIndex]) {
    let y = 0;
    for z_local in 0..ctx.interior_h {
        for x_local in 0..ctx.interior_w {
            let x = ctx.overhang + x_local;
            let z = ctx.overhang + z_local;
            if let Some(i) = ctx.dims.index(x, y, z) {
                voxels[i] = idx;
            }
        }
    }
}

fn fill_walls(ctx: &StructCtx<'_>, height: u32, idx: PaletteIndex, voxels: &mut [PaletteIndex]) {
    // Cap the requested height at the volume's actual Y extent so a stray
    // out-of-range `height=` does not panic. `dims.y` is derived from the
    // module's own `max_wall_height + roof_extra + 1`, so under normal
    // lowering this never trims; defensive against a hand-built `BlockArray`.
    let top = height.min(ctx.dims.y.saturating_sub(1));
    for y in 1..=top {
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
                if let Some(i) = ctx.dims.index(x, y, z) {
                    voxels[i] = idx;
                }
            }
        }
    }
}

fn fill_roof(
    member: &Member,
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(kind) = parse_roof_kind(member, diagnostics) else {
        return;
    };
    // `mat_slot=` is advisory for every roof kind today — the generator
    // hardcodes the per-kind base block (spruce_stairs for sloped roofs,
    // spruce_planks for flat) because per-theme roof species are not
    // wired through yet. We still resolve the slot so a binding that
    // points anywhere else fires a deferred-member warning (otherwise the
    // user's intent would silently be replaced). The resolved state
    // itself is never interned: leaving the palette free of an
    // unreferenced entry keeps the on-disk NBT tight.
    if let Some(state) = resolve_member_state(
        member,
        ctx.scope,
        ctx.materials,
        diagnostics,
        ctx.theme_missing,
    ) && state.id != kind.base_block_id()
    {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "`{}` roofs currently emit `{}`; the `mat_slot=` binding to `{}` was not applied",
                kind.name(),
                kind.base_block_id(),
                state.id,
            ),
        ));
    }

    match kind {
        RoofKind::Gable => fill_roof_gable(ctx, palette, voxels),
        RoofKind::Shed => fill_roof_shed(member, ctx, palette, voxels, diagnostics),
        RoofKind::Hip => fill_roof_hip(ctx, palette, voxels),
        RoofKind::Flat => fill_roof_flat(ctx, palette, voxels),
    }
}

fn fill_roof_gable(ctx: &StructCtx<'_>, palette: &mut Palette, voxels: &mut [PaletteIndex]) {
    let roof_w = ctx.dims.x;
    let roof_h = ctx.dims.z;
    let ridge_axis = gable_ridge_axis(roof_w, roof_h);
    // Intern each face's state once so a 99-voxel cottage roof costs four
    // `palette.intern` calls instead of one per voxel. The face → palette
    // index table is a small array because [`StairFace`] has four
    // variants; iteration order pins the palette layout for the lockfile
    // hash.
    let face_table = [
        StairFace::LowSlope,
        StairFace::HighSlope,
        StairFace::ApexLow,
        StairFace::ApexHigh,
    ];
    let mut face_indices = [PaletteIndex::AIR; 4];
    for (slot, face) in face_indices.iter_mut().zip(face_table.iter().copied()) {
        *slot = palette.intern(gable_stair_state(ridge_axis, face));
    }
    for GableVoxel { pos, face } in gable_voxels(roof_w, roof_h, ctx.wall_top) {
        let idx = match face {
            StairFace::LowSlope => face_indices[0],
            StairFace::HighSlope => face_indices[1],
            StairFace::ApexLow => face_indices[2],
            StairFace::ApexHigh => face_indices[3],
        };
        if let Some(i) = ctx.dims.index(pos.0, pos.1, pos.2) {
            voxels[i] = idx;
        }
    }
}

fn fill_roof_shed(
    member: &Member,
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(slope_to) = shed_slope_to(member, diagnostics) else {
        return;
    };
    let slope_idx = palette.intern(shed_stair_state(slope_to, ShedFace::Slope));
    let apex_idx = palette.intern(shed_stair_state(slope_to, ShedFace::Apex));
    for ShedVoxel { pos, face } in shed_voxels(ctx.dims.x, ctx.dims.z, ctx.wall_top, slope_to) {
        let idx = match face {
            ShedFace::Slope => slope_idx,
            ShedFace::Apex => apex_idx,
        };
        if let Some(i) = ctx.dims.index(pos.0, pos.1, pos.2) {
            voxels[i] = idx;
        }
    }
}

fn fill_roof_hip(ctx: &StructCtx<'_>, palette: &mut Palette, voxels: &mut [PaletteIndex]) {
    let roof_w = ctx.dims.x;
    let roof_h = ctx.dims.z;
    // Hip and gable share the same long-axis-wins-with-x-tiebreak ridge
    // rule (`spec/compilation.md` §4.5 falls through to §4.3). Reusing
    // `gable_ridge_axis` keeps the two paths from drifting if the
    // tiebreak rule ever changes.
    let ridge_axis = gable_ridge_axis(roof_w, roof_h);
    // Intern per voxel: `palette.intern` dedupes, so each face's state
    // lands at exactly one slot, in the order [`hip_voxels`] visits the
    // face for the first time. That order is fixed by the generator's
    // layer iteration, so the palette layout is deterministic without
    // a separate face → slot table. The match-on-face indirection a
    // pre-intern table requires is what made a slot mis-mapping
    // possible the moment `HipFace` grew or reordered; folding the
    // intern call into the voxel loop closes that gap.
    for HipVoxel { pos, face } in hip_voxels(roof_w, roof_h, ctx.wall_top) {
        let idx = palette.intern(hip_stair_state(ridge_axis, face));
        if let Some(i) = ctx.dims.index(pos.0, pos.1, pos.2) {
            voxels[i] = idx;
        }
    }
}

fn fill_roof_flat(ctx: &StructCtx<'_>, palette: &mut Palette, voxels: &mut [PaletteIndex]) {
    let deck_idx = palette.intern(flat_block_state());
    for (x, y, z) in flat_voxels(ctx.dims.x, ctx.dims.z, ctx.wall_top) {
        if let Some(i) = ctx.dims.index(x, y, z) {
            voxels[i] = deck_idx;
        }
    }
}

/// Resolve a `roof` member's `kind=` to a [`RoofKind`].
///
/// Pushes a `W_DEFERRED_MEMBER` warning and returns `None` when the
/// `kind=` is missing, typed wrong, or names a kind outside the supported
/// set. Keeping the dispatch table in [`RoofKind::from_ident`] and the
/// diagnostic phrasing here lets each side stay self-contained.
fn parse_roof_kind(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<RoofKind> {
    let Some(raw) = ident_value(member, "kind") else {
        let reason = if member.intent_state.contains_key("kind") {
            "roof `kind=` must be one of gable, shed, hip, flat"
        } else {
            "missing `kind=` (expected one of gable, shed, hip, flat)"
        };
        diagnostics.push(diag_deferred_member_reason(member, reason));
        return None;
    };
    if let Some(k) = RoofKind::from_ident(raw) {
        return Some(k);
    }
    diagnostics.push(diag_deferred_member_reason(
        member,
        &format!("unknown roof `kind={raw}` (expected one of gable, shed, hip, flat)"),
    ));
    None
}

/// Resolve a shed roof's `slope_to=` argument.
///
/// Required for `kind=shed` because the slope direction has no sensible
/// default — picking one silently would let a typo emit a roof that
/// peaks on the wrong wall. Missing or mis-typed `slope_to=` therefore
/// surfaces a `W_DEFERRED_MEMBER` warning.
fn shed_slope_to(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<WallSide> {
    let Some(raw) = ident_value(member, "slope_to") else {
        let reason = if member.intent_state.contains_key("slope_to") {
            "shed `slope_to=` must be one of front, back, left, right"
        } else {
            "shed roof requires `slope_to=` (one of front, back, left, right)"
        };
        diagnostics.push(diag_deferred_member_reason(member, reason));
        return None;
    };
    if let Some(side) = WallSide::from_ident(raw) {
        return Some(side);
    }
    diagnostics.push(diag_deferred_member_reason(
        member,
        &format!("unknown shed `slope_to={raw}` (expected one of front, back, left, right)"),
    ));
    None
}

fn carve_door(
    member: &Member,
    ctx: &StructCtx<'_>,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(side) = side_of(member, diagnostics) else {
        return;
    };
    // A door needs at least one wall row to carve into. Without a positive
    // wall height there is nothing above the floor to open up; the
    // envelope phase has already written roof voxels at y=1, and carving
    // them would punch a gap into the roof.
    if ctx.wall_top < 1 {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "door requires a `walls` member with positive `height=` to carve into",
        ));
        return;
    }
    let len = wall_length(side, ctx.interior_w, ctx.interior_h);
    let at = match ident_value(member, "at") {
        // `at=center`: round half-up so an even-width wall picks the
        // column at `len/2`. Documented in spec/syntax.md §5.4. For odd
        // widths the two formulas coincide.
        Some("center") => len / 2,
        Some(other) => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!("door `at={other}` is not yet supported (use `at=center`)"),
            ));
            return;
        }
        None => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                "door without `at=` is not yet supported (use `at=center`)",
            ));
            return;
        }
    };
    // Doors carve a 1-wide opening starting at y=1 (the row just above
    // the floor), capped at the wall top so a short-wall door cannot
    // overwrite roof voxels written in the envelope phase. The door block
    // itself (`oak_door`, hinge/half/facing/open) is not yet placed; that
    // landed deferred along with per-theme door materials.
    let top = ctx.wall_top.min(2);
    for v in 1..=top {
        let Some((x, y, z)) = wall_local_to_grid(
            side,
            at,
            v,
            ctx.overhang,
            ctx.interior_w,
            ctx.interior_h,
            ctx.dims,
        ) else {
            continue;
        };
        if let Some(i) = ctx.dims.index(x, y, z) {
            voxels[i] = PaletteIndex::AIR;
        }
    }
}

fn fill_window(
    member: &Member,
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(side) = side_of(member, diagnostics) else {
        return;
    };
    let Some(offset) = nonneg_int(member, "offset") else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "window without `offset=` is not yet supported",
        ));
        return;
    };
    let Some(y_start) = nonneg_int(member, "y") else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "window without `y=` is not yet supported",
        ));
        return;
    };
    let Some((sw, sh)) = size_value(member, "size") else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "window without `size=WxH` is not yet supported",
        ));
        return;
    };
    let sym = bool_value(member, "sym").unwrap_or(false);
    let Some(idx) = palette_index_for(
        member,
        ctx.scope,
        ctx.materials,
        palette,
        diagnostics,
        ctx.theme_missing,
    ) else {
        return;
    };

    let len = wall_length(side, ctx.interior_w, ctx.interior_h);
    if offset.saturating_add(sw) > len {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "window extends beyond the `{}` wall (offset={offset} size={sw}x{sh}, wall length={len})",
                side_name(side),
            ),
        ));
        return;
    }
    if y_start.saturating_add(sh) > ctx.dims.y {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "window extends above the struct (y={y_start} size={sw}x{sh}, dims.y={})",
                ctx.dims.y,
            ),
        ));
        return;
    }
    let rect = WindowRect {
        side,
        offset,
        y_start,
        width: sw,
        height: sh,
        palette_index: idx,
    };
    paint_window_rect(ctx, rect, voxels);
    if sym {
        let mirror_offset = len.saturating_sub(offset).saturating_sub(sw);
        if mirror_offset == offset {
            // The mirror sits exactly on top of the primary; emitting it
            // again would be a no-op so we silently coalesce.
            return;
        }
        // Reject overlapping mirrors: a `sym=true` window asks for a
        // *pair*, not one wide span. If the two rectangles intersect the
        // user almost certainly wrote a window that is more than half as
        // wide as the wall — diagnose and skip the mirror so the primary
        // is still emitted cleanly.
        let primary_end = offset.saturating_add(sw);
        let mirror_end = mirror_offset.saturating_add(sw);
        let overlap = offset < mirror_end && mirror_offset < primary_end;
        if overlap {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "`sym=true` window at offset={offset} size={sw}x{sh} on the `{}` wall would overlap its mirror (wall length={len}); the mirror was skipped",
                    side_name(side),
                ),
            ));
            return;
        }
        paint_window_rect(
            ctx,
            WindowRect {
                offset: mirror_offset,
                ..rect
            },
            voxels,
        );
    }
}

#[derive(Debug, Clone, Copy)]
struct WindowRect {
    side: WallSide,
    offset: u32,
    y_start: u32,
    width: u32,
    height: u32,
    palette_index: PaletteIndex,
}

fn paint_window_rect(ctx: &StructCtx<'_>, rect: WindowRect, voxels: &mut [PaletteIndex]) {
    for du in 0..rect.width {
        for dv in 0..rect.height {
            let Some((x, y, z)) = wall_local_to_grid(
                rect.side,
                rect.offset + du,
                rect.y_start + dv,
                ctx.overhang,
                ctx.interior_w,
                ctx.interior_h,
                ctx.dims,
            ) else {
                continue;
            };
            if let Some(i) = ctx.dims.index(x, y, z) {
                voxels[i] = rect.palette_index;
            }
        }
    }
}

fn side_of(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<WallSide> {
    let Some(raw) = ident_value(member, "side") else {
        // Distinguish "missing entirely" (no `side=` key) from "wrong
        // type" (`side=` present but its value is not an identifier). A
        // silent return on the missing case would let a `door at=center`
        // line lower to nothing without telling the author, which breaks
        // the module-level promise that every dropped member surfaces a
        // diagnostic.
        let reason = if member.intent_state.contains_key("side") {
            "`side=` must be one of front, back, left, right"
        } else {
            "missing `side=` (expected one of front, back, left, right)"
        };
        diagnostics.push(diag_deferred_member_reason(member, reason));
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

fn side_name(side: WallSide) -> &'static str {
    match side {
        WallSide::Front => "front",
        WallSide::Back => "back",
        WallSide::Left => "left",
        WallSide::Right => "right",
    }
}

fn diag_struct_no_size(s: &StructIr) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::StructNoSize,
        severity: Severity::Warning,
        span: s.span.clone(),
        primary: format!(
            "struct `{}` has no `size=WxH`; block-array lowering skipped it",
            s.name,
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "add a `size=WxH` header to give the struct a voxel footprint".to_owned(),
        }],
    }
}

fn diag_no_theme_bound(s: &StructIr) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::NoThemeBound,
        severity: Severity::Warning,
        span: s.span.clone(),
        primary: format!(
            "struct `{}` has no theme bound; every `mat_slot=` will lower to air",
            s.name,
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "declare exactly one `theme NAME:` in the module, or wait until M3 \
                      site-level `place ... theme=` resolves multi-theme files"
                .to_owned(),
        }],
    }
}

fn diag_deferred_member(member: &Member) -> Diagnostic {
    let role = role_name(&member.role);
    diag_deferred_member_reason(
        member,
        &format!("`{role}` is not yet handled by block-array lowering"),
    )
}

fn diag_deferred_member_reason(member: &Member, reason: &str) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::DeferredMember,
        severity: Severity::Warning,
        span: member.span.clone(),
        primary: reason.to_owned(),
        notes: vec![DiagnosticNote {
            span: None,
            message: "block-array lowering currently voxelises floor, walls, door, window, \
                      and roof (kind=gable|shed|hip|flat); other roles will be added as their \
                      lowering rules are spec'd"
                .to_owned(),
        }],
    }
}

fn diag_abstract_token(member: &Member, token: &str, slot: &ValueWithSpan) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::AbstractTokenDeferred,
        severity: Severity::Warning,
        span: member_or_slot_span(member, slot),
        primary: format!(
            "abstract token `@{token}` cannot be lowered without the registry pack; the cell falls back to air",
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message:
                "use a canonical block token (e.g. `@oak_planks`) until the registry pack ships"
                    .to_owned(),
        }],
    }
}

fn diag_unknown_abstract_token(
    member: &Member,
    token: &str,
    suggestion: Option<&str>,
    slot: &ValueWithSpan,
) -> Diagnostic {
    let primary = format!(
        "abstract token `@{token}` is not declared by the registry pack's materials catalog",
    );
    let mut notes = Vec::with_capacity(2);
    if let Some(s) = suggestion {
        notes.push(DiagnosticNote {
            span: None,
            message: format!("did you mean `@{s}`?"),
        });
    }
    notes.push(DiagnosticNote {
        span: None,
        message: "abstract material tokens must be declared in the pack's `materials` catalog \
                  (see `spec/materials-themes.md` §7.2)"
            .to_owned(),
    });
    Diagnostic {
        code: DiagnosticCode::UnknownAbstractToken,
        severity: Severity::Error,
        span: member_or_slot_span(member, slot),
        primary,
        notes,
    }
}

/// Prefer the slot-binding span (which points at the `@token`) over the
/// member line so the warning underlines the exact value that could not be
/// lowered.
fn member_or_slot_span(member: &Member, slot: &ValueWithSpan) -> Span {
    if slot.span.start == 0 && slot.span.end == 0 {
        member.span.clone()
    } else {
        slot.span.clone()
    }
}

fn role_name(role: &MemberRole) -> &str {
    match role {
        MemberRole::Floor => "floor",
        MemberRole::Walls => "walls",
        MemberRole::Door => "door",
        MemberRole::Window => "window",
        MemberRole::Roof => "roof",
        MemberRole::Stair => "stair",
        MemberRole::Level => "level",
        MemberRole::PressurePlate => "pressure_plate",
        MemberRole::Circuit => "circuit",
        MemberRole::Place => "place",
        MemberRole::Connect => "connect",
        MemberRole::Other(name) => name.as_str(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_array::BlockState;
    use crate::{lower, parse, resolve};

    fn lowered(source: &str) -> BlockArrayIr {
        let module = parse(source).expect("parse");
        let ir = lower(&module);
        let resolution = resolve(&ir);
        lower_to_block_array(&ir, &resolution, None)
    }

    fn lowered_with_resolver(
        source: &str,
        resolver: &dyn AbstractMaterialResolver,
    ) -> BlockArrayIr {
        let module = parse(source).expect("parse");
        let ir = lower(&module);
        let resolution = resolve(&ir);
        lower_to_block_array(&ir, &resolution, Some(resolver))
    }

    struct FakeResolver {
        entries: Vec<(&'static str, &'static str)>,
    }

    impl AbstractMaterialResolver for FakeResolver {
        fn lookup(&self, token: &str) -> Option<BlockState> {
            self.entries
                .iter()
                .find(|(t, _)| *t == token)
                .map(|(_, id)| BlockState::bare(format!("minecraft:{id}")))
        }

        fn known_tokens(&self) -> Vec<String> {
            self.entries.iter().map(|(t, _)| (*t).to_owned()).collect()
        }
    }

    fn block_id(ba: &BlockArray, x: u32, y: u32, z: u32) -> &str {
        let i = ba.dims.index(x, y, z).expect("in-range coordinate");
        let pi = ba.voxels[i];
        ba.palette.entries[usize::from(pi.0)].id.as_str()
    }

    fn deferred_count(out: &BlockArrayIr) -> usize {
        out.diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .count()
    }

    #[test]
    fn floor_only_fills_y_zero_plane() {
        let src = "theme t:\n  slot f -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").expect("structure lowered");
        assert_eq!(ba.dims, Dims { x: 3, y: 1, z: 3 });
        for z in 0..3 {
            for x in 0..3 {
                assert_eq!(block_id(ba, x, 0, z), "minecraft:cobblestone");
            }
        }
        assert!(
            out.diagnostics.is_empty(),
            "no diagnostics expected, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn walls_only_fills_outline_above_floor() {
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=3x3\n  walls mat_slot=w height=2\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims, Dims { x: 3, y: 3, z: 3 });
        // y=0 stays air everywhere — there is no floor in this struct.
        for z in 0..3 {
            for x in 0..3 {
                assert_eq!(block_id(ba, x, 0, z), BlockState::AIR_ID);
            }
        }
        // y=1 and y=2 carry the outline; the centre cell stays air.
        for y in 1..=2 {
            assert_eq!(block_id(ba, 1, y, 1), BlockState::AIR_ID, "centre at y={y}");
            for z in 0..3 {
                for x in 0..3 {
                    let on_edge = x == 0 || x == 2 || z == 0 || z == 2;
                    let expected = if on_edge {
                        "minecraft:cobblestone"
                    } else {
                        BlockState::AIR_ID
                    };
                    assert_eq!(block_id(ba, x, y, z), expected, "({x},{y},{z})");
                }
            }
        }
    }

    #[test]
    fn floor_and_walls_combine() {
        let src = "theme t:\n  slot f -> @oak_planks\n  slot w -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n  walls mat_slot=w height=2\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims, Dims { x: 3, y: 3, z: 3 });
        // Floor plane.
        for z in 0..3 {
            for x in 0..3 {
                assert_eq!(block_id(ba, x, 0, z), "minecraft:oak_planks");
            }
        }
        // Walls outline at y=1.
        assert_eq!(block_id(ba, 0, 1, 0), "minecraft:cobblestone");
        assert_eq!(block_id(ba, 1, 1, 1), BlockState::AIR_ID);
    }

    #[test]
    fn unknown_role_warns_and_skips() {
        // `stair` is in the keyword table but no phase claims it yet, so
        // it must surface as DeferredMember without touching voxels.
        let src = "theme t:\n  slot f -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n  stair side=front\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(deferred_count(&out), 1);
        for z in 0..3 {
            for x in 0..3 {
                assert_eq!(block_id(ba, x, 0, z), "minecraft:cobblestone");
            }
        }
    }

    #[test]
    fn missing_theme_warns_and_air_fills() {
        let src = "struct s size=3x3\n  floor mat_slot=f\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::NoThemeBound),
        );
        for z in 0..3 {
            for x in 0..3 {
                assert_eq!(block_id(ba, x, 0, z), BlockState::AIR_ID);
            }
        }
    }

    #[test]
    fn already_diagnosed_slot_does_not_re_warn() {
        // The resolver emits E_UNRESOLVED_SLOT for `mat_slot=missing`. We
        // must NOT also emit `W_DEFERRED_MEMBER` or
        // `W_ABSTRACT_TOKEN_DEFERRED` for the same span — double diagnosis
        // would teach a user there are two unrelated problems when there
        // is one.
        let src =
            "theme t:\n  slot f -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=missing\n";
        let out = lowered(src);
        assert!(
            !out.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::DeferredMember
                    || d.code == DiagnosticCode::AbstractTokenDeferred),
            "no follow-on diagnostics expected, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn abstract_token_lifts_through_supplied_resolver() {
        // When `lower_to_block_array` is given a resolver that knows the
        // bound abstract token, the cell must lower to the catalog's
        // canonical id instead of staying air with W_ABSTRACT_TOKEN_DEFERRED.
        let resolver = FakeResolver {
            entries: vec![
                ("floor.wood.broadleaf", "oak_planks"),
                ("wall.stone.cobble", "cobblestone"),
            ],
        };
        let src = "theme t:\n  \
                   slot f -> @floor.wood.broadleaf\n  \
                   slot w -> @wall.stone.cobble\n\n\
                   struct s size=3x3\n  \
                   floor mat_slot=f\n  \
                   walls mat_slot=w height=2\n";
        let out = lowered_with_resolver(src, &resolver);
        assert!(
            out.diagnostics
                .iter()
                .all(|d| d.code != DiagnosticCode::AbstractTokenDeferred),
            "no abstract-token deferral expected when the resolver covers every token, got {:?}",
            out.diagnostics,
        );
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(block_id(ba, 1, 0, 1), "minecraft:oak_planks");
        assert_eq!(block_id(ba, 0, 1, 0), "minecraft:cobblestone");
    }

    #[test]
    fn unknown_abstract_token_emits_e_unknown_abstract_token() {
        // When the resolver does not declare the bound token, lowering must
        // surface E_UNKNOWN_ABSTRACT_TOKEN with the nearest-declared
        // candidate as a note. Cell falls back to air.
        let resolver = FakeResolver {
            entries: vec![("floor.wood.broadleaf", "oak_planks")],
        };
        let src = "theme t:\n  \
                   slot f -> @floor.wood.broadlef\n\n\
                   struct s size=3x3\n  \
                   floor mat_slot=f\n";
        let out = lowered_with_resolver(src, &resolver);
        let diag = out
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::UnknownAbstractToken)
            .expect("expected E_UNKNOWN_ABSTRACT_TOKEN, got {:?}");
        assert_eq!(diag.severity, Severity::Error);
        assert!(
            diag.notes
                .iter()
                .any(|n| n.message.contains("floor.wood.broadleaf")),
            "expected suggestion note in {:?}",
            diag.notes,
        );
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(block_id(ba, 1, 0, 1), BlockState::AIR_ID);
    }

    #[test]
    fn struct_without_size_is_skipped_with_warning() {
        let src = "theme t:\n  slot f -> @cobblestone\n\nstruct s\n  floor mat_slot=f\n";
        let out = lowered(src);
        assert!(!out.structures.contains_key("struct::s"));
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::StructNoSize),
        );
    }

    #[test]
    fn state_literal_round_trips_through_palette() {
        // Bracketed tokens are not yet emitted by the surface parser, so
        // this exercises the palette/material path directly to lock the
        // canonical-id and property-bag contract before the state-literal
        // grammar lands.
        let mut palette = Palette::new_with_air();
        let token = ValueWithSpan::from_value(crate::ast::Value::new(
            ValueKind::Token("oak_log[axis=x]".to_owned()),
            0..16,
        ));
        let bs = resolve_block_state(&token, None).unwrap();
        let idx = palette.intern(bs);
        assert_eq!(palette.entries[usize::from(idx.0)].id, "minecraft:oak_log");
        assert_eq!(
            palette.entries[usize::from(idx.0)]
                .properties
                .get("axis")
                .map(String::as_str),
            Some("x"),
        );
    }

    // --- door / window / roof voxelisation ----------------------------------

    #[test]
    fn phase_order_independent_of_source_order() {
        // door is written BEFORE walls in source; phase ordering must still
        // run massing first, then openings, so the door's AIR carve survives
        // through the wall fill.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  door side=front at=center\n  walls mat_slot=w height=3\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        // Front wall is z = dims.z - 1 = 4. Center x = (5-1)/2 = 2. Door y=1,2.
        assert_eq!(block_id(ba, 2, 1, 4), BlockState::AIR_ID);
        assert_eq!(block_id(ba, 2, 2, 4), BlockState::AIR_ID);
        // Wall corners survived.
        assert_eq!(block_id(ba, 0, 1, 0), "minecraft:cobblestone");
    }

    #[test]
    fn roof_increases_dims_y_by_ceil_half_span() {
        // size=9x7, walls height=4, kind=gable, overhang=0.
        // roof bbox short axis = min(9, 7) = 7 → ridge_extra = ceil(7/2) = 4.
        // dims.y = 1 + 4 + 4 = 9.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=9x7\n  walls mat_slot=w height=4\n  roof kind=gable mat_slot=r\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims, Dims { x: 9, y: 9, z: 7 });
    }

    #[test]
    fn roof_overhang_extends_xz_dims_and_shifts_walls() {
        // overhang=1 → dims.x = 9+2 = 11, dims.z = 7+2 = 9.
        // Floor is the 9x7 interior placed at x∈[1, 9], z∈[1, 7].
        let src = "theme t:\n  slot f -> @oak_planks\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=9x7\n  floor mat_slot=f\n  walls mat_slot=w height=4\n  roof kind=gable mat_slot=r overhang=1\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims.x, 11);
        assert_eq!(ba.dims.z, 9);
        // Floor inside the interior, air at the overhang ring.
        assert_eq!(block_id(ba, 1, 0, 1), "minecraft:oak_planks");
        assert_eq!(block_id(ba, 9, 0, 7), "minecraft:oak_planks");
        assert_eq!(block_id(ba, 0, 0, 0), BlockState::AIR_ID);
        assert_eq!(block_id(ba, 10, 0, 8), BlockState::AIR_ID);
        // Wall corner shifted to (1, 1, 1) rather than (0, 1, 0).
        assert_eq!(block_id(ba, 1, 1, 1), "minecraft:cobblestone");
        assert_eq!(block_id(ba, 0, 1, 0), BlockState::AIR_ID);
    }

    #[test]
    fn gable_roof_places_stairs_with_facing() {
        let src = "theme t:\n  slot r -> @spruce_stairs\n\nstruct s size=9x7\n  walls mat_slot=r height=4\n  roof kind=gable mat_slot=r overhang=1\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        // Layer 0 of the roof sits at y=5. Ridge along x (long axis with
        // overhang dims.x=11, dims.z=9 → span=9 along z).
        let north_eave = block_state_at(ba, 0, 5, 0);
        assert_eq!(north_eave.id, "minecraft:spruce_stairs");
        assert_eq!(north_eave.properties.get("facing").unwrap(), "south");
        assert_eq!(north_eave.properties.get("half").unwrap(), "bottom");
        let south_eave = block_state_at(ba, 0, 5, 8);
        assert_eq!(south_eave.properties.get("facing").unwrap(), "north");
        // Apex: gable_extra_height(9) = 5 → y = 4 + 5 = 9, z = 4 (centre).
        let apex = block_state_at(ba, 0, 9, 4);
        assert_eq!(apex.properties.get("half").unwrap(), "top");
        assert_eq!(apex.properties.get("facing").unwrap(), "south");
    }

    #[test]
    fn door_carves_opening_through_front_wall() {
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=9x7\n  walls mat_slot=w height=4\n  door side=front at=center\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        // Front wall at z=6 (no overhang). Center x = (9-1)/2 = 4. y=1,2.
        assert_eq!(block_id(ba, 4, 1, 6), BlockState::AIR_ID);
        assert_eq!(block_id(ba, 4, 2, 6), BlockState::AIR_ID);
        // Surrounding wall cells still cobblestone.
        assert_eq!(block_id(ba, 3, 1, 6), "minecraft:cobblestone");
        assert_eq!(block_id(ba, 4, 3, 6), "minecraft:cobblestone");
    }

    #[test]
    fn window_places_glass_with_symmetry() {
        let src = "theme t:\n  slot w -> @cobblestone\n  slot g -> @glass_pane\n\nstruct s size=9x7\n  walls mat_slot=w height=4\n  window side=front offset=2 y=2 size=2x2 sym=true mat_slot=g\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        // Front wall at z=6. Primary rectangle: x∈[2,4), y∈[2,4).
        for dx in 0..2 {
            for dy in 0..2 {
                assert_eq!(
                    block_id(ba, 2 + dx, 2 + dy, 6),
                    "minecraft:glass_pane",
                    "primary ({},{})",
                    2 + dx,
                    2 + dy,
                );
            }
        }
        // Mirror: wall length = 9, mirror_offset = 9 - 2 - 2 = 5 → x∈[5,7).
        for dx in 0..2 {
            for dy in 0..2 {
                assert_eq!(
                    block_id(ba, 5 + dx, 2 + dy, 6),
                    "minecraft:glass_pane",
                    "mirror ({},{})",
                    5 + dx,
                    2 + dy,
                );
            }
        }
    }

    #[test]
    fn window_out_of_bounds_warns_and_skips() {
        let src = "theme t:\n  slot w -> @cobblestone\n  slot g -> @glass_pane\n\nstruct s size=5x5\n  walls mat_slot=w height=4\n  window side=front offset=3 y=2 size=3x2 mat_slot=g\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        let deferred = deferred_count(&out);
        assert_eq!(deferred, 1);
        // Front wall at z=4 should retain cobblestone (no glass painted).
        for x in 0..5 {
            assert_eq!(block_id(ba, x, 2, 4), "minecraft:cobblestone");
        }
    }

    #[test]
    fn unknown_roof_kind_warns_and_skips() {
        // `pyramid` sits outside the supported gable|shed|hip|flat set,
        // so lowering must surface a deferred-member warning and emit no
        // roof voxels (dims.y stays at `1 + wall_height` because the
        // unknown kind contributes 0 to `max_roof_extra_height`).
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=4\n  roof kind=pyramid mat_slot=r\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(deferred_count(&out), 1);
        assert_eq!(ba.dims.y, 5);
    }

    #[test]
    fn shed_roof_voxelises_with_slope_to_front() {
        // size=5x5, walls height=3, shed slope_to=front, no overhang.
        // slope_span = roof_h = 5 → extra_height = 5 → dims.y = 1 + 3 + 5 = 9.
        // Slope axis is z (Front=+z). Layer 0 (y=4) sits at z=0; apex
        // (y=8) sits at z=4. Stairs facing south.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=shed slope_to=front mat_slot=r\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims.y, 9);
        let layer0 = block_state_at(ba, 0, 4, 0);
        assert_eq!(layer0.id, "minecraft:spruce_stairs");
        assert_eq!(layer0.properties.get("facing").unwrap(), "south");
        assert_eq!(layer0.properties.get("half").unwrap(), "bottom");
        let apex = block_state_at(ba, 0, 8, 4);
        assert_eq!(apex.properties.get("half").unwrap(), "top");
    }

    #[test]
    fn shed_roof_without_slope_to_emits_deferred_warning() {
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=shed mat_slot=r\n";
        let out = lowered(src);
        assert_eq!(deferred_count(&out), 1);
        let primary = &out.diagnostics[0].primary;
        assert!(
            primary.contains("slope_to"),
            "expected slope_to mention, got {primary}",
        );
    }

    #[test]
    fn hip_roof_voxelises_square_footprint() {
        // size=5x5, walls height=3. hip_extra_height = ceil(5/2) = 3.
        // dims.y = 1 + 3 + 3 = 7 (so highest valid y is 6). Apex sits at
        // y = wall_top + extra_height = 6, single cell at (2, 6, 2).
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=hip mat_slot=r\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims.y, 7);
        let apex = block_state_at(ba, 2, 6, 2);
        assert_eq!(apex.id, "minecraft:spruce_stairs");
        assert_eq!(apex.properties.get("half").unwrap(), "top");
        // North-west corner of layer 0 uses `shape=outer_left`.
        let nw_corner = block_state_at(ba, 0, 4, 0);
        assert_eq!(nw_corner.properties.get("shape").unwrap(), "outer_left");
        assert_eq!(nw_corner.properties.get("facing").unwrap(), "south");
    }

    #[test]
    fn flat_roof_voxelises_single_layer_of_planks() {
        // size=5x5, walls height=3, flat → extra_height=1, dims.y = 5.
        // Every cell at y=4 is spruce_planks.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_planks\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims.y, 5);
        for z in 0..5 {
            for x in 0..5 {
                assert_eq!(block_id(ba, x, 4, z), "minecraft:spruce_planks");
            }
        }
        assert_eq!(deferred_count(&out), 0);
    }

    #[test]
    fn flat_roof_with_mismatched_mat_slot_emits_warning() {
        // Flat hardcodes spruce_planks; binding to anything else (e.g.
        // spruce_stairs which is the sloped-roof default) must warn so
        // the user notices the binding was dropped.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("spruce_planks")
                    && d.primary.contains("spruce_stairs")),
            "expected mat_slot mismatch diagnostic, got {:?}",
            out.diagnostics,
        );
    }

    fn block_state_at(ba: &BlockArray, x: u32, y: u32, z: u32) -> &BlockState {
        let i = ba.dims.index(x, y, z).expect("in-range coord");
        let pi = ba.voxels[i];
        &ba.palette.entries[usize::from(pi.0)]
    }

    // --- regression coverage for review feedback ----------------------------

    #[test]
    fn door_without_side_emits_deferred_warning() {
        // A `door at=center` line with no `side=` used to drop silently
        // because `side_of` short-circuited on the missing key. Every
        // dropped member must surface a diagnostic.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  door at=center\n";
        let out = lowered(src);
        assert_eq!(deferred_count(&out), 1);
        let primary = &out.diagnostics[0].primary;
        assert!(
            primary.contains("missing `side="),
            "expected missing-side reason, got {primary}",
        );
    }

    #[test]
    fn window_with_non_ident_side_emits_deferred_warning() {
        // `side=` present but typed wrong (here as an integer literal).
        // The `wrong type` branch in `side_of` must fire so the user
        // hears about it.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot g -> @glass_pane\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  window side=3 offset=1 y=1 size=1x1 mat_slot=g\n";
        let out = lowered(src);
        let deferred = deferred_count(&out);
        assert!(deferred >= 1, "expected a side= diagnostic, got {deferred}");
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("`side=`")),
            "expected a `side=` mention in diagnostics: {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn sym_window_overlap_skips_mirror_with_warning() {
        // wall length=6, offset=2, size=3 → mirror_offset = 6-2-3 = 1.
        // [2..5) and [1..4) overlap — the mirror would fuse with the
        // primary into one wide span. We diagnose and keep only the
        // primary so the user notices.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot g -> @glass_pane\n\nstruct s size=6x5\n  walls mat_slot=w height=4\n  window side=front offset=2 y=2 size=3x1 sym=true mat_slot=g\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("overlap")),
            "expected overlap diagnostic, got {:?}",
            out.diagnostics,
        );
        // Primary rectangle [x=2..5, y=2] painted.
        for x in 2..5 {
            assert_eq!(block_id(ba, x, 2, 4), "minecraft:glass_pane");
        }
        // Mirror cells outside the primary stay cobblestone (x=1).
        assert_eq!(block_id(ba, 1, 2, 4), "minecraft:cobblestone");
    }

    #[test]
    fn door_capped_at_wall_top_does_not_punch_through_roof() {
        // walls height=1 → wall_top=1. Door y=1..=2 would carve a hole at
        // y=2 which the roof's south-eave layer occupies. Capping at
        // wall_top keeps the roof intact.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=1\n  roof kind=gable mat_slot=r\n  door side=front at=center\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        // Door carves only y=1 of the front wall.
        assert_eq!(block_id(ba, 2, 1, 4), BlockState::AIR_ID);
        // y=2 on the front-eave row of the roof must still be stairs.
        // span = min(5,5) = 5, ridge axis = x, low slope at z=0 layer 0,
        // high slope at z=4 layer 0, y = wall_top+1 = 2.
        let south_eave = block_state_at(ba, 2, 2, 4);
        assert_eq!(south_eave.id, "minecraft:spruce_stairs");
    }

    #[test]
    fn door_without_walls_emits_deferred_warning() {
        // No walls member → wall_top=0. The door cannot carve anything
        // and must complain instead of doing nothing silently.
        let src = "theme t:\n  slot f -> @oak_planks\n\nstruct s size=5x5\n  floor mat_slot=f\n  door side=front at=center\n";
        let out = lowered(src);
        assert!(
            out.diagnostics.iter().any(|d| d.primary.contains("walls")),
            "expected walls-required diagnostic, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn at_center_picks_right_of_centre_on_even_width_walls() {
        // size=8x5 → wall length 8. `at=center` should pick column 4 (the
        // right half-block of the geometric centre), not column 3, so the
        // door is consistent with round-half-up semantics.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=8x5\n  walls mat_slot=w height=3\n  door side=front at=center\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        // Front wall z=4. y=1 air at x=4, cobblestone at x=3.
        assert_eq!(block_id(ba, 4, 1, 4), BlockState::AIR_ID);
        assert_eq!(block_id(ba, 3, 1, 4), "minecraft:cobblestone");
    }

    #[test]
    fn gable_with_mismatched_mat_slot_emits_warning() {
        // The roof generator hardcodes spruce_stairs; a theme that binds
        // `slot roof -> @oak_stairs` must hear that its choice was not
        // applied rather than silently getting the wrong species.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @oak_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=gable mat_slot=r\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("oak_stairs") && d.primary.contains("spruce_stairs")),
            "expected mat_slot mismatch diagnostic, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn gable_with_matching_mat_slot_stays_silent() {
        // The cottage case: theme binds the slot to spruce_stairs, the
        // generator emits spruce_stairs — no warning.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=gable mat_slot=r\n";
        let out = lowered(src);
        assert_eq!(
            deferred_count(&out),
            0,
            "expected silence on matching mat_slot, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn even_span_gable_apex_uses_half_top() {
        // size=8x4 → roof span (short axis) = 4 (even). The apex layer
        // must cap with two half=top rows or the ridge has an open V.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=8x4\n  walls mat_slot=w height=4\n  roof kind=gable mat_slot=r\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        // gable_extra_height(4) = 2 layers. Apex layer at y = 4+2 = 6.
        let apex_low = block_state_at(ba, 0, 6, 1);
        let apex_high = block_state_at(ba, 0, 6, 2);
        assert_eq!(apex_low.properties.get("half").unwrap(), "top");
        assert_eq!(apex_high.properties.get("half").unwrap(), "top");
    }
}
