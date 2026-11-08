//! `compile_routing`: every net's dust from the tile that drives it to each input it feeds, apart
//! from every other net's, or `E_ROUTE_CONGESTION` naming the net with nowhere left to run
//! (`spec/redstone` "Place-and-route").

use cairn_lang_core::Edition;
use cairn_lang_redstone::{
    CircuitLayout, LayoutStage, PlacedTile, ScopedCircuitLayout, TileKind, compile_routing,
};

use crate::common::{EXAMPLES, findings, load_example, placed};

#[test]
fn each_example_routes_with_its_pads_and_risers_where_placement_laid_them() {
    for (name, _) in EXAMPLES {
        for edition in [Edition::Java, Edition::Bedrock] {
            let placed = placed(&load_example(name), edition);
            let out = compile_routing(&placed);
            assert_eq!(out.diagnostics, [], "{name} on {edition:?}");
            let (before, after) = (&placed.scopes[0].ir, &out.scoped.scopes[0].ir);
            assert_eq!(after.stage, LayoutStage::Routed);
            let fixed = |layout: &CircuitLayout| -> Vec<PlacedTile> {
                layout
                    .tiles
                    .iter()
                    .filter(|tile| !matches!(tile.kind, TileKind::Cell { .. }))
                    .cloned()
                    .collect()
            };
            assert_eq!(fixed(after), fixed(before), "{name} on {edition:?}");
            let kinds = |layout: &CircuitLayout| -> Vec<TileKind> {
                layout.tiles.iter().map(|tile| tile.kind).collect()
            };
            assert_eq!(kinds(after), kinds(before), "{name} on {edition:?}");
        }
    }
}

#[test]
fn a_signal_with_no_room_left_to_run_is_refused_on_its_driver() {
    let source = load_example("crossbar.crn")
        .replace("struct crossbar size=12x10", "struct crossbar size=8x6");
    let out = compile_routing(&placed(&source, Edition::Java));
    assert!(out.scoped.scopes.is_empty(), "a refused scope is left out");
    assert_eq!(
        findings(&source, &out.diagnostics),
        [(
            "E_ROUTE_CONGESTION",
            "pressure_plate id=plate1 at=front.outside offset=0 y=0 -> sig.a",
            "struct=crossbar: the sensor's signal finds no way to every input it feeds through the reservation (region 10x8, void=4) without running beside another signal's dust".to_owned(),
            vec![(
                None,
                "Fix: widen the struct's `size=WxH`, increase `void`, or split the logic across several scopes, each with its own `circuit` line".to_owned(),
            )],
        )],
    );
}

#[test]
fn no_circuit_routes_to_nothing() {
    let out = compile_routing(&ScopedCircuitLayout::default());
    assert_eq!(out.diagnostics, []);
    assert_eq!(out.scoped, ScopedCircuitLayout::default());
}
