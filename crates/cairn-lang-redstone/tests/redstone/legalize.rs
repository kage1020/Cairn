//! `compile_legalize`: every part of the reservation as the block it is built as, each torch,
//! repeater, comparator and dust in the state the circuit rests in with its sensors off
//! (`spec/redstone` "Place-and-route").

use std::collections::BTreeSet;

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockIdSet, BlockState, TargetRegistry};
use cairn_lang_redstone::geometry::{Rel, Voxel};
use cairn_lang_redstone::{
    CircuitBlock, CircuitLayout, LayoutStage, ScopedCircuitLayout, TileKind, compile_legalize,
};

use crate::common::{EXAMPLES, delayed, load_example};

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
            assert_eq!(layout.stage, LayoutStage::Legalized);
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
                    block.output_signal.is_some(),
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
fn no_circuit_legalizes_to_nothing() {
    let out = compile_legalize(&ScopedCircuitLayout::default(), None);
    assert_eq!(out.diagnostics, []);
    assert_eq!(out.scoped, ScopedCircuitLayout::default());
}
