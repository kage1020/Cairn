//! `spec/redstone` "Verification": each `assert latency` against the legalized circuit of its
//! scope, counting the ticks of the slowest path from one signal to where the other is read.

use std::collections::HashMap;

use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use cairn_lang_core::intent::{AssertIr, IntentModule, Member};

use crate::delay::{BUFFER_REPEATER_TICKS, DUST_ATTENUATION_LIMIT, NetGraph};
use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::geometry::Voxel;
use crate::grid::Grid;
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::{NetRef, PortName};
use crate::placement_ir::{CircuitLayout, ScopedCircuitLayout, TileKind};
use crate::synth::latency_text;
use crate::tile::{RISER_TICKS, TilePort};

/// An `E_SIM_ASSERTION_FAILED` for each `assert latency` whose slowest path on its scope's
/// legalized circuit takes more ticks than its bound.
#[must_use]
pub fn verify_latency(module: &IntentModule, legalized: &ScopedCircuitLayout) -> Vec<Diagnostic> {
    let bounds = latency_bounds(module);
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for entry in &legalized.scopes {
        let scope = format!("{}={}:", entry.kind.label(), entry.name);
        let Ok(grid) = Grid::of(&entry.ir) else {
            continue;
        };
        for bound in bounds
            .get(&(entry.kind, entry.name.as_str()))
            .into_iter()
            .flatten()
        {
            let Some(path) = slowest_path(&grid, &entry.ir, bound.from, bound.to) else {
                // Synth refuses a bound whose end does not depend on its start.
                continue;
            };
            if path.ticks > bound.bound {
                let diagnostic = failure(&scope, &entry.ir, bound, &path);
                if !diagnostics.contains(&diagnostic) {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }
    diagnostics
}

/// One `assert latency` line.
struct LatencyBound<'a> {
    from: &'a DottedRef,
    to: &'a DottedRef,
    bound: u32,
    span: &'a Span,
}

/// Each scope's `assert latency` lines.
fn latency_bounds(module: &IntentModule) -> HashMap<(ScopeKind, &str), Vec<LatencyBound<'_>>> {
    scope_asserts(module)
        .into_iter()
        .map(|(scope, asserts)| {
            let bounds = asserts
                .into_iter()
                .filter_map(|assertion| match assertion {
                    AssertIr::Latency {
                        from,
                        to,
                        bound,
                        span,
                    } => Some(LatencyBound {
                        from,
                        to,
                        bound: *bound,
                        span,
                    }),
                    _ => None,
                })
                .collect();
            (scope, bounds)
        })
        .collect()
}

/// Each scope's `assert` lines, those under its members included, keyed as a
/// [`ScopedCircuitLayout`] entry names its scope.
pub(crate) fn scope_asserts(module: &IntentModule) -> HashMap<(ScopeKind, &str), Vec<&AssertIr>> {
    fn collect<'a>(asserts: &'a [AssertIr], members: &'a [Member], out: &mut Vec<&'a AssertIr>) {
        out.extend(asserts);
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
    let mut out: HashMap<(ScopeKind, &str), Vec<&AssertIr>> = HashMap::new();
    for (kind, name, asserts, members) in scopes {
        collect(asserts, members, out.entry((kind, name)).or_default());
    }
    out
}

/// The slowest path a change takes from one net to where another is read, hop by hop.
struct Path {
    /// Every tick on the path, cells, buffer repeaters and a riser alike.
    ticks: u32,
    /// The buffer repeaters on its wire.
    buffers: u32,
    /// The cells it passes and the riser it ends at, in order.
    hops: Vec<Hop>,
}

/// One step of a [`Path`].
enum Hop {
    /// Into `tiles[tile]`, the netlist's cell `cell`, by `port`, over `buffers` repeaters, then
    /// `ticks` through the cell.
    Cell {
        tile: usize,
        port: PortName,
        buffers: u32,
        ticks: u32,
    },
    /// Into the riser `tiles[tile]` under an actuator reading `to`, over `buffers` repeaters.
    Riser { tile: usize, buffers: u32 },
}

/// When a change at a cell's output arrives, and the input it came in by.
#[derive(Clone, Copy)]
struct Arrival {
    ticks: u32,
    /// Index into the cell tile's `inputs`.
    via: usize,
}

/// The buffer repeaters a change passes on its way along `net` to the input at `at`.
fn buffers_to(grid: &Grid, layout: &CircuitLayout, net: usize, at: Voxel) -> u32 {
    let graph = NetGraph::of(grid, layout, net);
    let driver = &layout.tiles[layout.nets[net].driver];
    let template = driver.kind.template();
    let sources: Vec<(Voxel, u8)> = template
        .groups
        .iter()
        .flatten()
        .map(|rel| (driver.at(*rel), 15))
        .collect();
    let came = graph.reach(&sources).came;
    let buffers: Vec<Voxel> = layout.nets[net].wire.buffers.iter().map(|b| b.at).collect();
    let mut count = 0u32;
    let mut cursor = at;
    while let Some(&from) = came.get(&cursor) {
        if buffers.contains(&from) {
            count += 1;
        }
        cursor = from;
    }
    count
}

/// The slowest path from the net defining `from` to each riser under an actuator reading `to`,
/// or to the net defining `to` when no actuator reads it; `None` when either is undefined or no
/// path joins them.
fn slowest_path(
    grid: &Grid,
    layout: &CircuitLayout,
    from: &DottedRef,
    to: &DottedRef,
) -> Option<Path> {
    let start = *layout.signal_defs.get(from)?;
    let end = *layout.signal_defs.get(to)?;
    let cells: Vec<usize> = (0..layout.tiles.len())
        .filter(|&i| matches!(layout.tiles[i].kind, TileKind::Cell { .. }))
        .collect();
    let mut arrivals: Vec<Option<Arrival>> = Vec::with_capacity(cells.len());
    let arrival_at = |net: NetRef, arrivals: &[Option<Arrival>]| -> Option<u32> {
        if net == start {
            return Some(0);
        }
        match net {
            NetRef::Cell(j) => arrivals.get(j as usize).copied().flatten().map(|a| a.ticks),
            _ => None,
        }
    };
    // Cells are in topological order, so every driver's arrival is known before its cell's.
    for &tile in &cells {
        let placed = &layout.tiles[tile];
        let TileKind::Cell { cell } = placed.kind else {
            unreachable!("only cells are kept")
        };
        let mut best: Option<Arrival> = None;
        for (index, input) in placed.inputs.iter().enumerate() {
            let TilePort::Cell(port) = input.port else {
                continue;
            };
            let Some(at) = arrival_at(layout.nets[input.net].source, &arrivals) else {
                continue;
            };
            let buffers = sink_at(layout, tile, input.port)
                .map_or(0, |voxel| buffers_to(grid, layout, input.net, voxel));
            let ticks = at
                .saturating_add(buffers.saturating_mul(BUFFER_REPEATER_TICKS))
                .saturating_add(cell.port_delay_ticks(port));
            if best.is_none_or(|b| ticks > b.ticks) {
                best = Some(Arrival { ticks, via: index });
            }
        }
        arrivals.push(best);
    }

    let at_end = arrival_at(end, &arrivals)?;
    let riser = slowest_riser(grid, layout, end);
    let mut hops = Vec::new();
    let mut path_buffers = 0u32;
    if let Some((tile, buffers)) = riser {
        path_buffers = buffers;
        hops.push(Hop::Riser { tile, buffers });
    }
    let mut net = end;
    while net != start {
        let NetRef::Cell(j) = net else {
            unreachable!("only a cell's arrival is reached from a net other than the start")
        };
        let tile = cells[j as usize];
        let placed = &layout.tiles[tile];
        let via = arrivals[j as usize]
            .expect("a cell the path reaches arrived by one of its inputs")
            .via;
        let input = placed.inputs[via];
        let TilePort::Cell(port) = input.port else {
            unreachable!("a cell arrives by one of its operands")
        };
        let TileKind::Cell { cell } = placed.kind else {
            unreachable!("only cells are kept")
        };
        let buffers = sink_at(layout, tile, input.port)
            .map_or(0, |voxel| buffers_to(grid, layout, input.net, voxel));
        path_buffers = path_buffers.saturating_add(buffers);
        hops.push(Hop::Cell {
            tile,
            port,
            buffers,
            ticks: cell.port_delay_ticks(port),
        });
        net = layout.nets[input.net].source;
    }
    hops.reverse();

    let riser_ticks = riser.map_or(0, |(_, buffers)| {
        buffers
            .saturating_mul(BUFFER_REPEATER_TICKS)
            .saturating_add(RISER_TICKS)
    });
    Some(Path {
        ticks: at_end.saturating_add(riser_ticks),
        buffers: path_buffers,
        hops,
    })
}

/// The riser that `end` reaches over the most buffer repeaters, and how many.
fn slowest_riser(grid: &Grid, layout: &CircuitLayout, end: NetRef) -> Option<(usize, u32)> {
    let net = layout.nets.iter().position(|n| n.source == end)?;
    let mut riser: Option<(usize, u32)> = None;
    for sink in &layout.nets[net].sinks {
        if layout.tiles[sink.tile].kind != TileKind::Riser {
            continue;
        }
        let buffers = sink_at(layout, sink.tile, sink.port)
            .map_or(0, |voxel| buffers_to(grid, layout, net, voxel));
        if riser.is_none_or(|(_, slowest)| buffers > slowest) {
            riser = Some((sink.tile, buffers));
        }
    }
    riser
}

/// The voxel the input `port` of the layout's tile `tile` reads.
fn sink_at(layout: &CircuitLayout, tile: usize, port: TilePort) -> Option<Voxel> {
    let placed = &layout.tiles[tile];
    placed.kind.template().port(port).map(|p| placed.at(p.at))
}

fn plural(n: u32, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The `E_SIM_ASSERTION_FAILED` for `bound`, which `path` takes longer than.
fn failure(scope: &str, ir: &CircuitLayout, bound: &LatencyBound<'_>, path: &Path) -> Diagnostic {
    let riser = path.hops.iter().any(|hop| matches!(hop, Hop::Riser { .. }));
    let split = match (path.buffers.saturating_mul(BUFFER_REPEATER_TICKS), riser) {
        (0, false) => "all in its cells".to_owned(),
        (0, true) => format!("{RISER_TICKS} in the torches up to the actuator"),
        (n, false) => format!("{n} in buffer repeaters on the routed wire"),
        (n, true) => format!(
            "{n} in buffer repeaters on the routed wire and {RISER_TICKS} in the torches up to the actuator"
        ),
    };
    let primary = format!(
        "{scope} {text} fails on {edition}: its slowest path from {from} to {to} takes {ticks}, \
         {split}",
        text = latency_text(bound.from, bound.to, bound.bound),
        edition = ir.edition.as_str(),
        from = bound.from,
        to = bound.to,
        ticks = plural(path.ticks, "tick", "ticks"),
    );
    let mut diagnostic = Diagnostic::new(
        DiagnosticCode::SimAssertionFailed,
        bound.span.clone(),
        primary,
    );
    for (index, hop) in path.hops.iter().enumerate() {
        let (span, message) = match *hop {
            Hop::Cell {
                tile,
                port,
                buffers,
                ticks,
            } => {
                let node = &ir.tiles[tile];
                let TileKind::Cell { cell } = node.kind else {
                    unreachable!("a cell hop is at a cell")
                };
                let wire = match buffers {
                    0 => String::new(),
                    n => format!("{} on the wire, then ", repeaters(n)),
                };
                let message = format!(
                    "{lead} cell #{tile} (`{name}`) by port `{port}`: {wire}{through} through \
                     the cell",
                    lead = if index == 0 {
                        "the path enters"
                    } else {
                        "then"
                    },
                    name = cell.name(),
                    port = port.as_str(),
                    through = plural(ticks, "tick", "ticks"),
                );
                (node.span.clone(), message)
            }
            Hop::Riser { tile, buffers } => {
                let node = &ir.tiles[tile];
                let message = format!(
                    "{lead} the actuator reading {to}: {wire} on the wire to the torches under \
                     it, then {up} up to it",
                    lead = if index == 0 {
                        "the path ends at"
                    } else {
                        "then"
                    },
                    to = bound.to,
                    wire = match buffers {
                        0 => "no buffer repeater".to_owned(),
                        n => repeaters(n),
                    },
                    up = plural(RISER_TICKS, "tick", "ticks"),
                );
                (node.span.clone(), message)
            }
        };
        diagnostic = diagnostic.with_note(span, message);
    }
    let wire = match path.buffers {
        0 => "the wire carries no buffer repeater, so only fewer cells on the path take ticks off"
            .to_owned(),
        n => format!(
            "the wire carries {}, one wherever it runs past {DUST_ATTENUATION_LIMIT} blocks of \
             dust, so laying what reads a signal nearer what drives it needs fewer",
            repeaters(n),
        ),
    };
    diagnostic.with_footer(wire).with_footer(format!(
        "Fix: relax the bound to `<= {measured}`, or shorten the path; the logic is never \
         rewritten to meet a bound",
        measured = path.ticks,
    ))
}

/// `n buffer repeater(s)`.
fn repeaters(n: u32) -> String {
    plural(n, "buffer repeater", "buffer repeaters")
}
