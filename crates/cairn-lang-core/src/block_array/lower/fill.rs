//! Painting a `fill` member's box with a material, or pouring water into it once every other
//! member is painted (`spec/blockstate` "`waterlogged`").

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::args::{diag_deferred_member_noted, diag_invalid_member, ident_or_ignore};
use crate::block_array::material::TargetRegistry;
use crate::block_array::{BlockState, PaletteIndex};

use super::canvas::{Canvas, MemberCanvas, ScopePalette};
use super::members::{StructCtx, palette_index_for, unplaced_walls_note};
use super::waterlogged::WATERLOGGED;

/// A cell of the block array, `(x, y, z)`.
type Cell = (u32, u32, u32);

/// A corner as its line writes it, measured from the footprint's low corner.
type Corner = (u64, u64, u64);

/// The note on a corner that is missing or not a triple, saying what one is.
const CORNERS: &str = "write `from=` and `to=` as two opposite corners of the box, each a \
                       coordinate triple `x,y,z`, as in `from=1,1,1 to=3,2,3`";

/// The note on a fill refused for its `fluid=`, naming the one fluid there is.
const FLUIDS: &str = "`fluid=` takes `water`, the one fluid a fill pours";

/// What a deferral note says happens once the member it waits on builds.
const FILLED: &str = "the box is filled";

/// The cells of a fill's box in the block array, both corners included.
#[derive(Debug, Clone, Copy)]
struct CellBox {
    low: Cell,
    high: Cell,
}

impl CellBox {
    fn cells(self) -> impl Iterator<Item = Cell> {
        let Self { low, high } = self;
        (low.1..=high.1).flat_map(move |y| {
            (low.2..=high.2).flat_map(move |z| (low.0..=high.0).map(move |x| (x, y, z)))
        })
    }
}

/// Paint a `fill` that writes no `fluid=` with its `mat_slot=` material, in every cell of its box.
pub(super) fn fill_material(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let cells = match fill_box(member, y_offset, ctx) {
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
        None => fill_box(member, y_offset, ctx),
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
        let holds_water = registry.and_then(|registry| registry.holds_water(state));
        if state.properties.contains_key(WATERLOGGED) || holds_water != Some(true) {
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

/// The cells a fill's box holds in the block array, or the finding that leaves it unbuilt.
fn fill_box(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
) -> Result<CellBox, Box<Diagnostic>> {
    let from = corner(member, "from")?;
    let to = corner(member, "to")?;
    for (key, at) in [("from", from), ("to", to)] {
        if at.0 >= u64::from(ctx.interior_w) || at.2 >= u64::from(ctx.interior_h) {
            return Err(Box::new(outside_the_footprint(member, key, at, ctx)));
        }
    }
    let top = if to.1 >= from.1 {
        Top::of("to", to, y_offset)
    } else {
        Top::of("from", from, y_offset)
    };
    let written = &ctx.written;
    let above_written = top.row >= written.height;
    if above_written && !written.unplaced_walls && top.row >= written.reach {
        return Err(Box::new(above_the_struct(member, top, ctx)));
    }
    // From here the repair is on another member, so the fill only defers.
    let deferral = if above_written && written.unplaced_walls {
        Some(unplaced_walls_note("these rows", FILLED))
    } else if above_written {
        Some(format!(
            "a `roof` whose `kind=` or `slope_to=` does not read may rise to these rows; {FILLED} \
             once that roof draws"
        ))
    } else if top.row >= ctx.dims.y {
        Some(ctx.shortfall().note(FILLED))
    } else {
        None
    };
    if let Some(note) = deferral {
        return Err(Box::new(diag_deferred_member_noted(
            member,
            &past_the_struct(top, ctx),
            &note,
        )));
    }
    // The footprint and row checks above bound every component to the array.
    let cell = |(x, y, z): Corner| {
        let grid = |v: u64| u32::try_from(v).unwrap_or(u32::MAX);
        (
            grid(x).saturating_add(ctx.overhang),
            grid(y).saturating_add(y_offset),
            grid(z).saturating_add(ctx.overhang),
        )
    };
    let (a, b) = (cell(from), cell(to));
    Ok(CellBox {
        low: (a.0.min(b.0), a.1.min(b.1), a.2.min(b.2)),
        high: (a.0.max(b.0), a.1.max(b.1), a.2.max(b.2)),
    })
}

/// A fill's `key=` corner, or the refusal of a missing one or one that is not a triple.
fn corner(member: &Member, key: &'static str) -> Result<Corner, Box<Diagnostic>> {
    let refuse = |primary: String| {
        Box::new(diag_invalid_member(
            member,
            primary,
            vec![CORNERS.to_owned()],
        ))
    };
    let Some(raw) = member.arguments.get(key) else {
        return Err(refuse(format!(
            "fill has no `{key}=`, so nothing says which cells its box holds"
        )));
    };
    match raw.value.kind {
        ValueKind::Coord { x, y, z } => Ok((x, y, z)),
        _ => Err(refuse(format!(
            "fill `{key}=` must be a coordinate triple, not {}",
            raw.value.describe(),
        ))),
    }
}

/// The refusal of a corner outside the struct's footprint.
fn outside_the_footprint(
    member: &Member,
    key: &str,
    (x, y, z): Corner,
    ctx: &StructCtx<'_>,
) -> Diagnostic {
    let (width, depth) = (ctx.interior_w, ctx.interior_h);
    diag_invalid_member(
        member,
        format!(
            "fill `{key}={x},{y},{z}` lies outside the struct's `size={width}x{depth}` footprint"
        ),
        vec![format!(
            "a corner's `x` runs from 0 to {} and its `z` from 0 to {}",
            width.saturating_sub(1),
            depth.saturating_sub(1),
        )],
    )
}

/// The higher of a fill's two corners, and the struct row it reaches.
#[derive(Debug, Clone, Copy)]
struct Top {
    key: &'static str,
    corner: Corner,
    /// The `level y=` the fill is written under.
    level: u32,
    row: u32,
}

impl Top {
    fn of(key: &'static str, corner: Corner, level: u32) -> Self {
        let y = u32::try_from(corner.1).unwrap_or(u32::MAX);
        Self {
            key,
            corner,
            level,
            row: y.saturating_add(level),
        }
    }

    /// The corner as a message names it, with the level and struct row under a raised level.
    fn named(self) -> String {
        let (x, y, z) = self.corner;
        let written = format!("`{}={x},{y},{z}`", self.key);
        if self.level == 0 {
            written
        } else {
            format!(
                "{written} under `level y={}` (row {})",
                self.level, self.row
            )
        }
    }
}

/// The primary of a fill whose top row is at or above `dims.y`.
fn past_the_struct(top: Top, ctx: &StructCtx<'_>) -> String {
    format!(
        "fill {} does not fit in the struct (dims.y={})",
        top.named(),
        ctx.dims.y,
    )
}

/// The refusal of a fill reaching above every row the struct as written reaches.
fn above_the_struct(member: &Member, top: Top, ctx: &StructCtx<'_>) -> Diagnostic {
    let struct_top = ctx.written.height.saturating_sub(1);
    let level = top.level;
    let runs_to = |highest: u32| format!("a corner's `y` runs from 0 to {highest}");
    let mut notes = vec![if level == 0 {
        format!("{}, the struct's top row", runs_to(struct_top))
    } else if let Some(highest) = struct_top.checked_sub(level) {
        format!(
            "{} under `level y={level}`, the struct's top row being {struct_top}",
            runs_to(highest),
        )
    } else {
        format!("the struct's top row is {struct_top}, below this fill's `level y={level}`")
    }];
    notes.extend(ctx.unread_roof_note());
    let built = ctx.dims.y;
    if built <= struct_top {
        let rows = if built == struct_top {
            format!("row {built} waits")
        } else {
            format!("rows {built} to {struct_top} wait")
        };
        notes.push(format!("{rows} on {}", ctx.shortfall().members()));
    }
    notes.push(if level > struct_top {
        "lower the fill's `level`, or raise the `walls` to reach its rows".to_owned()
    } else {
        "lower the fill's box, or raise the `walls` to reach its rows".to_owned()
    });
    diag_invalid_member(member, past_the_struct(top, ctx), notes)
}
