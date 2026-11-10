//! Edition Netlist IR: each Netlist IR cell's realisation for the target edition, the middle tier
//! of `spec/redstone` "Edition differences".

use cairn_lang_core::Edition;
use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use cairn_lang_core::logic_library::StageCount;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::logic_ir::{ScopeKind, Scoped, ScopedEntry};
use crate::netlist_ir::{CellPortDriver, NetRef, NetlistInput, NetlistOutput, PortName};

/// Edition-specific realisation of a [`crate::netlist_ir::LogicalCell`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
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
    /// Bedrock OR — two repeaters feeding a common dust line.
    BedrockRepeaterOr,
    /// Java NOT — a single inverter torch.
    JavaInverterTorch,
    /// Bedrock NOT — a single inverter torch.
    BedrockInverterTorch,
    /// Java XOR — the torch NORs of `a` and of `b`, each with the NOR of the two, onto a common
    /// dust line.
    JavaTorchXor,
    /// Bedrock XOR — the torch NORs of `a` and of `b`, each with the NOR of the two, onto a common
    /// dust line.
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
    /// Java DELAY — repeaters in series, two ticks a stage.
    JavaRepeaterDelay {
        /// Stages the repeaters take in all.
        stages: StageCount,
    },
    /// Bedrock DELAY — repeaters in series, two ticks a stage.
    BedrockRepeaterDelay {
        /// Stages the repeaters take in all.
        stages: StageCount,
    },
    /// Java PULSE — the torch NOR of inverted `a` and of `a` through repeaters of
    /// `2 * stages + 1` ticks.
    JavaTorchPulse {
        /// Stages the pulse lasts at most.
        stages: StageCount,
    },
    /// Bedrock PULSE — the torch NOR of inverted `a` and of `a` through repeaters of
    /// `2 * stages + 1` ticks.
    BedrockTorchPulse {
        /// Stages the pulse lasts at most.
        stages: StageCount,
    },
    /// Java rising-edge detector — a one-stage PULSE.
    JavaTorchEdgeRising,
    /// Bedrock rising-edge detector — a one-stage PULSE.
    BedrockTorchEdgeRising,
    /// Java falling-edge detector — the torch NOR of `a` and of inverted `a` through a repeater.
    JavaTorchEdgeFalling,
    /// Bedrock falling-edge detector — the torch NOR of `a` and of inverted `a` through a
    /// repeater.
    BedrockTorchEdgeFalling,
    /// Java LATCH — two torch NORs each powered by the other, `set` gated by a held `reset`.
    JavaTorchLatch,
    /// Bedrock LATCH — two torch NORs each powered by the other, `set` gated by a held `reset`.
    BedrockTorchLatch,
    /// Java COUNTER — `stages` flip-flops of locking repeaters in a twisted ring.
    JavaRepeaterCounter {
        /// Flip-flops in the ring.
        stages: StageCount,
    },
    /// Bedrock COUNTER — `stages` flip-flops of locking repeaters in a twisted ring.
    BedrockRepeaterCounter {
        /// Flip-flops in the ring.
        stages: StageCount,
    },
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
            | Self::JavaTorchMux
            | Self::JavaRepeaterDelay { .. }
            | Self::JavaTorchPulse { .. }
            | Self::JavaTorchEdgeRising
            | Self::JavaTorchEdgeFalling
            | Self::JavaTorchLatch
            | Self::JavaRepeaterCounter { .. } => Edition::Java,
            Self::BedrockTorchAnd
            | Self::BedrockRepeaterOr
            | Self::BedrockInverterTorch
            | Self::BedrockTorchXor
            | Self::BedrockTorchNand
            | Self::BedrockTorchNor
            | Self::BedrockTorchMux
            | Self::BedrockRepeaterDelay { .. }
            | Self::BedrockTorchPulse { .. }
            | Self::BedrockTorchEdgeRising
            | Self::BedrockTorchEdgeFalling
            | Self::BedrockTorchLatch
            | Self::BedrockRepeaterCounter { .. } => Edition::Bedrock,
        }
    }

    /// The cell's name in `spec/redstone` "Edition differences", such as `ComparatorAND`.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::JavaComparatorAnd => "ComparatorAND",
            Self::BedrockTorchAnd => "TorchAND",
            Self::JavaRepeaterOr | Self::BedrockRepeaterOr => "RepeaterOR",
            Self::JavaInverterTorch | Self::BedrockInverterTorch => "InverterTorch",
            Self::JavaTorchXor | Self::BedrockTorchXor => "TorchXOR",
            Self::JavaTorchNand | Self::BedrockTorchNand => "TorchNAND",
            Self::JavaTorchNor | Self::BedrockTorchNor => "TorchNOR",
            Self::JavaTorchMux | Self::BedrockTorchMux => "TorchMUX",
            Self::JavaRepeaterDelay { .. } | Self::BedrockRepeaterDelay { .. } => "RepeaterDelay",
            Self::JavaTorchPulse { .. } | Self::BedrockTorchPulse { .. } => "TorchPulse",
            Self::JavaTorchEdgeRising | Self::BedrockTorchEdgeRising => "TorchEdgeRising",
            Self::JavaTorchEdgeFalling | Self::BedrockTorchEdgeFalling => "TorchEdgeFalling",
            Self::JavaTorchLatch | Self::BedrockTorchLatch => "TorchLatch",
            Self::JavaRepeaterCounter { .. } | Self::BedrockRepeaterCounter { .. } => {
                "RepeaterCounter"
            }
        }
    }

    /// Tick delay from input `port` to this cell's output, or for a macro to the last change one
    /// change at `port` makes there, excluding buffer repeaters.
    #[must_use]
    pub fn port_delay_ticks(self, port: PortName) -> u64 {
        match self {
            // The side operand passes the inverter before the comparator. Asked for a port an AND
            // does not have, it errs on the slow side.
            Self::JavaComparatorAnd => match port {
                PortName::A => 1,
                PortName::B | PortName::Sel | PortName::Set | PortName::Reset => 2,
            },
            // A falling `reset` re-arms `set` through its two-tick hold and three torches. Asked
            // for a port a latch does not have, it errs on the slow side.
            Self::JavaTorchLatch | Self::BedrockTorchLatch => match port {
                PortName::Set => 4,
                PortName::A | PortName::B | PortName::Sel | PortName::Reset => 5,
            },
            Self::JavaRepeaterDelay { stages } | Self::BedrockRepeaterDelay { stages } => {
                2 * u64::from(stages.get())
            }
            // The pulse ends once `a` is out of its repeaters, a tick more than its stages, and
            // through the NOR.
            Self::JavaTorchPulse { stages } | Self::BedrockTorchPulse { stages } => {
                2 * u64::from(stages.get()) + 2
            }
            Self::JavaTorchEdgeRising | Self::BedrockTorchEdgeRising => 4,
            Self::JavaRepeaterCounter { .. }
            | Self::BedrockRepeaterCounter { .. }
            | Self::JavaTorchEdgeFalling
            | Self::BedrockTorchEdgeFalling => 3,
            Self::JavaRepeaterOr
            | Self::BedrockRepeaterOr
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
        }
    }

    /// Whether this cell's output leaves at full strength, whatever reached its inputs; when it
    /// does not, the output must never be weaker, while the cell is on, than whichever input on a
    /// port it passes on ([`Self::port_regenerates`]) has spent the most.
    #[must_use]
    pub const fn regenerates(self) -> bool {
        match self {
            Self::JavaRepeaterOr
            | Self::BedrockRepeaterOr
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
            | Self::BedrockTorchMux
            | Self::JavaRepeaterDelay { .. }
            | Self::BedrockRepeaterDelay { .. }
            | Self::JavaTorchPulse { .. }
            | Self::BedrockTorchPulse { .. }
            | Self::JavaTorchEdgeRising
            | Self::BedrockTorchEdgeRising
            | Self::JavaTorchEdgeFalling
            | Self::BedrockTorchEdgeFalling
            | Self::JavaTorchLatch
            | Self::BedrockTorchLatch
            | Self::JavaRepeaterCounter { .. }
            | Self::BedrockRepeaterCounter { .. } => true,
            Self::JavaComparatorAnd => false,
        }
    }

    /// Whether this cell restores the strength of a signal entering by `port`, so the wire into
    /// `port` ends a run of dust there.
    #[must_use]
    pub const fn port_regenerates(self, port: PortName) -> bool {
        match self {
            // The side operand ends at the inverter. Asked for a port an AND does not have, it
            // errs on the strict side.
            Self::JavaComparatorAnd => match port {
                PortName::A | PortName::Sel | PortName::Set | PortName::Reset => false,
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
