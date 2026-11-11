//! `compile_routing`: every net's dust from the tile that drives it to each input it feeds, apart
//! from every other net's, or `E_ROUTE_CONGESTION` naming the net with nowhere left to run
//! (`spec/redstone` "Place-and-route").

use std::collections::BTreeSet;

use cairn_lang_core::block_array::{BlockArrayIr, Face, ResolvedBlock};
use cairn_lang_core::intent::{Actuator, SENSOR_HOSTS};
use cairn_lang_core::{Edition, lower, parse};
use cairn_lang_redstone::geometry::Voxel;
use cairn_lang_redstone::{
    CircuitLayout, LayoutStage, PlacedTile, ScopedCircuitLayout, TileKind, compile_routing,
};

use crate::common::{EXAMPLES, LONG_WIRE_SOURCE, built, findings, legalized, load_example, placed};

/// A lever and a plate through an `or` to a door down a 40-wide floor, with a raw block on the
/// floor row beside where the wire would run straight.
const FLOOR_ROW: &str = "\
theme t:
  slot wall -> @oak_planks

struct reach size=40x6
  floor mat_slot=wall
  walls mat_slot=wall height=3
  door id=front side=front at=center
  lever id=l at=inside.back offset=1 y=0 -> sig.a
  pressure_plate id=p at=inside.front offset=1 y=0 -> sig.b
  raw_block mat=minecraft:stone at=30,0,2
  logic sig.c = sig.a or sig.b
  door[id=front] opened_by=sig.c
  circuit region=floor void=4
";

/// Every source routed by the tests that walk each circuit's dust.
fn routed_sources() -> Vec<String> {
    EXAMPLES
        .iter()
        .map(|(name, _)| load_example(name))
        .chain([LONG_WIRE_SOURCE.to_owned(), FLOOR_ROW.to_owned()])
        .collect()
}

/// Cells of a struct, `(x, y, z)` from its origin.
type Cells = BTreeSet<(i32, i32, i32)>;

/// The floor-row cells of `key` a component holds, and those of a sensor, a Tier 0 part or a raw
/// member, which power what is beside them, the row over the floor row's included.
fn components(built: &BlockArrayIr, key: &str) -> (Cells, Cells) {
    let unknown = [
        "repeater",
        "comparator",
        "redstone_torch",
        "redstone_dust",
        "raw_block",
        "raw_fill",
    ];
    let mut held = BTreeSet::new();
    let mut signalling = BTreeSet::new();
    for member in &built.members[key] {
        let role = member.role.as_str();
        let signals = SENSOR_HOSTS.contains(&role) || unknown.contains(&role);
        if !signals
            && !Actuator::ALL
                .iter()
                .any(|actuator| actuator.keyword() == role)
        {
            continue;
        }
        for (x, y, z) in member.resolved_state.iter().flat_map(ResolvedBlock::cells) {
            let cell = (
                i32::try_from(x).expect("small"),
                i32::try_from(y).expect("small"),
                i32::try_from(z).expect("small"),
            );
            if y == 0 {
                held.insert(cell);
            }
            if signals && y <= 1 {
                signalling.insert(cell);
            }
        }
    }
    (held, signalling)
}

#[test]
fn each_example_routes_with_its_pads_and_risers_where_placement_laid_them() {
    for (name, _) in EXAMPLES {
        for edition in [Edition::Java, Edition::Bedrock] {
            let placed = placed(&load_example(name), edition);
            let out = compile_routing(&placed);
            assert_eq!(out.diagnostics, [], "{name} on {edition:?}");
            let (before, after) = (&placed.scopes[0].ir, &out.scoped.scopes[0].ir);
            assert_eq!(after.stage(), LayoutStage::Routed);
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

/// The top layer stays clear of what the struct powers, but for a pad's own dust under a cell
/// only its sensor powers.
#[test]
fn dust_runs_in_the_top_layer_only_under_a_floor_cell_nothing_powers_but_its_own_sensor() {
    for source in routed_sources() {
        for edition in [Edition::Java, Edition::Bedrock] {
            let intent = lower(&parse(&source).expect("parse"));
            let out = compile_routing(&placed(&source, edition));
            assert_eq!(out.diagnostics, [], "{edition:?}\n{source}");
            let layout = &out.scoped.scopes[0].ir;
            let (held, signalling) = components(&built(&intent, edition), &layout.structure);
            let top = layout.region.top();
            for (net, circuit) in layout.nets.iter().enumerate() {
                let driver = &layout.tiles[circuit.driver];
                let own = matches!(driver.kind, TileKind::Pad { .. })
                    .then_some((driver.origin.x, driver.origin.z));
                for at in circuit.wire.dust.iter().filter(|at| at.y == top) {
                    let (x, z) = (at.x, at.z);
                    assert!(
                        !held.contains(&(x, 0, z)),
                        "{edition:?} net {net}: dust at {at:?} under a component\n{source}",
                    );
                    let heard: BTreeSet<(i32, i32)> = [
                        (x - 1, 0, z),
                        (x + 1, 0, z),
                        (x, 0, z - 1),
                        (x, 0, z + 1),
                        (x, 1, z),
                    ]
                    .into_iter()
                    .filter(|cell| signalling.contains(cell))
                    .map(|(x, _, z)| (x, z))
                    .collect();
                    assert!(
                        heard.is_empty() || own.is_some_and(|own| heard == BTreeSet::from([own])),
                        "{edition:?} net {net}: dust at {at:?} under a cell {heard:?} powers\n{source}",
                    );
                }
            }
        }
    }
}

/// Dust beside dust joins it, along a layer or up and down a step that no block over the lower
/// dust cuts.
#[test]
fn no_net_dust_joins_another_net_dust() {
    let faces = [Face::North, Face::South, Face::East, Face::West];
    for source in routed_sources() {
        for edition in [Edition::Java, Edition::Bedrock] {
            let layout = &legalized(&source, edition).scopes[0].ir;
            let id = |at: Voxel| {
                layout
                    .blocks
                    .iter()
                    .find(|block| block.at == at)
                    .map(|block| block.state.id.as_str())
            };
            let owner = |at: Voxel| {
                (id(at) == Some("minecraft:redstone_wire"))
                    .then(|| {
                        layout
                            .nets
                            .iter()
                            .position(|circuit| circuit.wire.dust.contains(&at))
                    })
                    .flatten()
            };
            let cut = |lower: Voxel| id(lower.up()) == Some("minecraft:stone");
            for (net, circuit) in layout.nets.iter().enumerate() {
                for &at in circuit
                    .wire
                    .dust
                    .iter()
                    .filter(|at| owner(**at) == Some(net))
                {
                    for beside in faces.into_iter().map(|face| at.step(face)) {
                        let joined = [
                            Some(beside),
                            (!cut(at)).then(|| beside.up()),
                            (!cut(beside.down())).then(|| beside.down()),
                        ];
                        for near in joined.into_iter().flatten() {
                            let other = owner(near);
                            assert!(
                                other.is_none_or(|other| other == net),
                                "{edition:?}: net {net}'s dust at {at:?} joins net {other:?}'s at \
                                 {near:?}\n{source}",
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn no_circuit_routes_to_nothing() {
    let out = compile_routing(&ScopedCircuitLayout::default());
    assert_eq!(out.diagnostics, []);
    assert_eq!(out.scoped, ScopedCircuitLayout::default());
}
