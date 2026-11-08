//! Stage 4 of `spec/redstone` "Place-and-route": every voxel of the reservation as a block, each
//! torch, repeater, comparator and dust in the state the circuit rests in with its sensors off.

use cairn_lang_core::block_array::{BlockState, TargetRegistry};

use crate::diagnostic::{Diagnostic, DiagnosticCode, error_with_footer};
use crate::geometry::{SIDES, Voxel};
use crate::grid::{Grid, Slot};
use crate::part::Part;
use crate::placement_ir::{CircuitBlock, CircuitLayout, LayoutStage, ScopedCircuitLayout};
use crate::sim::{Connection, Order, Sim};

/// The ticks a circuit is given to come to rest before legalization gives up on it.
pub(crate) const REST_TICKS: u32 = 256;

/// Output of a [`compile_legalize`] run: the legalized circuits and the findings.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct LegalizeOutput {
    /// Every circuit with its blocks.
    pub scoped: ScopedCircuitLayout,
    /// Findings raised by the pass, in scope order.
    pub diagnostics: Vec<Diagnostic>,
}

/// Writes every block of every circuit of `delayed`, its ids read from `registry`'s tokens, or
/// the vanilla ones without it.
#[must_use]
pub fn compile_legalize(
    delayed: &ScopedCircuitLayout,
    registry: Option<&dyn TargetRegistry>,
) -> LegalizeOutput {
    let mut out = LegalizeOutput::default();
    for entry in &delayed.scopes {
        let scope = format!("{}={}:", entry.kind.label(), entry.name);
        match legalize(&entry.ir, registry) {
            Some(layout) => out.scoped.push(entry.kind, entry.name.clone(), layout),
            None => out.diagnostics.push(error_with_footer(
                DiagnosticCode::RouteCongestion,
                entry.ir.region.span.clone(),
                format!("{scope} the circuit under this struct does not come to rest with its sensors off, so it has no state to be built in"),
                "Fix: report this as a compiler bug, with the struct; meanwhile widen the struct's `size=WxH` so the circuit is laid differently",
            )),
        }
    }
    out
}

/// A circuit's parts as built, and the state each part starts from.
pub(crate) struct Built {
    pub(crate) parts: Vec<(Voxel, Part)>,
    pub(crate) seeds: Vec<(Voxel, bool)>,
}

/// Every part of `layout` as it is built, the blocks under base-plane cells included.
pub(crate) fn built(layout: &CircuitLayout) -> Option<Built> {
    let grid = Grid::of(layout).ok()?;
    let top = layout.region.top();
    let mut parts = Vec::new();
    for z in 0..i32::try_from(layout.region.depth).ok()? {
        for y in 0..=top {
            for x in 0..i32::try_from(layout.region.width).ok()? {
                let at = Voxel::new(x, y, z);
                let part = match grid.get(at)? {
                    Slot::Support => Some(Part::Block),
                    Slot::Part(part) => Some(part),
                    Slot::Dust(_) => Some(Part::Dust),
                    Slot::Open | Slot::NoDust
                        if y == top && layout.surface.column(x, z).is_some_and(|c| c.filled) =>
                    {
                        Some(Part::Block)
                    }
                    Slot::Open | Slot::NoDust | Slot::Air => None,
                };
                if let Some(part) = part {
                    parts.push((at, part));
                }
            }
        }
    }
    let seeds = layout
        .tiles
        .iter()
        .flat_map(|tile| {
            let template = tile.kind.template();
            template
                .seeds
                .iter()
                .map(|(rel, on)| (tile.at(*rel), *on))
                .collect::<Vec<_>>()
        })
        .collect();
    Some(Built { parts, seeds })
}

/// The circuit as it rests with nothing driving it.
pub(crate) fn rest(layout: &CircuitLayout) -> Option<Sim> {
    let Built { parts, seeds } = built(layout)?;
    let mut sim = Sim::new(parts, seeds, Order::Forward);
    sim.settle(REST_TICKS).then_some(sim)
}

fn legalize(
    layout: &CircuitLayout,
    registry: Option<&dyn TargetRegistry>,
) -> Option<CircuitLayout> {
    let Built { parts, .. } = built(layout)?;
    let sim = rest(layout)?;
    let lookup = |token: &str, base: &str| {
        registry.and_then(|r| r.lookup(token)).map_or_else(
            || BlockState::bare(base),
            |state| BlockState::bare(state.id),
        )
    };
    let mut blocks = Vec::with_capacity(parts.len());
    for (at, part) in parts {
        let (mut state, output) = match part {
            Part::Block => (lookup("circuit.block", "minecraft:stone"), None),
            Part::Dust => {
                let mut state = lookup("redstone_dust.default", "minecraft:redstone_wire");
                let sides = sim.sides(at);
                for (face, side) in SIDES.into_iter().zip(sides) {
                    let value = match side {
                        Connection::None => "none",
                        Connection::Side => "side",
                        Connection::Up => "up",
                    };
                    state
                        .properties
                        .insert(face.as_str().to_owned(), value.to_owned());
                }
                state
                    .properties
                    .insert("power".to_owned(), sim.power(at).to_string());
                (state, None)
            }
            Part::Torch { facing } => {
                let mut state = lookup("redstone_torch.wall", "minecraft:redstone_wall_torch");
                state
                    .properties
                    .insert("facing".to_owned(), facing.as_str().to_owned());
                state
                    .properties
                    .insert("lit".to_owned(), sim.is_on(at).to_string());
                (state, None)
            }
            Part::Repeater { facing, delay } => {
                let mut state = lookup("repeater.default", "minecraft:repeater");
                state
                    .properties
                    .insert("delay".to_owned(), delay.to_string());
                state
                    .properties
                    .insert("facing".to_owned(), facing.opposite().as_str().to_owned());
                state
                    .properties
                    .insert("locked".to_owned(), sim.is_locked(at).to_string());
                state
                    .properties
                    .insert("powered".to_owned(), sim.is_on(at).to_string());
                (state, None)
            }
            Part::Comparator { facing, subtract } => {
                let mut state = lookup("comparator.default", "minecraft:comparator");
                let output = sim.output(at);
                state
                    .properties
                    .insert("facing".to_owned(), facing.opposite().as_str().to_owned());
                state.properties.insert(
                    "mode".to_owned(),
                    if subtract { "subtract" } else { "compare" }.to_owned(),
                );
                state
                    .properties
                    .insert("powered".to_owned(), (output > 0).to_string());
                (state, Some(output))
            }
        };
        state.properties.sort_keys();
        if let Some(id) = registry.and_then(|r| r.spelling(&state)) {
            state.id = id;
        }
        blocks.push(CircuitBlock {
            at,
            state,
            output_signal: output,
        });
    }
    let mut layout = layout.clone();
    layout.blocks = blocks;
    layout.stage = LayoutStage::Legalized;
    Some(layout)
}
