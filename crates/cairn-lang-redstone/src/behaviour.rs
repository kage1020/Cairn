//! `spec/redstone` "Verification": each `assert truth` and `assert always` against the legalized
//! circuit of its scope, run from rest through every combination of the sensors it depends on.

use std::collections::BTreeSet;

use cairn_lang_core::ast::{DottedRef, TruthRow};
use cairn_lang_core::error::Span;
use cairn_lang_core::intent::{AssertIr, IntentModule};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::geometry::Voxel;
use crate::legalize::{REST_TICKS, rest};
use crate::netlist_ir::NetRef;
use crate::placement_ir::{CircuitLayout, ScopedCircuitLayout, TileKind};
use crate::sim::Sim;
use crate::tile::PadKind;
use crate::verify::scope_asserts;

/// The most sensors an assertion's signals may depend on and still be checked, since a check runs
/// the circuit through every combination of them.
pub const MAX_DRIVEN_SENSORS: usize = 10;

/// An `E_SIM_ASSERTION_FAILED` for each `assert truth` and `assert always` that some run of its
/// scope's legalized circuit breaks, and a `W_SIM_ASSERTION_UNCHECKED` for each it cannot run.
#[must_use]
pub fn verify_behaviour(module: &IntentModule, legalized: &ScopedCircuitLayout) -> Vec<Diagnostic> {
    let asserts = scope_asserts(module);
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for entry in &legalized.scopes {
        let Some(rest) = rest(&entry.ir) else {
            // Legalization refuses a circuit that never comes to rest.
            continue;
        };
        let circuit = Circuit::of(
            &entry.ir,
            rest,
            format!("{}={}:", entry.kind.label(), entry.name),
        );
        for assertion in asserts
            .get(&(entry.kind, entry.name.as_str()))
            .into_iter()
            .flatten()
        {
            let found = match assertion {
                AssertIr::Truth {
                    inputs,
                    output,
                    rows,
                    span,
                } => circuit.truth(inputs, output, rows, span),
                AssertIr::Always {
                    antecedent,
                    consequent,
                    within,
                    span,
                } => circuit.always(antecedent, consequent, *within, span),
                _ => None,
            };
            if let Some(diagnostic) = found
                && !diagnostics.contains(&diagnostic)
            {
                diagnostics.push(diagnostic);
            }
        }
    }
    diagnostics
}

/// A legalized circuit, at rest, and where each of its signals is read.
struct Circuit<'a> {
    layout: &'a CircuitLayout,
    rest: Sim,
    /// `struct=name:`, the prefix of every finding.
    scope: String,
    /// Each cell's tile, in the netlist's order.
    cells: Vec<usize>,
    /// Each tile's output dust, where its output reads on while any of it is powered.
    outs: Vec<Vec<Voxel>>,
}

/// The circuit as it runs from the tick its sensors are switched on.
struct Run {
    sim: Sim,
    /// The inputs of the observers' pads, switched off after the first tick.
    pulses: Vec<Voxel>,
}

impl Run {
    fn tick(&mut self) {
        self.sim.step();
        for at in self.pulses.drain(..) {
            self.sim.set_input(at, 0);
        }
    }

    fn is_settled(&self) -> bool {
        self.pulses.is_empty() && self.sim.is_settled()
    }

    /// Runs until the circuit comes to rest, or [`REST_TICKS`] have run.
    fn settle(&mut self) {
        for _ in 0..REST_TICKS {
            if self.is_settled() {
                return;
            }
            self.tick();
        }
    }
}

impl<'a> Circuit<'a> {
    fn of(layout: &'a CircuitLayout, rest: Sim, scope: String) -> Self {
        let cells = (0..layout.tiles.len())
            .filter(|&i| matches!(layout.tiles[i].kind, TileKind::Cell { .. }))
            .collect();
        let outs = layout
            .tiles
            .iter()
            .map(|tile| {
                let template = tile.kind.template();
                template.outs.iter().map(|rel| tile.at(*rel)).collect()
            })
            .collect();
        Self {
            layout,
            rest,
            scope,
            cells,
            outs,
        }
    }

    /// The tiles of the pads `sensor`'s signal enters by.
    fn pads(&self, sensor: u32) -> impl Iterator<Item = usize> + '_ {
        (0..self.layout.tiles.len()).filter(move |&i| {
            let tile = &self.layout.tiles[i];
            matches!(tile.kind, TileKind::Pad { .. })
                && tile
                    .output
                    .is_some_and(|net| self.layout.nets[net].source == NetRef::Input(sensor))
        })
    }

    /// The signal the netlist's input `sensor` carries.
    fn sensor_name(&self, sensor: u32) -> String {
        self.layout
            .signal_defs
            .iter()
            .find_map(|(name, net)| (*net == NetRef::Input(sensor)).then(|| name.to_string()))
            .unwrap_or_default()
    }

    /// Every sensor `nets` depend on, in the netlist's order.
    fn sensors(&self, nets: &[NetRef]) -> Vec<u32> {
        let mut found = BTreeSet::new();
        let mut stack = nets.to_vec();
        let mut seen = BTreeSet::new();
        while let Some(net) = stack.pop() {
            match net {
                NetRef::Input(sensor) => {
                    found.insert(sensor);
                }
                NetRef::Cell(cell) => {
                    if !seen.insert(cell) {
                        continue;
                    }
                    let Some(&tile) = self.cells.get(cell as usize) else {
                        continue;
                    };
                    stack.extend(
                        self.layout.tiles[tile]
                            .inputs
                            .iter()
                            .map(|input| self.layout.nets[input.net].source),
                    );
                }
            }
        }
        found.into_iter().collect()
    }

    /// The circuit at tick 0 of the run that switches on `on`, every other sensor off.
    fn start(&self, on: &[u32]) -> Run {
        let mut run = Run {
            sim: self.rest.clone(),
            pulses: Vec::new(),
        };
        for &sensor in on {
            for pad in self.pads(sensor) {
                let tile = &self.layout.tiles[pad];
                let input = tile.origin.up();
                run.sim.set_input(input, 15);
                if tile.kind
                    == (TileKind::Pad {
                        pad: PadKind::Observer,
                    })
                {
                    run.pulses.push(input);
                }
            }
        }
        run
    }

    /// Whether `net` reads on where what drives it leaves it: a sensor's pad, or a cell's output.
    fn reads(&self, sim: &Sim, net: NetRef, on: &[u32]) -> bool {
        let tile = match net {
            NetRef::Input(sensor) => {
                let mut pads = self.pads(sensor).peekable();
                if pads.peek().is_none() {
                    return on.contains(&sensor);
                }
                return pads.any(|pad| self.out_on(sim, pad));
            }
            NetRef::Cell(cell) => self.cells.get(cell as usize).copied(),
        };
        tile.is_some_and(|tile| self.out_on(sim, tile))
    }

    fn out_on(&self, sim: &Sim, tile: usize) -> bool {
        self.outs[tile].iter().any(|at| sim.power(*at) > 0)
    }

    /// The risers under the actuators reading `net`.
    fn risers(&self, net: NetRef) -> Vec<usize> {
        self.layout
            .nets
            .iter()
            .filter(|circuit_net| circuit_net.source == net)
            .flat_map(|circuit_net| &circuit_net.sinks)
            .map(|sink| sink.tile)
            .filter(|&tile| self.layout.tiles[tile].kind == TileKind::Riser)
            .collect()
    }

    /// Whether `net` reads on at the top of every riser reading it, or where what drives it
    /// leaves it when no actuator reads it.
    fn reads_where_read(&self, sim: &Sim, net: NetRef, risers: &[usize], on: &[u32]) -> bool {
        if risers.is_empty() {
            return self.reads(sim, net, on);
        }
        risers.iter().all(|&riser| {
            let tile = &self.layout.tiles[riser];
            tile.kind
                .template()
                .probe
                .is_some_and(|probe| sim.is_on(tile.at(probe)))
        })
    }

    /// Every combination of `sensors` switched on, in counting order, the first sensor the
    /// highest bit, or the warning for an assertion that depends on too many of them.
    fn runs(&self, sensors: &[u32], text: &str, span: &Span) -> Result<Vec<Vec<u32>>, Diagnostic> {
        if sensors.len() > MAX_DRIVEN_SENSORS {
            return Err(Diagnostic::new(
                DiagnosticCode::SimAssertionUnchecked,
                span.clone(),
                format!(
                    "{scope} {text} is not checked: its signals depend on {count} sensors, more than the {MAX_DRIVEN_SENSORS} a check runs the circuit through every combination of",
                    scope = self.scope,
                    count = sensors.len(),
                ),
            )
            .with_footer(
                "Fix: split the assertion into several whose signals each depend on fewer sensors",
            ));
        }
        let count = sensors.len();
        Ok((0..1u32 << count)
            .map(|row| {
                sensors
                    .iter()
                    .enumerate()
                    .filter(|(bit, _)| row >> (count - 1 - bit) & 1 == 1)
                    .map(|(_, sensor)| *sensor)
                    .collect()
            })
            .collect())
    }

    /// `with sig.a and sig.b switched on`, or `with no sensor switched on`.
    fn switched(&self, on: &[u32]) -> String {
        let names: Vec<String> = on.iter().map(|&sensor| self.sensor_name(sensor)).collect();
        match names.as_slice() {
            [] => "with no sensor switched on".to_owned(),
            [one] => format!("with {one} switched on"),
            [rest @ .., last] => format!("with {} and {last} switched on", rest.join(", ")),
        }
    }

    fn truth(
        &self,
        inputs: &[DottedRef],
        output: &DottedRef,
        rows: &[TruthRow],
        span: &Span,
    ) -> Option<Diagnostic> {
        let input_nets: Vec<NetRef> = inputs
            .iter()
            .map(|input| self.layout.signal_defs.get(input).copied())
            .collect::<Option<_>>()?;
        let output_net = *self.layout.signal_defs.get(output)?;
        let text = truth_text(inputs, output);
        let mut nets = input_nets.clone();
        nets.push(output_net);
        let runs = match self.runs(&self.sensors(&nets), &text, span) {
            Ok(runs) => runs,
            Err(diagnostic) => return Some(diagnostic),
        };
        for on in runs {
            let mut run = self.start(&on);
            run.settle();
            let read: String = input_nets
                .iter()
                .map(|net| {
                    if self.reads(&run.sim, *net, &on) {
                        '1'
                    } else {
                        '0'
                    }
                })
                .collect();
            let out = self.reads(&run.sim, output_net, &on);
            let Some(row) = rows.iter().find(|row| {
                row.output.is_some_and(|expected| expected != out) && covers(&row.inputs, &read)
            }) else {
                continue;
            };
            let primary = format!(
                "{scope} {text} fails on {edition}: {switched}, the circuit rests with its inputs reading `{read}` and {output} reading {out}",
                scope = self.scope,
                edition = self.layout.edition.as_str(),
                switched = self.switched(&on),
                out = u8::from(out),
            );
            return Some(
                Diagnostic::new(DiagnosticCode::SimAssertionFailed, span.clone(), primary)
                    .with_note(
                        row.span.clone(),
                        format!("this row says {output} reads {}", u8::from(!out)),
                    )
                    .with_footer(format!(
                        "Fix: correct the row, or what drives {output}; the logic is never rewritten to meet an assertion"
                    )),
            );
        }
        None
    }

    fn always(
        &self,
        antecedent: &DottedRef,
        consequent: &DottedRef,
        within: u32,
        span: &Span,
    ) -> Option<Diagnostic> {
        let from = *self.layout.signal_defs.get(antecedent)?;
        let to = *self.layout.signal_defs.get(consequent)?;
        let text =
            format!("`assert always({antecedent} -> eventually {consequent} within {within})`");
        let risers = self.risers(to);
        let runs = match self.runs(&self.sensors(&[from, to]), &text, span) {
            Ok(runs) => runs,
            Err(diagnostic) => return Some(diagnostic),
        };
        for on in runs {
            let mut run = self.start(&on);
            let mut trace = vec![self.read_pair(&run.sim, from, to, &risers, &on)];
            for _ in 0..REST_TICKS {
                if run.is_settled() {
                    break;
                }
                run.tick();
                trace.push(self.read_pair(&run.sim, from, to, &risers, &on));
            }
            // Past the last tick nothing changes, so it stands for every tick after it.
            let last = trace.len() - 1;
            let window = usize::try_from(within).unwrap_or(usize::MAX);
            for (tick, &(cause, _)) in trace.iter().enumerate() {
                let end = tick.saturating_add(window).min(last);
                if !cause || trace[tick..=end].iter().any(|&(_, effect)| effect) {
                    continue;
                }
                let late = trace[end + 1..]
                    .iter()
                    .position(|&(_, effect)| effect)
                    .map(|offset| end + 1 + offset);
                return Some(self.always_failure(
                    &text,
                    (antecedent, consequent),
                    &on,
                    (tick, late),
                    &risers,
                    span,
                ));
            }
        }
        None
    }

    fn read_pair(
        &self,
        sim: &Sim,
        from: NetRef,
        to: NetRef,
        risers: &[usize],
        on: &[u32],
    ) -> (bool, bool) {
        (
            self.reads(sim, from, on),
            self.reads_where_read(sim, to, risers, on),
        )
    }

    fn always_failure(
        &self,
        text: &str,
        (antecedent, consequent): (&DottedRef, &DottedRef),
        on: &[u32],
        (tick, late): (usize, Option<usize>),
        risers: &[usize],
        span: &Span,
    ) -> Diagnostic {
        let then = match late {
            Some(at) => format!(
                "{consequent} not until tick {at}, {} later",
                ticks(at - tick)
            ),
            None => format!("{consequent} never does"),
        };
        let primary = format!(
            "{scope} {text} fails on {edition}: {switched}, {antecedent} reads on at tick {tick} and {then}",
            scope = self.scope,
            edition = self.layout.edition.as_str(),
            switched = self.switched(on),
        );
        let mut diagnostic =
            Diagnostic::new(DiagnosticCode::SimAssertionFailed, span.clone(), primary);
        if let Some(&riser) = risers.first() {
            diagnostic = diagnostic.with_note(
                self.layout.tiles[riser].span.clone(),
                format!("{consequent} is read at the torches under this actuator"),
            );
        }
        diagnostic.with_footer(match late {
            Some(at) => format!(
                "Fix: relax the bound to `within {}`, or shorten the path; the logic is never rewritten to meet an assertion",
                at - tick
            ),
            None => format!(
                "Fix: correct the assertion, or what drives {consequent}; the logic is never rewritten to meet an assertion"
            ),
        })
    }
}

/// Whether the pattern `row`, one `0`, `1` or `-` per input, covers the inputs `read`.
fn covers(row: &str, read: &str) -> bool {
    row.len() == read.len()
        && row
            .chars()
            .zip(read.chars())
            .all(|(want, got)| want == '-' || want == got)
}

/// `` `assert truth(sig.a, sig.b -> sig.o)` ``.
fn truth_text(inputs: &[DottedRef], output: &DottedRef) -> String {
    let inputs: Vec<String> = inputs.iter().map(ToString::to_string).collect();
    format!("`assert truth({} -> {output})`", inputs.join(", "))
}

fn ticks(n: usize) -> String {
    format!("{n} {}", if n == 1 { "tick" } else { "ticks" })
}
