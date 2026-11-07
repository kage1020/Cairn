//! Painting a `roof` member.

use std::convert::Infallible;

use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::roof::{
    RoofKind, RoofVoxel, flat_block_state, flat_voxels, gable_ridge_axis, gable_stair_state,
    gable_voxels, hip_stair_state, hip_voxels, shed_stair_state, shed_voxels,
};
use crate::block_array::{BlockState, PaletteIndex};

use super::canvas::{MemberCanvas, ScopePalette};
use super::geometry_material::{Family, GeometryMemberDescription, geometry_material_id};
use super::members::{StructCtx, resolve_member_state};
use super::roof_kind::{parse_roof_kind, roof_draws, shed_slope_to};

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
    let Ok(base_id) = geometry_material_id(
        member,
        ctx.scope,
        resolved.as_ref(),
        || Ok::<_, Infallible>(kind.base_block_id().to_owned()),
        &GeometryMemberDescription {
            subject: format!("`{}` roof", kind.name()),
            states_from: "the geometry",
            requires: kind.paints_stairs().then_some(Family::Stair),
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
                &base_id,
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
                &base_id,
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
                &base_id,
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
            &base_id,
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
