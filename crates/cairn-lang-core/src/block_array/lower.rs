//! Intent IR → block-array IR lowering for M2 (floor and walls only).
//!
//! The pass is total: every struct ends up in
//! [`BlockArrayIr::structures`], every issue surfaces as a warning on
//! [`BlockArrayIr::diagnostics`]. That keeps `cairn lower` useful even on
//! a half-finished module — the operator can see what voxels did lower,
//! and the diagnostic stream tells them what was skipped and why.
//!
//! Defs are skipped at this layer: they only concretise via a `site`
//! `place ... use=def_name` reference, and site lowering is M3 work. Sites
//! themselves are also skipped for the same reason.

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::check::{Diagnostic, DiagnosticCode, DiagnosticNote, Severity};
use crate::error::Span;
use crate::intent::{IntentModule, Member, MemberRole, StructIr, ValueWithSpan};
use crate::resolve::{Resolution, ScopeResolution};

use super::material::{MaterialDeferred, resolve_block_state};
use super::{BlockArray, BlockArrayIr, Dims, Palette, PaletteIndex};

/// Lower every `struct` in `intent` into a [`BlockArray`].
///
/// Pairs each struct with its [`ScopeResolution`] from `resolution` so the
/// material lookups go through the same theme bindings `cairn check` and
/// `cairn info` already used. Members whose roles are not yet voxelised
/// (door, window, roof, ...) are reported via `W_DEFERRED_MEMBER` and skipped.
#[must_use]
pub fn lower_to_block_array(intent: &IntentModule, resolution: &Resolution) -> BlockArrayIr {
    let mut structures: IndexMap<String, BlockArray> = IndexMap::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for s in &intent.structs {
        let key = format!("struct::{}", s.name);
        let scope = resolution.scopes.get(&key);
        // `lower_struct` returns `None` only after it has already pushed a
        // diagnostic (no `size=`, etc.), so the skip here is silent on
        // purpose — diagnosing twice would teach a reader the struct had
        // two unrelated problems instead of one.
        if let Some(ba) = lower_struct(s, scope, &mut diagnostics) {
            structures.insert(key, ba);
        }
    }

    BlockArrayIr {
        structures,
        diagnostics,
    }
}

fn lower_struct(
    s: &StructIr,
    scope: Option<&ScopeResolution>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<BlockArray> {
    let Some(size) = s.size.as_ref() else {
        diagnostics.push(diag_struct_no_size(s));
        return None;
    };

    let theme_missing = scope.is_none_or(|sc| sc.bound_theme.is_none());
    if theme_missing {
        diagnostics.push(diag_no_theme_bound(s));
    }

    let max_wall_height = max_wall_height(&s.members);
    let dims = Dims {
        x: size.w.get(),
        y: max_wall_height.saturating_add(1),
        z: size.h.get(),
    };
    let mut palette = Palette::new_with_air();
    let mut voxels = vec![PaletteIndex::AIR; dims.volume()];

    for member in &s.members {
        lower_member(
            member,
            scope,
            dims,
            &mut palette,
            &mut voxels,
            diagnostics,
            theme_missing,
        );
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

fn lower_member(
    member: &Member,
    scope: Option<&ScopeResolution>,
    dims: Dims,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
    theme_missing: bool,
) {
    match &member.role {
        MemberRole::Floor => {
            let Some(idx) = palette_index_for(member, scope, palette, diagnostics, theme_missing)
            else {
                return;
            };
            fill_floor(dims, idx, voxels);
        }
        MemberRole::Walls => {
            let Some(height) = wall_height(member, diagnostics) else {
                return;
            };
            let Some(idx) = palette_index_for(member, scope, palette, diagnostics, theme_missing)
            else {
                return;
            };
            fill_walls(dims, height, idx, voxels);
        }
        MemberRole::Door
        | MemberRole::Window
        | MemberRole::Roof
        | MemberRole::Stair
        | MemberRole::Level
        | MemberRole::PressurePlate
        | MemberRole::Circuit
        | MemberRole::Place
        | MemberRole::Connect
        | MemberRole::Other(_) => {
            diagnostics.push(diag_deferred_member(member));
        }
    }
}

/// Resolve a member's `mat_slot=` binding into a palette index.
///
/// Returns `None` (and writes nothing into the palette) when:
/// - the scope had no theme bound (`theme_missing` short-circuits silently;
///   the `W_NO_THEME_BOUND` warning was already emitted once per struct),
/// - the member never carried a `mat_slot=`,
/// - the resolver already flagged the slot via `E_UNRESOLVED_SLOT` (the
///   binding has `slot_value == None`),
/// - the value lowered as an abstract token (a `W_ABSTRACT_TOKEN_DEFERRED`
///   warning is emitted in that branch),
/// - the value was not a token at all (`E_UNKNOWN_SLOT_TARGET` already
///   fired during resolve, so no second diagnostic here).
fn palette_index_for(
    member: &Member,
    scope: Option<&ScopeResolution>,
    palette: &mut Palette,
    diagnostics: &mut Vec<Diagnostic>,
    theme_missing: bool,
) -> Option<PaletteIndex> {
    if theme_missing {
        return None;
    }
    let scope = scope?;
    let binding = scope.members.get(&member.span.start)?;
    let slot_value: &ValueWithSpan = binding.slot_value.as_ref()?;
    match resolve_block_state(slot_value) {
        Ok(state) => Some(palette.intern(state)),
        Err(MaterialDeferred::Abstract(token)) => {
            diagnostics.push(diag_abstract_token(member, &token, slot_value));
            None
        }
        Err(MaterialDeferred::AlreadyDiagnosed) => None,
    }
}

/// Largest `height=` value across every `walls` member in the struct. Walls
/// without an `height=` contribute `0` (they will not voxelise without a
/// height anyway). Returns a `u32`, capped at `u32::MAX` if a member declares
/// a value beyond that — practically unreachable but cheaper than a
/// `try_into` cascade.
fn max_wall_height(members: &[Member]) -> u32 {
    members
        .iter()
        .filter(|m| matches!(m.role, MemberRole::Walls))
        .filter_map(height_value)
        .max()
        .unwrap_or(0)
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

fn fill_floor(dims: Dims, idx: PaletteIndex, voxels: &mut [PaletteIndex]) {
    let y = 0;
    for z in 0..dims.z {
        for x in 0..dims.x {
            if let Some(i) = dims.index(x, y, z) {
                voxels[i] = idx;
            }
        }
    }
}

fn fill_walls(dims: Dims, height: u32, idx: PaletteIndex, voxels: &mut [PaletteIndex]) {
    // Cap the requested height at the volume's actual Y extent so a stray
    // out-of-range `height=` does not panic. `dims.y` is derived from the
    // module's own `max_wall_height + 1`, so under normal lowering this
    // never trims; defensive against a hand-built `BlockArray`.
    let top = height.min(dims.y.saturating_sub(1));
    for y in 1..=top {
        for z in 0..dims.z {
            for x in 0..dims.x {
                let on_edge = x == 0 || x + 1 == dims.x || z == 0 || z + 1 == dims.z;
                if on_edge && let Some(i) = dims.index(x, y, z) {
                    voxels[i] = idx;
                }
            }
        }
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
            message: "block-array lowering currently voxelises `floor` and `walls`; other \
                      roles will be added as their lowering rules are spec'd"
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
        lower_to_block_array(&ir, &resolution)
    }

    fn block_id(ba: &BlockArray, x: u32, y: u32, z: u32) -> &str {
        let i = ba.dims.index(x, y, z).expect("in-range coordinate");
        let pi = ba.voxels[i];
        ba.palette.entries[usize::from(pi.0)].id.as_str()
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
    fn deferred_role_warns_and_skips() {
        let src = "theme t:\n  slot f -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n  door side=front at=center\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(
            out.diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::DeferredMember)
                .count(),
            1,
        );
        // The door warning must not have stamped any voxel.
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
        let bs = resolve_block_state(&token).unwrap();
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
}
