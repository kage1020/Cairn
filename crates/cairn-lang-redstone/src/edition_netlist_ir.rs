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
//! diagnostic will land alongside the sequential-macro or observer PR that
//! introduces the first cell that needs it.

use cairn_lang_core::Edition;
use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};

use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{CellPortDriver, NetRef, NetlistInput, NetlistOutput};

/// Edition-specific realisation of a [`crate::netlist_ir::LogicalCell`]
/// (`spec/redstone` §14.6). Each variant names both the target edition
/// and the physical implementation family a downstream placer will
/// materialise, so a bug that pairs a Java AND cell with a Bedrock torch
/// tile is a type error, not a runtime mishap.
///
/// The variant set covers every `(LogicalCell, Edition)` combination that
/// today's synth path can reach — `And` / `Or` / `Not` on both editions —
/// plus a single [`EditionCell::Reserved`] catch-all for the parser-
/// unreachable cells (`Xor` / `Nand` / `Nor` / `Mux`) whose Java / Bedrock
/// realisations have not been pinned yet. Reserved is intentionally
/// edition-agnostic: the follow-up PR that teaches the surface parser
/// those primitives will also split Reserved into per-edition variants,
/// same shape as the And / Or / Not pairs.
///
/// `#[non_exhaustive]` for the same reason [`crate::netlist_ir::LogicalCell`]
/// carries the attribute — adding a sequential-macro cell (`latch` /
/// `pulse` / `delay` / `edge_*` / `counter`, §14.1) or a new Edition
/// (Education) later should not be a breaking change for downstream
/// exhaustive matches.
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
    /// Placeholder for the parser-unreachable
    /// [`crate::netlist_ir::LogicalCell`]s (`Xor` / `Nand` / `Nor` /
    /// `Mux`). Never constructed on today's synth path; kept exhaustive
    /// so a downstream `match` never has to reach for a wildcard arm.
    Reserved,
}

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
