//! The voxel diff that closes the lift loop (`spec/ecosystem-interop` "Diffing a lift"): the
//! blocks a structure file holds compared cell by cell with the blocks a source builds, scored as
//! `block_iou`, `state_accuracy` and `residual_ratio`, and the cells that differ clustered into
//! regions, each with a likely cause and, where an `edit` line is found to fix it, that line.

mod bodies;
mod patch;
mod regions;

use serde::Serialize;

use crate::block_array::{BlockArrayIr, BlockState};
use crate::intent::IntentModule;
use crate::transliterate::GridUnit;

pub use patch::{BUILD_BUDGET, MAX_EDITS, Patch, suggest_patch};
pub use regions::{Cause, Fix, MemberRef, Region, regions};

/// The ids a cell holds no block at, whichever of them the file or the build writes.
pub const AIR_IDS: [&str; 3] = ["minecraft:air", "minecraft:cave_air", "minecraft:void_air"];

/// What a lift converges at (`spec/ecosystem-interop` "Reverse direction: the compiler
/// transliterates, an LLM lifts").
pub const CONVERGENCE: Thresholds = Thresholds {
    block_iou: 0.985,
    state_accuracy: 0.995,
    residual_ratio: 0.05,
};

/// The values the three metrics converge at.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Thresholds {
    /// The least `block_iou`.
    pub block_iou: f64,
    /// The least `state_accuracy`.
    pub state_accuracy: f64,
    /// The most `residual_ratio`.
    pub residual_ratio: f64,
}

/// The cells one comparison counts, from which each metric is a ratio.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct Counts {
    /// Cells the structure holds a block at.
    pub structure_blocks: u64,
    /// Cells the build holds a block at.
    pub built_blocks: u64,
    /// Cells both hold a block at, the same or not.
    pub both_blocks: u64,
    /// Cells both hold a block of the same id at.
    pub matched: u64,
    /// Matched cells whose states agree as the target builds them.
    pub states_matched: u64,
    /// Cells of the build whose block a `raw_block` or `raw_fill` built.
    pub residual: u64,
}

impl Counts {
    /// Matched cells over the cells either side holds a block at; `1` when neither holds one.
    #[must_use]
    pub fn block_iou(&self) -> f64 {
        let union = self.structure_blocks + self.built_blocks - self.both_blocks;
        ratio(self.matched, union, 1.0)
    }

    /// Matched cells whose states agree over every matched cell; `1` when none is matched.
    #[must_use]
    pub fn state_accuracy(&self) -> f64 {
        ratio(self.states_matched, self.matched, 1.0)
    }

    /// Built cells a raw member built over every built cell; `0` when nothing is built.
    #[must_use]
    pub fn residual_ratio(&self) -> f64 {
        ratio(self.residual, self.built_blocks, 0.0)
    }

    /// The three metrics.
    #[must_use]
    pub fn metrics(&self) -> Metrics {
        Metrics {
            block_iou: self.block_iou(),
            state_accuracy: self.state_accuracy(),
            residual_ratio: self.residual_ratio(),
        }
    }

    /// The cells that keep a comparison from being exact: the voxel XOR and the matched cells
    /// whose states differ.
    #[must_use]
    pub fn differing(&self) -> u64 {
        let union = self.structure_blocks + self.built_blocks - self.both_blocks;
        (union - self.matched) + (self.matched - self.states_matched)
    }

    fn add(&mut self, other: &Self) {
        self.structure_blocks += other.structure_blocks;
        self.built_blocks += other.built_blocks;
        self.both_blocks += other.both_blocks;
        self.matched += other.matched;
        self.states_matched += other.states_matched;
        self.residual += other.residual;
    }
}

/// `part / whole`, or `empty` when `whole` is 0.
fn ratio(part: u64, whole: u64, empty: f64) -> f64 {
    if whole == 0 {
        return empty;
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "a count of cells stays far below 2^52, where f64 is exact"
    )]
    let (part, whole) = (part as f64, whole as f64);
    part / whole
}

/// The `block_iou`, `state_accuracy` and `residual_ratio` of one comparison.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Metrics {
    /// Matched cells over the cells either side holds a block at.
    pub block_iou: f64,
    /// Matched cells whose states agree over every matched cell.
    pub state_accuracy: f64,
    /// Built cells a raw member built over every built cell.
    pub residual_ratio: f64,
}

impl Metrics {
    /// Whether every metric meets `at`.
    #[must_use]
    pub fn converged(&self, at: &Thresholds) -> bool {
        self.shortfalls(at).is_empty()
    }

    /// Each metric that misses `at`, as `block IoU = 0.962 < threshold 0.985`.
    #[must_use]
    pub fn shortfalls(&self, at: &Thresholds) -> Vec<String> {
        let mut out = Vec::new();
        if self.block_iou < at.block_iou {
            out.push(format!(
                "block IoU = {} < threshold {}",
                floor3(self.block_iou),
                at.block_iou
            ));
        }
        if self.state_accuracy < at.state_accuracy {
            out.push(format!(
                "state_accuracy = {} < threshold {}",
                floor3(self.state_accuracy),
                at.state_accuracy
            ));
        }
        if self.residual_ratio > at.residual_ratio {
            out.push(format!(
                "residual raw = {} > threshold {}",
                percent(self.residual_ratio),
                percent(at.residual_ratio)
            ));
        }
        out
    }
}

/// How far `value * 1000` may stray from the exact product; a ratio of fewer than 10^8 cells
/// that is not a thousandth lies further than this from one.
const SCALED_SLACK: f64 = 1e-9;

/// `value` with three decimals, rounded down, so a printed value never reaches a threshold the
/// value misses.
#[must_use]
pub fn floor3(value: f64) -> String {
    format!("{:.3}", (value * 1000.0 + SCALED_SLACK).floor() / 1000.0)
}

/// `value` as a percentage with one decimal, rounded up, for the same reason.
#[must_use]
pub fn percent(value: f64) -> String {
    let thousandths = (value * 1000.0 - SCALED_SLACK).ceil();
    let tenths = if thousandths > 0.0 {
        thousandths / 10.0
    } else {
        0.0
    };
    if tenths.fract() == 0.0 {
        format!("{tenths:.0}%")
    } else {
        format!("{tenths:.1}%")
    }
}

/// What a region's cells are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionKind {
    /// The structure holds a block the build leaves air.
    Missing,
    /// The build holds a block where the structure has air.
    Extra,
    /// Both hold a block, of different ids.
    Replaced,
    /// Both hold a block of one id, its states differing.
    State,
}

impl RegionKind {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Extra => "extra",
            Self::Replaced => "replaced",
            Self::State => "state",
        }
    }
}

/// One property a state region's block holds otherwise in the build.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct StateDifference {
    /// The property.
    pub key: String,
    /// The structure's value, absent where it holds none.
    pub structure: Option<String>,
    /// The build's value, absent where it holds none.
    pub built: Option<String>,
}

/// A structure file compared with what a source builds, body by body.
#[derive(Debug, Clone)]
pub struct Comparison {
    /// Each body of the structure with the body of the build it is compared with.
    pub bodies: Vec<BodyComparison>,
    /// Every body's counts, summed.
    pub counts: Counts,
}

/// One body of the structure and the body of the build it is compared with.
#[derive(Debug, Clone)]
pub struct BodyComparison {
    /// The structure's body, `None` for a body of the build the structure holds none for.
    pub structure: Option<String>,
    /// The build's body, a `struct` or a `site`; `None` for a body of the structure the build
    /// holds none for.
    pub source: Option<String>,
    /// The cells counted.
    pub counts: Counts,
    built: bodies::BuiltBody,
    differences: Vec<Difference>,
}

impl BodyComparison {
    /// The name a region of this comparison is reported under: the build's, else the
    /// structure's.
    #[must_use]
    pub fn name(&self) -> &str {
        self.source
            .as_deref()
            .or(self.structure.as_deref())
            .unwrap_or_default()
    }
}

/// A cell the structure and the build disagree at.
#[derive(Debug, Clone)]
struct Difference {
    cell: (u32, u32, u32),
    kind: RegionKind,
    /// The structure's block, if it holds one.
    expected: Option<BlockState>,
    /// The build's block, if it holds one.
    built: Option<BlockState>,
    /// For a state difference, each property that differs.
    states: Vec<StateDifference>,
    /// What last wrote the cell of the build.
    by: bodies::By,
}

/// Compare the blocks of `units`, the bodies a structure file holds, with the blocks `built`
/// holds, as `spec/ecosystem-interop` "Diffing a lift" pairs and counts them.
#[must_use]
pub fn compare(units: &[GridUnit], built: &BlockArrayIr) -> Comparison {
    let built_bodies = bodies::built_bodies(built);
    let mut out = Vec::new();
    let mut counts = Counts::default();
    for (unit, body) in bodies::pair(units, built_bodies) {
        let compared = bodies::compare_pair(unit, body);
        counts.add(&compared.counts);
        out.push(compared);
    }
    Comparison {
        bodies: out,
        counts,
    }
}

/// The verdict on a lift: its metrics, the regions that differ and the patch found for them.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiffReport {
    /// Whether every metric meets [`CONVERGENCE`].
    pub converged: bool,
    /// `E_DECOMPILE_DIFF` when the lift has not converged.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
    /// The whole comparison's metrics.
    #[serde(flatten)]
    pub metrics: Metrics,
    /// What the metrics converge at.
    pub thresholds: Thresholds,
    /// The cells the metrics are ratios of.
    pub counts: Counts,
    /// Each pair of bodies compared.
    pub bodies: Vec<BodyReport>,
    /// The regions that differ, the largest first.
    pub regions: Vec<Region>,
    /// The `edit` lines found to bring the build nearer the structure, with the metrics it then
    /// reaches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch: Option<Patch>,
}

/// One pair of bodies in a [`DiffReport`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BodyReport {
    /// The structure's body, absent for a body the structure holds none for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structure: Option<String>,
    /// The build's body, absent for a body the build holds none for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// The pair's metrics.
    #[serde(flatten)]
    pub metrics: Metrics,
    /// The pair's counts.
    pub counts: Counts,
}

/// What [`diff`] reads: the structure's bodies, and a source with what it builds.
pub struct DiffInput<'a> {
    /// The structure file's bodies, as an import decodes them.
    pub units: &'a [GridUnit],
    /// The source text.
    pub source: &'a str,
    /// The source's Intent IR, its edits applied.
    pub module: &'a IntentModule,
    /// What the source builds at the structure's target.
    pub built: &'a BlockArrayIr,
    /// What a source text builds at the same target, `None` when any pass raises an error.
    pub build: &'a dyn Fn(&str) -> Option<BlockArrayIr>,
}

/// Compare, cluster and patch: the report `cairn diff-blocks` prints.
#[must_use]
pub fn diff(input: &DiffInput<'_>) -> DiffReport {
    let comparison = compare(input.units, input.built);
    let mut found = regions(&comparison, input.source);
    let patch = suggest_patch(input, &comparison, &mut found);
    let metrics = comparison.counts.metrics();
    let converged = metrics.converged(&CONVERGENCE);
    DiffReport {
        converged,
        code: (!converged).then_some("E_DECOMPILE_DIFF"),
        metrics,
        thresholds: CONVERGENCE,
        counts: comparison.counts,
        bodies: comparison
            .bodies
            .iter()
            .map(|body| BodyReport {
                structure: body.structure.clone(),
                source: body.source.clone(),
                metrics: body.counts.metrics(),
                counts: body.counts,
            })
            .collect(),
        regions: found,
        patch,
    }
}

/// Whether `id` names air.
fn is_air(id: &str) -> bool {
    AIR_IDS.contains(&id)
}
