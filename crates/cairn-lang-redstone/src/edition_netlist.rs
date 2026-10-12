//! Netlist IR → Edition Netlist IR lowering: each logical cell's realisation for the target
//! edition, the middle tier of `spec/redstone` "Edition differences".

use cairn_lang_core::Edition;

use crate::edition_netlist_ir::{
    EditionCell, EditionCellNode, EditionNetlistIr, ScopedEditionNetlistIr,
};
use crate::netlist_ir::{LogicalCell, NetlistIr, ScopedNetlistIr};

/// Lowers a [`ScopedNetlistIr`] to a [`ScopedEditionNetlistIr`] for `edition`.
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
