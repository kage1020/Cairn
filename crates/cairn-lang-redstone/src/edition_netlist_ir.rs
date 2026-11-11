//! Edition Netlist IR: each Netlist IR cell's realisation for the target edition, the middle tier
//! of `spec/redstone` "Edition differences".

use cairn_lang_core::Edition;
use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::logic_ir::{ScopeKind, Scoped, ScopedEntry};
use crate::netlist_ir::{CellPortDriver, NetRef, NetlistInput, NetlistOutput, PortName};

/// Edition-specific realisation of a [`crate::netlist_ir::LogicalCell`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(strum::EnumIter))]
#[non_exhaustive]
pub enum EditionCell {
    /// Java AND — a comparator in subtract mode with port `a` at its rear and port `b` inverted
    /// onto its side at full strength.
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
    /// Java XOR — two torch NORs feeding a common dust line, each with one operand inverted.
    JavaTorchXor,
    /// Bedrock XOR — two torch NORs feeding a common dust line, each with one operand inverted.
    BedrockTorchXor,
    /// Java NAND — two inverter torches feeding a common dust line.
    JavaTorchNand,
    /// Bedrock NAND — two inverter torches feeding a common dust line.
    BedrockTorchNand,
    /// Java NOR — one torch on a block both operands power.
    JavaTorchNor,
    /// Bedrock NOR — one torch on a block both operands power.
    BedrockTorchNor,
    /// Java 2:1 MUX — two torch NORs feeding a common dust line, one of `sel` and inverted `a`,
    /// the other of inverted `sel` and inverted `b`.
    JavaTorchMux,
    /// Bedrock 2:1 MUX — two torch NORs feeding a common dust line, one of `sel` and inverted `a`,
    /// the other of inverted `sel` and inverted `b`.
    BedrockTorchMux,
}

impl EditionCell {
    /// The edition this variant realises.
    #[must_use]
    pub const fn edition(self) -> Edition {
        match self {
            Self::JavaComparatorAnd
            | Self::JavaRepeaterOr
            | Self::JavaInverterTorch
            | Self::JavaTorchXor
            | Self::JavaTorchNand
            | Self::JavaTorchNor
            | Self::JavaTorchMux => Edition::Java,
            Self::BedrockTorchAnd
            | Self::BedrockTorchOr
            | Self::BedrockInverterTorch
            | Self::BedrockTorchXor
            | Self::BedrockTorchNand
            | Self::BedrockTorchNor
            | Self::BedrockTorchMux => Edition::Bedrock,
        }
    }

    /// The cell's name in `spec/redstone` "Edition differences", such as `ComparatorAND`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::JavaComparatorAnd => "ComparatorAND",
            Self::BedrockTorchAnd => "TorchAND",
            Self::JavaRepeaterOr => "RepeaterOR",
            Self::BedrockTorchOr => "TorchOR",
            Self::JavaInverterTorch | Self::BedrockInverterTorch => "InverterTorch",
            Self::JavaTorchXor | Self::BedrockTorchXor => "TorchXOR",
            Self::JavaTorchNand | Self::BedrockTorchNand => "TorchNAND",
            Self::JavaTorchNor | Self::BedrockTorchNor => "TorchNOR",
            Self::JavaTorchMux | Self::BedrockTorchMux => "TorchMUX",
        }
    }

    /// Tick delay from input `port` to this cell's output, excluding buffer repeaters.
    #[must_use]
    pub const fn port_delay_ticks(self, port: PortName) -> u32 {
        match self {
            // The side operand passes the inverter before the comparator. An AND has no `sel`, so
            // asked for one it errs on the slow side.
            Self::JavaComparatorAnd => match port {
                PortName::A => 1,
                PortName::B | PortName::Sel => 2,
            },
            Self::JavaRepeaterOr
            | Self::JavaInverterTorch
            | Self::BedrockInverterTorch
            | Self::JavaTorchNand
            | Self::BedrockTorchNand
            | Self::JavaTorchNor
            | Self::BedrockTorchNor => 1,
            // The slowest path from each port passes two torches in series.
            Self::BedrockTorchAnd
            | Self::JavaTorchXor
            | Self::BedrockTorchXor
            | Self::JavaTorchMux
            | Self::BedrockTorchMux => 2,
            Self::BedrockTorchOr => 0,
        }
    }

    /// Whether this cell's output leaves at full strength, whatever reached its inputs; when it
    /// does not, the output must never be weaker, while the cell is on, than whichever input on a
    /// port it passes on ([`Self::port_regenerates`]) has spent the most.
    #[must_use]
    pub const fn regenerates(self) -> bool {
        match self {
            Self::JavaRepeaterOr
            | Self::JavaInverterTorch
            | Self::BedrockTorchAnd
            | Self::BedrockInverterTorch
            | Self::JavaTorchXor
            | Self::BedrockTorchXor
            | Self::JavaTorchNand
            | Self::BedrockTorchNand
            | Self::JavaTorchNor
            | Self::BedrockTorchNor
            | Self::JavaTorchMux
            | Self::BedrockTorchMux => true,
            Self::JavaComparatorAnd | Self::BedrockTorchOr => false,
        }
    }

    /// Whether this cell restores the strength of a signal entering by `port`, so the wire into
    /// `port` ends a run of dust there.
    #[must_use]
    pub const fn port_regenerates(self, port: PortName) -> bool {
        match self {
            // The side operand ends at the inverter. An AND has no `sel`, so asked for one it errs
            // on the strict side.
            Self::JavaComparatorAnd => match port {
                PortName::A | PortName::Sel => false,
                PortName::B => true,
            },
            _ => self.regenerates(),
        }
    }
}

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
