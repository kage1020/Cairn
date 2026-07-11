//! Intent IR → Logic IR lowering.
//!
//! Walks each scope of an [`IntentModule`], collects sensor bindings as
//! [`InputPort`]s, actuator bindings as [`OutputPort`]s, and lowers every
//! `logic sig.X = <expr>` line into a topologically ordered DAG of
//! [`GateNode`]s. Diagnostics fire fail-loud per `spec/lint` §11.3: an
//! unbound signal, a duplicate driver, or a cycle stops the scope from
//! reaching the returned [`ScopedLogicIr`]; only warnings survive alongside
//! a well-formed IR.
//!
//! Common subexpression elimination is done inline while lowering — two
//! `logic` lines whose RHS builds the same gate over the same operands
//! collapse to one node. That keeps the follow-up placement / route PRs
//! from paying for structurally redundant fanout the source never asked
//! for.
//!
//! Detection contract:
//! - **Sensor**: any member whose surface `-> sig.X` tail parses to a
//!   [`ValueKind::DotRef`] with head `sig`. The current cairn-lang-core
//!   `PressurePlate` recognizer is the only sensor role wired up today;
//!   this pass is intentionally structural so a follow-up PR that adds
//!   `lever` / `button` / `observer` roles gets input ports for free.
//! - **Actuator**: any member carrying one of the argument keys listed in
//!   [`ACTUATOR_ARG_KEYS`] (`opened_by` / `powered_by` / `lit_by` /
//!   `fired_by`, per `spec/redstone` §14.2) whose value is `sig.X`.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::logic_ir::{
    GateKind, GateNode, InputPort, LogicIr, OutputPort, ScopeKind, ScopedLogicIr,
    ScopedLogicIrEntry, SignalRef,
};
use cairn_lang_core::ast::{DottedRef, Expr, ValueKind};
use cairn_lang_core::error::Span;
use cairn_lang_core::intent::{
    DefIr, IntentModule, LogicBinding, Member, MemberBody, SiteIr, StructIr,
};

/// Argument keys that the surface treats as actuator bindings (per
/// `spec/redstone` §14.2). Any member whose `intent_state` contains one of
/// these with a `sig.X` value is registered as an [`OutputPort`].
///
/// The list is deliberately closed rather than open — a typo like
/// `oepend_by=sig.X` should surface as `E_UNKNOWN_KEYWORD` on the arg from
/// the `check` pass (a future extension), not silently become a live
/// output port here.
const ACTUATOR_ARG_KEYS: &[&str] = &["opened_by", "powered_by", "lit_by", "fired_by"];

/// The head segment (`sig`) that identifies a dotted reference as a
/// redstone signal name, rather than a member id or a place ref.
const SIGNAL_HEAD: &str = "sig";

/// Successful synth output: the per-scope Logic IR plus any warnings
/// collected alongside it. Errors abort the containing scope's IR
/// construction and land in [`Vec<Diagnostic>`] instead.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SynthOutput {
    /// Per-scope Logic IR produced by the walk. Scopes that hit an error
    /// during construction are absent; their diagnostics still land in
    /// [`Self::diagnostics`].
    pub scoped: ScopedLogicIr,
    /// All findings collected across every scope, in source order. Both
    /// errors and warnings — the caller filters by severity when deciding
    /// exit status.
    pub diagnostics: Vec<Diagnostic>,
}

/// Lower an [`IntentModule`] to a [`SynthOutput`].
///
/// One [`LogicIr`] entry per struct / def / site whose body carries at
/// least one sensor, actuator, or `logic` binding. Scopes with none of
/// those are omitted so the JSON dump stays proportional to redstone
/// content.
pub fn synthesize(module: &IntentModule) -> SynthOutput {
    let mut out = SynthOutput::default();
    for s in &module.structs {
        lower_struct(s, &mut out);
    }
    for d in &module.defs {
        lower_def(d, &mut out);
    }
    for s in &module.sites {
        lower_site(s, &mut out);
    }
    out
}

fn lower_struct(s: &StructIr, out: &mut SynthOutput) {
    let mut collected = ScopeCollected::default();
    collect_body(&s.members, &s.logic, &mut collected);
    finish_scope(ScopeKind::Struct, &s.name, &collected, out);
}

fn lower_def(d: &DefIr, out: &mut SynthOutput) {
    let mut collected = ScopeCollected::default();
    collect_body(&d.members, &d.logic, &mut collected);
    finish_scope(ScopeKind::Def, &d.name, &collected, out);
}

fn lower_site(s: &SiteIr, out: &mut SynthOutput) {
    let mut collected = ScopeCollected::default();
    // A site body carries `place` / `connect` members which never emit
    // sensor bindings themselves; the walk still descends in case a future
    // extension lets a site host `pressure_plate` or `door` directly.
    collect_body(&s.placements, &s.logic, &mut collected);
    finish_scope(ScopeKind::Site, &s.name, &collected, out);
}

/// Per-scope working set built during the collection pass. Kept separate
/// from [`LogicIr`] because a mid-collection error must never surface a
/// half-built IR to the caller — the finish step decides whether the
/// working set becomes an IR or an error report.
#[derive(Debug, Default)]
struct ScopeCollected<'a> {
    /// Sensors, in source order.
    sensors: Vec<PendingSensor>,
    /// Actuators, in source order. Resolved to [`OutputPort`]s in the
    /// finish step so an undefined driver can be reported without
    /// pretending the actuator was wired.
    actuators: Vec<PendingActuator>,
    /// `logic sig.X = <expr>` lines, in source order.
    bindings: Vec<PendingBinding<'a>>,
}

#[derive(Debug)]
struct PendingSensor {
    name: DottedRef,
    span: Span,
}

#[derive(Debug)]
struct PendingActuator {
    /// The actuator argument that carried the `sig.X` value (`opened_by`,
    /// `powered_by`, ...). Preserved so a follow-up PR that renders
    /// actuator-specific messages has the key at hand without re-walking
    /// the member.
    #[allow(dead_code)]
    arg_key: &'static str,
    driver_name: DottedRef,
    span: Span,
}

#[derive(Debug)]
struct PendingBinding<'a> {
    lhs: &'a DottedRef,
    rhs: &'a Expr,
    span: Span,
}

fn collect_body<'a>(
    members: &'a [Member],
    logic: &'a [LogicBinding],
    out: &mut ScopeCollected<'a>,
) {
    for m in members {
        collect_member(m, out);
    }
    for b in logic {
        out.bindings.push(PendingBinding {
            lhs: &b.lhs,
            rhs: &b.rhs,
            span: b.span.clone(),
        });
    }
}

fn collect_member<'a>(m: &'a Member, out: &mut ScopeCollected<'a>) {
    // Sensor: `-> sig.X` tail on the surface line.
    if let Some(binding) = &m.binding
        && let ValueKind::DotRef(dr) = &binding.kind
        && dr.head() == SIGNAL_HEAD
    {
        out.sensors.push(PendingSensor {
            name: dr.clone(),
            span: binding.span.clone(),
        });
    }

    // Actuator: any `opened_by=` / `powered_by=` / ... arg whose value is
    // a `sig.X` dotted ref. The check is structural rather than keyed on
    // member role so a follow-up recognizer PR does not need a parallel
    // change here.
    for (key, vspan) in &m.intent_state.fields {
        let Some(target_key) = ACTUATOR_ARG_KEYS.iter().find(|k| **k == key.as_str()) else {
            continue;
        };
        if let ValueKind::DotRef(dr) = &vspan.value.kind
            && dr.head() == SIGNAL_HEAD
        {
            out.actuators.push(PendingActuator {
                arg_key: target_key,
                driver_name: dr.clone(),
                span: vspan.span.clone(),
            });
        }
    }

    // Nested body (e.g. `level y=0` block) — same triple of members / logic /
    // asserts as the top-level, so recurse. Asserts are collected but not
    // consumed by M6-PR1; their evaluation lands in a follow-up PR.
    let MemberBody {
        members: children,
        logic,
        asserts: _,
    } = &m.children;
    for child in children {
        collect_member(child, out);
    }
    for b in logic {
        out.bindings.push(PendingBinding {
            lhs: &b.lhs,
            rhs: &b.rhs,
            span: b.span.clone(),
        });
    }
}

/// Finalise one scope's collected state into a Logic IR (or a bag of
/// diagnostics if construction failed). The two paths share a scope name
/// prefix on their error message so a multi-scope dump still tells the
/// author which struct / def / site produced each finding.
///
/// The pipeline is dense but linear (sensor registration → duplicate
/// detection → sensor collision → DFS lowering → actuator resolution →
/// unused-signal audit) and each step operates on the same working set;
/// splitting into per-step free functions would multiply the shared
/// argument bag more than it would clarify the flow, so the length is
/// spent as one function and clippy's limit is opted out of at this
/// specific site.
#[allow(clippy::too_many_lines)]
fn finish_scope(kind: ScopeKind, name: &str, collected: &ScopeCollected<'_>, out: &mut SynthOutput) {
    if collected.sensors.is_empty()
        && collected.actuators.is_empty()
        && collected.bindings.is_empty()
    {
        return;
    }

    let mut ir = LogicIr::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    // Sensors → InputPorts, and register signal definitions.
    for sensor in &collected.sensors {
        // Duplicate sensor names across two `-> sig.X` sites is not a
        // synth error today — the reference resolution just picks the
        // first source. A follow-up PR can promote this to a
        // `W_LOGIC_DUPLICATE_SENSOR` once the surface diagnostic
        // vocabulary catches up.
        if ir.signal_defs.contains_key(&sensor.name) {
            continue;
        }
        let idx = u32::try_from(ir.inputs.len()).expect("input port index fits in u32");
        ir.inputs.push(InputPort {
            name: sensor.name.clone(),
            span: sensor.span.clone(),
        });
        ir.signal_defs
            .insert(sensor.name.clone(), SignalRef::Input(idx));
    }

    // Pre-index bindings by their LHS name for the DFS lowering. Duplicate
    // LHS across `logic` lines is `E_LOGIC_MULTIPLE_DRIVERS`; the first
    // definition wins so a downstream reference is still resolved.
    let mut bindings_by_lhs: HashMap<DottedRef, usize> = HashMap::new();
    for (idx, b) in collected.bindings.iter().enumerate() {
        if let Some(first_idx) = bindings_by_lhs.get(b.lhs) {
            let first = &collected.bindings[*first_idx];
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::LogicMultipleDrivers,
                    b.span.clone(),
                    format!(
                        "{scope_label} `logic {lhs} = ...` redefines a signal already driven earlier in this scope",
                        scope_label = scope_label(kind, name),
                        lhs = b.lhs,
                    ),
                )
                .with_note(first.span.clone(), "first declared here")
                .with_footer(
                    "Fix: rename this binding, or delete one of the two `logic` lines so the signal has a single driver.",
                ),
            );
            continue;
        }
        bindings_by_lhs.insert(b.lhs.clone(), idx);
    }

    // Sensor / binding LHS collision: a name driven by a sensor cannot
    // also be redefined by `logic`. Same code as the `logic` / `logic`
    // collision because the underlying constraint is the same — one
    // driver per signal.
    for b in &collected.bindings {
        if let Some(SignalRef::Input(input_idx)) = ir.signal_defs.get(b.lhs) {
            let sensor_span = ir.inputs[*input_idx as usize].span.clone();
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::LogicMultipleDrivers,
                    b.span.clone(),
                    format!(
                        "{scope_label} `logic {lhs} = ...` conflicts with a sensor already driving `{lhs}`",
                        scope_label = scope_label(kind, name),
                        lhs = b.lhs,
                    ),
                )
                .with_note(sensor_span, "sensor emits this signal here")
                .with_footer(
                    "Fix: rename the `logic` LHS, or remove the sensor's `-> sig.<name>` tail.",
                ),
            );
        }
    }

    // Lower each binding's RHS, memoising by (kind, inputs) for CSE. The
    // `bindings_by_lhs` map above ensures we look up the first driver only,
    // so a duplicate `logic` line does not re-enter the lowering.
    let mut ctx = LoweringCtx {
        kind,
        scope_name: name,
        bindings: &collected.bindings,
        bindings_by_lhs: &bindings_by_lhs,
        in_progress: HashMap::new(),
        failed_lhs: HashSet::new(),
        cse: HashMap::new(),
    };

    let ordered_lhs: Vec<&DottedRef> = collected
        .bindings
        .iter()
        .enumerate()
        .filter_map(|(idx, b)| {
            // Only visit the first binding per LHS (subsequent are already
            // flagged as multiple-drivers) and skip those that collide
            // with a sensor (already flagged just above).
            if bindings_by_lhs.get(b.lhs).copied() == Some(idx)
                && !matches!(ir.signal_defs.get(b.lhs), Some(SignalRef::Input(_)))
            {
                Some(b.lhs)
            } else {
                None
            }
        })
        .collect();

    for lhs in &ordered_lhs {
        // Skip if a previous DFS pass already lowered it as a transitive
        // dependency, or already failed once — either way we've already
        // emitted the definitive diagnostic and should not re-enter.
        if ir.signal_defs.contains_key(*lhs) || ctx.failed_lhs.contains(*lhs) {
            continue;
        }
        if let Err(err) = lower_binding(lhs, &mut ir, &mut ctx, &mut diagnostics) {
            // Stash for cascade suppression: any downstream RHS or
            // actuator referencing this LHS gets a silent skip so the
            // user sees the root cause once, not once per consumer.
            let _ = err;
            ctx.failed_lhs.insert((*lhs).clone());
        }
    }

    let failed_lhs = ctx.failed_lhs.clone();

    // Resolve actuators against the final signal_defs map. Absent = unbound,
    // unless the driver name was declared by a `logic` line whose lowering
    // already failed — in which case the root-cause diagnostic has already
    // been queued and a cascade would just double-report the same problem.
    for act in &collected.actuators {
        match ir.signal_defs.get(&act.driver_name).copied() {
            Some(driver) => ir.outputs.push(OutputPort {
                name: act.driver_name.clone(),
                driver,
                span: act.span.clone(),
            }),
            None if failed_lhs.contains(&act.driver_name) => {}
            None => diagnostics.push(unbound_signal_diagnostic(
                kind,
                name,
                &act.driver_name,
                act.span.clone(),
                &ir,
                &bindings_by_lhs,
                "actuator argument",
            )),
        }
    }

    // Usage tracking: any signal name that appears on the RHS of a `logic`
    // binding or on an actuator's driver argument is "consumed". A `logic`
    // LHS that never appears in that set is dead code. Purely syntactic so
    // it catches both gate-producing bindings and bare `logic sig.X =
    // sig.Y` aliases the CSE'd gate reachability walk would miss.
    let mut consumed: HashSet<DottedRef> = HashSet::new();
    for b in &collected.bindings {
        collect_refs(b.rhs, &mut consumed);
    }
    for act in &collected.actuators {
        consumed.insert(act.driver_name.clone());
    }
    for b in &collected.bindings {
        // Only flag the *first* driver of each LHS (subsequent are already
        // covered by `E_LOGIC_MULTIPLE_DRIVERS`) and only when the binding
        // itself lowered successfully — a failed lowering already surfaced
        // the actionable diagnostic.
        if bindings_by_lhs.get(b.lhs).copied() != Some(binding_index_of(&collected.bindings, b))
            || failed_lhs.contains(b.lhs)
        {
            continue;
        }
        if !consumed.contains(b.lhs) {
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::LogicUnusedSignal,
                    b.span.clone(),
                    format!(
                        "{scope_label} `logic {lhs} = ...` is defined but never consumed by an actuator or downstream logic",
                        scope_label = scope_label(kind, name),
                        lhs = b.lhs,
                    ),
                )
                .with_footer(
                    "Fix: wire an actuator to this signal, reference it from another `logic` binding, or remove the binding if it is dead code.",
                ),
            );
        }
    }

    let scope_has_error = diagnostics
        .iter()
        .any(|d| d.code.severity() == cairn_lang_core::check::Severity::Error);
    out.diagnostics.extend(diagnostics);
    if !scope_has_error {
        out.scoped.scopes.push(ScopedLogicIrEntry {
            kind,
            name: name.to_owned(),
            ir,
        });
    }
}

/// DFS lowering context for one scope. Held on a separate struct so the
/// recursive helpers can pass a single `&mut` instead of threading five
/// arguments.
struct LoweringCtx<'a> {
    kind: ScopeKind,
    scope_name: &'a str,
    bindings: &'a [PendingBinding<'a>],
    bindings_by_lhs: &'a HashMap<DottedRef, usize>,
    /// `sig.X` currently being lowered — presence during a lookup indicates
    /// a dependency cycle. Value = span of the outermost `logic` line in
    /// the chain, so the diagnostic can point at the culprit.
    in_progress: HashMap<DottedRef, Span>,
    /// `logic` LHS names whose lowering already failed. Every downstream
    /// reference (RHS reference or actuator resolution) checks this set
    /// before emitting a fresh `E_LOGIC_UNBOUND_SIGNAL` so the user sees
    /// the root cause once, not once per consumer.
    failed_lhs: HashSet<DottedRef>,
    /// Common-subexpression cache. Key is `(kind, inputs)`; value is the
    /// gate node index that already computes the tuple.
    cse: HashMap<(GateKind, Vec<SignalRef>), u32>,
}

/// Sentinel returned by [`lower_binding`] / [`lower_expr`] when a
/// diagnostic has already been queued and the caller should abandon the
/// current branch without pushing a placeholder. The specific error text
/// lives on `diagnostics`; this type just controls control flow.
struct LoweringFailed;

fn lower_binding<'a>(
    lhs: &'a DottedRef,
    ir: &mut LogicIr,
    ctx: &mut LoweringCtx<'a>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<SignalRef, LoweringFailed> {
    let Some(&binding_idx) = ctx.bindings_by_lhs.get(lhs) else {
        return Err(LoweringFailed);
    };
    let binding = &ctx.bindings[binding_idx];

    ctx.in_progress.insert(lhs.clone(), binding.span.clone());
    let result = lower_expr(binding.rhs, ir, ctx, diagnostics);
    ctx.in_progress.remove(lhs);

    match result {
        Ok(sig) => {
            ir.signal_defs.insert(lhs.clone(), sig);
            Ok(sig)
        }
        Err(err) => Err(err),
    }
}

fn lower_expr<'a>(
    expr: &'a Expr,
    ir: &mut LogicIr,
    ctx: &mut LoweringCtx<'a>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<SignalRef, LoweringFailed> {
    match expr {
        Expr::Ref(dr) => resolve_ref(dr, ir, ctx, diagnostics),
        Expr::And(a, b) => {
            let ai = lower_expr(a, ir, ctx, diagnostics)?;
            let bi = lower_expr(b, ir, ctx, diagnostics)?;
            Ok(intern_gate(
                ir,
                ctx,
                GateKind::And2,
                vec![ai, bi],
                Span::default(),
            ))
        }
        Expr::Or(a, b) => {
            let ai = lower_expr(a, ir, ctx, diagnostics)?;
            let bi = lower_expr(b, ir, ctx, diagnostics)?;
            Ok(intern_gate(
                ir,
                ctx,
                GateKind::Or2,
                vec![ai, bi],
                Span::default(),
            ))
        }
        Expr::Not(a) => {
            let ai = lower_expr(a, ir, ctx, diagnostics)?;
            Ok(intern_gate(
                ir,
                ctx,
                GateKind::Not,
                vec![ai],
                Span::default(),
            ))
        }
        // `Expr` is `#[non_exhaustive]`; the current parser produces only
        // the four variants above but a future extension (`xor(a, b)`,
        // `mux(sel=..., a=..., b=...)`, ...) will land as new variants.
        // Reject them with a targeted diagnostic here so a future PR
        // wiring in call syntax adds the lowering case in one place
        // instead of silently producing a wrong DAG.
        _ => {
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::LogicUnboundSignal,
                    Span::default(),
                    format!(
                        "{scope_label} logic expression uses a primitive kind the M6-PR1 combinational lowering does not yet support",
                        scope_label = scope_label(ctx.kind, ctx.scope_name),
                    ),
                )
                .with_footer(
                    "Fix: rewrite using `and` / `or` / `not`, or wait for a follow-up PR that lands the missing primitive.",
                ),
            );
            Err(LoweringFailed)
        }
    }
}

fn resolve_ref<'a>(
    dr: &'a DottedRef,
    ir: &mut LogicIr,
    ctx: &mut LoweringCtx<'a>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<SignalRef, LoweringFailed> {
    // Already defined (sensor input or previously lowered binding).
    if let Some(sig) = ir.signal_defs.get(dr).copied() {
        return Ok(sig);
    }

    // Cycle: this ref is currently mid-lowering.
    if let Some(seed_span) = ctx.in_progress.get(dr).cloned() {
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::LogicCycle,
                seed_span,
                format!(
                    "{scope_label} `logic {name} = ...` depends on itself through the signal graph",
                    scope_label = scope_label(ctx.kind, ctx.scope_name),
                    name = dr,
                ),
            )
            .with_footer(
                "Fix: break the cycle by adding a sensor driving one of the signals, or introduce a `latch(...)` macro (out of scope for M6-PR1 combinational synthesis).",
            ),
        );
        return Err(LoweringFailed);
    }

    // Root cause already surfaced — skip the cascade.
    if ctx.failed_lhs.contains(dr) {
        return Err(LoweringFailed);
    }

    // Defined by a downstream `logic` line: recurse.
    if ctx.bindings_by_lhs.contains_key(dr) {
        return match lower_binding(dr, ir, ctx, diagnostics) {
            Ok(sig) => Ok(sig),
            Err(err) => {
                ctx.failed_lhs.insert(dr.clone());
                Err(err)
            }
        };
    }

    // Unbound: no sensor, no `logic` line defines it.
    let scope_label_str = scope_label(ctx.kind, ctx.scope_name);
    let mut candidates: Vec<String> = ir
        .inputs
        .iter()
        .map(|p| p.name.to_string())
        .chain(
            ctx.bindings_by_lhs
                .keys()
                .map(std::string::ToString::to_string),
        )
        .collect();
    candidates.sort();
    candidates.dedup();
    let mut diag = Diagnostic::new(
        DiagnosticCode::LogicUnboundSignal,
        Span::default(),
        format!(
            "{scope_label_str} `{dr}` is referenced but not defined by any sensor or earlier `logic` line",
        ),
    );
    if candidates.is_empty() {
        diag = diag.with_footer(
            "Fix: add a sensor row such as `pressure_plate ... -> sig.<name>` or a `logic sig.<name> = ...` binding in this scope.",
        );
    } else {
        let joined = candidates.join(", ");
        diag = diag.with_footer(format!(
            "Valid signals in scope: {joined}. Fix: rename to a defined signal, or add a sensor / `logic` line driving `{dr}`.",
        ));
    }
    diagnostics.push(diag);
    Err(LoweringFailed)
}

/// Reuse a gate node whose `(kind, inputs)` tuple was already interned, or
/// push a fresh one. Returns the [`SignalRef::Gate`] handle either way so
/// the caller does not have to branch on the CSE cache.
fn intern_gate(
    ir: &mut LogicIr,
    ctx: &mut LoweringCtx<'_>,
    kind: GateKind,
    inputs: Vec<SignalRef>,
    span: Span,
) -> SignalRef {
    debug_assert_eq!(
        inputs.len(),
        kind.arity(),
        "gate arity mismatch: {kind:?} expected {} inputs, got {}",
        kind.arity(),
        inputs.len(),
    );
    let key = (kind, inputs.clone());
    if let Some(&idx) = ctx.cse.get(&key) {
        return SignalRef::Gate(idx);
    }
    let idx = u32::try_from(ir.nodes.len()).expect("gate node index fits in u32");
    ir.nodes.push(GateNode { kind, inputs, span });
    ctx.cse.insert(key, idx);
    SignalRef::Gate(idx)
}

fn unbound_signal_diagnostic(
    kind: ScopeKind,
    scope_name: &str,
    dr: &DottedRef,
    span: Span,
    ir: &LogicIr,
    bindings_by_lhs: &HashMap<DottedRef, usize>,
    source_label: &str,
) -> Diagnostic {
    let mut candidates: Vec<String> = ir
        .inputs
        .iter()
        .map(|p| p.name.to_string())
        .chain(bindings_by_lhs.keys().map(std::string::ToString::to_string))
        .collect();
    candidates.sort();
    candidates.dedup();
    let label = scope_label(kind, scope_name);
    let mut diag = Diagnostic::new(
        DiagnosticCode::LogicUnboundSignal,
        span,
        format!(
            "{label} {source_label} references `{dr}`, but no sensor emits it and no `logic` line defines it in this scope",
        ),
    );
    if candidates.is_empty() {
        diag = diag.with_footer(
            "Fix: add a sensor row such as `pressure_plate ... -> sig.<name>` or a `logic sig.<name> = ...` binding in this scope.",
        );
    } else {
        let joined = candidates.join(", ");
        diag = diag.with_footer(format!(
            "Valid signals in scope: {joined}. Fix: rename to a defined signal, or drive `{dr}` from a sensor / `logic` line.",
        ));
    }
    diag
}

/// Walk an `Expr` tree and add every `sig.X` reference it names into
/// `out`. Used by the unused-signal detection so a bare-ref binding
/// (`logic sig.a = sig.b`) still marks `sig.b` as consumed.
fn collect_refs(expr: &Expr, out: &mut HashSet<DottedRef>) {
    match expr {
        Expr::Ref(dr) => {
            out.insert(dr.clone());
        }
        Expr::And(a, b) | Expr::Or(a, b) => {
            collect_refs(a, out);
            collect_refs(b, out);
        }
        Expr::Not(a) => collect_refs(a, out),
        _ => {}
    }
}

/// Positional index of a specific `PendingBinding` inside the collected
/// slice. Used by the unused-signal detection to tell "is this the first
/// binding under this LHS" from "a duplicate driver flagged elsewhere"
/// without threading a per-binding tag through the collector.
fn binding_index_of(bindings: &[PendingBinding<'_>], target: &PendingBinding<'_>) -> usize {
    bindings
        .iter()
        .position(|b| std::ptr::eq(b, target))
        .expect("binding referenced from the same slice it came from")
}

/// Prefix every diagnostic's primary message with the scope kind + name so
/// a multi-scope module's findings read unambiguously in the CLI dump.
fn scope_label(kind: ScopeKind, name: &str) -> String {
    let label = match kind {
        ScopeKind::Struct => "struct",
        ScopeKind::Def => "def",
        ScopeKind::Site => "site",
    };
    format!("{label}={name}:")
}
