//! Netlist IR → Edition Netlist IR lowering.
//!
//! Rewrites each [`crate::netlist_ir::ScopedNetlistIrEntry`] into a
//! [`ScopedEditionNetlistIrEntry`] by mapping every
//! [`crate::netlist_ir::CellNode`] to an [`EditionCellNode`] whose
//! [`EditionCell`] tag names the target-edition realisation of the source
//! [`crate::netlist_ir::LogicalCell`] — the middle tier of the three-tier
//! cell library documented in `spec/redstone` §14.6.
//!
//! The pass is a single forward walk. Every [`crate::netlist_ir::CellNode`]
//! becomes exactly one [`EditionCellNode`]; drivers, inputs, outputs, and
//! `signal_defs` are copied verbatim. The Netlist IR already carries the
//! topological invariant (`NetRef::Cell(j)` in `cells[i]` satisfies
//! `j < i`), so this stage preserves it by construction.
//!
//! No diagnostics are emitted: CSE / cycle / unbound-signal reporting
//! ran in [`crate::synth::synthesize`], the Logical-Cell selection ran in
//! [`crate::netlist::compile_netlist`], and this stage is a pure lookup.
//! QC / BUD refusal (`E_NO_PORTABLE_IMPL`, §14.6) is not scaffolded here
//! because none of today's reachable
//! [`crate::netlist_ir::LogicalCell`] variants require update-order
//! semantics; the diagnostic will land alongside the sequential-macro or
//! observer PR that introduces the first cell that needs it.

use cairn_lang_core::Edition;

use crate::edition_netlist_ir::{
    EditionCell, EditionCellNode, EditionNetlistIr, ScopedEditionNetlistIr,
};
use crate::netlist_ir::{LogicalCell, NetlistIr, ScopedNetlistIr};

/// Lower a [`ScopedNetlistIr`] to a [`ScopedEditionNetlistIr`] against
/// `edition`.
///
/// One [`EditionNetlistIr`] entry per non-empty [`NetlistIr`] in the
/// input; empty scopes remain elided (they are already dropped by
/// [`ScopedNetlistIr::push`], so this pass simply walks whatever the
/// Netlist IR pass emitted).
#[must_use]
pub fn compile_edition_netlist(
    scoped: &ScopedNetlistIr,
    edition: Edition,
) -> ScopedEditionNetlistIr {
    let mut out = ScopedEditionNetlistIr::new();
    for entry in &scoped.scopes {
        let ir = compile_scope(&entry.ir, edition);
        out.push(entry.kind, entry.name.clone(), ir);
    }
    out
}

fn compile_scope(source: &NetlistIr, edition: Edition) -> EditionNetlistIr {
    let mut out = EditionNetlistIr::new(edition);
    out.inputs.clone_from(&source.inputs);
    out.outputs.clone_from(&source.outputs);
    out.cells = source
        .cells
        .iter()
        .map(|cell| EditionCellNode {
            cell: select_edition_cell(cell.cell, edition),
            drivers: cell.drivers.clone(),
            span: cell.span.clone(),
        })
        .collect();
    out.signal_defs.clone_from(&source.signal_defs);
    out
}

/// Look up the edition-specific realisation of `logical` for `edition`.
///
/// `And` / `Or` / `Not` are the only [`LogicalCell`] variants the synth
/// path can reach today, so those pairs are pinned; the rest fall through
/// to [`EditionCell::Reserved`] until the parser change that makes them
/// reachable also pins their Java / Bedrock realisations.
fn select_edition_cell(logical: LogicalCell, edition: Edition) -> EditionCell {
    match (edition, logical) {
        (Edition::Java, LogicalCell::And) => EditionCell::JavaComparatorAnd,
        (Edition::Java, LogicalCell::Or) => EditionCell::JavaRepeaterOr,
        (Edition::Java, LogicalCell::Not) => EditionCell::JavaInverterTorch,
        (Edition::Bedrock, LogicalCell::And) => EditionCell::BedrockTorchAnd,
        (Edition::Bedrock, LogicalCell::Or) => EditionCell::BedrockTorchOr,
        (Edition::Bedrock, LogicalCell::Not) => EditionCell::BedrockInverterTorch,
        (_, LogicalCell::Xor | LogicalCell::Nand | LogicalCell::Nor | LogicalCell::Mux) => {
            EditionCell::Reserved
        }
    }
}
