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
//! QC / BUD refusal (`E_NO_PORTABLE_IMPL`, §14.6) joins the pass
//! alongside the first cell that needs it (sequential-macro or observer
//! families), since none of today's reachable
//! [`crate::netlist_ir::LogicalCell`] variants require update-order
//! semantics.

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
    debug_assert!(
        out.cells.iter().all(|c| c.cell.edition() == edition),
        "compile_scope produced a cell whose edition tag disagrees with the container's",
    );
    out
}

/// Look up the edition-specific realisation of `logical` for `edition`.
///
/// The match is fully exhaustive over `(Edition, LogicalCell)` — no
/// wildcard arm. That gives compile-time protection against two silent
/// regressions:
///
/// 1. **New `Edition` variant** (e.g. `Education`) — every arm below
///    fails to compile until the third edition's realisation is chosen,
///    preventing a silent "matches Java" fallthrough.
/// 2. **New `LogicalCell` variant** — same story via
///    `#[non_exhaustive]`-within-crate matching, since both types live
///    in this crate.
///
/// The `*Unpinned` variants for `Xor` / `Nand` / `Nor` / `Mux` are the
/// safety net for the third silent regression: **new surface syntax that
/// makes an already-reserved cell reachable**. Renaming the variant to
/// its pinned physical form (e.g. `JavaXorUnpinned` → some
/// `JavaComparatorXor`) is the natural editing motion at every mapping
/// site, so the parser expansion cannot slip through without also
/// choosing a physical implementation.
fn select_edition_cell(logical: LogicalCell, edition: Edition) -> EditionCell {
    match (edition, logical) {
        (Edition::Java, LogicalCell::And) => EditionCell::JavaComparatorAnd,
        (Edition::Java, LogicalCell::Or) => EditionCell::JavaRepeaterOr,
        (Edition::Java, LogicalCell::Not) => EditionCell::JavaInverterTorch,
        (Edition::Java, LogicalCell::Xor) => EditionCell::JavaXorUnpinned,
        (Edition::Java, LogicalCell::Nand) => EditionCell::JavaNandUnpinned,
        (Edition::Java, LogicalCell::Nor) => EditionCell::JavaNorUnpinned,
        (Edition::Java, LogicalCell::Mux) => EditionCell::JavaMuxUnpinned,
        (Edition::Bedrock, LogicalCell::And) => EditionCell::BedrockTorchAnd,
        (Edition::Bedrock, LogicalCell::Or) => EditionCell::BedrockTorchOr,
        (Edition::Bedrock, LogicalCell::Not) => EditionCell::BedrockInverterTorch,
        (Edition::Bedrock, LogicalCell::Xor) => EditionCell::BedrockXorUnpinned,
        (Edition::Bedrock, LogicalCell::Nand) => EditionCell::BedrockNandUnpinned,
        (Edition::Bedrock, LogicalCell::Nor) => EditionCell::BedrockNorUnpinned,
        (Edition::Bedrock, LogicalCell::Mux) => EditionCell::BedrockMuxUnpinned,
    }
}
