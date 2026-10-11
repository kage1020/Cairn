//! Stage 4 of `spec/redstone` "Place-and-route": fills each cell's and pad's `buffer_coords` with
//! the repeaters the delay pass counted, read back off the same routed trees.

use crate::diagnostic::Diagnostic;
use crate::pass::{OpenScope, lower_scopes, open_scope};
use crate::placement_ir::{
    BufferCoord, BufferSegment, CellIdentity, PlacementIr, ScopedPlacementIr,
    ScopedPlacementIrEntry,
};
use crate::repeater_sites::{Fed, RepeaterSites, repeater_sites_of_scope};

/// Output of a [`compile_crossing`] run: the legalized scopes and the findings.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct CrossingOutput {
    /// Placement IR for every scope legalization kept, with `buffer_coords` filled.
    pub scoped: ScopedPlacementIr,
    /// Findings raised by the pass, in scope order.
    pub diagnostics: Vec<Diagnostic>,
}

impl CrossingOutput {
    /// An empty output.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// Lowers a delayed [`ScopedPlacementIr`] into a legalized one, leaving out a scope refused with
/// an error.
#[must_use]
pub fn compile_crossing(delayed: &ScopedPlacementIr) -> CrossingOutput {
    let (scoped, diagnostics) = lower_scopes(delayed, |entry| {
        legalize_scope(entry).map(|ir| (ir, Vec::new()))
    });
    debug_assert!(
        diagnostics
            .iter()
            .all(|d| d.severity() == d.code.severity()),
        "a diagnostic renders with its code's severity: every producer in \
         this pass has to agree with `DiagnosticCode::severity`, including \
         one written after the builders below",
    );
    CrossingOutput {
        scoped,
        diagnostics,
    }
}

type ScopeLegalization = Result<PlacementIr, Diagnostic>;

fn legalize_scope(entry: &ScopedPlacementIrEntry) -> ScopeLegalization {
    let Some(OpenScope {
        mut ir,
        region,
        nets,
        ..
    }) = open_scope(entry, "delayed", "crossing legalization")?
    else {
        return Ok(entry.ir.clone());
    };

    // No coord is contested: a repeater stands only on its own net's dust.
    let sites = repeater_sites_of_scope(&ir, entry, "delayed", &region, &nets.trees)?;
    let allocation = allocate_buffer_coords(&ir, &sites);

    for (index, (cell, buffers)) in ir.cells.iter_mut().zip(allocation.per_cell).enumerate() {
        let identity = CellIdentity::new(index, cell.coord, entry);
        cell.phase.legalize_at(buffers, identity);
    }

    for (index, (output, buffers)) in ir.outputs.iter_mut().zip(allocation.per_output).enumerate() {
        let identity = CellIdentity::output(index, output.pad, entry);
        output.phase.legalize_at(buffers, identity);
    }

    Ok(ir)
}

fn allocate_buffer_coords(ir: &PlacementIr, sites: &RepeaterSites<'_>) -> BufferAllocation {
    let mut per_cell: Vec<Vec<BufferCoord>> = Vec::with_capacity(ir.cells.len());
    for (cell_index, cell) in ir.cells.iter().enumerate() {
        let mut buffers_for_cell: Vec<BufferCoord> = Vec::new();
        for driver in &cell.drivers {
            buffers_for_cell.extend(
                sites
                    .along(driver.net, cell.coord)
                    .unwrap_or_else(|| {
                        Fed::Cell(cell_index).is_not_a_terminal(driver.net, cell.coord)
                    })
                    .into_iter()
                    .map(|coord| BufferCoord::new(BufferSegment::Port(driver.port), coord)),
            );
        }
        debug_assert!(
            buffers_for_cell.iter().all(|b| matches!(
                b.port,
                BufferSegment::Port(port) if cell.drivers.iter().any(|d| d.port == port)
            )),
            "BufferCoord::port must reference a driver on cells[{cell_index}]",
        );
        per_cell.push(buffers_for_cell);
    }

    let mut per_output: Vec<Vec<BufferCoord>> = Vec::with_capacity(ir.outputs.len());
    for (output_index, output) in ir.outputs.iter().enumerate() {
        let buffers_for_output: Vec<BufferCoord> = sites
            .along(output.driver, output.pad)
            .unwrap_or_else(|| {
                Fed::Output(output_index).is_not_a_terminal(output.driver, output.pad)
            })
            .into_iter()
            .map(|coord| BufferCoord::new(BufferSegment::Out, coord))
            .collect();
        debug_assert!(
            buffers_for_output
                .iter()
                .all(|b| matches!(b.port, BufferSegment::Out)),
            "a buffer on the wire to outputs[{output_index}] must name the outward segment",
        );
        per_output.push(buffers_for_output);
    }

    BufferAllocation {
        per_cell,
        per_output,
    }
}

struct BufferAllocation {
    per_cell: Vec<Vec<BufferCoord>>,
    per_output: Vec<Vec<BufferCoord>>,
}

#[cfg(test)]
mod tests;
