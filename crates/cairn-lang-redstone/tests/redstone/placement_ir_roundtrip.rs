//! A dumped Placement IR reads back to the same cells, span emptied, at every stage. The shared
//! bus is there because neither example produces `buffer_coords`.

use cairn_lang_core::Edition;
use cairn_lang_redstone::{PlacedCellNode, PlacementStage, ScopedPlacementIr};

use crate::common::{
    delayed_from_source, legalized_from_source, load_example, placement_from_source,
    routed_from_source, shared_bus_source,
};

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

/// Returns how many cells read back and how many of those carry buffer coords.
fn assert_cells_round_trip(
    label: &str,
    stage: PlacementStage,
    ir: &ScopedPlacementIr,
) -> (usize, usize) {
    let dump = serde_json::to_value(ir).expect("Placement IR serialises");
    let mut read = 0;
    let mut buffered = 0;
    for (scope, entry) in ir.scopes.iter().enumerate() {
        // A scope that placed no cells has no `cells` key.
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
                    assert_eq!(buffered, 0, "{label}: neither example should need buffers");
                }
            }
        }
    }
}

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
                "{label}: expected buffer coords in the dump: {buffered}",
            );
        }
    }
}
