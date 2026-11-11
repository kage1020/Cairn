//! `compile_legalize`: every part of the reservation as the block it is built as, each torch,
//! repeater, comparator and dust in the state the circuit rests in with its sensors off
//! (`spec/redstone` "Place-and-route").

use std::collections::BTreeSet;
use std::fmt::Write as _;

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockIdSet, BlockState, TargetRegistry};
use cairn_lang_redstone::geometry::{Rel, Voxel};
use cairn_lang_redstone::{
    CircuitBlock, CircuitLayout, LayoutStage, ScopedCircuitLayout, TileKind, compile_legalize,
};

use crate::common::{
    COMPILER_BUG_FIX, EXAMPLES, LONG_WIRE_SOURCE, delayed, findings, load_example,
};

/// A target that spells every token `test:<token>`.
struct Spelled;

impl TargetRegistry for Spelled {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        Some(BlockState::bare(format!("test:{token}")))
    }

    fn known_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        None
    }
}

/// A target that declares the circuit's block and none of its redstone.
struct BlockOnly;

impl TargetRegistry for BlockOnly {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        (token == "circuit.block").then(|| BlockState::bare("test:circuit.block"))
    }

    fn known_tokens(&self) -> Vec<String> {
        vec!["circuit.block".to_owned()]
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        None
    }
}

fn legalized(
    source: &str,
    edition: Edition,
    registry: Option<&dyn TargetRegistry>,
) -> CircuitLayout {
    let out = compile_legalize(&delayed(source, edition), registry);
    assert_eq!(out.diagnostics, [], "{edition:?}");
    out.scoped.scopes[0].ir.clone()
}

/// `state` as `id[key=value,...]`.
fn spelled(state: &BlockState) -> String {
    let properties: Vec<String> = state
        .properties
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect();
    format!("{}[{}]", state.id, properties.join(","))
}

fn block_at(layout: &CircuitLayout, at: Voxel) -> &CircuitBlock {
    layout
        .blocks
        .iter()
        .find(|block| block.at == at)
        .unwrap_or_else(|| panic!("a block at {at:?}"))
}

#[test]
fn each_example_legalizes_to_one_block_per_part_inside_its_reservation() {
    for (name, _) in EXAMPLES {
        for edition in [Edition::Java, Edition::Bedrock] {
            let layout = legalized(&load_example(name), edition, None);
            assert_eq!(layout.stage(), LayoutStage::Legalized);
            let voxels: BTreeSet<_> = layout.blocks.iter().map(|block| block.at).collect();
            assert_eq!(
                voxels.len(),
                layout.blocks.len(),
                "{name}: one block a voxel"
            );
            assert!(
                layout
                    .blocks
                    .iter()
                    .all(|block| layout.region.contains(block.at)),
                "{name}: every block inside the reservation",
            );
            let ids: BTreeSet<&str> = layout.blocks.iter().map(|b| b.state.id.as_str()).collect();
            assert!(
                ids.is_subset(&BTreeSet::from([
                    "minecraft:comparator",
                    "minecraft:redstone_wall_torch",
                    "minecraft:redstone_wire",
                    "minecraft:repeater",
                    "minecraft:stone",
                ])),
                "{name}: {ids:?}",
            );
            for block in &layout.blocks {
                assert_eq!(
                    block.entity.is_some(),
                    block.state.id == "minecraft:comparator",
                    "{name}: {block:?}",
                );
            }
            let floor = (0..i32::try_from(layout.region.depth).expect("small"))
                .flat_map(|z| {
                    (0..i32::try_from(layout.region.width).expect("small")).map(move |x| (x, z))
                })
                .all(|(x, z)| block_at(&layout, Voxel::new(x, 0, z)).state.id == "minecraft:stone");
            assert!(floor, "{name}: the bottom layer is laid whole");
        }
    }
}

/// `layout`'s blocks drawn layer by layer from the top, a row of text per `z` along `x`: `#` a
/// circuit block, `-` dust and `=` powered dust, `*` a lit torch and `i` an unlit one, a repeater
/// as the arrow it drives along, `c` or `s` a comparator comparing or subtracting, `.` air.
fn drawn(layout: &CircuitLayout) -> String {
    let mut out = String::new();
    for y in (0..=layout.region.top()).rev() {
        writeln!(out, "y={y}").expect("a String takes every write");
        for z in 0..i32::try_from(layout.region.depth).expect("small") {
            for x in 0..i32::try_from(layout.region.width).expect("small") {
                let block = layout
                    .blocks
                    .iter()
                    .find(|block| block.at == Voxel::new(x, y, z));
                out.push(block.map_or('.', |block| {
                    let state = &block.state;
                    let property = |key: &str| state.properties.get(key).map(String::as_str);
                    match state.id.as_str() {
                        "minecraft:redstone_wire" if property("power") == Some("0") => '-',
                        "minecraft:redstone_wire" => '=',
                        "minecraft:redstone_wall_torch" if property("lit") == Some("true") => '*',
                        "minecraft:redstone_wall_torch" => 'i',
                        "minecraft:comparator" if property("mode") == Some("subtract") => 's',
                        "minecraft:comparator" => 'c',
                        "minecraft:repeater" => match property("facing") {
                            Some("east") => '<',
                            Some("west") => '>',
                            Some("north") => 'v',
                            _ => '^',
                        },
                        _ => '#',
                    }
                }));
            }
            out.push('\n');
        }
    }
    out
}

/// Every choice in the stages is ordered, so one source lays one circuit, the circuit block filling
/// the top layer under the floor wherever it powers nothing and cuts no climb.
#[test]
fn each_example_lays_the_circuit_it_always_lays() {
    for (name, _) in EXAMPLES {
        let stem = name.trim_end_matches(".crn").replace('-', "_");
        for edition in [Edition::Java, Edition::Bedrock] {
            let layout = legalized(&load_example(name), edition, None);
            insta::assert_snapshot!(
                format!("lays_{stem}_on_{}", edition.as_str()),
                drawn(&layout)
            );
        }
    }
}

#[test]
fn a_riser_rests_with_its_lower_torch_lit_and_the_one_under_the_actuator_out() {
    let layout = legalized(&load_example("redstone-door.crn"), Edition::Java, None);
    let riser = layout
        .tiles
        .iter()
        .find(|tile| tile.kind == TileKind::Riser)
        .expect("the door's riser");
    let facing = |rel| spelled(&block_at(&layout, riser.at(rel)).state);
    let parts: Vec<String> = [
        Rel::new(0, 0, 0),
        Rel::new(-1, 0, 0),
        Rel::new(-1, -1, 0),
        Rel::new(-2, -1, 0),
        Rel::new(-3, -1, 0),
    ]
    .into_iter()
    .map(facing)
    .collect();
    assert_eq!(
        parts,
        [
            "minecraft:redstone_wall_torch[facing=west,lit=false]",
            "minecraft:stone[]",
            "minecraft:redstone_wall_torch[facing=west,lit=true]",
            "minecraft:stone[]",
            "minecraft:redstone_wire[east=side,north=none,power=0,south=none,west=side]",
        ],
    );
}

/// The delay alone takes 264 ticks to light the riser.
#[test]
fn a_circuit_rests_with_its_sensors_off_however_long_its_path() {
    let source = "theme t:\n  slot wall -> @oak_planks\n\n\
                  struct slow size=272x6\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
                  door id=d side=front at=center\n  \
                  lever id=l at=inside.back offset=1 y=0 -> sig.a\n  \
                  logic sig.d = delay(not sig.a, 132)\n  \
                  door[id=d] opened_by=sig.d\n  \
                  circuit region=floor void=3\n";
    for edition in [Edition::Java, Edition::Bedrock] {
        let layout = legalized(source, edition, None);
        let riser = layout
            .tiles
            .iter()
            .find(|tile| tile.kind == TileKind::Riser)
            .expect("the door's riser");
        let torches: Vec<String> = [Rel::new(0, 0, 0), Rel::new(-1, -1, 0)]
            .into_iter()
            .map(|rel| spelled(&block_at(&layout, riser.at(rel)).state))
            .collect();
        assert_eq!(
            torches,
            [
                "minecraft:redstone_wall_torch[facing=south,lit=true]",
                "minecraft:redstone_wall_torch[facing=south,lit=false]",
            ],
            "{edition:?}",
        );
    }
}

#[test]
fn every_block_takes_its_id_from_the_target() {
    let layout = legalized(
        &load_example("control-room.crn"),
        Edition::Bedrock,
        Some(&Spelled),
    );
    let ids: BTreeSet<&str> = layout.blocks.iter().map(|b| b.state.id.as_str()).collect();
    assert_eq!(
        ids,
        BTreeSet::from([
            "test:circuit.block",
            "test:redstone_dust.default",
            "test:redstone_torch.wall",
            "test:repeater.default",
        ]),
    );
}

#[test]
fn a_target_lacking_a_circuit_token_refuses_the_circuit_naming_each_it_lacks() {
    let source = load_example("control-room.crn");
    let out = compile_legalize(&delayed(&source, Edition::Bedrock), Some(&BlockOnly));
    assert_eq!(
        findings(&source, &out.diagnostics),
        [(
            "E_UNKNOWN_ABSTRACT_TOKEN",
            "circuit region=floor void=4",
            "struct=control_room: the circuit under this struct is built of \
             `@redstone_dust.default`, `@redstone_torch.wall`, `@repeater.default`, which the \
             registry pack's materials catalog does not declare"
                .to_owned(),
            vec![(
                None,
                "Fix: declare each in the pack's `materials` catalog (see \
                 `spec/materials-themes` \"Tier-1 tokens\")"
                    .to_owned(),
            )],
        )],
    );
    assert_eq!(out.scoped, ScopedCircuitLayout::default());
}

#[test]
fn a_buffer_on_another_net_dust_is_refused_as_a_compiler_bug_naming_where() {
    let mut delayed = delayed(LONG_WIRE_SOURCE, Edition::Java);
    let layout = &mut delayed.scopes[0].ir;
    let at = layout.nets[1].wire.dust[0];
    layout.nets[0].wire.buffers[0].at = at;
    let out = compile_legalize(&delayed, None);
    assert_eq!(
        findings(LONG_WIRE_SOURCE, &out.diagnostics),
        [(
            "E_ROUTE_CONGESTION",
            "circuit region=floor void=4",
            format!(
                "struct=reach: the circuit under this struct has a part at ({}, {}, {}) where none \
                 fits",
                at.x, at.y, at.z,
            ),
            vec![(None, COMPILER_BUG_FIX.to_owned())],
        )],
    );
    assert_eq!(out.scoped, ScopedCircuitLayout::default());
}

#[test]
fn no_circuit_legalizes_to_nothing() {
    let out = compile_legalize(&ScopedCircuitLayout::default(), None);
    assert_eq!(out.diagnostics, []);
    assert_eq!(out.scoped, ScopedCircuitLayout::default());
}
