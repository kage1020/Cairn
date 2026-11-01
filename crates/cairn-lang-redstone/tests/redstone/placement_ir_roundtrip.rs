//! A `cairn synth --stage <s>` dump reads back as the Placement IR it was written from, every span
//! emptied, at every stage. The other fixtures supply what neither example has: buffers on cells
//! and on an output, and a scope with no cells.

use cairn_lang_core::Edition;
use cairn_lang_redstone::{PlacedCellNode, PlacedOutputNode, PlacementStage, ScopedPlacementIr};

use crate::common::{
    IDENTITY_WIRE_SOURCE, LONG_OUTWARD_WIRE_SOURCE, delayed_from_source, legalized_from_source,
    load_example, placement_from_source, routed_from_source, shared_bus_source,
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

/// `ir` with every span emptied, as its dump reads back.
fn without_spans(ir: &ScopedPlacementIr) -> ScopedPlacementIr {
    let mut ir = ir.clone();
    for entry in &mut ir.scopes {
        if let Some(region) = &mut entry.ir.region {
            region.span = 0..0;
        }
        for input in &mut entry.ir.inputs {
            input.span = 0..0;
        }
        for output in &mut entry.ir.outputs {
            output.span = 0..0;
        }
        for cell in &mut entry.ir.cells {
            cell.span = 0..0;
        }
    }
    ir
}

/// Asserts that `ir`'s dump reads back as `ir` but for its spans, every node at `stage`, and
/// dumps again unchanged.
fn assert_dump_round_trips(label: &str, stage: PlacementStage, ir: &ScopedPlacementIr) {
    let dump = serde_json::to_string_pretty(ir).expect("Placement IR serialises");
    let back: ScopedPlacementIr = serde_json::from_str(&dump)
        .unwrap_or_else(|err| panic!("{label}: the dump does not read back: {err}"));
    assert_eq!(back, without_spans(ir), "{label}: read back differently");
    for entry in &back.scopes {
        let stages: Vec<PlacementStage> = entry
            .ir
            .cells
            .iter()
            .map(PlacedCellNode::stage)
            .chain(entry.ir.outputs.iter().map(PlacedOutputNode::stage))
            .collect();
        assert_eq!(
            stages,
            vec![stage; stages.len()],
            "{label}: scope `{}` must read back at the {} stage",
            entry.name,
            stage.as_str(),
        );
    }
    assert_eq!(
        serde_json::to_string_pretty(&back).expect("read-back IR serialises"),
        dump,
        "{label}: re-dumps differently",
    );
}

/// What a Placement IR places over all its scopes, and how much of it carries buffer coords.
#[derive(Debug, Default)]
struct Census {
    cells: usize,
    buffered_cells: usize,
    outputs: usize,
    buffered_outputs: usize,
}

fn census(ir: &ScopedPlacementIr) -> Census {
    let mut census = Census::default();
    for entry in &ir.scopes {
        census.cells += entry.ir.cells.len();
        census.buffered_cells += entry
            .ir
            .cells
            .iter()
            .filter(|cell| !cell.buffer_coords().is_empty())
            .count();
        census.outputs += entry.ir.outputs.len();
        census.buffered_outputs += entry
            .ir
            .outputs
            .iter()
            .filter(|output| !output.buffer_coords().is_empty())
            .count();
    }
    census
}

#[test]
fn every_example_dump_reads_back_equal_at_every_stage() {
    for example in ["redstone-door.crn", "crossbar.crn"] {
        let source = load_example(example);
        for edition in [Edition::Java, Edition::Bedrock] {
            for (stage, ir) in every_stage(&source, edition) {
                let label = format!("{example} {edition:?} --stage {}", stage.as_str());
                assert_dump_round_trips(&label, stage, &ir);
                let Census {
                    cells,
                    buffered_cells,
                    outputs,
                    buffered_outputs,
                } = census(&ir);
                assert!(
                    cells > 0 && outputs > 0,
                    "{label}: the example must place a cell and an output",
                );
                assert_eq!(
                    (buffered_cells, buffered_outputs),
                    (0, 0),
                    "{label}: neither example should need buffers",
                );
            }
        }
    }
}

#[test]
fn a_dump_whose_cells_carry_buffer_coords_reads_back_equal() {
    for (stage, ir) in every_stage(&shared_bus_source(), Edition::Java) {
        let label = format!("16-cell bus --stage {}", stage.as_str());
        assert_dump_round_trips(&label, stage, &ir);
        let Census {
            cells,
            buffered_cells,
            ..
        } = census(&ir);
        assert_eq!(cells, 16, "{label}: the fixture is the 16-cell chain");
        if stage == PlacementStage::Crossing {
            assert!(
                buffered_cells >= 2,
                "{label}: expected buffer coords on the cells: {buffered_cells}",
            );
        }
    }
}

#[test]
fn a_dump_whose_output_carries_buffer_coords_reads_back_equal() {
    for (stage, ir) in every_stage(LONG_OUTWARD_WIRE_SOURCE, Edition::Java) {
        let label = format!("long outward wire --stage {}", stage.as_str());
        assert_dump_round_trips(&label, stage, &ir);
        let Census {
            outputs,
            buffered_outputs,
            ..
        } = census(&ir);
        assert_eq!(outputs, 1, "{label}: the fixture has one door");
        if stage == PlacementStage::Crossing {
            assert_eq!(
                buffered_outputs, 1,
                "{label}: expected buffer coords on the door"
            );
        }
    }
}

#[test]
fn a_dump_of_a_scope_with_no_cells_reads_back_equal() {
    for (stage, ir) in every_stage(IDENTITY_WIRE_SOURCE, Edition::Java) {
        let label = format!("identity wire --stage {}", stage.as_str());
        assert_dump_round_trips(&label, stage, &ir);
        let Census { cells, outputs, .. } = census(&ir);
        assert_eq!(
            (cells, outputs),
            (0, 1),
            "{label}: the fixture is one door and no cell",
        );
    }
}
