//! Painting one member of the paint set with the generator its role names.

use crate::diagnostic::Diagnostic;
use crate::intent::{BindingKey, Member, MemberRole};

use super::canvas::{LazySlot, MemberCanvas, ScopePalette};
use super::cell_box::Shape;
use super::chest::fill_chest;
use super::component::{
    BUTTON, DAYLIGHT, DISPENSER, LAMP, LEVER, OBSERVER, PISTON, fill_component,
};
use super::door::{HungDoor, fill_door};
use super::fill::fill_material;
use super::frame::Frame;
use super::members::{StructCtx, bound_block, palette_index_for, resolve_member_state};
use super::part::{Part, fill_part};
use super::plate::fill_pressure_plate;
use super::raw::fill_raw;
use super::roof_fill::fill_roof;
use super::sizing::wall_height;
use super::stair::fill_stair;
use super::window::fill_window;

/// What the openings phase leaves to finish once every opening is cut.
#[derive(Default)]
pub(super) struct Openings {
    /// Each `door` that stands.
    pub(super) hung: Vec<HungDoor>,
    /// Each frame a theme binds on an opening that was cut, in paint-list order.
    pub(super) frames: Vec<Frame>,
}

/// Paint one member of the paint set with the generator its role names; a `door` that stands, and
/// the frame of an opening that was cut, are added to `openings`.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per member role, each calling that role's generator"
)]
pub(super) fn paint_member(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    openings: &mut Openings,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &member.role {
        // `flatten_members` drops a `floor` at a non-zero offset, so this is the ground plane.
        MemberRole::Floor => fill_floor(member, ctx, palette, canvas, diagnostics),
        MemberRole::Walls => fill_walls(member, y_offset, ctx, palette, canvas, diagnostics),
        MemberRole::Fill => fill_material(member, y_offset, ctx, palette, canvas, diagnostics),
        // `flatten_members` drops a `roof` at a non-zero offset, so this is the struct's plane.
        MemberRole::Roof => fill_roof(member, ctx, palette, canvas, diagnostics),
        MemberRole::Stair => fill_stair(member, y_offset, ctx, palette, canvas, diagnostics),
        MemberRole::Door => {
            let hung = &mut openings.hung;
            let frame = fill_door(member, y_offset, ctx, palette, canvas, hung, diagnostics);
            openings.frames.extend(frame);
        }
        MemberRole::Window => {
            let frame = fill_window(member, y_offset, ctx, palette, canvas, diagnostics);
            openings.frames.extend(frame);
        }
        MemberRole::PressurePlate => {
            fill_pressure_plate(member, y_offset, ctx, palette, canvas, diagnostics);
        }
        MemberRole::Lever => {
            fill_component(&LEVER, member, y_offset, ctx, palette, canvas, diagnostics);
        }
        MemberRole::Button => {
            fill_component(&BUTTON, member, y_offset, ctx, palette, canvas, diagnostics);
        }
        MemberRole::Daylight => {
            fill_component(
                &DAYLIGHT,
                member,
                y_offset,
                ctx,
                palette,
                canvas,
                diagnostics,
            );
        }
        MemberRole::Observer => {
            fill_component(
                &OBSERVER,
                member,
                y_offset,
                ctx,
                palette,
                canvas,
                diagnostics,
            );
        }
        MemberRole::Lamp => {
            fill_component(&LAMP, member, y_offset, ctx, palette, canvas, diagnostics);
        }
        MemberRole::Piston => {
            fill_component(&PISTON, member, y_offset, ctx, palette, canvas, diagnostics);
        }
        MemberRole::Dispenser => {
            fill_component(
                &DISPENSER,
                member,
                y_offset,
                ctx,
                palette,
                canvas,
                diagnostics,
            );
        }
        MemberRole::Chest => fill_chest(member, y_offset, ctx, palette, canvas, diagnostics),
        MemberRole::Repeater => {
            fill_part(
                Part::Repeater,
                member,
                y_offset,
                ctx,
                palette,
                canvas,
                diagnostics,
            );
        }
        MemberRole::Comparator => {
            fill_part(
                Part::Comparator,
                member,
                y_offset,
                ctx,
                palette,
                canvas,
                diagnostics,
            );
        }
        MemberRole::RedstoneTorch => {
            fill_part(
                Part::Torch,
                member,
                y_offset,
                ctx,
                palette,
                canvas,
                diagnostics,
            );
        }
        MemberRole::RedstoneDust => {
            fill_part(
                Part::Dust,
                member,
                y_offset,
                ctx,
                palette,
                canvas,
                diagnostics,
            );
        }
        MemberRole::RawBlock => {
            fill_raw(
                member,
                Shape::Cell,
                y_offset,
                ctx,
                palette,
                canvas,
                diagnostics,
            );
        }
        MemberRole::RawFill => {
            fill_raw(
                member,
                Shape::Box,
                y_offset,
                ctx,
                palette,
                canvas,
                diagnostics,
            );
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

fn fill_floor(
    member: &Member,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
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
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let trim = bound_block(member, BindingKey::Trim, ctx, diagnostics);
    let Some(height) = wall_height(member, diagnostics) else {
        return;
    };
    let Some(state) = resolve_member_state(
        member,
        ctx.scope,
        ctx.registry,
        diagnostics,
        ctx.theme_missing,
    )
    .bound() else {
        return;
    };
    let mut wall = LazySlot::new(state);
    let mut trim = trim.map(LazySlot::new);
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
                let corner = (x_local == 0 || x_local + 1 == ctx.interior_w)
                    && (z_local == 0 || z_local + 1 == ctx.interior_h);
                let slot = match &mut trim {
                    Some(trim) if corner => trim,
                    _ => &mut wall,
                };
                let x = ctx.overhang + x_local;
                let z = ctx.overhang + z_local;
                canvas.paint((x, y, z), || slot.intern(palette));
            }
        }
    }
}
