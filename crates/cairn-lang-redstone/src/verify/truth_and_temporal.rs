//! `spec/redstone` "Verification": each `assert truth` and `assert always` against the legalized
//! circuit of its scope, run from rest through every combination of the sensors it depends on.

use std::collections::{BTreeSet, HashMap};

use cairn_lang_core::ast::{DottedRef, TruthRow};
use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::error::Span;
use cairn_lang_core::intent::{AssertIr, IntentModule, Member, MemberRole};

use crate::diagnostic::{COMPILER_BUG_FIX, Diagnostic, DiagnosticCode};
use crate::geometry::Voxel;
use crate::legalize::rest;
use crate::logic_ir::ScopeKind;
use crate::netlist_ir::NetRef;
use crate::placement::structures;
use crate::placement_ir::{CircuitLayout, LayoutStage, ScopedCircuitLayout, TileKind};
use crate::sim::Sim;
use crate::synth::signal_named_by;
use crate::tile::PadKind;

use super::scope_asserts;

/// The most sensors an assertion's signals may depend on and still be checked, since a check
/// runs every combination of them.
pub const MAX_ASSERTION_SENSORS: usize = 10;

// A run is a count with one bit for each sensor.
const _: () = assert!(MAX_ASSERTION_SENSORS < u32::BITS as usize);

/// An `E_SIM_ASSERTION_FAILED` for each `assert truth` and `assert always` a run of its scope's
/// legalized circuit breaks, and a `W_SIM_ASSERTION_UNCHECKED` for each no run judges, each
/// scope's in source order; `built` says which `def` a `place` builds.
///
/// # Panics
///
/// On a circuit legalization has not reached.
#[must_use]
pub fn verify_truth_and_temporal(
    legalized: &ScopedCircuitLayout,
    module: &IntentModule,
    built: &BlockArrayIr,
) -> Vec<Diagnostic> {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    for ((kind, name), asserts) in scope_asserts(module) {
        let asserts: Vec<Checked<'_>> = asserts.into_iter().filter_map(Checked::of).collect();
        if asserts.is_empty() {
            continue;
        }
        let scope = format!("{}={name}:", kind.label());
        let mut laid = legalized
            .scopes
            .iter()
            .filter(|entry| entry.kind == kind && entry.name == name)
            .peekable();
        if laid.peek().is_none() {
            // A def no `place` builds is `W_UNUSED_DEF`, and nothing of it is run.
            if kind != ScopeKind::Def || !structures(built, kind, name).is_empty() {
                diagnostics.extend(asserts.iter().map(|assertion| unlaid(&scope, assertion)));
            }
            continue;
        }
        let observers = observer_signals(module, kind, name);
        for entry in laid {
            assert_eq!(
                entry.ir.stage,
                LayoutStage::Legalized,
                "{scope} assertions are run on the circuit legalization produces",
            );
            let rest = rest(&entry.ir);
            debug_assert!(
                rest.is_some(),
                "legalization refuses a circuit that never comes to rest"
            );
            let Some(rest) = rest else {
                continue;
            };
            let circuit = Circuit::of(&entry.ir, rest, &scope, &observers);
            for &assertion in &asserts {
                if let Some(diagnostic) = circuit.check(assertion)
                    && !diagnostics.contains(&diagnostic)
                {
                    diagnostics.push(diagnostic);
                }
            }
        }
    }
    diagnostics
}

/// An `assert truth` or `assert always`, the two kinds a run of the circuit checks.
#[derive(Clone, Copy)]
enum Checked<'a> {
    Truth {
        inputs: &'a [DottedRef],
        output: &'a DottedRef,
        rows: &'a [TruthRow],
        span: &'a Span,
    },
    Always {
        antecedent: &'a DottedRef,
        consequent: &'a DottedRef,
        within: u32,
        span: &'a Span,
    },
}

impl<'a> Checked<'a> {
    fn of(assertion: &'a AssertIr) -> Option<Self> {
        match assertion {
            AssertIr::Truth {
                inputs,
                output,
                rows,
                span,
            } => Some(Self::Truth {
                inputs,
                output,
                rows,
                span,
            }),
            AssertIr::Always {
                antecedent,
                consequent,
                within,
                span,
            } => Some(Self::Always {
                antecedent,
                consequent,
                within: *within,
                span,
            }),
            _ => None,
        }
    }

    /// The assertion as a finding quotes it.
    fn text(self) -> String {
        match self {
            Self::Truth { inputs, output, .. } => truth_text(inputs, output),
            Self::Always {
                antecedent,
                consequent,
                within,
                ..
            } => always_text(antecedent, consequent, within),
        }
    }

    const fn span(self) -> &'a Span {
        match self {
            Self::Truth { span, .. } | Self::Always { span, .. } => span,
        }
    }
}

/// The `W_SIM_ASSERTION_UNCHECKED` for `assertion` in a scope that lays no circuit to run.
fn unlaid(scope: &str, assertion: &Checked<'_>) -> Diagnostic {
    Diagnostic::new(
        DiagnosticCode::SimAssertionUnchecked,
        assertion.span().clone(),
        format!(
            "{scope} {text} is not checked: the scope lays no circuit to run, its logic having no cell to build and no actuator to drive",
            text = assertion.text(),
        ),
    )
    .with_footer(
        "Fix: have an actuator read a signal the assertion names, so that the scope lays a circuit, or remove the assertion",
    )
}

/// The signals the `observer`s of a scope's body give, those under its members included.
fn observer_signals<'a>(
    module: &'a IntentModule,
    kind: ScopeKind,
    name: &str,
) -> Vec<&'a DottedRef> {
    fn collect<'a>(members: &'a [Member], out: &mut Vec<&'a DottedRef>) {
        for member in members {
            if member.role == MemberRole::Observer && member.selector.is_none() {
                out.extend(member.binding.as_ref().and_then(signal_named_by));
            }
            collect(&member.children.members, out);
        }
    }
    let members = match kind {
        ScopeKind::Struct => module
            .structs
            .iter()
            .find(|s| s.name == name)
            .map(|s| s.members.as_slice()),
        ScopeKind::Def => module
            .defs
            .iter()
            .find(|d| d.name == name)
            .map(|d| d.members.as_slice()),
        ScopeKind::Site => None,
    };
    let mut out = Vec::new();
    collect(members.unwrap_or_default(), &mut out);
    out
}

/// A legalized circuit, at rest, and where each of its signals is read.
struct Circuit<'a> {
    layout: &'a CircuitLayout,
    rest: Sim,
    /// `struct=name:` or `def=name:`, the prefix of every finding.
    scope: &'a str,
    /// Each cell's tile, in the netlist's order.
    cells: Vec<usize>,
    /// The tiles of the pads each sensor's signal enters by.
    pads: HashMap<u32, Vec<usize>>,
    /// The sensors that are observers.
    observers: BTreeSet<u32>,
    /// Each tile's output dust, where its output reads on while any of it is powered.
    outs: Vec<Vec<Voxel>>,
    /// The most ticks a run is given to come to rest.
    budget: u32,
}

/// The circuit as it runs from the tick its sensors are switched on.
struct Run {
    sim: Sim,
    /// The sensors on now.
    on: Vec<u32>,
    /// The observers switched on, each with the inputs of its pads, switched off after the first
    /// tick.
    pulses: Vec<(u32, Vec<Voxel>)>,
}

impl Run {
    fn tick(&mut self) {
        self.sim.step();
        for (sensor, inputs) in self.pulses.drain(..) {
            for at in inputs {
                self.sim.set_input(at, 0);
            }
            self.on.retain(|&on| on != sensor);
        }
    }

    fn is_settled(&self) -> bool {
        self.pulses.is_empty() && self.sim.is_settled()
    }

    /// Runs until the circuit comes to rest or `budget` ticks have run; whether it came to rest.
    fn settle(&mut self, budget: u32) -> bool {
        for _ in 0..budget {
            if self.is_settled() {
                return true;
            }
            self.tick();
        }
        self.is_settled()
    }
}

/// Where the consequent of an `always` is read.
#[derive(Clone, Copy)]
enum Point {
    /// The upper torch of the riser `tiles[riser]`, at `torch`.
    Riser { riser: usize, torch: Option<Voxel> },
    /// Where what drives the net leaves it.
    Driven(NetRef),
}

/// When the consequent of an `always` follows one tick its antecedent reads on in a run.
struct Follow {
    /// The sensors the run switched on.
    on: Vec<u32>,
    tick: usize,
    /// The first tick from `tick` on which the consequent reads on at `point`, the point of the
    /// run it reaches last; `None` when it never does there.
    at: Option<usize>,
    point: usize,
}

impl Follow {
    /// Whether it follows later than `other`, never being the latest of all.
    fn is_later_than(&self, other: &Self) -> bool {
        later(
            self.at.map(|at| at - self.tick),
            other.at.map(|at| at - other.tick),
        )
    }
}

/// Whether the tick `a` is later than `b`, `None` being never.
const fn later(a: Option<usize>, b: Option<usize>) -> bool {
    match (a, b) {
        (None, Some(_)) => true,
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

/// What the antecedent and the consequent at each of its points read, tick by tick.
type Trace = Vec<(bool, Vec<bool>)>;

impl<'a> Circuit<'a> {
    fn of(layout: &'a CircuitLayout, rest: Sim, scope: &'a str, observers: &[&DottedRef]) -> Self {
        let cells = layout.cell_tiles().map(|(index, _)| index).collect();
        let mut pads: HashMap<u32, Vec<usize>> = HashMap::new();
        for (index, tile) in layout.tiles.iter().enumerate() {
            if let (TileKind::Pad { .. }, Some(net)) = (tile.kind, tile.output)
                && let NetRef::Input(sensor) = layout.nets[net].source
            {
                pads.entry(sensor).or_default().push(index);
            }
        }
        let observers: BTreeSet<u32> = observers
            .iter()
            .filter_map(|name| match layout.signal_defs.get(*name) {
                Some(&NetRef::Input(sensor)) => Some(sensor),
                _ => None,
            })
            .collect();
        debug_assert!(
            pads.iter().all(|(sensor, tiles)| {
                tiles.iter().all(|&tile| {
                    let pulses = layout.tiles[tile].kind
                        == (TileKind::Pad {
                            pad: PadKind::Observer,
                        });
                    pulses == observers.contains(sensor)
                })
            }),
            "an observer's signal enters by an observer's pad, and no other sensor's does",
        );
        let outs = layout
            .tiles
            .iter()
            .map(|tile| {
                let template = tile.kind.template();
                template.outs.iter().map(|rel| tile.at(*rel)).collect()
            })
            .collect();
        let budget = rest.rest_ticks();
        Self {
            layout,
            rest,
            scope,
            cells,
            pads,
            observers,
            outs,
            budget,
        }
    }

    /// The finding `assertion` earns on this circuit, if any.
    fn check(&self, assertion: Checked<'_>) -> Option<Diagnostic> {
        match assertion {
            Checked::Truth {
                inputs,
                output,
                rows,
                span,
            } => self.truth(inputs, output, rows, span),
            Checked::Always {
                antecedent,
                consequent,
                within,
                span,
            } => self.always(antecedent, consequent, within, span),
        }
    }

    /// The net `signal` names.
    fn net(&self, signal: &DottedRef) -> Option<NetRef> {
        let net = self.layout.signal_defs.get(signal).copied();
        debug_assert!(
            net.is_some(),
            "synth refuses an assertion naming a signal nothing in its scope drives",
        );
        net
    }

    /// The signal the netlist's input `sensor` carries.
    fn sensor_name(&self, sensor: u32) -> String {
        let name = self
            .layout
            .signal_defs
            .iter()
            .find_map(|(name, net)| (*net == NetRef::Input(sensor)).then(|| name.to_string()));
        debug_assert!(name.is_some(), "a sensor carries the signal it is bound to");
        name.unwrap_or_default()
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
                    let tile = self.cells.get(cell as usize);
                    debug_assert!(tile.is_some(), "every cell of the netlist has a tile");
                    let Some(&tile) = tile else {
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
            on: on.to_vec(),
            pulses: Vec::new(),
        };
        for &sensor in on {
            let inputs: Vec<Voxel> = self
                .pads
                .get(&sensor)
                .into_iter()
                .flatten()
                .map(|&pad| self.layout.tiles[pad].origin.up())
                .collect();
            for &at in &inputs {
                run.sim.set_input(at, 15);
            }
            if self.observers.contains(&sensor) {
                run.pulses.push((sensor, inputs));
            }
        }
        run
    }

    /// Whether `net` reads on where what drives it leaves it: a sensor's pad, or a cell's output.
    fn reads(&self, run: &Run, net: NetRef) -> bool {
        match net {
            NetRef::Input(sensor) => match self.pads.get(&sensor) {
                Some(pads) => pads.iter().any(|&pad| self.out_on(&run.sim, pad)),
                // Nothing in the circuit reads it, so it has no pad and reads as the run sets it.
                None => run.on.contains(&sensor),
            },
            NetRef::Cell(cell) => {
                let tile = self.cells.get(cell as usize);
                debug_assert!(tile.is_some(), "every cell of the netlist has a tile");
                tile.is_some_and(|&tile| self.out_on(&run.sim, tile))
            }
        }
    }

    fn out_on(&self, sim: &Sim, tile: usize) -> bool {
        self.outs[tile].iter().any(|at| sim.power(*at) > 0)
    }

    /// The upper torch of each riser under an actuator reading `signal` by that name, or where
    /// `net` is driven when no actuator reads it.
    fn points(&self, signal: &DottedRef, net: NetRef) -> Vec<Point> {
        let risers: Vec<Point> = self
            .layout
            .riser_signals
            .iter()
            .filter(|(_, read)| *read == signal)
            .map(|(&riser, _)| {
                let tile = &self.layout.tiles[riser];
                let torch = tile.kind.template().probe.map(|probe| tile.at(probe));
                debug_assert!(torch.is_some(), "a riser reads out at its upper torch");
                Point::Riser { riser, torch }
            })
            .collect();
        if risers.is_empty() {
            vec![Point::Driven(net)]
        } else {
            risers
        }
    }

    fn reads_at(&self, run: &Run, point: Point) -> bool {
        match point {
            Point::Riser { torch, .. } => torch.is_some_and(|at| run.sim.is_on(at)),
            Point::Driven(net) => self.reads(run, net),
        }
    }

    /// Every combination of `sensors` switched on, in counting order, the first sensor the
    /// highest bit, or the warning for an assertion that depends on too many of them.
    fn runs(&self, sensors: &[u32], text: &str, span: &Span) -> Result<Vec<Vec<u32>>, Diagnostic> {
        if sensors.len() > MAX_ASSERTION_SENSORS {
            return Err(Diagnostic::new(
                DiagnosticCode::SimAssertionUnchecked,
                span.clone(),
                format!(
                    "{scope} {text} is not checked: its signals depend on {count} sensors, and a check runs every combination of at most {MAX_ASSERTION_SENSORS}",
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
            .map(|input| self.net(input))
            .collect::<Option<_>>()?;
        let output_net = self.net(output)?;
        let text = truth_text(inputs, output);
        let mut nets = input_nets.clone();
        nets.push(output_net);
        let runs = match self.runs(&self.sensors(&nets), &text, span) {
            Ok(runs) => runs,
            Err(diagnostic) => return Some(diagnostic),
        };
        let mut reached: BTreeSet<String> = BTreeSet::new();
        let mut unsettled: Option<Vec<u32>> = None;
        for on in runs {
            let mut run = self.start(&on);
            if !run.settle(self.budget) {
                unsettled.get_or_insert(on);
                continue;
            }
            let read: String = input_nets
                .iter()
                .map(|net| if self.reads(&run, *net) { '1' } else { '0' })
                .collect();
            let out = self.reads(&run, output_net);
            if let Some(row) = rows.iter().find(|row| {
                row.output.is_some_and(|expected| expected != out) && covers(&row.inputs, &read)
            }) {
                return Some(self.truth_failure(&text, output, &on, (&read, out), row, span));
            }
            reached.insert(read);
        }
        if let Some(on) = unsettled {
            return Some(self.unsettled(&text, &on, span));
        }
        let unreached: Vec<&TruthRow> = rows
            .iter()
            .filter(|row| {
                row.output.is_some() && !reached.iter().any(|read| covers(&row.inputs, read))
            })
            .collect();
        if unreached.is_empty() {
            return None;
        }
        Some(self.unreached(&text, &unreached, &self.sensors(&input_nets), span))
    }

    fn truth_failure(
        &self,
        text: &str,
        output: &DottedRef,
        on: &[u32],
        (read, out): (&str, bool),
        row: &TruthRow,
        span: &Span,
    ) -> Diagnostic {
        let primary = format!(
            "{scope} {text} fails on {edition}: {switched}, the circuit rests with its inputs reading `{read}` and {output} reading {out}",
            scope = self.scope,
            edition = self.layout.edition.as_str(),
            switched = self.switched(on),
            out = u8::from(out),
        );
        Diagnostic::new(DiagnosticCode::SimAssertionFailed, span.clone(), primary)
            .with_note(
                row.span.clone(),
                format!("this row says {output} reads {}", u8::from(!out)),
            )
            .with_footer(format!(
                "Fix: correct the row, or what drives {output}; the logic is never rewritten to meet an assertion"
            ))
    }

    /// The `W_SIM_ASSERTION_UNCHECKED` for a truth table every run meets with `rows` no run
    /// reaches, `sensors` those its inputs depend on.
    fn unreached(
        &self,
        text: &str,
        rows: &[&TruthRow],
        sensors: &[u32],
        span: &Span,
    ) -> Diagnostic {
        let primary = match rows.len() {
            1 => format!(
                "{} {text} has a row no run reaches, which is not checked",
                self.scope
            ),
            n => format!(
                "{} {text} has {n} rows no run reaches, which are not checked",
                self.scope
            ),
        };
        let mut diagnostic =
            Diagnostic::new(DiagnosticCode::SimAssertionUnchecked, span.clone(), primary);
        for row in rows {
            diagnostic = diagnostic.with_note(
                row.span.clone(),
                format!(
                    "no run rests with the inputs reading what `{}` matches",
                    row.inputs
                ),
            );
        }
        for &sensor in sensors.iter().filter(|s| self.observers.contains(s)) {
            let pulse = format!(
                "{} is on only for the tick of this observer's pulse, so no run rests with it on",
                self.sensor_name(sensor),
            );
            diagnostic = match self.pads.get(&sensor).and_then(|pads| pads.first()) {
                Some(&pad) => diagnostic.with_note(self.layout.tiles[pad].span.clone(), pulse),
                None => diagnostic.with_footer(pulse),
            };
        }
        diagnostic.with_footer(
            "Fix: remove the rows no run reaches, or assert over inputs the runs switch to them",
        )
    }

    /// The `W_SIM_ASSERTION_UNCHECKED` for an assertion no run breaks while the run that
    /// switches on `on` is still changing when its ticks run out.
    fn unsettled(&self, text: &str, on: &[u32], span: &Span) -> Diagnostic {
        Diagnostic::new(
            DiagnosticCode::SimAssertionUnchecked,
            span.clone(),
            format!(
                "{scope} {text} is not checked on {edition}: {switched}, the circuit is still changing after {ticks}, the most a run is given",
                scope = self.scope,
                edition = self.layout.edition.as_str(),
                switched = self.switched(on),
                ticks = ticks(usize::try_from(self.budget).unwrap_or(usize::MAX)),
            ),
        )
        .with_footer(COMPILER_BUG_FIX)
    }

    fn always(
        &self,
        antecedent: &DottedRef,
        consequent: &DottedRef,
        within: u32,
        span: &Span,
    ) -> Option<Diagnostic> {
        let from = self.net(antecedent)?;
        let to = self.net(consequent)?;
        let text = always_text(antecedent, consequent, within);
        let points = self.points(consequent, to);
        let runs = match self.runs(&self.sensors(&[from, to]), &text, span) {
            Ok(runs) => runs,
            Err(diagnostic) => return Some(diagnostic),
        };
        let window = usize::try_from(within).unwrap_or(usize::MAX);
        let mut latest: Option<Follow> = None;
        let mut unsettled: Option<Vec<u32>> = None;
        for on in runs {
            let Some(trace) = self.trace(&on, from, &points) else {
                unsettled.get_or_insert(on);
                continue;
            };
            let Some(follow) = slowest_follow(&trace, window) else {
                continue;
            };
            let follow = Follow { on, ..follow };
            if latest
                .as_ref()
                .is_none_or(|latest| follow.is_later_than(latest))
            {
                latest = Some(follow);
            }
        }
        if let Some(follow) = latest {
            return Some(self.always_failure(
                &text,
                (antecedent, consequent),
                &follow,
                &points,
                span,
            ));
        }
        unsettled.map(|on| self.unsettled(&text, &on, span))
    }

    /// What `from` and the consequent at each of `points` read from tick 0 of the run that
    /// switches on `on` until the circuit rests; `None` when it is still changing once its ticks
    /// run out.
    fn trace(&self, on: &[u32], from: NetRef, points: &[Point]) -> Option<Trace> {
        let mut run = self.start(on);
        let read = |run: &Run| {
            (
                self.reads(run, from),
                points
                    .iter()
                    .map(|&point| self.reads_at(run, point))
                    .collect(),
            )
        };
        let mut trace = vec![read(&run)];
        for _ in 0..self.budget {
            if run.is_settled() {
                return Some(trace);
            }
            run.tick();
            trace.push(read(&run));
        }
        run.is_settled().then_some(trace)
    }

    fn always_failure(
        &self,
        text: &str,
        (antecedent, consequent): (&DottedRef, &DottedRef),
        follow: &Follow,
        points: &[Point],
        span: &Span,
    ) -> Diagnostic {
        let tick = follow.tick;
        let then = match follow.at {
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
            switched = self.switched(&follow.on),
        );
        let mut diagnostic =
            Diagnostic::new(DiagnosticCode::SimAssertionFailed, span.clone(), primary);
        if let Point::Riser { riser, .. } = points[follow.point] {
            let message = match points.len() {
                1 => format!("{consequent} is read at the torches under this actuator"),
                n => format!(
                    "{consequent} is read at the torches under this actuator, the slowest of the {n} that read it"
                ),
            };
            diagnostic = diagnostic.with_note(self.layout.tiles[riser].span.clone(), message);
        }
        diagnostic.with_footer(match follow.at {
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

/// In one run's `trace`, the tick the antecedent reads on that the consequent follows latest,
/// later than `window` ticks, at the point it reaches last; `None` when it follows every such tick
/// in time. Its `on` is left for the caller.
fn slowest_follow(trace: &Trace, window: usize) -> Option<Follow> {
    let points = trace.first().map_or(0, |(_, effects)| effects.len());
    // The first tick from each tick on which each point reads on; past the last tick nothing
    // changes, so a point off then stays off.
    let next: Vec<Vec<Option<usize>>> = (0..points)
        .map(|point| {
            let mut next = vec![None; trace.len()];
            let mut seen = None;
            for tick in (0..trace.len()).rev() {
                if trace[tick].1[point] {
                    seen = Some(tick);
                }
                next[tick] = seen;
            }
            next
        })
        .collect();
    let mut slowest: Option<Follow> = None;
    for (tick, &(cause, _)) in trace.iter().enumerate() {
        if !cause {
            continue;
        }
        let mut follow = Follow {
            on: Vec::new(),
            tick,
            at: next.first().and_then(|next| next[tick]),
            point: 0,
        };
        for (point, next) in next.iter().enumerate().skip(1) {
            if later(next[tick], follow.at) {
                follow.at = next[tick];
                follow.point = point;
            }
        }
        let late = follow.at.is_none_or(|at| at - tick > window);
        if late
            && slowest
                .as_ref()
                .is_none_or(|slowest| follow.is_later_than(slowest))
        {
            slowest = Some(follow);
        }
    }
    slowest
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

/// `` `assert always(sig.a -> eventually sig.b within 3)` ``.
fn always_text(antecedent: &DottedRef, consequent: &DottedRef, within: u32) -> String {
    format!("`assert always({antecedent} -> eventually {consequent} within {within})`")
}

fn ticks(n: usize) -> String {
    format!("{n} {}", if n == 1 { "tick" } else { "ticks" })
}

#[cfg(test)]
mod tests;
