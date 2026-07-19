//! Edition Netlist IR — edition-specific realisation of each Netlist IR cell.
//!
//! Sits one lowering step above the [Netlist IR](crate::netlist_ir) and one
//! below the Placement IR that later passes will grow on top. The edition
//! pass ([`crate::edition_netlist::compile_edition_netlist`]) walks every
//! [`crate::netlist_ir::ScopedNetlistIrEntry`] once and rewrites each
//! [`crate::netlist_ir::CellNode`] into an [`EditionCellNode`] whose
//! [`EditionCell`] tag names the target-edition realisation of the source
//! [`crate::netlist_ir::LogicalCell`] — the middle rung of the three-tier
//! cell library documented in `spec/redstone` §14.6 (`Logical Cell → Edition
//! Cell → Physical Tile`).
//!
//! The pass is structural only: driver arrays, net references, input /
//! output ports, and `signal_defs` are copied verbatim from the source
//! Netlist IR. Delay is still not carried — per `spec/redstone` §14.4 /
//! §14.8 delay is first determined in the Placement IR, one step further
//! down the pipeline.
//!
//! QC / BUD refusal (`E_NO_PORTABLE_IMPL`, §14.6) is not scaffolded here
//! because none of the currently reachable [`crate::netlist_ir::LogicalCell`]
//! variants (`And` / `Or` / `Not`) require update-order semantics; the
//! diagnostic joins the pass alongside the first cell that needs it
//! (sequential-macro or observer families).

use cairn_lang_core::Edition;
use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{CellPortDriver, NetRef, NetlistInput, NetlistOutput};

/// Edition-specific realisation of a [`crate::netlist_ir::LogicalCell`]
/// (`spec/redstone` §14.6). Each variant carries both a target edition
/// and a physical implementation family, so pairing a Java AND cell with
/// a Bedrock torch tile is a type error, not a runtime mishap.
///
/// The variant set covers *every* `(Edition, LogicalCell)` combination.
/// The pinned pairs (`ComparatorAnd` / `TorchAnd` / `RepeaterOr` /
/// `TorchOr` / `InverterTorch`) name the physical implementation the
/// downstream placer will materialise. The `*Unpinned` pairs are named
/// placeholders for the parser-unreachable cells (`Xor` / `Nand` / `Nor`
/// / `Mux`) whose Java / Bedrock realisations have not been chosen yet —
/// keeping them as concrete per-edition variants (rather than one
/// edition-agnostic `Reserved`) makes container/cell edition parity a
/// pure naming invariant, and makes the eventual pinning a rename in the
/// one match arm that produces the variant instead of a fresh enum
/// entry. No wildcard fall-through arm exists in
/// [`crate::edition_netlist::compile_edition_netlist`], so a future
/// third `Edition` variant (Education) triggers a compile error at every
/// mapping site rather than silently degrading to the wrong realisation.
///
/// `#[non_exhaustive]` for the same reason [`crate::netlist_ir::LogicalCell`]
/// carries the attribute — adding a sequential-macro cell (`latch` /
/// `pulse` / `delay` / `edge_*` / `counter`, §14.1) later should not be a
/// breaking change for downstream exhaustive matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
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
    /// Java NOT — a single inverter torch. Structurally shared with
    /// Bedrock but kept edition-tagged so a later placer can pick the
    /// correct tile orientation without re-deriving the edition (spec
    /// §14.6 lists orientation among the edition-absorbed differences).
    JavaInverterTorch,
    /// Bedrock NOT — a single inverter torch, edition-tagged for the same
    /// reason as [`EditionCell::JavaInverterTorch`].
    BedrockInverterTorch,
    /// Java XOR — unpinned placeholder. Reachable only via hand-built IR
    /// today (the surface parser cannot emit `xor`); the mapping arm in
    /// [`crate::edition_netlist::compile_edition_netlist`] will rename this
    /// variant to its pinned form the same time the parser change lands.
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
    /// The target edition this variant realises. Used by
    /// [`crate::edition_netlist::compile_edition_netlist`] to
    /// `debug_assert!` that every cell in an [`EditionNetlistIr`]
    /// agrees with its container's [`EditionNetlistIr::edition`].
    #[must_use]
    pub fn edition(self) -> Edition {
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

    /// Base tick delay contributed by this cell's physical realisation,
    /// exclusive of any implicit buffer repeaters the delay-insertion
    /// pass adds for driver segments beyond the dust attenuation limit.
    ///
    /// `spec/redstone` §14.4 ties tick counts to the cell selection
    /// plus the routed wire length. This method exposes the first
    /// half — the constant tick contribution of the physical tile —
    /// so [`crate::delay::compile_delay`] can compose it with the
    /// per-driver buffer count without re-deriving the edition split
    /// each time.
    ///
    /// The canonical numbers follow Minecraft's baseline redstone
    /// physics: a comparator in subtract mode delays 1 tick, a
    /// repeater on its default `delay=1` setting delays 1 tick, a
    /// redstone torch inverter delays 1 tick, and a bare dust merge
    /// carries no cell-level tick. The two-torch Bedrock AND is
    /// therefore 2 ticks (NAND→NAND stacked in series), and the
    /// Bedrock OR is a bare dust merge with no cell tick (matching
    /// the [`Self::BedrockTorchOr`] doc-comment's "no repeater
    /// necessary" note).
    ///
    /// The `*Unpinned` variants are parser-unreachable placeholders
    /// today (Xor / Nand / Nor / Mux); they return
    /// [`UNPINNED_BASE_DELAY_TICKS`], a pessimistic sentinel that
    /// sits **strictly above** every pinned base delay so a future
    /// pinning that lands new physics without touching this table
    /// would over-estimate rather than under-estimate — the same
    /// "reserved but not yet pinned" pattern
    /// [`crate::edition_netlist::compile_edition_netlist`] uses for
    /// their variants. Keeping the sentinel distinct from any real
    /// value also makes the pinning migration observable in delay
    /// dumps: an `_Unpinned` cell with `delay_ticks =
    /// UNPINNED_BASE_DELAY_TICKS` is visibly different from a pinned
    /// 2-tick cell, so a future PR that flips `JavaXorUnpinned` to
    /// `JavaXorComparatorPair` and hard-codes 2 ticks shifts every
    /// downstream regression that was silently accepting the sentinel.
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
}

/// Pessimistic base-delay sentinel returned by
/// [`EditionCell::base_delay_ticks`] for every parser-unreachable
/// `*Unpinned` variant. Strictly above every pinned base delay in the
/// table (currently 2 ticks for `BedrockTorchAnd`) so a future rename
/// that lands new physics without touching the table over-estimates
/// rather than silently inheriting a real value; distinct from any
/// pinned tick so an `_Unpinned` cell's delay is visibly different in
/// a JSON dump. Not `pub` — external callers cannot construct
/// `_Unpinned` cells via the parser today, so the sentinel is a
/// crate-internal invariant.
const UNPINNED_BASE_DELAY_TICKS: u32 = 3;

/// One cell in the DAG, tagged with its edition-specific realisation.
///
/// Drivers and span are copied verbatim from the source
/// [`crate::netlist_ir::CellNode`]; the pass is a pure structural rewrite,
/// so downstream consumers can index by position (`[A, B]` for two-input
/// gates, `[A]` for `Not`, `[Sel, A, B]` for `Mux`) exactly as they do on
/// the Netlist IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditionCellNode {
    /// Edition-specific realisation chosen for this cell.
    pub cell: EditionCell,
    /// Port → net drivers, in canonical port order carried across from
    /// the Netlist IR.
    pub drivers: Vec<CellPortDriver>,
    /// Byte range of the originating `logic ...` sub-expression, inherited
    /// from the source [`crate::netlist_ir::CellNode`] (and originally from
    /// the [`crate::logic_ir::GateNode`] behind it).
    #[serde(skip)]
    pub span: Span,
}

/// The Edition Netlist IR for one struct/def/site body.
///
/// One instance per [`crate::netlist_ir::NetlistIr`] handed to the pass;
/// the target edition is pinned on the entry so a JSON dump makes clear
/// which library the cells were selected from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EditionNetlistIr {
    /// Edition this IR was compiled for.
    pub edition: Edition,
    /// Sensor-driven nets, copied verbatim from the source Netlist IR.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<NetlistInput>,
    /// Actuator-driven nets, copied verbatim from the source Netlist IR.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<NetlistOutput>,
    /// Edition-tagged cells, in the same topological order as the source
    /// [`crate::netlist_ir::NetlistIr::cells`] (`NetRef::Cell(j)` in
    /// `cells[i]` still satisfies `j < i`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<EditionCellNode>,
    /// `sig.NAME` → the net that defines it, copied verbatim from the
    /// source Netlist IR.
    ///
    /// Serialises as a JSON object keyed by the flattened dotted name —
    /// the same custom serialisation the Logic IR and Netlist IR use — so
    /// consumers see a uniform shape across all three IRs.
    #[serde(
        skip_serializing_if = "IndexMap::is_empty",
        serialize_with = "serialize_signal_defs"
    )]
    pub signal_defs: IndexMap<DottedRef, NetRef>,
}

fn serialize_signal_defs<S: Serializer>(
    defs: &IndexMap<DottedRef, NetRef>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut map = serializer.serialize_map(Some(defs.len()))?;
    for (name, net) in defs {
        map.serialize_entry(&name.to_string(), net)?;
    }
    map.end()
}

impl EditionNetlistIr {
    /// Empty Edition Netlist IR for the given edition — no inputs, no
    /// outputs, no cells.
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

    /// `true` when this scope produced zero inputs, zero outputs, and
    /// zero cells. Matches [`crate::netlist_ir::NetlistIr::is_empty`] so
    /// scope elision behaves the same across both IRs.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty() && self.outputs.is_empty() && self.cells.is_empty()
    }
}

/// Ordered list of `(scope kind, scope name)` → [`EditionNetlistIr`]
/// entries covering an entire `.crn` module.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct ScopedEditionNetlistIr {
    /// Per-scope Edition Netlist IR, in source order across the whole
    /// module.
    pub scopes: Vec<ScopedEditionNetlistIrEntry>,
}

impl ScopedEditionNetlistIr {
    /// Empty list.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Append one scope's Edition Netlist IR. Empty scopes are elided so
    /// a module without any redstone renders as `[]`, matching the
    /// Netlist IR's elision.
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

    /// `true` when no scope produced a non-empty Edition Netlist IR.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.scopes.is_empty()
    }
}

/// One entry in a [`ScopedEditionNetlistIr`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ScopedEditionNetlistIrEntry {
    /// Which family of scope this Edition Netlist IR came from.
    pub kind: ScopeKind,
    /// Source-level name of the scope.
    pub name: String,
    /// Edition Netlist IR selected from the scope's Netlist IR.
    pub ir: EditionNetlistIr,
}
