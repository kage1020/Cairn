//! Sources compiled as far as a unit test needs them.

use std::fmt::Write as _;

use cairn_lang_core::block_array::{
    BlockArrayIr, BlockIdSet, BlockState, TargetRegistry, lower_to_block_array,
};
use cairn_lang_core::check::Severity;
use cairn_lang_core::{Edition, IntentModule, lower, parse, resolve};

use crate::delay::compile_delay;
use crate::diagnostic::Diagnostic;
use crate::edition_netlist::compile_edition_netlist;
use crate::edition_netlist_ir::ScopedEditionNetlistIr;
use crate::legalize::{compile_legalize, rest};
use crate::netlist::compile_netlist;
use crate::netlist_ir::NetRef;
use crate::placement::compile_placement;
use crate::placement_ir::{CircuitLayout, ScopedCircuitLayout, TileKind};
use crate::routing::compile_routing;
use crate::synth::synthesize;

/// `source` parsed and lowered to the Intent IR.
pub(crate) fn module(source: &str) -> IntentModule {
    lower(&parse(source).expect("the fixture parses"))
}

/// A target that spells every token as the block of its name, so every member stands as written.
pub(crate) struct Named;

impl TargetRegistry for Named {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        let name = token.split('.').next()?;
        Some(BlockState::bare(format!("minecraft:{name}")))
    }

    fn known_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        None
    }
}

/// `module` built for `edition` against [`Named`].
pub(crate) fn built(module: &IntentModule, edition: Edition) -> BlockArrayIr {
    lower_to_block_array(module, &resolve(module, Some(edition)), Some(&Named))
}

/// `module`'s logic as `edition`'s cells, failing on a synth error.
pub(crate) fn netlist(module: &IntentModule, edition: Edition) -> ScopedEditionNetlistIr {
    let synth = synthesize(module);
    assert_clean("synth", &synth.diagnostics);
    compile_edition_netlist(&compile_netlist(&synth.scoped), edition)
}

/// The placement pass's circuits and findings for `source`.
pub(crate) fn placed(source: &str, edition: Edition) -> (ScopedCircuitLayout, Vec<Diagnostic>) {
    let module = module(source);
    let out = compile_placement(
        &netlist(&module, edition),
        &module,
        &built(&module, edition),
    );
    (out.scoped, out.diagnostics)
}

/// `source`'s circuits through every pass, failing on any finding.
pub(crate) fn legalized(source: &str, edition: Edition) -> ScopedCircuitLayout {
    let (placed, findings) = placed(source, edition);
    assert_clean("placement", &findings);
    let routed = compile_routing(&placed);
    assert_clean("routing", &routed.diagnostics);
    let delayed = compile_delay(&routed.scoped);
    assert_clean("delay", &delayed.diagnostics);
    let legalized = compile_legalize(&delayed.scoped, None);
    assert_clean("legalize", &legalized.diagnostics);
    legalized.scoped
}

/// Each riser's signal and whether its torch is lit, once `layout` settles from rest with the pads
/// of the signals in `on` driven and every other pad off.
pub(crate) fn drive(layout: &CircuitLayout, on: &[&str]) -> Vec<(String, bool)> {
    let named = |net: NetRef| {
        layout
            .signal_defs
            .iter()
            .find_map(|(name, defines)| (*defines == net).then(|| name.to_string()))
            .unwrap_or_else(|| panic!("a signal defined by {net:?}"))
    };
    let mut sim = rest(layout).expect("the fixture's circuit comes to rest");
    for tile in &layout.tiles {
        let (TileKind::Pad { .. }, Some(net)) = (tile.kind, tile.output) else {
            continue;
        };
        if on.contains(&named(layout.nets[net].source).as_str()) {
            sim.set_input(tile.origin.up(), 15);
        }
    }
    assert!(sim.settle(256), "the fixture's circuit settles");
    layout
        .tiles
        .iter()
        .filter(|tile| tile.kind == TileKind::Riser)
        .map(|tile| {
            let net = tile.inputs[0].net;
            (named(layout.nets[net].source), sim.is_on(tile.origin))
        })
        .collect()
}

fn assert_clean(pass: &str, diagnostics: &[Diagnostic]) {
    assert!(
        diagnostics.iter().all(|d| d.severity() != Severity::Error),
        "the fixture must clear {pass}: {diagnostics:#?}",
    );
}

/// `layout` drawn layer by layer from the top, each layer a row of text per `z` running along `x`:
/// `#` a block, `-` dust, a repeater as the arrow it drives along, `c`/`s` a comparator comparing
/// or subtracting, `*` a torch, `o` a voxel kept empty, `.` an open one.
pub(crate) fn render(layout: &CircuitLayout) -> String {
    use cairn_lang_core::block_array::Face;

    use crate::grid::{Grid, Slot};
    use crate::part::Part;

    let grid = Grid::of(layout).expect("the fixture's layout lays");
    let arrow = |face: Face| match face {
        Face::East => '>',
        Face::West => '<',
        Face::South => 'v',
        _ => '^',
    };
    let mut out = String::new();
    for y in (0..=layout.region.top()).rev() {
        writeln!(out, "y={y}").expect("a String takes every write");
        for z in 0..i32::try_from(layout.region.depth).expect("a small fixture") {
            for x in 0..i32::try_from(layout.region.width).expect("a small fixture") {
                let at = crate::geometry::Voxel::new(x, y, z);
                out.push(match grid.get(at).expect("inside the reservation") {
                    Slot::Open | Slot::NoDust => '.',
                    Slot::Air => 'o',
                    Slot::Support | Slot::Part(Part::Block) => '#',
                    Slot::Dust(_) | Slot::Part(Part::Dust) => '-',
                    Slot::Part(Part::Torch { .. }) => '*',
                    Slot::Part(Part::Repeater { facing, .. }) => arrow(facing),
                    Slot::Part(Part::Comparator { subtract, .. }) => {
                        if subtract {
                            's'
                        } else {
                            'c'
                        }
                    }
                });
            }
            out.push('\n');
        }
    }
    out
}
