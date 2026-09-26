//! Reading a dumped Placement IR back: every placed cell the pipeline
//! produces, at every stage `cairn synth --stage <s>` can stop at,
//! serialises to the flat wire form and deserialises back to the same
//! cell.
//!
//! Driven by real pipeline output rather than hand-built cells, because
//! the claim is about what a dump of a `.crn` a user can write reads
//! back as: both redstone examples, for both editions, at each of the
//! four stages, plus a shared bus that makes the crossing pass
//! materialise buffer coords — neither example has anything to
//! legalize, so without it the `buffer_coords` key would never be
//! read back from real output.
//!
//! The span is the one field the wire form does not carry, so the
//! comparison is against the pipeline's cells with their spans emptied,
//! and the re-serialised dump must match the original byte for byte.

use std::fmt::Write as _;

use cairn_lang_core::Edition;
use cairn_lang_redstone::{
    PlacedCellNode, PlacementStage, ScopedPlacementIr, compile_crossing, compile_delay,
};

mod common;

use common::{load_example, placement_from_source, routed_from_source};

/// The Placement IR of `source` after each of the four place-and-route
/// stages, paired with the stage tag its cells must carry.
fn every_stage(source: &str, edition: Edition) -> Vec<(PlacementStage, ScopedPlacementIr)> {
    let placed = placement_from_source(source, edition);
    let routed = routed_from_source(source, edition);
    let delayed = compile_delay(&routed);
    assert!(
        delayed.diagnostics.is_empty(),
        "fixture must delay cleanly: {:?}",
        delayed.diagnostics,
    );
    let legalized = compile_crossing(&delayed.scoped);
    assert!(
        legalized.diagnostics.is_empty(),
        "fixture must legalize cleanly: {:?}",
        legalized.diagnostics,
    );
    vec![
        (PlacementStage::Placement, placed),
        (PlacementStage::Route, routed),
        (PlacementStage::Delay, delayed.scoped),
        (PlacementStage::Crossing, legalized.scoped),
    ]
}

/// Dump every cell of `ir`, read the dump back, and require the cells
/// and the re-dumped JSON to equal the originals. Returns how many
/// cells were read back and how many of them carried buffer coords.
fn assert_cells_round_trip(
    label: &str,
    stage: PlacementStage,
    ir: &ScopedPlacementIr,
) -> (usize, usize) {
    let dump = serde_json::to_value(ir).expect("Placement IR serialises");
    let mut read = 0;
    let mut buffered = 0;
    for (scope, entry) in ir.scopes.iter().enumerate() {
        let cells_json = &dump[scope]["ir"]["cells"];
        let back: Vec<PlacedCellNode> =
            serde_json::from_value(cells_json.clone()).unwrap_or_else(|err| {
                panic!("{label}: scope `{}` does not read back: {err}", entry.name)
            });

        let expected: Vec<PlacedCellNode> = entry
            .ir
            .cells
            .iter()
            .cloned()
            .map(|mut cell| {
                cell.span = 0..0;
                cell
            })
            .collect();
        assert_eq!(
            back, expected,
            "{label}: scope `{}` read back differently",
            entry.name
        );
        assert!(
            back.iter().all(|cell| cell.stage() == stage),
            "{label}: every cell read back must carry the {} stage",
            stage.as_str(),
        );
        assert_eq!(
            &serde_json::to_value(&back).expect("read-back cells serialise"),
            cells_json,
            "{label}: scope `{}` re-dumps differently",
            entry.name,
        );
        read += back.len();
        buffered += back
            .iter()
            .filter(|c| !c.buffer_coords().is_empty())
            .count();
    }
    (read, buffered)
}

#[test]
fn every_example_cell_reads_back_equal_at_every_stage() {
    for example in ["redstone-door.crn", "crossbar.crn"] {
        let source = load_example(example);
        for edition in [Edition::Java, Edition::Bedrock] {
            for (stage, ir) in every_stage(&source, edition) {
                let label = format!("{example} {edition:?} --stage {}", stage.as_str());
                let (read, _) = assert_cells_round_trip(&label, stage, &ir);
                assert!(
                    read > 0,
                    "{label}: the example must place at least one cell"
                );
            }
        }
    }
}

/// The 16-cell bus from `tests/crossing.rs`: every cell reads `sig.b`
/// off one trunk, which is long enough to need repeaters, so the
/// crossing-stage dump carries `buffer_coords` and the read-back has
/// to fill [`PlacedCellNode::buffer_coords`] from it.
#[test]
fn a_legalized_dump_with_buffer_coords_reads_back_equal() {
    let mut source = String::from(
        r"
theme t:
  slot wall -> @oak_planks

struct chain size=60x5
  floor mat_slot=wall

  pressure_plate id=pa at=front.outside offset=0 y=0 -> sig.a
  pressure_plate id=pb at=inside.front  offset=0 y=0 -> sig.b

  logic sig.s0 = sig.a and sig.b
",
    );
    for i in 1..16 {
        writeln!(
            source,
            "  logic sig.s{i} = sig.s{prev} and sig.b",
            prev = i - 1
        )
        .expect("writing to a String cannot fail");
    }
    source.push_str(
        r"
  door id=d side=front at=center mat_slot=wall opened_by=sig.s15

  circuit region=floor void=2
",
    );

    for (stage, ir) in every_stage(&source, Edition::Java) {
        let label = format!("16-cell bus --stage {}", stage.as_str());
        let (read, buffered) = assert_cells_round_trip(&label, stage, &ir);
        assert_eq!(read, 16, "{label}: the fixture is the 16-cell chain");
        if stage == PlacementStage::Crossing {
            assert!(
                buffered >= 2,
                "{label}: the bus must put buffer coords in the dump for this to test \
                 reading them back: {buffered}",
            );
        }
    }
}
