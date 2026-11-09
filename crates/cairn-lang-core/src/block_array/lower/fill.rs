//! Painting a `fill` member's box with a material, or pouring water into it in the `fluids` phase
//! (`spec/blockstate` "`waterlogged`").

use std::collections::HashMap;

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::args::{diag_invalid_member, ident_or_ignore};
use crate::block_array::material::TargetRegistry;
use crate::block_array::{BlockState, PaletteIndex};

use super::canvas::{Canvas, MemberCanvas, ScopePalette};
use super::cell_box::{CellBox, Shape, written_cells};
use super::members::{StructCtx, palette_index_for};
use super::waterlogged::WATERLOGGED;

/// The note on a fill refused for its `fluid=`, naming the one fluid there is.
const FLUIDS: &str = "`fluid=` takes `water`, the one fluid a fill pours";

/// Paint a `fill` that writes no `fluid=` with its `mat_slot=` material, in every cell of its box.
pub(super) fn fill_material(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    debug_assert!(
        !member.pours_fluid(),
        "a water fill pours in the fluids phase"
    );
    let cells = match written_cells(member, Shape::Box, y_offset, ctx) {
        Ok(cells) => cells,
        Err(refusal) => {
            diagnostics.push(*refusal);
            return;
        }
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
    for cell in cells.cells() {
        canvas.paint(cell, || idx);
    }
}

/// Pour each `fill` of the `fluids` phase into the cells its box holds on `canvas`.
pub(super) fn pour_fluids(
    bucket: Vec<(u32, u32, &Member)>,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut Canvas,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (index, y_offset, member) in bucket {
        if let Some(cells) = water_box(member, y_offset, ctx, diagnostics) {
            pour_water(index, cells, ctx.registry, palette, canvas);
        }
    }
}

/// The box a water fill pours into, or `None` after the finding that refuses it.
fn water_box(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<CellBox> {
    // Read before anything can refuse the fill, so an unreadable kind is reported now.
    let kind = ident_or_ignore(member, "kind", "`source`");
    let written_kind = kind.as_ref().ok().copied().flatten();
    let cells = match pour_refusal(member, written_kind) {
        Some(refusal) => Err(Box::new(refusal)),
        None => written_cells(member, Shape::Box, y_offset, ctx),
    };
    let cells = cells.map_err(|refusal| diagnostics.push(*refusal)).ok();
    if let Err(argument) = kind {
        diagnostics.push(argument.report(if cells.is_some() {
            "the fill pours source water, as `kind=source` does"
        } else {
            "this fill is not built either way — see the finding on the same line"
        }));
    }
    cells
}

/// The refusal of a water fill's `fluid=`, `kind=` or `mat_slot=`, or `None` when it pours.
fn pour_refusal(member: &Member, kind: Option<&str>) -> Option<Diagnostic> {
    let raw = member.written("fluid")?;
    let fluid = match &raw.value.kind {
        ValueKind::Ident(fluid) if fluid == "water" => None,
        ValueKind::Ident(fluid) => Some(format!("unknown fill `fluid={fluid}`")),
        _ => Some(format!(
            "fill `fluid=` must be an identifier, not {}",
            raw.value.describe(),
        )),
    };
    if let Some(primary) = fluid {
        return Some(diag_invalid_member(
            member,
            primary,
            vec![FLUIDS.to_owned()],
        ));
    }
    // A `mat_slot=` that failed to hoist is still written.
    if member.mat_slot.is_some() || member.arguments.contains_key("mat_slot") {
        return Some(diag_invalid_member(
            member,
            "fill writes both `mat_slot=` and `fluid=`, so nothing says whether it paints a \
             material or pours water"
                .to_owned(),
            vec![
                "keep `mat_slot=` to paint the box with a material, or `fluid=` to pour water \
                 into it"
                    .to_owned(),
            ],
        ));
    }
    match kind {
        None | Some("source") => None,
        Some(other) => Some(diag_invalid_member(
            member,
            format!("unknown fill `kind={other}`"),
            vec![
                "`kind=` takes `source`, a still water source in each cell the fill reaches; \
                 flowing water is not built yet"
                    .to_owned(),
            ],
        )),
    }
}

/// Pour water into `cells` for the member at `index` of the flattened paint list: a source into
/// each cell that holds no block, and `waterlogged=true` onto each block there that holds water and
/// whose state does not write it, repainted under the member that painted it.
fn pour_water(
    index: u32,
    cells: CellBox,
    registry: Option<&dyn TargetRegistry>,
    palette: &mut ScopePalette,
    canvas: &mut Canvas,
) {
    let mut waterlogs: HashMap<PaletteIndex, bool> = HashMap::new();
    for cell in cells.cells() {
        let Some(slot) = canvas.slot_at(cell) else {
            continue;
        };
        if slot == PaletteIndex::AIR {
            canvas
                .for_member(index)
                .paint(cell, || palette.intern(water_source()));
            continue;
        }
        let state = palette.state(usize::from(slot.0));
        let waterlogged = *waterlogs.entry(slot).or_insert_with(|| {
            !state.properties.contains_key(WATERLOGGED)
                && registry.and_then(|registry| registry.holds_water(state)) == Some(true)
        });
        if !waterlogged {
            continue;
        }
        let mut held = state.clone();
        held.properties
            .insert(WATERLOGGED.to_owned(), "true".to_owned());
        let Some(owner) = canvas.owner_at(cell) else {
            continue;
        };
        canvas
            .for_member(owner)
            .paint(cell, || palette.intern(held));
    }
}

/// A still water source, the block a water fill pours into a cell that holds none.
fn water_source() -> BlockState {
    let mut properties = IndexMap::new();
    properties.insert("level".to_owned(), "0".to_owned());
    BlockState {
        id: "minecraft:water".to_owned(),
        properties,
    }
}
