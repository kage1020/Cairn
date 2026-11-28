use cairn_lang_core::Edition;

use super::*;
use crate::fixtures::placed;
use crate::placement_ir::Wire;
use crate::routing::compile_routing;

/// A door opened off a plate across a 40-wide floor, on a wire that needs a repeater.
const LONG_WIRE: &str = "\
theme t:
  slot wall -> @oak_planks

struct pass size=40x6
  floor mat_slot=wall
  walls mat_slot=wall height=3
  door id=d side=front at=center
  pressure_plate id=p at=inside.front offset=1 y=0 -> sig.a
  door[id=d] opened_by=sig.a
  circuit region=floor void=3
";

fn routed(source: &str) -> ScopedCircuitLayout {
    let (placed, findings) = placed(source, Edition::Java);
    assert_eq!(findings, []);
    let out = compile_routing(&placed);
    assert_eq!(out.diagnostics, []);
    out.scoped
}

/// A second strand of dust beside the wire leaves no block of it joining only the two either side
/// of it, so no repeater fits anywhere along it.
#[test]
fn an_input_no_repeater_lifts_far_enough_is_refused_at_the_tile_that_reads_it() {
    let mut routed = routed(LONG_WIRE);
    let buffered = compile_delay(&routed);
    assert_eq!(buffered.diagnostics, []);
    assert_eq!(buffered.scoped.scopes[0].ir.nets[0].wire.buffers.len(), 1);
    let layout = &mut routed.scopes[0].ir;
    let grid = Grid::of(layout).expect("the routed circuit lays");
    let wire = &layout.nets[0].wire;
    let strand: Vec<Voxel> =
        wire.dust
            .iter()
            .filter_map(|at| {
                SIDES.into_iter().map(|face| at.step(face)).find(|beside| {
                    grid.get(*beside) == Some(Slot::Open) && !wire.air.contains(beside)
                })
            })
            .collect();
    layout.nets[0].wire.dust.extend(strand);
    let sink = layout.nets[0].sinks[0].tile;
    let span = layout.tiles[sink].span.clone();
    let out = compile_delay(&routed);
    assert_eq!(
        out.diagnostics,
        [error_with_footer(
            DiagnosticCode::AttenuationLimit,
            span,
            "struct=pass: the riser under this actuator reads `sig.a` at 0 over 22 blocks of dust, \
             more than the 15 a signal runs, and needs 2, and no stretch of that dust runs \
             straight and level where a buffer repeater fits"
                .to_owned(),
            "Fix: move what drives `sig.a` nearer, or line it up with this input so that its wire \
             runs straight and level for three blocks, where a buffer repeater fits",
        )]
    );
    assert_eq!(out.scoped, ScopedCircuitLayout::default());
}

#[test]
fn a_short_wire_into_a_port_that_needs_more_names_the_port_its_reading_and_its_need() {
    let source = "theme t:\n  slot wall -> @cobblestone\nstruct s size=16x12\n  \
                  floor mat_slot=wall\n  walls mat_slot=wall height=4\n  \
                  roof kind=flat mat_slot=wall overhang=1\n  \
                  lever id=in0 at=inside.front offset=1 y=0 -> sig.a\n  \
                  lever id=in1 at=inside.front offset=7 y=0 -> sig.b\n  \
                  lever id=in2 at=inside.left offset=5 y=0 -> sig.s\n  \
                  lamp id=out0 at=inside.back offset=1 y=1 lit_by=sig.o\n  \
                  logic sig.o = mux(sel=sig.s, a=sig.a, b=sig.b)\n  circuit region=floor void=4\n";
    let routed = routed(source);
    let span = routed.scopes[0].ir.tiles[0].span.clone();
    assert_eq!(
        compile_delay(&routed).diagnostics,
        [error_with_footer(
            DiagnosticCode::AttenuationLimit,
            span,
            "struct=s: the `sel` input of this `TorchMUX` cell reads `sig.s` at 8 over 7 blocks \
             of dust and needs 9, and no stretch of that dust runs straight and level where a \
             buffer repeater fits"
                .to_owned(),
            "Fix: move what drives `sig.s` nearer, or line it up with this input so that its wire \
             runs straight and level for three blocks, where a buffer repeater fits",
        )]
    );
}

#[test]
fn a_wire_laid_over_another_part_is_refused_as_a_compiler_bug_naming_where() {
    let mut routed = routed(LONG_WIRE);
    let layout = &mut routed.scopes[0].ir;
    let torch = layout
        .tiles
        .iter()
        .find(|tile| tile.kind == TileKind::Riser)
        .expect("the door's riser")
        .origin;
    layout.nets[0].wire.dust.push(torch);
    let span = layout.region.span.clone();
    let out = compile_delay(&routed);
    assert_eq!(
        out.diagnostics,
        [error_with_footer(
            DiagnosticCode::RouteCongestion,
            span,
            format!(
                "struct=pass: the circuit under this struct has a part at ({}, {}, {}) where none \
                 fits",
                torch.x, torch.y, torch.z,
            ),
            "Fix: report this as a compiler bug, with the struct; meanwhile widen the struct's \
             `size=WxH` so the circuit is laid differently",
        )]
    );
    assert_eq!(out.scoped, ScopedCircuitLayout::default());
}

/// A repeater of another net driving into the dust from the side would lock a buffer there, so the
/// buffer moves to the next stretch one fits.
#[test]
fn no_buffer_stands_where_a_diode_drives_into_the_dust_from_the_side() {
    let mut routed = routed(LONG_WIRE);
    let first = compile_delay(&routed).scoped.scopes[0].ir.nets[0]
        .wire
        .buffers[0];
    let layout = &mut routed.scopes[0].ir;
    let grid = Grid::of(layout).expect("the routed circuit lays");
    let (side, beside) = SIDES
        .into_iter()
        .filter(|face| *face != first.facing && *face != first.facing.opposite())
        .map(|face| (face, first.at.step(face)))
        .find(|(_, beside)| grid.get(*beside) == Some(Slot::Open))
        .expect("an open voxel beside the buffer");
    let mut stray = layout.nets[0].clone();
    stray.sinks.clear();
    stray.wire = Wire {
        dust: vec![beside],
        buffers: vec![Buffer {
            at: beside,
            facing: side.opposite(),
        }],
        ..Wire::default()
    };
    layout.nets.push(stray);
    let out = compile_delay(&routed);
    assert_eq!(out.diagnostics, []);
    let buffers = &out.scoped.scopes[0].ir.nets[0].wire.buffers;
    assert_eq!(buffers.len(), 1, "{buffers:?}");
    assert_ne!(buffers[0].at, first.at, "{buffers:?}");
}
