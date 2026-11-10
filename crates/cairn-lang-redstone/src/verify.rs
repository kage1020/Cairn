//! `spec/redstone` "Verification": each `assert latency` against the legalized circuit of its
//! scope, counting the ticks of the slowest path from one signal to where the other is read.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use cairn_lang_core::intent::{AssertIr, IntentModule, Member};

use crate::delay::{BUFFER_REPEATER_TICKS, DUST_ATTENUATION_LIMIT};
use crate::diagnostic::{Diagnostic, DiagnosticCode, latency_text};
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{NetRef, PortName};
use crate::placement_ir::{
    BufferSegment, PlacedCellNode, PlacedOutputNode, PlacementIr, PlacementStage, ScopedPlacementIr,
};
use crate::saturating_index;

/// An `E_SIM_ASSERTION_FAILED` for each `assert latency` whose slowest path on its scope's
/// legalized circuit takes more ticks than its bound, each scope's in source order.
///
/// # Panics
///
/// On a cell or pad crossing legalization has not reached.
#[must_use]
pub fn verify_latency(legalized: &ScopedPlacementIr, module: &IntentModule) -> Vec<Diagnostic> {
    let asserts = latency_asserts(module);
    let mut diagnostics = Vec::new();
    for entry in &legalized.scopes {
        let scope = format!("{}={}:", entry.kind.label(), entry.name);
        assert!(
            entry
                .ir
                .cells
                .iter()
                .map(PlacedCellNode::stage)
                .chain(entry.ir.outputs.iter().map(PlacedOutputNode::stage))
                .all(|stage| stage == PlacementStage::Crossing),
            "{scope} latency is counted on the circuit crossing legalization produces",
        );
        for assertion in asserts
            .get(&(entry.kind, entry.name.as_str()))
            .into_iter()
            .flatten()
        {
            let path = slowest_path(&entry.ir, assertion.from, assertion.to);
            debug_assert!(
                path.is_some(),
                "synth refuses a bound naming an undefined signal, or whose end does not depend \
                 on its start",
            );
            let Some(path) = path else {
                continue;
            };
            if path.ticks() > u64::from(assertion.bound) {
                diagnostics.push(failure(&scope, &entry.ir, assertion, &path));
            }
        }
    }
    diagnostics
}

/// One `assert latency` line.
struct LatencyAssert<'a> {
    from: &'a DottedRef,
    to: &'a DottedRef,
    bound: u32,
    span: &'a Span,
}

/// Each scope's `assert latency` lines in source order, those under its members included, keyed
/// as a [`ScopedPlacementIr`] entry names its scope; a name given twice keeps the first scope.
fn latency_asserts(module: &IntentModule) -> HashMap<(ScopeKind, &str), Vec<LatencyAssert<'_>>> {
    fn collect<'a>(
        asserts: &'a [AssertIr],
        members: &'a [Member],
        out: &mut Vec<LatencyAssert<'a>>,
    ) {
        for assertion in asserts {
            if let AssertIr::Latency {
                from,
                to,
                bound,
                span,
            } = assertion
            {
                out.push(LatencyAssert {
                    from,
                    to,
                    bound: *bound,
                    span,
                });
            }
        }
        for member in members {
            collect(&member.children.asserts, &member.children.members, out);
        }
    }
    let scopes = module
        .structs
        .iter()
        .map(|s| (ScopeKind::Struct, s.name.as_str(), &s.asserts, &s.members))
        .chain(
            module
                .defs
                .iter()
                .map(|d| (ScopeKind::Def, d.name.as_str(), &d.asserts, &d.members)),
        )
        .chain(
            module
                .sites
                .iter()
                .map(|s| (ScopeKind::Site, s.name.as_str(), &s.asserts, &s.placements)),
        );
    let mut out: HashMap<(ScopeKind, &str), Vec<LatencyAssert<'_>>> = HashMap::new();
    for (kind, name, asserts, members) in scopes {
        let Entry::Vacant(slot) = out.entry((kind, name)) else {
            continue;
        };
        let mut found = Vec::new();
        collect(asserts, members, &mut found);
        found.sort_by_key(|a| (a.span.start, a.span.end));
        slot.insert(found);
    }
    out
}

/// The slowest path a change takes from one net to where another is read, hop by hop.
struct Path {
    /// The cells it passes and the pad it ends at, in order.
    hops: Vec<Hop>,
}

impl Path {
    /// Every tick on the path, cells and buffer repeaters alike.
    fn ticks(&self) -> u64 {
        self.hops
            .iter()
            .map(Hop::ticks)
            .fold(0, u64::saturating_add)
    }

    /// The buffer repeaters on its wires.
    fn buffers(&self) -> u32 {
        self.hops
            .iter()
            .map(Hop::buffers)
            .fold(0, u32::saturating_add)
    }
}

/// One step of a [`Path`].
enum Hop {
    /// Into `cells[cell]` by `port`, over `buffers` repeaters, then `ticks` through the cell.
    Cell {
        cell: usize,
        port: PortName,
        buffers: u32,
        ticks: u64,
    },
    /// Onto `outputs[output]`'s pad, over `buffers` repeaters.
    Pad { output: usize, buffers: u32 },
}

impl Hop {
    /// The buffer repeaters on the wire into this hop.
    const fn buffers(&self) -> u32 {
        match *self {
            Self::Cell { buffers, .. } | Self::Pad { buffers, .. } => buffers,
        }
    }

    /// The ticks of the wire into this hop and of the cell it enters.
    fn ticks(&self) -> u64 {
        let through = match *self {
            Self::Cell { ticks, .. } => ticks,
            Self::Pad { .. } => 0,
        };
        u64::from(self.buffers().saturating_mul(BUFFER_REPEATER_TICKS)).saturating_add(through)
    }
}

/// When a change at a cell's output arrives, and the driver it came in by.
#[derive(Clone, Copy)]
struct Arrival {
    ticks: u64,
    /// Index into the cell's `drivers`.
    via: usize,
}

/// The slowest path from the net defining `from` to the slowest actuator pad reading `to`, or to
/// the net defining `to` when no actuator reads it; `None` when either is undefined or no path
/// joins them.
fn slowest_path(ir: &PlacementIr, from: &DottedRef, to: &DottedRef) -> Option<Path> {
    let start = *ir.signal_defs.get(from)?;
    let end = *ir.signal_defs.get(to)?;

    let mut arrivals: Vec<Option<Arrival>> = Vec::with_capacity(ir.cells.len());
    let arrival_at = |net: NetRef, arrivals: &[Option<Arrival>]| -> Option<u64> {
        if net == start {
            return Some(0);
        }
        match net {
            NetRef::Input(_) => None,
            NetRef::Cell(j) => arrivals.get(j as usize).copied().flatten().map(|a| a.ticks),
        }
    };
    // Cells are in topological order, so every driver's arrival is known before its cell's.
    for cell in &ir.cells {
        let mut best: Option<Arrival> = None;
        for (index, driver) in cell.drivers.iter().enumerate() {
            let Some(at) = arrival_at(driver.net, &arrivals) else {
                continue;
            };
            let ticks = at
                .saturating_add(u64::from(
                    port_buffers(cell, driver.port).saturating_mul(BUFFER_REPEATER_TICKS),
                ))
                .saturating_add(cell.cell.port_delay_ticks(driver.port));
            if best.is_none_or(|b| ticks > b.ticks) {
                best = Some(Arrival { ticks, via: index });
            }
        }
        arrivals.push(best);
    }
    arrival_at(end, &arrivals)?;

    let mut pad: Option<(usize, u32)> = None;
    for (index, output) in ir.outputs.iter().enumerate() {
        if output.name != *to {
            continue;
        }
        let buffers = saturating_index(output.buffer_coords().len());
        if pad.is_none_or(|(_, slowest)| buffers > slowest) {
            pad = Some((index, buffers));
        }
    }

    let mut hops = Vec::new();
    if let Some((output, buffers)) = pad {
        hops.push(Hop::Pad { output, buffers });
    }
    let mut net = end;
    while net != start {
        let NetRef::Cell(j) = net else {
            unreachable!("only a cell's arrival is reached from a net other than the start")
        };
        let cell = &ir.cells[j as usize];
        let via = arrivals[j as usize]
            .expect("a cell the path reaches arrived by one of its drivers")
            .via;
        let driver = &cell.drivers[via];
        hops.push(Hop::Cell {
            cell: j as usize,
            port: driver.port,
            buffers: port_buffers(cell, driver.port),
            ticks: cell.cell.port_delay_ticks(driver.port),
        });
        net = driver.net;
    }
    hops.reverse();
    Some(Path { hops })
}

/// The buffer repeaters on the wire into `cell`'s `port`.
fn port_buffers(cell: &PlacedCellNode, port: PortName) -> u32 {
    let on_port = cell
        .buffer_coords()
        .iter()
        .filter(|b| b.port == BufferSegment::Port(port));
    saturating_index(on_port.count())
}

fn plural(n: impl Into<u64>, one: &str, many: &str) -> String {
    let n = n.into();
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The `E_SIM_ASSERTION_FAILED` for `assertion`, whose bound `path` takes longer than.
fn failure(
    scope: &str,
    ir: &PlacementIr,
    assertion: &LatencyAssert<'_>,
    path: &Path,
) -> Diagnostic {
    let buffers = path.buffers();
    let split = match buffers.saturating_mul(BUFFER_REPEATER_TICKS) {
        0 => "all in its cells".to_owned(),
        n => format!("{n} in buffer repeaters on the routed wire"),
    };
    let primary = format!(
        "{scope} {text} fails on {edition}: its slowest path from {from} to {to} takes {ticks}, \
         {split}",
        text = latency_text(assertion.from, assertion.to, assertion.bound),
        edition = ir.edition.as_str(),
        from = assertion.from,
        to = assertion.to,
        ticks = plural(path.ticks(), "tick", "ticks"),
    );
    let mut diagnostic = Diagnostic::new(
        DiagnosticCode::SimAssertionFailed,
        assertion.span.clone(),
        primary,
    );
    for (index, hop) in path.hops.iter().enumerate() {
        let (span, message) = match *hop {
            Hop::Cell {
                cell,
                port,
                buffers,
                ticks,
            } => {
                let node = &ir.cells[cell];
                let wire = match buffers {
                    0 => String::new(),
                    n => format!("{} on the wire, then ", repeaters(n)),
                };
                let message = format!(
                    "{lead} cell #{cell} (`{name}`) by port `{port}`: {wire}{through} through \
                     the cell",
                    lead = if index == 0 {
                        "the path enters"
                    } else {
                        "then"
                    },
                    name = node.cell.name(),
                    port = port.as_str(),
                    through = plural(ticks, "tick", "ticks"),
                );
                (node.span.clone(), message)
            }
            Hop::Pad { output, buffers } => {
                let node = &ir.outputs[output];
                let message = format!(
                    "{lead} the pad of the actuator reading {to}: {wire} on the wire to it",
                    lead = if index == 0 {
                        "the path ends at"
                    } else {
                        "then"
                    },
                    to = node.name,
                    wire = match buffers {
                        0 => "no buffer repeater".to_owned(),
                        n => repeaters(n),
                    },
                );
                (node.span.clone(), message)
            }
        };
        diagnostic = diagnostic.with_note(span, message);
    }
    let wire = match buffers {
        0 => "the path's wires carry no buffer repeater, so only a path through fewer or faster \
              cells takes ticks off"
            .to_owned(),
        n => format!(
            "the path's wires carry {}, one wherever a wire runs past {DUST_ATTENUATION_LIMIT} \
             blocks of dust, so a smaller `circuit` region, which puts the pads and the cells \
             closer together, needs fewer",
            repeaters(n),
        ),
    };
    let measured = path.ticks();
    let fix = if u32::try_from(measured).is_ok() {
        format!(
            "Fix: relax the bound to `<= {measured}`, or shorten the path; the logic is never \
             rewritten to meet a bound"
        )
    } else {
        format!(
            "Fix: shorten the path, which takes more ticks than a bound can name, at most {}; \
             the logic is never rewritten to meet a bound",
            u32::MAX
        )
    };
    diagnostic.with_footer(wire).with_footer(fix)
}

/// `n buffer repeater(s)`.
fn repeaters(n: u32) -> String {
    plural(n, "buffer repeater", "buffer repeaters")
}

#[cfg(test)]
mod tests;
