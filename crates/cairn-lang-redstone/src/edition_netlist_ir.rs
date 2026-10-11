//! Edition Netlist IR: the target-edition realisation of each Netlist IR cell.
//!
//! The edition pass ([`crate::edition_netlist::compile_edition_netlist`]) rewrites each
//! [`crate::netlist_ir::CellNode`] into an [`EditionCellNode`] whose [`EditionCell`] names the
//! realisation of its [`crate::netlist_ir::LogicalCell`], the middle tier of the cell library
//! (`spec/redstone` "Edition differences"). Drivers, ports and `signal_defs` are copied unchanged,
//! and delay is still not carried.
//!
//! Nothing here refuses with `E_NO_PORTABLE_IMPL` (QC / BUD, `spec/redstone` "Edition
//! differences"), because no reachable cell (`And`, `Or`, `Not`) depends on update order.

use cairn_lang_core::Edition;
use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::logic_ir::{ScopeKind, Scoped, ScopedEntry};
use crate::netlist_ir::{CellPortDriver, NetRef, NetlistInput, NetlistOutput};

/// Edition-specific realisation of a [`crate::netlist_ir::LogicalCell`] (`spec/redstone` "Edition
/// differences").
///
/// Each variant names both an edition and a physical family, so pairing a Java cell with a Bedrock
/// tile is a type error. Every `(Edition, LogicalCell)` pair has a variant. The `*Unpinned` ones
/// stand for the reserved cells (`Xor`, `Nand`, `Nor`, `Mux`), whose realisations are not chosen;
/// they are per edition rather than one `Reserved`, so a cell's edition is always its variant's
/// and pinning one is a rename.
///
/// `#[non_exhaustive]` for the same reason as [`crate::netlist_ir::LogicalCell`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EditionCell {
    /// Java AND — comparator in subtract mode, two operands feeding one
    /// comparator whose side input equals the far-hand operand.
    JavaComparatorAnd,
    /// Bedrock AND — two redstone torches inverting an OR into an AND,
    /// avoiding the comparator-subtract construction that behaves
    /// differently on Bedrock.
    BedrockTorchAnd,
    /// Java OR — two repeaters feeding a common dust line.
    JavaRepeaterOr,
    /// Bedrock OR — dust merge onto a common line; no repeater necessary
    /// because Bedrock's redstone signal propagation matches this shape
    /// directly.
    BedrockTorchOr,
    /// Java NOT — a single inverter torch. The same structure as on Bedrock, but edition-tagged
    /// because tile orientation is an edition difference (`spec/redstone` "Edition differences").
    JavaInverterTorch,
    /// Bedrock NOT — a single inverter torch, edition-tagged for the same
    /// reason as [`EditionCell::JavaInverterTorch`].
    BedrockInverterTorch,
    /// Java XOR — unpinned placeholder, reachable only through hand-built IR.
    JavaXorUnpinned,
    /// Bedrock XOR — unpinned placeholder; pair of
    /// [`EditionCell::JavaXorUnpinned`].
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

    /// Base tick delay of this cell's physical realisation, excluding the buffer repeaters delay
    /// insertion adds to the nets feeding it.
    ///
    /// Per `spec/redstone` "Time model", ticks come from the cell selection plus the routed wire
    /// length; this is the cell half, which [`crate::delay::compile_delay`] adds to the wire half.
    /// A subtract-mode comparator, a repeater at `delay=1` and a torch take 1 tick each and a dust
    /// merge none, so the two-torch Bedrock AND (two inversions in series) takes 2 and the Bedrock
    /// OR 0.
    ///
    /// The `*Unpinned` placeholders return a sentinel strictly above every pinned delay, so a
    /// pinning that forgets this table over-estimates rather than under-estimates, and a dump
    /// shows an unpinned cell's delay as unlike any real one.
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

    /// Whether this cell's output leaves at full strength, whatever
    /// strength reached its inputs.
    ///
    /// Dust loses one unit of signal per block, so the attenuation
    /// limit (`spec/redstone` "Place-and-route") runs from the last
    /// component that restores strength. A torch does, and a repeater
    /// does. Two of the pinned cells do not:
    ///
    /// - [`Self::BedrockTorchOr`] is a dust merge: its output is the
    ///   strength that arrived, less the block it spends.
    /// - [`Self::JavaComparatorAnd`] is a comparator, and a comparator
    ///   never outputs more than its rear input carries.
    ///
    /// The dust before either of them and the dust after it count as
    /// one run for the limit, though the cell stands between them, and
    /// the delay pass measures them as one.
    ///
    /// The `*Unpinned` placeholders answer `false`, the pessimistic answer that never leaves a run
    /// of dust unmeasured. Unlike [`Self::base_delay_ticks`]' sentinel it does not show in a dump,
    /// and it can make the delay pass refuse the scope with `E_ATTENUATION_LIMIT`; no surface
    /// syntax reaches them.
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

/// The base delay [`EditionCell::base_delay_ticks`] gives every `*Unpinned` cell.
const UNPINNED_BASE_DELAY_TICKS: u32 = 3;

/// 2 is the largest pinned base delay (`BedrockTorchAnd`). A pinned cell that reaches the sentinel
/// must raise it, and this bound with it, or a dump cannot tell the two apart.
const _: () = assert!(
    UNPINNED_BASE_DELAY_TICKS > 2,
    "UNPINNED_BASE_DELAY_TICKS must sit strictly above every pinned base_delay_ticks value",
);

/// One cell in the DAG, tagged with its edition-specific realisation.
///
/// Drivers and span are the source [`crate::netlist_ir::CellNode`]'s, in the same port order.
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

/// The Edition Netlist IR for one struct/def/site body, one per
/// [`crate::netlist_ir::NetlistIr`].
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
    /// Edition-tagged cells, in the same topological order as the source
    /// [`crate::netlist_ir::NetlistIr::cells`] (`NetRef::Cell(j)` in
    /// `cells[i]` still satisfies `j < i`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<EditionCellNode>,
    /// `sig.NAME` → the net that defines it, copied from the source Netlist IR and serialised the
    /// same way.
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

/// Every scope's [`EditionNetlistIr`], in source order across the whole
/// module.
pub type ScopedEditionNetlistIr = Scoped<EditionNetlistIr>;

/// One entry in a [`ScopedEditionNetlistIr`]: the Edition Netlist IR
/// selected from one scope's Netlist IR.
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
