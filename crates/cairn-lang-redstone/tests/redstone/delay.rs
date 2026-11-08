//! `compile_delay`: a buffer repeater wherever a net's dust would run too weak for an input it
//! feeds, each in a straight stretch of that net's own dust (`spec/redstone` "Place-and-route").

use cairn_lang_core::Edition;
use cairn_lang_redstone::{CircuitLayout, LayoutStage, ScopedCircuitLayout, compile_delay};

use crate::common::{LONG_WIRE_SOURCE, load_example, routed};

/// Each net's buffer repeaters, by net.
fn buffers(layout: &CircuitLayout) -> Vec<usize> {
    layout
        .nets
        .iter()
        .map(|net| net.wire.buffers.len())
        .collect()
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
            assert_eq!(after.stage, LayoutStage::Delayed);
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
            unbuffered.stage = LayoutStage::Routed;
            for net in &mut unbuffered.nets {
                net.wire.buffers.clear();
            }
            assert_eq!(
                &unbuffered, before,
                "{edition:?}: buffers are all the pass adds"
            );
        }
    }
}

#[test]
fn no_circuit_delays_to_nothing() {
    let out = compile_delay(&ScopedCircuitLayout::default());
    assert_eq!(out.diagnostics, []);
    assert_eq!(out.scoped, ScopedCircuitLayout::default());
}
