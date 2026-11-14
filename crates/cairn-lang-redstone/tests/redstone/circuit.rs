//! `build_circuits`: every phase from `logic` to the blocks laid in the layers under each built
//! struct, which rises over them (`spec/redstone` "Connection to the IR and phases").

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{Bbox, BlockArrayIr, Dims, PaletteIndex, ResolvedMember};
use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{
    CircuitEntity, CircuitLayout, Diagnostic, build_circuits, compile_legalize,
};

use crate::common::{EXAMPLES, Named, built, delayed, findings, load_example};

/// `source` built for `edition` before and after its circuits are laid, with their findings.
fn build(source: &str, edition: Edition) -> (BlockArrayIr, BlockArrayIr, Vec<Diagnostic>) {
    let intent = lower(&parse(source).expect("parse"));
    let before = built(&intent, edition);
    let mut after = before.clone();
    let found = build_circuits(&intent, &mut after, edition, Some(&Named));
    (before, after, found)
}

/// `source`'s one circuit, legalized against [`Named`].
fn circuit(source: &str, edition: Edition) -> CircuitLayout {
    let out = compile_legalize(&delayed(source, edition), Some(&Named));
    out.scoped.scopes[0].ir.clone()
}

/// `members` with every run and every geometry `void` higher.
fn risen(members: &[ResolvedMember], void: u32) -> Vec<ResolvedMember> {
    let mut members = members.to_vec();
    for member in &mut members {
        let geometry = &mut member.geometry;
        for cell in [
            &mut geometry.anchor,
            &mut geometry.declared_bbox.min,
            &mut geometry.declared_bbox.max,
            &mut geometry.actual_bbox.min,
            &mut geometry.actual_bbox.max,
        ] {
            cell.1 += void;
        }
        for run in member
            .resolved_state
            .iter_mut()
            .flat_map(|block| &mut block.runs)
        {
            run.from.1 += void;
        }
    }
    members
}

/// Each comparator of `layout` at its voxel under the struct, with its output strength.
fn comparators(layout: &CircuitLayout) -> Vec<((u32, u32, u32), i32)> {
    let under = layout.region.under();
    let coord = |c: i32| u32::try_from(c).expect("inside the reservation");
    let mut found: Vec<_> = layout
        .blocks
        .iter()
        .filter_map(|block| {
            let at = (
                coord(block.at.x),
                coord(block.at.y) + under,
                coord(block.at.z),
            );
            let CircuitEntity::Comparator { output_signal } = block.entity?;
            Some((at, i32::from(output_signal)))
        })
        .collect();
    found.sort_unstable();
    found
}

#[test]
fn each_example_lays_its_circuit_under_its_struct_which_rises_whole() {
    for (name, structure) in EXAMPLES {
        let source = load_example(name);
        for edition in [Edition::Java, Edition::Bedrock] {
            let (before, after, found) = build(&source, edition);
            assert_eq!(found, [], "{name} on {edition:?}");
            let key = format!("struct::{structure}");
            let layout = circuit(&source, edition);
            let void = layout.region.void();
            let (was, now) = (&before.structures[&key], &after.structures[&key]);
            assert_eq!(
                now.dims,
                Dims {
                    y: was.dims.y + void,
                    ..was.dims
                },
                "{name}"
            );

            let split = (was.dims.x * was.dims.z * void) as usize;
            assert_eq!(&now.voxels[split..], was.voxels.as_slice(), "{name}");
            let laid = now.voxels[..split]
                .iter()
                .filter(|slot| **slot != PaletteIndex::AIR)
                .count();
            assert_eq!(laid, layout.blocks.len(), "{name}: one voxel per block");

            let members = &after.members[&key];
            assert_eq!(members[0].role, "circuit", "{name}");
            assert_eq!(
                &source[members[0].span.clone()],
                format!("circuit region=floor void={void}"),
            );
            let cells: usize = members[0]
                .resolved_state
                .iter()
                .map(|block| block.cells().count())
                .sum();
            assert_eq!(cells, laid, "{name}");
            assert_eq!(
                members[0].geometry.declared_bbox,
                Bbox::spanning((0, 0, 0), (now.dims.x - 1, void - 1, now.dims.z - 1)),
                "{name}: the circuit declares the layers it reserves",
            );
            assert_eq!(
                members[1..],
                risen(&before.members[&key], void)[..],
                "{name}"
            );

            let mut entities: Vec<((u32, u32, u32), i32)> = now
                .block_entities
                .iter()
                .map(|entity| {
                    assert_eq!(entity.id, "minecraft:comparator", "{name}");
                    (entity.pos, entity.fields["OutputSignal"])
                })
                .collect();
            entities.sort_unstable();
            assert_eq!(entities, comparators(&layout), "{name} on {edition:?}");
        }
    }
}

#[test]
fn a_reservation_deeper_than_the_circuit_leaves_the_layers_under_it_empty() {
    let source = load_example("redstone-door.crn").replace(
        "circuit region=floor void=3",
        "circuit region=floor void=40",
    );
    let (before, after, found) = build(&source, Edition::Java);
    assert_eq!(found, []);
    let (was, now) = (
        &before.structures["struct::gatehouse"],
        &after.structures["struct::gatehouse"],
    );
    assert_eq!(now.dims.y, was.dims.y + 40);
    let layer = (was.dims.x * was.dims.z) as usize;
    let filled: Vec<usize> = (0..40)
        .filter(|y| {
            now.voxels[y * layer..(y + 1) * layer]
                .iter()
                .any(|slot| *slot != PaletteIndex::AIR)
        })
        .collect();
    assert_eq!(
        filled,
        [35, 36, 37, 38, 39],
        "the circuit lies in the top five layers"
    );
}

#[test]
fn a_placed_def_lowers_its_origin_by_the_layers_under_it() {
    let source = "theme t:\n  slot wall -> @oak_planks\n\n\
                  def booth size=9x7:\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
                  door id=d side=front at=center\n  \
                  pressure_plate id=p at=inside.front offset=2 y=0 -> sig.a\n  \
                  logic sig.n = not sig.a\n  door[id=d] opened_by=sig.n\n  \
                  circuit region=floor void=3\n\n\
                  site row:\n  place id=one use=booth theme=t at=origin\n  \
                  place id=two use=booth theme=t east_of=one gap=3\n";
    let (before, after, found) = build(source, Edition::Java);
    assert_eq!(found, []);
    for key in ["site::row::one", "site::row::two"] {
        let (was, now) = (&before.placements[key], &after.placements[key]);
        assert_eq!(
            now.origin,
            (was.origin.0, was.origin.1 - 3, was.origin.2),
            "{key}"
        );
        assert_eq!(
            now.dims,
            Dims {
                y: was.dims.y + 3,
                ..was.dims
            },
            "{key}"
        );
        assert_eq!(now.dims, after.structures[key].dims, "{key}");
    }
}

#[test]
fn a_finding_in_any_phase_leaves_every_struct_as_it_was() {
    let source = "theme t:\n  slot wall -> @oak_planks\n\n\
                  struct room size=9x7\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
                  door id=d side=front at=center\n  \
                  lever id=l at=inside.back offset=2 y=2 -> sig.a\n  \
                  door[id=d] opened_by=sig.a\n  circuit region=floor void=3\n";
    for edition in [Edition::Java, Edition::Bedrock] {
        let (before, after, found) = build(source, edition);
        let codes: Vec<(&str, &str)> = findings(source, &found)
            .into_iter()
            .map(|(code, at, _, _)| (code, at))
            .collect();
        assert_eq!(
            codes,
            [(
                "E_ROUTE_UNREACHABLE_COMPONENT",
                "lever id=l at=inside.back offset=2 y=2 -> sig.a"
            )],
        );
        assert_eq!(after, before, "{edition:?}");
    }
}

/// A lever through a `not` to a door, `asserts` after the region.
fn inverted(asserts: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct room size=9x7\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         door id=d side=front at=center\n  \
         lever id=l at=inside.back offset=2 y=0 -> sig.a\n  \
         logic sig.n = not sig.a\n  \
         door[id=d] opened_by=sig.n\n  circuit region=floor void=3\n{asserts}"
    )
}

#[test]
fn a_truth_row_the_circuit_breaks_stops_the_build_and_leaves_the_struct_as_it_was() {
    let line = "assert truth(sig.a -> sig.n) { 0 -> 0 }";
    let source = inverted(&format!("  {line}\n"));
    for edition in [Edition::Java, Edition::Bedrock] {
        let (before, after, found) = build(&source, edition);
        assert_eq!(
            findings(&source, &found)
                .into_iter()
                .map(|(code, at, _, _)| (code, at))
                .collect::<Vec<_>>(),
            [("E_SIM_ASSERTION_FAILED", line)],
            "{edition:?}",
        );
        assert_eq!(after, before, "{edition:?}");
    }
}

#[test]
fn an_assertion_left_unchecked_is_reported_and_the_circuit_still_laid() {
    // `sig.d` is another name of `sig.a`, so no run rests with the two apart.
    let line = "assert truth(sig.a, sig.d -> sig.n) { 10 -> 0; 01 -> 1 }";
    let source = inverted(&format!("  logic sig.d = sig.a\n  {line}\n"));
    for edition in [Edition::Java, Edition::Bedrock] {
        let (before, after, found) = build(&source, edition);
        assert_eq!(
            findings(&source, &found)
                .into_iter()
                .map(|(code, at, _, _)| (code, at))
                .collect::<Vec<_>>(),
            [("W_SIM_ASSERTION_UNCHECKED", line)],
            "{edition:?}",
        );
        let key = "struct::room";
        assert_eq!(
            after.structures[key].dims,
            Dims {
                y: before.structures[key].dims.y + 3,
                ..before.structures[key].dims
            },
            "{edition:?}",
        );
    }
}

#[test]
fn a_truth_table_is_run_only_once_the_latency_bounds_hold() {
    let bound = "assert latency(sig.a -> sig.n) <= 0";
    let source = inverted(&format!(
        "  {bound}\n  assert truth(sig.a -> sig.n) {{ 0 -> 0 }}\n"
    ));
    for edition in [Edition::Java, Edition::Bedrock] {
        let (before, after, found) = build(&source, edition);
        assert_eq!(
            findings(&source, &found)
                .into_iter()
                .map(|(code, at, _, _)| (code, at))
                .collect::<Vec<_>>(),
            [("E_SIM_ASSERTION_FAILED", bound)],
            "{edition:?}",
        );
        assert_eq!(after, before, "{edition:?}");
    }
}

/// `alpha` lays its circuit whole; `beta`'s misses its bound only once legalized.
#[test]
fn a_finding_on_one_struct_leaves_every_struct_as_it_was() {
    let source = "theme t:\n  slot wall -> @oak_planks\n\n\
                  struct alpha size=9x7\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
                  door id=d side=front at=center\n  \
                  pressure_plate id=p at=inside.front offset=2 y=0 -> sig.a\n  \
                  logic sig.n = not sig.a\n  door[id=d] opened_by=sig.n\n  \
                  circuit region=floor void=3\n\n\
                  struct beta size=9x7\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
                  door id=d side=front at=center\n  \
                  pressure_plate id=p at=inside.front offset=2 y=0 -> sig.b\n  \
                  logic sig.m = not sig.b\n  door[id=d] opened_by=sig.m\n  \
                  circuit region=floor void=3\n  \
                  assert latency(sig.b -> sig.m) <= 0\n";
    for edition in [Edition::Java, Edition::Bedrock] {
        let (before, after, found) = build(source, edition);
        let codes: Vec<(&str, &str)> = findings(source, &found)
            .into_iter()
            .map(|(code, at, _, _)| (code, at))
            .collect();
        assert_eq!(
            codes,
            [(
                "E_SIM_ASSERTION_FAILED",
                "assert latency(sig.b -> sig.m) <= 0"
            )],
            "{edition:?}",
        );
        assert_eq!(after, before, "{edition:?}");
    }
}
