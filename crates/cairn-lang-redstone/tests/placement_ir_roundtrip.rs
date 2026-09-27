//! Reading a dumped Placement IR back: every placed cell the pipeline
//! produces, at each of the four place-and-route stages
//! `cairn synth --stage <s>` can stop at, serialises to the flat wire
//! form and deserialises back to the same cell.
//!
//! Driven by real pipeline output rather than hand-built cells, because
//! the claim is about what a dump of a `.crn` a user can write reads
//! back as: both redstone examples, for both editions, at each of those
//! four stages, plus a shared bus that makes the crossing pass
//! materialise buffer coords — neither example has anything to
//! legalize, which the example test checks rather than assumes, so
//! without the bus the `buffer_coords` key would never be read back
//! from real output.
//!
//! The span is the one field the wire form does not carry, so the
//! comparison is against the pipeline's cells with their spans emptied,
//! and the read-back cells must re-serialise to the same JSON text as
//! the pipeline's own, byte for byte.

use cairn_lang_core::Edition;
use cairn_lang_redstone::{PlacedCellNode, PlacementStage, ScopedPlacementIr};

mod common;

use common::{
    delayed_from_source, legalized_from_source, load_example, placement_from_source,
    routed_from_source, shared_bus_source,
};

/// The Placement IR of `source` after each of the four place-and-route
/// stages, paired with the stage tag its cells must carry. Each is one
/// rung of the fixture ladder in `common`, so each stage is held to what
/// "ran clean" means there.
fn every_stage(source: &str, edition: Edition) -> [(PlacementStage, ScopedPlacementIr); 4] {
    [
        (
            PlacementStage::Placement,
            placement_from_source(source, edition),
        ),
        (PlacementStage::Route, routed_from_source(source, edition)),
        (PlacementStage::Delay, delayed_from_source(source, edition)),
        (
            PlacementStage::Crossing,
            legalized_from_source(source, edition),
        ),
    ]
}

/// Dump every cell of `ir`, read the dump back, and require the cells
/// to equal the originals and to re-dump to the same JSON text. Returns
/// how many cells were read back and how many of them carried buffer
/// coords.
fn assert_cells_round_trip(
    label: &str,
    stage: PlacementStage,
    ir: &ScopedPlacementIr,
) -> (usize, usize) {
    let dump = serde_json::to_value(ir).expect("Placement IR serialises");
    let mut read = 0;
    let mut buffered = 0;
    for (scope, entry) in ir.scopes.iter().enumerate() {
        // `cells` is left out of a scope that placed none, so an absent
        // key is an empty scope rather than a value that fails to read.
        let back: Vec<PlacedCellNode> = match dump[scope]["ir"].get("cells") {
            None => Vec::new(),
            Some(cells_json) => serde_json::from_value(cells_json.clone()).unwrap_or_else(|err| {
                panic!("{label}: scope `{}` does not read back: {err}", entry.name)
            }),
        };

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
            serde_json::to_string(&back).expect("read-back cells serialise"),
            serde_json::to_string(&entry.ir.cells).expect("pipeline cells serialise"),
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
                let (read, buffered) = assert_cells_round_trip(&label, stage, &ir);
                assert!(
                    read > 0,
                    "{label}: the example must place at least one cell"
                );
                if stage == PlacementStage::Crossing {
                    assert_eq!(
                        buffered, 0,
                        "{label}: the module doc says neither example has anything to \
                         legalize, which is why the bus test exists",
                    );
                }
            }
        }
    }
}

/// The 16-cell bus `common::shared_bus_source` builds: every cell reads
/// `sig.b` off one trunk, which is long enough to need repeaters, so
/// the crossing-stage dump carries `buffer_coords` and the read-back
/// has to fill [`PlacedCellNode::buffer_coords`] from it.
#[test]
fn a_legalized_dump_with_buffer_coords_reads_back_equal() {
    let source = shared_bus_source();
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
