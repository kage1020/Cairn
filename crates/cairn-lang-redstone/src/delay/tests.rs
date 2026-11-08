use cairn_lang_core::Edition;

use super::*;
use crate::fixtures::placed;
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
            "struct=pass: the riser under this actuator reads its signal too weak: the dust runs past 15 blocks with no straight stretch where a buffer repeater can restore it".to_owned(),
            "Fix: widen the struct's `size=WxH` or increase `void`, so the wire has room to run straight, or move what feeds it nearer",
        )]
    );
    assert_eq!(out.scoped, ScopedCircuitLayout::default());
}
