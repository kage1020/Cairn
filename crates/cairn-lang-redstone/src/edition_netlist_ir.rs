//! Edition Netlist IR: each Netlist IR cell's realisation for the target edition, the middle tier
//! of `spec/redstone` "Edition differences".

use cairn_lang_core::Edition;
use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::logic_ir::{ScopeKind, Scoped, ScopedEntry};
use crate::netlist_ir::{CellPortDriver, NetRef, NetlistInput, NetlistOutput};

/// Edition-specific realisation of a [`crate::netlist_ir::LogicalCell`]; the `*Unpinned` variants
/// stand for the reserved cells, whose realisations are not chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EditionCell {
    /// Java AND — a comparator in subtract mode with one operand inverted onto its side at full
    /// strength.
    JavaComparatorAnd,
    /// Bedrock AND — two torches inverting an OR.
    BedrockTorchAnd,
    /// Java OR — two repeaters feeding a common dust line.
    JavaRepeaterOr,
    /// Bedrock OR — a dust merge onto a common line.
    BedrockTorchOr,
    /// Java NOT — a single inverter torch.
    JavaInverterTorch,
    /// Bedrock NOT — a single inverter torch.
    BedrockInverterTorch,
    /// Java XOR — unpinned placeholder.
    JavaXorUnpinned,
    /// Bedrock XOR — unpinned placeholder.
    BedrockXorUnpinned,
    /// Java NAND — unpinned placeholder.
    JavaNandUnpinned,
    /// Bedrock NAND — unpinned placeholder.
    BedrockNandUnpinned,
    /// Java NOR — unpinned placeholder.
    JavaNorUnpinned,
    /// Bedrock NOR — unpinned placeholder.
    BedrockNorUnpinned,
    /// Java 2:1 MUX — unpinned placeholder.
    JavaMuxUnpinned,
    /// Bedrock 2:1 MUX — unpinned placeholder.
    BedrockMuxUnpinned,
}

impl EditionCell {
    /// The edition this variant realises.
    #[must_use]
    pub const fn edition(self) -> Edition {
        match self {
            Self::JavaComparatorAnd
            | Self::JavaRepeaterOr
            | Self::JavaInverterTorch
            | Self::JavaXorUnpinned
            | Self::JavaNandUnpinned
            | Self::JavaNorUnpinned
            | Self::JavaMuxUnpinned => Edition::Java,
            Self::BedrockTorchAnd
            | Self::BedrockTorchOr
            | Self::BedrockInverterTorch
            | Self::BedrockXorUnpinned
            | Self::BedrockNandUnpinned
            | Self::BedrockNorUnpinned
            | Self::BedrockMuxUnpinned => Edition::Bedrock,
        }
    }

    /// Base tick delay of this cell, excluding buffer repeaters; an `*Unpinned` cell returns a
    /// sentinel above every pinned delay.
    #[must_use]
    pub const fn base_delay_ticks(self) -> u32 {
        match self {
            Self::JavaComparatorAnd
            | Self::JavaRepeaterOr
            | Self::JavaInverterTorch
            | Self::BedrockInverterTorch => 1,
            Self::BedrockTorchAnd => 2,
            Self::BedrockTorchOr => 0,
            Self::JavaXorUnpinned
            | Self::JavaNandUnpinned
            | Self::JavaNorUnpinned
            | Self::JavaMuxUnpinned
            | Self::BedrockXorUnpinned
            | Self::BedrockNandUnpinned
            | Self::BedrockNorUnpinned
            | Self::BedrockMuxUnpinned => UNPINNED_BASE_DELAY_TICKS,
        }
    }

    /// Whether this cell's output leaves at full strength, whatever reached its inputs; the output
    /// of one that does not must never be weaker, while the cell is on, than the input that has
    /// spent the most, so the dust either side of it counts as one run for the attenuation limit.
    #[must_use]
    pub const fn regenerates(self) -> bool {
        match self {
            Self::JavaRepeaterOr
            | Self::JavaInverterTorch
            | Self::BedrockTorchAnd
            | Self::BedrockInverterTorch => true,
            Self::JavaComparatorAnd
            | Self::BedrockTorchOr
            | Self::JavaXorUnpinned
            | Self::JavaNandUnpinned
            | Self::JavaNorUnpinned
            | Self::JavaMuxUnpinned
            | Self::BedrockXorUnpinned
            | Self::BedrockNandUnpinned
            | Self::BedrockNorUnpinned
            | Self::BedrockMuxUnpinned => false,
        }
    }
}

const UNPINNED_BASE_DELAY_TICKS: u32 = 3;

// 2 is the largest pinned base delay, `BedrockTorchAnd`'s.
const _: () = assert!(
    UNPINNED_BASE_DELAY_TICKS > 2,
    "UNPINNED_BASE_DELAY_TICKS must sit strictly above every pinned base_delay_ticks value",
);

/// One cell in the DAG, tagged with its edition-specific realisation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditionCellNode {
    /// Edition-specific realisation chosen for this cell.
    pub cell: EditionCell,
    /// Port → net drivers, in the Netlist IR's port order.
    pub drivers: Vec<CellPortDriver>,
    /// Byte range inherited from the source [`crate::netlist_ir::CellNode`].
    #[serde(skip)]
    pub span: Span,
}

/// The Edition Netlist IR for one struct/def/site body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditionNetlistIr {
    /// Edition this IR was compiled for, which every cell's [`EditionCell::edition`] matches.
    pub edition: Edition,
    /// Sensor-driven nets, copied from the source Netlist IR.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<NetlistInput>,
    /// Actuator-driven nets, copied from the source Netlist IR.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<NetlistOutput>,
    /// Edition-tagged cells, in the source Netlist IR's topological order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<EditionCellNode>,
    /// `sig.NAME` → the net that defines it.
    #[serde(
        skip_serializing_if = "IndexMap::is_empty",
        serialize_with = "crate::logic_ir::serialize_signal_defs"
    )]
    pub signal_defs: IndexMap<DottedRef, NetRef>,
}

impl EditionNetlistIr {
    /// An empty Edition Netlist IR for `edition`.
    #[must_use]
    pub fn new(edition: Edition) -> Self {
        Self {
            edition,
            inputs: Vec::new(),
            outputs: Vec::new(),
            cells: Vec::new(),
            signal_defs: IndexMap::new(),
        }
    }

    /// `true` when the IR has no inputs, outputs or cells.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty() && self.outputs.is_empty() && self.cells.is_empty()
    }
}

/// Every scope's [`EditionNetlistIr`], in source order.
pub type ScopedEditionNetlistIr = Scoped<EditionNetlistIr>;

/// One entry in a [`ScopedEditionNetlistIr`].
pub type ScopedEditionNetlistIrEntry = ScopedEntry<EditionNetlistIr>;

impl ScopedEditionNetlistIr {
    /// Appends one scope's Edition Netlist IR, skipping an empty one.
    pub fn push(&mut self, kind: ScopeKind, name: String, ir: EditionNetlistIr) {
        if ir.is_empty() {
            debug_assert!(
                ir.signal_defs.is_empty(),
                "empty EditionNetlistIr carries orphan signal_defs entries: {:?}",
                ir.signal_defs.keys().collect::<Vec<_>>(),
            );
            return;
        }
        self.scopes
            .push(ScopedEditionNetlistIrEntry { kind, name, ir });
    }
}
