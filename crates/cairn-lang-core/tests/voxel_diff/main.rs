//! The voxel diff of a structure's blocks with what a source builds: the metrics, the pairing of
//! bodies, the regions with their causes, and the suggested patch.

#[path = "../common/mod.rs"]
pub mod common;

mod compare;
mod metrics;
mod patch;
mod regions;

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, lower_to_block_array};
use cairn_lang_core::transliterate::{BlockGrid, GridEntry, GridUnit};
use cairn_lang_core::voxel_diff::{DiffInput, DiffReport, diff};
use cairn_lang_core::{Severity, lower, parse, resolve};

/// What `source` builds with no registry pack, `None` when it does not parse or raises an error.
pub fn build(source: &str) -> Option<BlockArrayIr> {
    let module = parse(source).ok()?;
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    let out = lower_to_block_array(&ir, &resolution, None);
    let failed = resolution
        .diagnostics
        .iter()
        .chain(&out.diagnostics)
        .any(|d| d.severity() == Severity::Error);
    (!failed).then_some(out)
}

/// `array` as a structure file of it decodes, under `name`, every entry built as it is written.
pub fn unit_of(name: &str, array: &BlockArray) -> GridUnit {
    GridUnit {
        name: name.to_owned(),
        grid: BlockGrid {
            dims: array.dims,
            palette: array
                .palette
                .entries
                .iter()
                .cloned()
                .map(GridEntry::exact)
                .collect(),
            voxels: array.voxels.clone(),
        },
        comment: Vec::new(),
    }
}

/// The units of each struct `source` builds, named for it.
///
/// # Panics
///
/// When `source` does not build.
pub fn units(source: &str) -> Vec<GridUnit> {
    let built = build(source).unwrap_or_else(|| panic!("the structure's source builds:\n{source}"));
    built
        .structures
        .iter()
        .filter_map(|(key, array)| Some(unit_of(key.strip_prefix("struct::")?, array)))
        .collect()
}

/// `lift` diffed against `units`.
///
/// # Panics
///
/// When `lift` does not build.
pub fn report(units: &[GridUnit], lift: &str) -> DiffReport {
    let module = parse(lift).expect("the lift parses");
    let ir = lower(&module);
    let built = build(lift).unwrap_or_else(|| panic!("the lift builds:\n{lift}"));
    diff(&DiffInput {
        units,
        source: lift,
        module: &ir,
        built: &built,
        build: &build,
    })
}

/// Each region of `report` as the text report prints it.
pub fn region_lines(report: &DiffReport) -> Vec<String> {
    report.regions.iter().map(ToString::to_string).collect()
}

/// A theme for the fixtures: `floor`, `wall`, `glass` and `roof`.
pub const THEME: &str = "theme t:\n  \
                         slot floor -> @oak_planks\n  \
                         slot wall  -> @cobblestone\n  \
                         slot glass -> @glass_pane\n  \
                         slot roof  -> @spruce_stairs\n\n";

/// [`THEME`] and a 5x5 struct `s` of a floor, 3-high walls and then `rows`.
pub fn hut(rows: &str) -> String {
    format!(
        "{THEME}struct s size=5x5\n  floor mat_slot=floor\n  walls mat_slot=wall height=3\n{rows}"
    )
}
