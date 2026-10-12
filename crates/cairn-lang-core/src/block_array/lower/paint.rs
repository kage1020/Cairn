//! Painting one member of the paint set with the generator its role names.

use crate::diagnostic::Diagnostic;
use crate::intent::{Member, MemberRole};

use crate::block_array::PaletteIndex;

use super::canvas::{MemberCanvas, ScopePalette};
use super::door::carve_door;
use super::members::{StructCtx, palette_index_for};
use super::plate::fill_pressure_plate;
use super::roof_fill::fill_roof;
use super::sizing::wall_height;
use super::stair::fill_stair;
use super::window::fill_window;

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

pub(super) fn fill_floor(ctx: &StructCtx<'_>, idx: PaletteIndex, canvas: &mut MemberCanvas<'_>) {
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
