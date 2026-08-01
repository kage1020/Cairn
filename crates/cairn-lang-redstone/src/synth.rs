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
//! collapse to one node. Symmetric gates (`and`/`or`/`xor`/`nand`/`nor`)
//! canonicalise operand order before the CSE lookup so
//! `sig.a or sig.b` and `sig.b or sig.a` share a node too.
//!
//! Detection contract:
//! - **Sensor**: any member whose surface `-> sig.X` tail parses to a
//!   [`ValueKind::DotRef`] with head `sig`. The current cairn-lang-core
//!   `PressurePlate` recognizer is the only sensor role wired up today;
//!   this pass is intentionally structural so a follow-up recognizer for
//!   `lever` / `button` / `observer` gets input ports for free.
//! - **Actuator**: any member carrying one of the argument keys listed in
//!   [`ACTUATOR_ARG_KEYS`] (`opened_by` / `powered_by` / `lit_by` /
//!   `fired_by`, per `spec/redstone` §14.2) whose value is `sig.X`.
//!
//! Cascade suppression: any signal name that has already produced an
//! `E_LOGIC_UNBOUND_SIGNAL` (either because a `logic` binding for it
//! failed to lower or because a raw reference to it was unresolved) is
//! recorded in `failed_lhs`. Every downstream reference — RHS lookups,
//! actuator resolution — checks the set before emitting another
//! diagnostic, so a single root cause produces exactly one finding.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::logic_ir::{
    GateKind, GateNode, InputPort, LogicIr, OutputPort, ScopeKind, ScopedLogicIr,
    ScopedLogicIrEntry, SignalRef,
};
use cairn_lang_core::ast::{DottedRef, Expr, ValueKind};
use cairn_lang_core::check::Severity;
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

/// Successful synth output: the per-scope Logic IR plus every diagnostic
/// collected across the module. Errors abort the containing scope's IR
/// construction; the scope simply does not appear in [`Self::scoped`].
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
    // member role so a follow-up recognizer does not need a parallel
    // change here.
    for (key, vspan) in &m.intent_state.fields {
        if !ACTUATOR_ARG_KEYS.contains(&key.as_str()) {
            continue;
        }
        if let ValueKind::DotRef(dr) = &vspan.value.kind
            && dr.head() == SIGNAL_HEAD
        {
            out.actuators.push(PendingActuator {
                driver_name: dr.clone(),
                span: vspan.span.clone(),
            });
        }
    }

    // Nested body (e.g. `level y=0` block) — same triple of members /
    // logic / asserts as the top-level, so recurse. Asserts are collected
    // but not consumed by the combinational lowering; their evaluation
    // lands on a later pass.
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

/// Finalise one scope's collected state. Success writes a
/// [`ScopedLogicIrEntry`] onto `out.scoped`; every finding — including
/// warnings that survive alongside a well-formed IR — appends to
/// `out.diagnostics`. Each step below owns one specific transformation of
/// the working set so the top-level flow reads as a linear pipeline.
fn finish_scope(
    kind: ScopeKind,
    name: &str,
    collected: &ScopeCollected<'_>,
    out: &mut SynthOutput,
) {
    if collected.sensors.is_empty()
        && collected.actuators.is_empty()
        && collected.bindings.is_empty()
    {
        return;
    }

    let mut ir = LogicIr::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut multiple_drivers_reported: HashSet<DottedRef> = HashSet::new();

    register_sensors(&collected.sensors, &mut ir);

    let bindings_by_lhs = build_binding_index(
        &collected.bindings,
        kind,
        name,
        &mut diagnostics,
        &mut multiple_drivers_reported,
    );

    report_sensor_binding_collisions(
        &collected.bindings,
        &ir,
        kind,
        name,
        &mut diagnostics,
        &mut multiple_drivers_reported,
    );

    let failed_lhs = lower_all_bindings(
        &collected.bindings,
        &bindings_by_lhs,
        kind,
        name,
        &mut ir,
        &mut diagnostics,
    );

    resolve_actuators(
        &collected.actuators,
        &bindings_by_lhs,
        &failed_lhs,
        kind,
        name,
        &mut ir,
        &mut diagnostics,
    );

    audit_unused_signals(
        &collected.bindings,
        &collected.actuators,
        &bindings_by_lhs,
        &failed_lhs,
        kind,
        name,
        &mut diagnostics,
    );

    let scope_has_error = diagnostics.iter().any(|d| d.severity == Severity::Error);
    out.diagnostics.extend(diagnostics);
    if !scope_has_error {
        out.scoped.scopes.push(ScopedLogicIrEntry {
            kind,
            name: name.to_owned(),
            ir,
        });
    }
}

/// Register each sensor's `-> sig.X` binding as an [`InputPort`] and its
/// `signal_defs` entry. A repeated sensor name across two `-> sig.X`
/// sources currently silently prefers the first; the reference-resolution
/// path keeps working and a `W_LOGIC_DUPLICATE_SENSOR` code can promote
/// this later once the surface diagnostic vocabulary catches up.
fn register_sensors(sensors: &[PendingSensor], ir: &mut LogicIr) {
    for sensor in sensors {
        if ir.signal_defs.contains_key(&sensor.name) {
            continue;
        }
        let idx = safe_index(ir.inputs.len());
        ir.inputs.push(InputPort {
            name: sensor.name.clone(),
            span: sensor.span.clone(),
        });
        ir.signal_defs
            .insert(sensor.name.clone(), SignalRef::Input(idx));
    }
}

/// Pre-index bindings by their LHS name so DFS lowering can look up a
/// single first-declared driver per name. Extra `logic` lines with an
/// already-taken LHS get a `E_LOGIC_MULTIPLE_DRIVERS` note pointing back
/// at the winner; the culprit LHS is recorded in `reported` so a later
/// sensor collision on the same name does not double-report.
fn build_binding_index(
    bindings: &[PendingBinding<'_>],
    kind: ScopeKind,
    scope_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
    reported: &mut HashSet<DottedRef>,
) -> HashMap<DottedRef, usize> {
    let mut by_lhs: HashMap<DottedRef, usize> = HashMap::new();
    for (idx, b) in bindings.iter().enumerate() {
        if let Some(first_idx) = by_lhs.get(b.lhs) {
            if reported.insert(b.lhs.clone()) {
                let first = &bindings[*first_idx];
                let label = scope_label(kind, scope_name);
                diagnostics.push(
                    Diagnostic::new(
                        DiagnosticCode::LogicMultipleDrivers,
                        b.span.clone(),
                        format!(
                            "{label} `logic {lhs} = ...` redefines a signal already driven earlier in this scope",
                            lhs = b.lhs,
                        ),
                    )
                    .with_note(first.span.clone(), "first declared here")
                    .with_footer(
                        "Fix: rename this binding, or delete one of the `logic` lines so the signal has a single driver.",
                    ),
                );
            }
            continue;
        }
        by_lhs.insert(b.lhs.clone(), idx);
    }
    by_lhs
}

/// Emit an `E_LOGIC_MULTIPLE_DRIVERS` for each `logic sig.X = ...` whose
/// LHS a sensor already emits. The `reported` set makes sure a given LHS
/// gets flagged at most once even if it collides both with a sensor and
/// with an earlier `logic` line.
fn report_sensor_binding_collisions(
    bindings: &[PendingBinding<'_>],
    ir: &LogicIr,
    kind: ScopeKind,
    scope_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
    reported: &mut HashSet<DottedRef>,
) {
    for b in bindings {
        let Some(SignalRef::Input(input_idx)) = ir.signal_defs.get(b.lhs) else {
            continue;
        };
        if !reported.insert(b.lhs.clone()) {
            continue;
        }
        let sensor_span = ir.inputs[*input_idx as usize].span.clone();
        let label = scope_label(kind, scope_name);
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::LogicMultipleDrivers,
                b.span.clone(),
                format!(
                    "{label} `logic {lhs} = ...` conflicts with a sensor already driving `{lhs}`",
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

/// Lower each first-declared binding's RHS into the DAG. Bindings whose
/// LHS collides with a sensor are already flagged and skipped so the
/// sensor's `SignalRef` remains the sole definition. Returns the set of
/// LHS names whose lowering failed — the actuator resolution and unused
/// audit consult it to suppress cascaded diagnostics.
fn lower_all_bindings<'a>(
    bindings: &'a [PendingBinding<'a>],
    bindings_by_lhs: &HashMap<DottedRef, usize>,
    kind: ScopeKind,
    scope_name: &str,
    ir: &mut LogicIr,
    diagnostics: &mut Vec<Diagnostic>,
) -> HashSet<DottedRef> {
    let mut ctx = LoweringCtx {
        kind,
        scope_name,
        bindings,
        bindings_by_lhs,
        in_progress: HashMap::new(),
        failed_lhs: HashSet::new(),
        cse: HashMap::new(),
    };

    for (idx, b) in bindings.iter().enumerate() {
        // Only visit the first binding per LHS; duplicates are already
        // flagged. Sensor-owned LHS is skipped so the sensor stays the
        // single driver.
        if bindings_by_lhs.get(b.lhs).copied() != Some(idx) {
            continue;
        }
        if matches!(ir.signal_defs.get(b.lhs), Some(SignalRef::Input(_))) {
            continue;
        }
        if ir.signal_defs.contains_key(b.lhs) || ctx.failed_lhs.contains(b.lhs) {
            continue;
        }
        if lower_binding(b.lhs, ir, &mut ctx, diagnostics).is_err() {
            ctx.failed_lhs.insert(b.lhs.clone());
        }
    }

    ctx.failed_lhs
}

/// Register actuator drivers against the resolved signal table. An
/// unresolved driver whose LHS is in `failed_lhs` was already reported
/// upstream; skip the cascade. Every other missing driver produces a
/// standalone `E_LOGIC_UNBOUND_SIGNAL`.
fn resolve_actuators(
    actuators: &[PendingActuator],
    bindings_by_lhs: &HashMap<DottedRef, usize>,
    failed_lhs: &HashSet<DottedRef>,
    kind: ScopeKind,
    scope_name: &str,
    ir: &mut LogicIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for act in actuators {
        match ir.signal_defs.get(&act.driver_name).copied() {
            Some(driver) => ir.outputs.push(OutputPort {
                name: act.driver_name.clone(),
                driver,
                span: act.span.clone(),
            }),
            None if failed_lhs.contains(&act.driver_name) => {}
            None => diagnostics.push(unbound_signal_diagnostic(
                kind,
                scope_name,
                &act.driver_name,
                act.span.clone(),
                ir,
                bindings_by_lhs,
                "actuator argument",
            )),
        }
    }
}

/// Walk every `logic sig.X = ...` first-declared binding and warn if
/// nothing consumes its LHS. Purely syntactic so it also catches bare-ref
/// bindings (`logic sig.a = sig.b`) that never produced a gate node and
/// would be invisible to a gate-only reachability walk.
fn audit_unused_signals(
    bindings: &[PendingBinding<'_>],
    actuators: &[PendingActuator],
    bindings_by_lhs: &HashMap<DottedRef, usize>,
    failed_lhs: &HashSet<DottedRef>,
    kind: ScopeKind,
    scope_name: &str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut consumed: HashSet<DottedRef> = HashSet::new();
    for b in bindings {
        collect_refs(b.rhs, &mut consumed);
    }
    for act in actuators {
        consumed.insert(act.driver_name.clone());
    }
    for (idx, b) in bindings.iter().enumerate() {
        if bindings_by_lhs.get(b.lhs).copied() != Some(idx) || failed_lhs.contains(b.lhs) {
            continue;
        }
        if consumed.contains(b.lhs) {
            continue;
        }
        let label = scope_label(kind, scope_name);
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::LogicUnusedSignal,
                b.span.clone(),
                format!(
                    "{label} `logic {lhs} = ...` is defined but never consumed by an actuator or downstream logic",
                    lhs = b.lhs,
                ),
            )
            .with_footer(
                "Fix: wire an actuator to this signal, reference it from another `logic` binding, or remove the binding if it is dead code.",
            ),
        );
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
    /// Signal names whose resolution already failed. Every downstream
    /// reference (RHS or actuator) checks this set before emitting a fresh
    /// `E_LOGIC_UNBOUND_SIGNAL` so the user sees the root cause once, not
    /// once per consumer. Populated both by `logic` binding failures and
    /// by raw-ref lookups that name a signal no source defines.
    failed_lhs: HashSet<DottedRef>,
    /// Common-subexpression cache keyed by the fully-populated
    /// [`GateKind`]. Symmetric variants are canonicalised at construction,
    /// so `a and b` and `b and a` share a cache slot.
    cse: HashMap<GateKind, u32>,
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
    let binding_span = ctx.bindings[binding_idx].span.clone();
    let rhs = ctx.bindings[binding_idx].rhs;

    ctx.in_progress.insert(lhs.clone(), binding_span.clone());
    let result = lower_expr(rhs, &binding_span, ir, ctx, diagnostics);
    ctx.in_progress.remove(lhs);

    match result {
        Ok(sig) => {
            ir.signal_defs.insert(lhs.clone(), sig);
            Ok(sig)
        }
        Err(err) => Err(err),
    }
}

/// Lower one boolean expression. Every operand is lowered independently
/// before the enclosing gate is built, and every recursive failure is
/// collected — a `logic sig.x = sig.undef1 or sig.undef2` reports both
/// unbound refs rather than shortcircuiting at the first.
fn lower_expr<'a>(
    expr: &'a Expr,
    binding_span: &Span,
    ir: &mut LogicIr,
    ctx: &mut LoweringCtx<'a>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<SignalRef, LoweringFailed> {
    match expr {
        Expr::Ref(dr) => resolve_ref(dr, binding_span, ir, ctx, diagnostics),
        Expr::And(a, b) => {
            let (ai, bi) = lower_binary(a, b, binding_span, ir, ctx, diagnostics)?;
            let (ai, bi) = canonicalize_pair(ai, bi);
            Ok(intern_gate(
                ir,
                ctx,
                GateKind::And2 { a: ai, b: bi },
                binding_span.clone(),
            ))
        }
        Expr::Or(a, b) => {
            let (ai, bi) = lower_binary(a, b, binding_span, ir, ctx, diagnostics)?;
            let (ai, bi) = canonicalize_pair(ai, bi);
            Ok(intern_gate(
                ir,
                ctx,
                GateKind::Or2 { a: ai, b: bi },
                binding_span.clone(),
            ))
        }
        Expr::Not(a) => {
            let ai = lower_expr(a, binding_span, ir, ctx, diagnostics)?;
            Ok(intern_gate(
                ir,
                ctx,
                GateKind::Not { a: ai },
                binding_span.clone(),
            ))
        }
        // `Expr` is `#[non_exhaustive]`; the current parser produces only
        // the four variants above. A future extension (`xor(a, b)`,
        // `mux(sel=..., a=..., b=...)`, ...) will land as new variants —
        // reject them fail-loud with a dedicated code so the missing
        // lowering site is signalled at the offending line rather than
        // hidden inside a wrong DAG.
        _ => {
            let label = scope_label(ctx.kind, ctx.scope_name);
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::LogicUnsupportedPrimitive,
                    binding_span.clone(),
                    format!(
                        "{label} logic expression uses a boolean primitive this synth pass does not yet lower",
                    ),
                )
                .with_footer(
                    "Fix: rewrite using `and` / `or` / `not`, or wait for a follow-up change that lands the missing primitive.",
                ),
            );
            Err(LoweringFailed)
        }
    }
}

/// Lower the two operands of a binary gate, collecting *both* independent
/// diagnostics before deciding whether to fail. Keeps every root cause on
/// one pass so the author does not have to fix and re-run to see the
/// second issue.
fn lower_binary<'a>(
    a: &'a Expr,
    b: &'a Expr,
    binding_span: &Span,
    ir: &mut LogicIr,
    ctx: &mut LoweringCtx<'a>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(SignalRef, SignalRef), LoweringFailed> {
    let ai = lower_expr(a, binding_span, ir, ctx, diagnostics);
    let bi = lower_expr(b, binding_span, ir, ctx, diagnostics);
    match (ai, bi) {
        (Ok(a), Ok(b)) => Ok((a, b)),
        _ => Err(LoweringFailed),
    }
}

fn resolve_ref<'a>(
    dr: &'a DottedRef,
    binding_span: &Span,
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
        let label = scope_label(ctx.kind, ctx.scope_name);
        diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::LogicCycle,
                seed_span,
                format!(
                    "{label} `logic {dr} = ...` depends on itself through the signal graph",
                ),
            )
            .with_footer(
                "Fix: break the cycle by adding a sensor driving one of the signals, or use a `latch(...)` macro once combinational synthesis grows sequential support.",
            ),
        );
        ctx.failed_lhs.insert(dr.clone());
        return Err(LoweringFailed);
    }

    // Root cause already reported once — silent skip.
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

    // Unbound: no sensor, no `logic` line defines it. Record so a later
    // actuator or sibling RHS reference does not repeat the same finding.
    diagnostics.push(unbound_signal_diagnostic(
        ctx.kind,
        ctx.scope_name,
        dr,
        binding_span.clone(),
        ir,
        ctx.bindings_by_lhs,
        "`logic` binding",
    ));
    ctx.failed_lhs.insert(dr.clone());
    Err(LoweringFailed)
}

/// Reuse a gate node whose fully-populated [`GateKind`] was already
/// interned, or push a fresh one. Returns the [`SignalRef::Gate`] handle
/// either way so the caller does not have to branch on the CSE cache.
fn intern_gate(
    ir: &mut LogicIr,
    ctx: &mut LoweringCtx<'_>,
    kind: GateKind,
    span: Span,
) -> SignalRef {
    if let Some(&idx) = ctx.cse.get(&kind) {
        return SignalRef::Gate(idx);
    }
    let idx = safe_index(ir.nodes.len());
    ir.nodes.push(GateNode { kind, span });
    ctx.cse.insert(kind, idx);
    SignalRef::Gate(idx)
}

/// Canonicalise the operand pair for a symmetric gate so
/// `and(a, b)` and `and(b, a)` share the same CSE cache slot. Uses a
/// deterministic total order over `SignalRef` — input ports sort before
/// gates, and inside each family the smaller index wins.
fn canonicalize_pair(a: SignalRef, b: SignalRef) -> (SignalRef, SignalRef) {
    if signal_rank(a) <= signal_rank(b) {
        (a, b)
    } else {
        (b, a)
    }
}

fn signal_rank(s: SignalRef) -> (u8, u32) {
    match s {
        SignalRef::Input(i) => (0, i),
        SignalRef::Gate(i) => (1, i),
    }
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

/// Saturating cast of a `usize` length into the `u32` index space used by
/// [`SignalRef`]. A `.crn` large enough to hit `u32::MAX` ports or gates
/// is well past any Cairn build the compiler can practically finish; the
/// value is clamped rather than panicking so a malicious input cannot use
/// this arithmetic path as a denial-of-service vector.
fn safe_index(len: usize) -> u32 {
    u32::try_from(len).unwrap_or(u32::MAX)
}
