//! `compile_delay`: a buffer repeater wherever a net's dust would run too weak for an input it
//! feeds, each in a straight stretch of that net's own dust (`spec/redstone` "Place-and-route").

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::Face;
use cairn_lang_redstone::geometry::Voxel;
use cairn_lang_redstone::{CircuitLayout, LayoutStage, ScopedCircuitLayout, compile_delay};

use crate::common::{EXAMPLES, LONG_WIRE_SOURCE, legalized, load_example, routed};

/// Each net's buffer repeaters, by net.
fn buffers(layout: &CircuitLayout) -> Vec<usize> {
    layout
        .nets
        .iter()
        .map(|net| net.wire.buffers.len())
        .collect()
}

/// `layout` as its JSON shows it, without its stage.
fn but_stage(layout: &CircuitLayout) -> serde_json::Value {
    let mut json = serde_json::to_value(layout).expect("a layout serializes");
    json.as_object_mut()
        .expect("a layout is an object")
        .remove("stage");
    json
}

#[test]
fn a_buffer_stands_in_its_own_net_dust_wherever_the_dust_would_run_too_weak() {
    let java_and_bedrock = |buffers: &'static [usize]| [buffers, buffers];
    let cases: [(String, [&[usize]; 2]); 4] = [
        (
            load_example("redstone-door.crn"),
            java_and_bedrock(&[0, 0, 0]),
        ),
        (
            load_example("control-room.crn"),
            [&[0, 1, 0, 2, 0, 1], &[0, 1, 0, 1, 1, 0]],
        ),
        (load_example("crossbar.crn"), [&[0, 0, 1, 0], &[0, 2, 0, 0]]),
        (LONG_WIRE_SOURCE.to_owned(), java_and_bedrock(&[1, 1, 0])),
    ];
    for (source, expected) in cases {
        for (edition, expected) in [Edition::Java, Edition::Bedrock].into_iter().zip(expected) {
            let routed = routed(&source, edition);
            let out = compile_delay(&routed);
            assert_eq!(out.diagnostics, [], "{edition:?}");
            let (before, after) = (&routed.scopes[0].ir, &out.scoped.scopes[0].ir);
            assert_eq!(after.stage(), LayoutStage::Delayed);
            assert_eq!(buffers(after), expected, "{edition:?}\n{source}");
            for (net, now) in after.nets.iter().enumerate() {
                for buffer in &now.wire.buffers {
                    assert!(
                        now.wire.dust.contains(&buffer.at),
                        "{edition:?} net {net}: {buffer:?} stands in the net's dust",
                    );
                }
            }
            let mut unbuffered = after.clone();
            for net in &mut unbuffered.nets {
                net.wire.buffers.clear();
            }
            assert_eq!(
                but_stage(&unbuffered),
                but_stage(before),
                "{edition:?}: buffers are all the pass adds"
            );
        }
    }
}

/// A buffer reads the block of dust behind it and drives the one ahead, so the dust runs straight
/// and level through the three, joins nothing beside it, and no diode drives into its side.
#[test]
fn each_buffer_stands_in_a_straight_level_stretch_nothing_joins_or_drives_from_the_side() {
    let sources = EXAMPLES
        .iter()
        .map(|(name, _)| load_example(name))
        .chain([LONG_WIRE_SOURCE.to_owned()]);
    let mut seen = 0;
    for source in sources {
        for edition in [Edition::Java, Edition::Bedrock] {
            let layout = &legalized(&source, edition).scopes[0].ir;
            let block = |at: Voxel| layout.blocks.iter().find(|block| block.at == at);
            let id = |at: Voxel| block(at).map(|block| block.state.id.as_str());
            for (net, circuit) in layout.nets.iter().enumerate() {
                for buffer in &circuit.wire.buffers {
                    seen += 1;
                    let (at, ahead) = (buffer.at, buffer.facing);
                    for end in [at.step(ahead), at.step(ahead.opposite())] {
                        assert_eq!(
                            id(end),
                            Some("minecraft:redstone_wire"),
                            "{edition:?} net {net}: {buffer:?} has dust at {end:?}\n{source}",
                        );
                    }
                    let sides = [Face::North, Face::South, Face::East, Face::West]
                        .into_iter()
                        .filter(|face| *face != ahead && *face != ahead.opposite());
                    for side in sides {
                        let beside = at.step(side);
                        for near in [beside, beside.up(), beside.down()] {
                            assert_ne!(
                                id(near),
                                Some("minecraft:redstone_wire"),
                                "{edition:?} net {net}: {buffer:?} joins dust at {near:?}\n{source}",
                            );
                        }
                        let drives_in = block(beside).is_some_and(|block| {
                            matches!(
                                block.state.id.as_str(),
                                "minecraft:repeater" | "minecraft:comparator"
                            ) && block.state.properties.get("facing").map(String::as_str)
                                == Some(side.as_str())
                        });
                        assert!(
                            !drives_in,
                            "{edition:?} net {net}: {buffer:?} driven from {beside:?}\n{source}",
                        );
                    }
                }
            }
        }
    }
    assert!(seen > 0, "the fixtures carry buffers");
}

#[test]
fn no_circuit_delays_to_nothing() {
    let out = compile_delay(&ScopedCircuitLayout::default());
    assert_eq!(out.diagnostics, []);
    assert_eq!(out.scoped, ScopedCircuitLayout::default());
}
