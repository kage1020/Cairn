//! Painting a `roof` member, and the material rule it shares with the
//! eave `stair`.

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::intent::{Member, ValueWithSpan};
use crate::resolve::ScopeResolution;

use crate::block_array::args::{diag_deferred_member_reason, member_or_slot_span};
use crate::block_array::openings::WallSide;
use crate::block_array::roof::{
    RoofKind, RoofVoxel, flat_block_state, flat_voxels, gable_ridge_axis, gable_stair_state,
    gable_voxels, hip_stair_state, hip_voxels, is_stair, shed_stair_state, shed_voxels,
};
use crate::block_array::{BlockState, PaletteIndex};

use super::canvas::{MemberCanvas, ScopePalette};
use super::members::{StructCtx, resolve_member_state};
use super::sizing::roof_draws;

pub(super) fn fill_roof(
    member: &Member,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(kind) = parse_roof_kind(member, diagnostics) else {
        return;
    };
    // A shed with no usable `slope_to=` draws nothing, so its material is never resolved.
    if roof_draws(member).is_none() {
        shed_slope_to(member, diagnostics);
        return;
    }
    let resolved = resolve_member_state(
        member,
        ctx.scope,
        ctx.registry,
        diagnostics,
        ctx.theme_missing,
    );
    let base_id = geometry_material_id(
        member,
        ctx.scope,
        resolved.as_ref(),
        kind.base_block_id(),
        &GeometryMemberDescription {
            subject: format!("`{}` roof", kind.name()),
            states_from: "the geometry",
            requires_stair: kind.paints_stairs(),
        },
        diagnostics,
    );

    let (roof_w, roof_h, wall_top) = (ctx.dims.x, ctx.dims.z, ctx.wall_top);
    match kind {
        RoofKind::Gable => {
            let ridge_axis = gable_ridge_axis(roof_w, roof_h);
            paint_roof(
                gable_voxels(roof_w, roof_h, wall_top),
                |face| gable_stair_state(ridge_axis, face),
                base_id,
                palette,
                canvas,
            );
        }
        RoofKind::Shed => {
            let Some(slope_to) = shed_slope_to(member, diagnostics) else {
                return;
            };
            paint_roof(
                shed_voxels(roof_w, roof_h, wall_top, slope_to),
                |face| shed_stair_state(slope_to, face),
                base_id,
                palette,
                canvas,
            );
        }
        RoofKind::Hip => {
            // Hip takes gable's ridge rule (`spec/compilation` "Hip roof voxel rules").
            let ridge_axis = gable_ridge_axis(roof_w, roof_h);
            paint_roof(
                hip_voxels(roof_w, roof_h, wall_top),
                |face| hip_stair_state(ridge_axis, face),
                base_id,
                palette,
                canvas,
            );
        }
        // A deck is one material, so its voxels share the one face `()`.
        RoofKind::Flat => paint_roof(
            flat_voxels(roof_w, roof_h, wall_top)
                .into_iter()
                .map(|pos| RoofVoxel { pos, face: () }),
            |()| flat_block_state(),
            base_id,
            palette,
            canvas,
        ),
    }
}

/// Paint a roof's voxels, interning each face's state under `base_id` on first use only.
fn paint_roof<F: Copy + PartialEq>(
    voxels: impl IntoIterator<Item = RoofVoxel<F>>,
    state_of: impl Fn(F) -> BlockState,
    base_id: &str,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
) {
    let mut claimed: Vec<(F, PaletteIndex)> = Vec::new();
    for RoofVoxel { pos, face } in voxels {
        canvas.paint(pos, || {
            if let Some(&(_, idx)) = claimed.iter().find(|(seen, _)| *seen == face) {
                return idx;
            }
            let mut state = state_of(face);
            base_id.clone_into(&mut state.id);
            let idx = palette.intern(state);
            claimed.push((face, idx));
            idx
        });
    }
}

/// How a geometry-driven member names itself and its blockstates in a material finding.
pub(super) struct GeometryMemberDescription {
    /// How the member names itself in a message, already quoted: `` `gable` roof ``.
    pub(super) subject: String,
    /// Where its blockstates come from, as a noun phrase.
    pub(super) states_from: &'static str,
    /// Whether it attaches stair blockstates, and so needs a stair material.
    pub(super) requires_stair: bool,
}

/// The block id a roof or eave stair paints, given its resolved `mat_slot=` binding.
///
/// A non-stair where a stair is needed is `E_INCOMPATIBLE_MATERIAL`; `fallback` is still returned
/// so the pass can finish.
pub(super) fn geometry_material_id<'a>(
    member: &Member,
    scope: Option<&ScopeResolution>,
    resolved: Option<&'a BlockState>,
    fallback: &'a str,
    shape: &GeometryMemberDescription,
    diagnostics: &mut Vec<Diagnostic>,
) -> &'a str {
    let Some(state) = resolved else {
        if member.mat_slot.is_some() {
            // Names the theme, so two placements under two themes write two findings.
            let under = scope
                .and_then(|scope| scope.bound_theme.as_deref())
                .map_or_else(String::new, |theme| format!(" under theme `{theme}`"));
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "{}'s `mat_slot=` did not resolve to a block id{under}; it falls back to `{fallback}`",
                    shape.subject,
                ),
            ));
        }
        return fallback;
    };
    let slot_value = scope
        .and_then(|scope| scope.members.get(&member.span.start))
        .and_then(|binding| binding.slot_value.as_ref());
    if shape.requires_stair && !is_stair(&state.id) {
        diagnostics.push(diag_incompatible_material(member, state, shape, slot_value));
        return fallback;
    }
    if !state.properties.is_empty() {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "{} derives its blockstates from {}; the `mat_slot=` binding to `{}[...]` also carried properties and was not applied verbatim",
                shape.subject,
                shape.states_from,
                state.id,
            ),
        ));
    }
    &state.id
}

/// A member whose geometry attaches blockstates was bound to a material that cannot carry them.
///
/// Anchored on the theme's slot value when there is one, since that is where the fix goes.
fn diag_incompatible_material(
    member: &Member,
    state: &BlockState,
    shape: &GeometryMemberDescription,
    slot_value: Option<&ValueWithSpan>,
) -> Diagnostic {
    let mut notes = vec![DiagnosticNote {
        span: None,
        message: "bind the slot to a `*_stairs` material — the registry pack's \
                  `roof.dark_wood`, `roof.light_wood`, `roof.warm_wood`, and \
                  `roof.cool_wood` all resolve to one"
            .to_owned(),
    }];
    if let Some(slot) = &member.mat_slot {
        notes.push(DiagnosticNote {
            span: None,
            message: format!(
                "reached through `mat_slot={}`, so every member reading that slot has it too",
                slot.name,
            ),
        });
    }
    Diagnostic {
        code: DiagnosticCode::IncompatibleMaterial,
        span: slot_value.map_or_else(|| member.span.clone(), |v| member_or_slot_span(member, v)),
        primary: format!(
            "{} derives `facing` / `half` / `shape` from {} and `{}` is not a stair, so those states have nowhere to go",
            shape.subject, shape.states_from, state.id,
        ),
        notes,
        data: Some(DiagnosticData::IncompatibleMaterial {
            id: state.id.clone(),
            required: "stair".to_owned(),
            slot: member.mat_slot.as_ref().map(|slot| slot.name.clone()),
            token: slot_value.and_then(|v| match &v.value.kind {
                ValueKind::Token(token) => Some(token.clone()),
                // Anything else was refused upstream as `E_UNKNOWN_SLOT_TARGET`.
                _ => None,
            }),
        }),
    }
}

/// Resolve a `roof` member's `kind=`, or `None` after a `W_DEFERRED_MEMBER`.
fn parse_roof_kind(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<RoofKind> {
    let Some(raw) = member.ident_value("kind") else {
        // A wrongly shaped value is named by its shape: `kind="shed"` spells a kind in the set.
        let reason = match member.intent_state.get("kind") {
            Some(raw) => format!(
                "roof `kind=` must be an identifier naming one of {}, got {}",
                RoofKind::names(),
                raw.value.kind_name(),
            ),
            None => format!("missing `kind=` (expected one of {})", RoofKind::names()),
        };
        diagnostics.push(diag_deferred_member_reason(member, &reason));
        return None;
    };
    if let Some(k) = RoofKind::from_ident(raw) {
        return Some(k);
    }
    diagnostics.push(diag_deferred_member_reason(
        member,
        &format!(
            "unknown roof `kind={raw}` (expected one of {})",
            RoofKind::names(),
        ),
    ));
    None
}

/// Resolve a shed roof's required `slope_to=`, or `None` after a `W_DEFERRED_MEMBER`.
pub(super) fn shed_slope_to(
    member: &Member,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<WallSide> {
    let Some(raw) = member.ident_value("slope_to") else {
        let reason = match member.intent_state.get("slope_to") {
            Some(written) => format!(
                "shed `slope_to=` must be one of front, back, left, right, not {}",
                written.value.describe(),
            ),
            None => "shed roof requires `slope_to=` (one of front, back, left, right)".to_owned(),
        };
        diagnostics.push(diag_deferred_member_reason(member, &reason));
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
