use cairn_lang_core::Edition;
use cairn_lang_core::error::Span;
use cairn_lang_core::{lower, parse};

use super::verify_latency;
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{NetRef, NetlistInput};
use crate::placement_ir::{
    BufferCoord, BufferSegment, CellCoord, PlacedOutputNode, PlacementIr, PlacementPhase,
};
use crate::test_fixtures::{reservation, scoped, sig};

/// A pad reading `sig.a` straight from its sensor over `buffers` repeaters, its `opened_by=`
/// at `span`.
fn pad(z: u32, buffers: u32, span: Span) -> PlacedOutputNode {
    let mut output =
        PlacedOutputNode::new(sig("a"), NetRef::Input(0), CellCoord::new(8, 0, z), span);
    output.phase = PlacementPhase::Legalized {
        wire_length: 8,
        local_delay_ticks: 0,
        buffer_coords: (0..buffers)
            .map(|x| BufferCoord::new(BufferSegment::Out, CellCoord::new(x, 0, z)))
            .collect(),
    };
    output
}

#[test]
fn a_signal_two_actuators_read_is_bounded_by_the_pad_with_more_buffer_repeaters() {
    let source = "struct s size=9x7\n  assert latency(sig.a -> sig.a) <= 1\n";
    let module = lower(&parse(source).expect("parse"));
    let mut ir = PlacementIr::new(Edition::Java);
    ir.region = Some(reservation(9, 7, 2));
    ir.inputs.push(NetlistInput {
        name: sig("a"),
        span: Span::default(),
    });
    ir.outputs = vec![pad(0, 1, 1..2), pad(2, 3, 3..4), pad(4, 2, 5..6)];
    ir.signal_defs.insert(sig("a"), NetRef::Input(0));

    let found = verify_latency(&scoped(ScopeKind::Struct, "s", ir), &module);
    let notes: Vec<(Option<Span>, &str)> = found
        .iter()
        .flat_map(|d| &d.notes)
        .map(|n| (n.span.clone(), n.message.as_str()))
        .collect();
    assert_eq!(
        found.iter().map(|d| d.primary.as_str()).collect::<Vec<_>>(),
        [
            "struct=s: `assert latency(sig.a -> sig.a) <= 1` fails on java: its slowest path \
             from sig.a to sig.a takes 3 ticks, 3 in buffer repeaters on the routed wire"
        ],
    );
    assert_eq!(
        notes,
        [
            (
                Some(3..4),
                "the path ends at the pad of the actuator reading sig.a: 3 buffer repeaters on \
                 the wire to it"
            ),
            (
                None,
                "the path's wires carry 3 buffer repeaters, one wherever a wire runs past 15 \
                 blocks of dust, so a smaller `circuit` region, which puts the pads and the \
                 cells closer together, needs fewer"
            ),
            (
                None,
                "Fix: relax the bound to `<= 3`, or shorten the path; the logic is never \
                 rewritten to meet a bound"
            ),
        ],
    );
}
