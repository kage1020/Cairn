//! Stage 4 of `spec/redstone` "Place-and-route": every voxel of the reservation as a block, each
//! torch, repeater, comparator and dust in the state the circuit rests in with its sensors off.

use std::collections::BTreeSet;

use cairn_lang_core::block_array::{BlockState, TargetRegistry};

use crate::diagnostic::{Diagnostic, DiagnosticCode, compiler_bug, error_with_footer, overlapping};
use crate::geometry::{SIDES, Voxel};
use crate::grid::{Collision, Grid, Slot};
use crate::part::Part;
use crate::placement_ir::{
    CircuitBlock, CircuitEntity, CircuitLayout, LayoutStage, ScopedCircuitLayout,
};
use crate::sim::{Connection, Order, Sim};

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
/// the vanilla ones without it, leaving out one that never rests or whose tokens it lacks.
///
/// # Panics
///
/// On a circuit delay insertion has not produced.
#[must_use]
pub fn compile_legalize(
    delayed: &ScopedCircuitLayout,
    registry: Option<&dyn TargetRegistry>,
) -> LegalizeOutput {
    let mut out = LegalizeOutput::default();
    for entry in &delayed.scopes {
        let scope = format!("{}={}:", entry.kind.label(), entry.name);
        assert_eq!(
            entry.ir.stage,
            LayoutStage::Delayed,
            "{scope} legalization builds the circuit delay insertion produces",
        );
        match legalize(&scope, &entry.ir, registry) {
            Ok(layout) => out.scoped.push(entry.kind, entry.name.clone(), layout),
            Err(diagnostic) => out.diagnostics.push(diagnostic),
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
    sim.settle(sim.rest_ticks()).then_some(sim)
}

fn legalize(
    scope: &str,
    layout: &CircuitLayout,
    registry: Option<&dyn TargetRegistry>,
) -> Result<CircuitLayout, Diagnostic> {
    let span = || layout.region.span.clone();
    if let Err(Collision { at }) = Grid::of(layout) {
        return Err(overlapping(scope, span(), at));
    }
    let (Some(Built { parts, .. }), Some(sim)) = (built(layout), rest(layout)) else {
        return Err(compiler_bug(
            span(),
            format!(
                "{scope} the circuit under this struct does not come to rest with its sensors \
                 off, so it has no state to be built in"
            ),
        ));
    };
    let mut missing = BTreeSet::new();
    let mut blocks = Vec::with_capacity(parts.len());
    for (at, part) in parts {
        let Resting {
            token,
            vanilla,
            properties,
            entity,
        } = resting(part, at, &sim);
        let id = match registry.map(|r| r.lookup(token)) {
            None => vanilla.to_owned(),
            Some(Some(state)) => state.id,
            Some(None) => {
                missing.insert(token);
                vanilla.to_owned()
            }
        };
        let mut state = BlockState::bare(id);
        for (key, value) in properties {
            state.properties.insert(key.to_owned(), value);
        }
        state.properties.sort_keys();
        if let Some(id) = registry.and_then(|r| r.spelling(&state)) {
            state.id = id;
        }
        blocks.push(CircuitBlock { at, state, entity });
    }
    if !missing.is_empty() {
        let tokens: Vec<String> = missing.iter().map(|token| format!("`@{token}`")).collect();
        return Err(error_with_footer(
            DiagnosticCode::UnknownAbstractToken,
            span(),
            format!(
                "{scope} the circuit under this struct is built of {}, which the registry \
                 pack's materials catalog does not declare",
                tokens.join(", ")
            ),
            "Fix: declare each in the pack's `materials` catalog (see `spec/materials-themes` \
             \"Tier-1 tokens\")",
        ));
    }
    let mut layout = layout.clone();
    layout.blocks = blocks;
    layout.stage = LayoutStage::Legalized;
    Ok(layout)
}

/// A part as the circuit rests: its token, its vanilla id, its properties and its block entity.
struct Resting {
    token: &'static str,
    vanilla: &'static str,
    properties: Vec<(&'static str, String)>,
    entity: Option<CircuitEntity>,
}

/// `part` at `at` as `sim` rests.
fn resting(part: Part, at: Voxel, sim: &Sim) -> Resting {
    match part {
        Part::Block => Resting {
            token: "circuit.block",
            vanilla: "minecraft:stone",
            properties: Vec::new(),
            entity: None,
        },
        Part::Dust => {
            let mut properties: Vec<(&'static str, String)> = SIDES
                .into_iter()
                .zip(sim.sides(at))
                .map(|(face, side)| {
                    let value = match side {
                        Connection::None => "none",
                        Connection::Side => "side",
                        Connection::Up => "up",
                    };
                    (face.as_str(), value.to_owned())
                })
                .collect();
            properties.push(("power", sim.power(at).to_string()));
            Resting {
                token: "redstone_dust.default",
                vanilla: "minecraft:redstone_wire",
                properties,
                entity: None,
            }
        }
        Part::Torch { facing } => Resting {
            token: "redstone_torch.wall",
            vanilla: "minecraft:redstone_wall_torch",
            properties: vec![
                ("facing", facing.as_str().to_owned()),
                ("lit", sim.is_on(at).to_string()),
            ],
            entity: None,
        },
        Part::Repeater { facing, delay } => Resting {
            token: "repeater.default",
            vanilla: "minecraft:repeater",
            properties: vec![
                ("delay", delay.ticks().to_string()),
                ("facing", facing.opposite().as_str().to_owned()),
                ("locked", sim.is_locked(at).to_string()),
                ("powered", sim.is_on(at).to_string()),
            ],
            entity: None,
        },
        Part::Comparator { facing, subtract } => {
            let output = sim.output(at);
            Resting {
                token: "comparator.default",
                vanilla: "minecraft:comparator",
                properties: vec![
                    ("facing", facing.opposite().as_str().to_owned()),
                    (
                        "mode",
                        if subtract { "subtract" } else { "compare" }.to_owned(),
                    ),
                    ("powered", (output > 0).to_string()),
                ],
                entity: Some(CircuitEntity::Comparator {
                    output_signal: output,
                }),
            }
        }
    }
}
