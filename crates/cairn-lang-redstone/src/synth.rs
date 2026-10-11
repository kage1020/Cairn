//! Intent IR → Logic IR lowering.
//!
//! Walks each scope of an [`IntentModule`], collects sensor bindings as [`InputPort`]s, actuator
//! bindings as [`OutputPort`]s, and lowers every `logic sig.X = <expr>` line into a topologically
//! ordered DAG of [`GateNode`]s. Per `spec/lint` "Error vs warning", an error (an unbound signal,
//! a duplicate driver, a cycle) keeps its scope out of the returned [`ScopedLogicIr`]; only
//! warnings survive beside a well-formed IR.
//!
//! Common subexpressions are shared while lowering: two `logic` lines whose right-hand sides build
//! the same gate over the same operands get one node, and a symmetric gate orders its operands
//! first, so `sig.a or sig.b` and `sig.b or sig.a` share one too.
//!
//! # Bindings
//!
//! `spec/redstone` "Signal binding" writes each binding on one component, so what a line is for
//! is read from the line and then held to that:
//! - **Sensor**: a `-> value` tail on a member whose keyword is in [`SENSOR_HOSTS`].
//! - **Actuator**: an argument key of [`ACTUATOR_BINDINGS`] on the component that table pairs it
//!   with. In the selector form (`door[id=front] opened_by=sig.x`) it drives the door
//!   `cairn_lang_core::intent::actuator_patch_target` picks, the lookup block-array lowering uses
//!   too; one that picks no single door is `E_LOGIC_UNRESOLVED_PATCH`. A component takes one
//!   binding per key, however its lines are written; a second is `E_LOGIC_DUPLICATE_BINDING`.
//!
//! Either side of a pair claims a binding, and the value is then checked against the claim: a
//! tail claims a sensor and an actuator key a wire whatever the value, and a `sig.X` value claims
//! one under any key. Each fault is reported by the one finding the other repairs would not
//! answer:
//! - the **host** cannot carry the binding: `E_LOGIC_MISPLACED_BINDING`, asked first, because no
//!   edit to the value makes `walls` carry a tail. For a `-> value` tail that is `check`'s
//!   `E_MISPLACED_BINDING`, which every command reports, so this pass only records what the tail
//!   would have driven, so nothing downstream reports the hole again;
//! - the **key** is one nothing reads: `E_LOGIC_UNKNOWN_BINDING_KEY`, with the nearest key;
//! - the **value** names no signal: `E_LOGIC_INVALID_SIGNAL`, the code a `logic` left-hand side
//!   takes, because all three positions follow one rule about one namespace.
//!
//! A pair that claims nothing on either side is an ordinary argument, whose key no pass checks, so
//! `hieght=3` is nobody's finding here. The `[selector]` carries the same pairs but is never a
//! binding site: a bracketed pair whose only fault is the brackets is `E_LOGIC_MISPLACED_BINDING`
//! saying so, and one that would be wrong outside them too is told that instead.
//!
//! # One finding per root cause
//!
//! A signal whose `logic` line failed to lower, or whose reference was unbound, is recorded in
//! `failed_lhs`; a signal a refused binding would have driven or consumed is recorded in
//! `ScopeCollected`'s `refused_*` sets. Every later reference (right-hand sides, actuators, the
//! `assert` walk, the unused-signal audit) checks those first.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::logic_ir::{
    GateKind, GateNode, InputPort, LogicIr, OutputPort, ScopeKind, ScopedLogicIr,
    ScopedLogicIrEntry, SignalRef,
};
use crate::saturating_index;
use cairn_lang_core::ast::{DottedRef, Expr, SIGNAL_HEAD, Value, ValueKind};
use cairn_lang_core::check::Severity;
use cairn_lang_core::error::Span;
use cairn_lang_core::intent::{
    AssertIr, DefIr, IntentModule, LogicBinding, Member, MemberBody, MemberRole, PatchTargetError,
    SiteIr, StructIr, ValueWithSpan, actuator_patch_target, known_keywords,
};
use cairn_lang_core::suggest::{did_you_mean, nearest_match};

/// Each actuator argument key from `spec/redstone` "Signal binding",
/// paired with the component keyword that carries it.
///
/// The spec writes each key on one component (`lamp ... lit_by=`, `piston ... powered_by=`,
/// `door ... opened_by=`, `dispenser ... fired_by=`), so a key on any other member is refused
/// rather than read as an output port with no component behind it. Only `door` is in the role
/// table, so the other three keys have no legal host and are refused wherever they are written;
/// adding a host keyword changes this table and not the check.
///
/// Public so `tests/redstone/actuator_schema.rs` can compare it with `MemberRole::arguments`
/// directly.
pub const ACTUATOR_BINDINGS: &[(&str, &str)] = &[
    ("opened_by", "door"),
    ("powered_by", "piston"),
    ("lit_by", "lamp"),
    ("fired_by", "dispenser"),
];

/// The component keywords that may carry a `->` sensor tail, owned by `cairn-lang-core` because
/// `check` asks the same question for every command.
pub use cairn_lang_core::intent::SENSOR_HOSTS;

/// The per-scope Logic IR and every diagnostic of the module. A scope with an error is left out of
/// [`Self::scoped`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SynthOutput {
    /// Per-scope Logic IR, without the scopes that hit an error.
    pub scoped: ScopedLogicIr,
    /// Every finding, errors and warnings, in source order.
    pub diagnostics: Vec<Diagnostic>,
}

/// Lowers an [`IntentModule`] to a [`SynthOutput`], with one [`LogicIr`] per struct, def or site
/// that holds a sensor, an actuator, a `logic` binding or an `assert`.
///
/// Expects the caller to have run `cairn_lang_core::check` and stopped on its errors, as
/// `cairn synth` does: a member whose keyword the role table does not know is skipped, since
/// `check` reports it as `E_UNKNOWN_KEYWORD`.
pub fn synthesize(module: &IntentModule) -> SynthOutput {
    // `IntentModule` keeps one vector per scope kind, so the scopes are merged back into the
    // source order the IR dump and the findings both promise.
    let mut scopes: Vec<ModuleScope<'_>> =
        Vec::with_capacity(module.structs.len() + module.defs.len() + module.sites.len());
    scopes.extend(module.structs.iter().map(ModuleScope::Struct));
    scopes.extend(module.defs.iter().map(ModuleScope::Def));
    scopes.extend(module.sites.iter().map(ModuleScope::Site));
    scopes.sort_by_key(ModuleScope::span);

    let mut out = SynthOutput::default();
    for scope in scopes {
        scope.lower(&mut out);
    }
    out
}

/// One top-level scope of a module, of any kind, so [`synthesize`] can sort all three into one
/// list.
enum ModuleScope<'a> {
    Struct(&'a StructIr),
    Def(&'a DefIr),
    Site(&'a SiteIr),
}

impl ModuleScope<'_> {
    /// Byte range of the scope's block, as the sort key. The start alone would order scopes; the
    /// end is carried so the key matches the diagnostic sort's.
    fn span(&self) -> (usize, usize) {
        match self {
            Self::Struct(struct_ir) => (struct_ir.span.start, struct_ir.span.end),
            Self::Def(def_ir) => (def_ir.span.start, def_ir.span.end),
            Self::Site(site_ir) => (site_ir.span.start, site_ir.span.end),
        }
    }

    /// Collects the scope's body and finishes it into `out`.
    ///
    /// A site's rows are walked like members although only its `logic` contributes, because
    /// refusing a fixture among them is `check::member_scope`'s job.
    fn lower(&self, out: &mut SynthOutput) {
        let (kind, name, members, logic, asserts) = match self {
            Self::Struct(struct_ir) => (
                ScopeKind::Struct,
                &struct_ir.name,
                &struct_ir.members,
                &struct_ir.logic,
                &struct_ir.asserts,
            ),
            Self::Def(def_ir) => (
                ScopeKind::Def,
                &def_ir.name,
                &def_ir.members,
                &def_ir.logic,
                &def_ir.asserts,
            ),
            Self::Site(site_ir) => (
                ScopeKind::Site,
                &site_ir.name,
                &site_ir.placements,
                &site_ir.logic,
                &site_ir.asserts,
            ),
        };
        let scope = ScopeRef { kind, name };
        let mut collected = ScopeCollected::default();
        let candidates = patch_candidates(members);
        collect_body(members, logic, asserts, &candidates, scope, &mut collected);
        finish_scope(scope, collected, out);
    }
}

impl ScopeCollected<'_> {
    /// Whether this scope has anything for the finish step to do.
    ///
    /// Destructured, so a field added later is a compile error here rather than a gap in the
    /// predicate.
    fn has_work(&self) -> bool {
        let Self {
            sensors,
            actuators,
            bindings,
            asserts,
            diagnostics,
            refused_drivers,
            refused_consumers,
        } = self;
        !(sensors.is_empty()
            && actuators.is_empty()
            && bindings.is_empty()
            && asserts.is_empty()
            && diagnostics.is_empty()
            && refused_drivers.is_empty()
            && refused_consumers.is_empty())
    }
}

/// One scope's bindings as collected, kept apart from [`LogicIr`] so a half-built IR never reaches
/// the caller; [`finish_scope`] decides whether they become one.
#[derive(Debug, Default)]
struct ScopeCollected<'a> {
    /// Findings raised while walking the body, where a binding's host is still known.
    diagnostics: Vec<Diagnostic>,
    /// `assert` properties, in source order. Not evaluated, but the signals they name are checked
    /// once `signal_defs` is complete.
    asserts: Vec<&'a AssertIr>,
    /// Signals a refused binding would have driven: a `-> sig.X` tail on a member that is not a
    /// sensor, or a `logic` left-hand side that is not a signal name ([`is_signal_name`]). A
    /// reference to one is not a second mistake, so the unbound-signal report skips it.
    refused_drivers: HashSet<DottedRef>,
    /// Signals a refused binding would have consumed: an actuator key on the wrong member, or a
    /// signal-valued key nothing reads. The author did wire them, so the unused-signal audit
    /// counts them as consumed.
    refused_consumers: HashSet<DottedRef>,
    /// Sensors, in source order.
    sensors: Vec<PendingSensor>,
    /// Accepted actuator bindings, in walk order, each filed under the component it drives (a
    /// patch under the door its selector picks), so a second binding on one door is found
    /// whichever line carries it. Resolved to [`OutputPort`]s only in the finish step, so an
    /// undefined driver is reported rather than wired.
    actuators: Vec<BoundActuator<'a>>,
    /// `logic sig.X = <expr>` lines, in source order.
    bindings: Vec<PendingBinding<'a>>,
}

#[derive(Debug)]
struct PendingSensor {
    name: DottedRef,
    span: Span,
}

/// One accepted actuator binding, filed under the component it drives.
#[derive(Debug)]
struct BoundActuator<'a> {
    /// The physical member the binding drives: the member itself, or the
    /// door a selector-form patch picks.
    host: &'a Member,
    key: &'a str,
    driver_name: &'a DottedRef,
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
    asserts: &'a [AssertIr],
    candidates: &[&'a Member],
    scope: ScopeRef<'_>,
    out: &mut ScopeCollected<'a>,
) {
    for m in members {
        collect_member(m, candidates, scope, out);
    }
    for b in logic {
        collect_binding(b, scope, out);
    }
    // The walk reaches a member's nested `logic` lines before the scope's own, and `MemberBody`
    // keeps members and `logic` apart, so only the spans still hold the order the author wrote.
    // Sorting by them numbers the IR's nodes in source order. `resolve_drivers` sorts its own
    // list to decide which driver keeps a signal; this one is the order the survivors lower in.
    out.bindings.sort_by_key(|b| (b.span.start, b.span.end));
    // `check_assert_refs` reports an unbound name at the first `assert` naming it, which has to
    // be the first in the file.
    out.asserts.extend(asserts);
    out.asserts.sort_by_key(|a| (a.span().start, a.span().end));
}

fn collect_member<'a>(
    m: &'a Member,
    candidates: &[&'a Member],
    scope: ScopeRef<'_>,
    out: &mut ScopeCollected<'a>,
) {
    // A member with an unknown keyword is `check`'s `E_UNKNOWN_KEYWORD`, so its bindings raise
    // nothing here. What they would have driven or consumed is still recorded, or
    // `lamp id=l1 lit_by=sig.x` would also earn an unused-signal warning.
    let unknown_keyword = matches!(m.role, MemberRole::Other(_));

    // Sensor: the tail claims a signal whatever its value, so `-> a` is reported rather than
    // ignored. The host is asked first, since no edit to the value makes `walls` carry a tail.
    if let Some(binding) = &m.binding {
        let named = signal_named_by(binding);
        if unknown_keyword || !SENSOR_HOSTS.contains(&m.role.keyword()) || m.selector.is_some() {
            // `check::binding` refuses this tail (a non-sensor keyword, or a sensor in the
            // selector form). Only the driver it takes away is recorded, so a `logic` line
            // reading the signal is not told that nothing defines it.
            if let Some(dr) = named {
                out.refused_drivers.insert(dr.clone());
            }
        } else if let Some(dr) = named {
            out.sensors.push(PendingSensor {
                name: dr.clone(),
                span: binding.span.clone(),
            });
        } else {
            out.diagnostics
                .push(diag_tail_names_no_signal(m, binding, scope));
        }
    }

    // Actuator: the key claims the binding whatever the value, so a well-spelled key with a
    // malformed value is still checked.
    for (key, vspan) in &m.intent_state.fields {
        let named = signal_named_by(&vspan.value);
        let Some(claim) = binding_claim(key, named) else {
            continue;
        };
        match claim {
            BindingClaim::UnreadKey => {
                if !unknown_keyword {
                    out.diagnostics
                        .push(diag_unknown_binding_key(m, key, vspan, scope));
                }
            }
            BindingClaim::Actuator(host) => {
                if unknown_keyword || m.role.keyword() != host {
                    if !unknown_keyword {
                        out.diagnostics
                            .push(diag_misplaced_actuator(m, key, host, vspan, scope));
                    }
                } else if let Some(dr) = named {
                    if accept_actuator(m, key, dr, vspan, candidates, scope, out) {
                        continue;
                    }
                } else {
                    out.diagnostics
                        .push(diag_argument_names_no_signal(m, key, vspan, scope));
                }
            }
        }
        if let Some(dr) = named {
            out.refused_consumers.insert(dr.clone());
        }
    }

    // The `[selector]` is never a binding site: `spec/redstone` "Signal binding" writes the
    // binding after the brackets. Block-array lowering refuses any selector attribute but `id=`
    // on a door patch; this walk answers for every host, in this pass's vocabulary.
    for (key, vspan) in m.selector.iter().flatten() {
        let named = signal_named_by(&vspan.value);
        let Some(claim) = binding_claim(key, named) else {
            continue;
        };
        if !unknown_keyword {
            // Report the fault that moving the pair out of the brackets would not fix:
            // `door[id=front,lit_by=sig.a]` moved out is still a `lit_by=` on a door. Only a key
            // this member hosts has the brackets as its one fault.
            out.diagnostics.push(match claim {
                BindingClaim::Actuator(host) if host == m.role.keyword() => {
                    diag_binding_inside_selector(m, key, vspan, scope)
                }
                BindingClaim::Actuator(host) => diag_misplaced_actuator(m, key, host, vspan, scope),
                BindingClaim::UnreadKey => diag_unknown_binding_key(m, key, vspan, scope),
            });
        }
        if let Some(dr) = named {
            out.refused_consumers.insert(dr.clone());
        }
    }

    let MemberBody {
        members: children,
        logic,
        asserts,
    } = &m.children;
    for child in children {
        collect_member(child, candidates, scope, out);
    }
    for b in logic {
        collect_binding(b, scope, out);
    }
    out.asserts.extend(asserts);
}

/// Registers a well-formed actuator binding on the component it drives (`m`, or the door a
/// selector-form patch picks), or refuses it; `false` when refused, so the caller still counts the
/// signal as consumed.
///
/// A patch that picks no door is `E_LOGIC_UNRESOLVED_PATCH`, and a second binding under one key on
/// one component `E_LOGIC_DUPLICATE_BINDING`. Of two bindings on one component, the one on the
/// component's own line is kept whichever the walk reaches first: a patch may be written above
/// its door, and the finding belongs on the patch.
fn accept_actuator<'a>(
    m: &'a Member,
    key: &'a str,
    driver: &'a DottedRef,
    vspan: &ValueWithSpan,
    candidates: &[&'a Member],
    scope: ScopeRef<'_>,
    out: &mut ScopeCollected<'a>,
) -> bool {
    let host = match actuator_host(m, candidates) {
        Ok(host) => host,
        Err(reason) => {
            out.diagnostics
                .push(diag_unresolved_patch(m, key, &reason, vspan, scope));
            return false;
        }
    };
    let incoming = BoundActuator {
        host,
        key,
        driver_name: driver,
        span: vspan.span.clone(),
    };
    let Some(slot) = out
        .actuators
        .iter_mut()
        .find(|b| std::ptr::eq(b.host, host) && b.key == key)
    else {
        out.actuators.push(incoming);
        return true;
    };
    // One line cannot carry a key twice, so a duplicate always involves a
    // patch, and the line that is not one is the component's own.
    let declares = std::ptr::eq(m, host);
    let refused = if declares {
        std::mem::replace(slot, incoming)
    } else {
        incoming
    };
    out.diagnostics
        .push(diag_duplicate_binding(slot, &refused, scope));
    if declares {
        out.refused_consumers.insert(refused.driver_name.clone());
    }
    declares
}

/// The members a selector-form patch in this scope may pick: every member
/// this pass walks, at any depth.
///
/// Block-array lowering looks only through a `level` directly in the body, but every deeper body
/// is a `check` error that `cairn synth` stops on, so on what reaches this pass the two views hold
/// the same doors. The wider one matches [`collect_member`], which reads bindings at any depth.
fn patch_candidates(members: &[Member]) -> Vec<&Member> {
    fn walk<'a>(members: &'a [Member], out: &mut Vec<&'a Member>) {
        for m in members {
            out.push(m);
            walk(&m.children.members, out);
        }
    }
    let mut out = Vec::with_capacity(members.len());
    walk(members, &mut out);
    out
}

/// The physical member an actuator binding on `m` drives: `m` itself, or,
/// for the selector form, the door its brackets pick.
fn actuator_host<'a>(
    m: &'a Member,
    candidates: &[&'a Member],
) -> Result<&'a Member, PatchTargetError> {
    if m.selector.is_none() {
        return Ok(m);
    }
    actuator_patch_target(m, candidates.iter().copied())
}

/// Whether a dotted name is a signal name: `sig.` and exactly one segment
/// after it (`spec/redstone` "Signal binding").
///
/// Asked where a signal is defined or wired: a `logic` left-hand side, and through
/// [`signal_named_by`] a sensor tail, an argument value or a selector attribute. A right-hand side
/// or `assert` reference is looked up among the defined signals instead, which only those
/// positions add to. Exactly one segment, because block-array lowering, which reads the actuator
/// side, refuses any other shape.
fn is_signal_name(dr: &DottedRef) -> bool {
    dr.head() == SIGNAL_HEAD && dr.tail().len() == 1
}

/// The signal reference a value names, or `None` for a value that names
/// no signal at all. See [`is_signal_name`].
fn signal_named_by(value: &Value) -> Option<&DottedRef> {
    match &value.kind {
        ValueKind::DotRef(dr) if is_signal_name(dr) => Some(dr),
        _ => None,
    }
}

/// Why a `key=value` pair is read as a binding the author meant to write.
#[derive(Debug, Clone, Copy)]
enum BindingClaim {
    /// The key is one of [`ACTUATOR_BINDINGS`], a binding whatever the value; carries the
    /// component that table pairs it with.
    Actuator(&'static str),
    /// The value is a signal under a key nothing reads, so a binding was meant. Built only when a
    /// signal was named.
    UnreadKey,
}

/// Whether a pair claims to be a binding, from either side: keying on the value alone would let a
/// well-spelled key with a malformed value through, and on the key alone would lose
/// [`DiagnosticCode::LogicUnknownBindingKey`].
fn binding_claim(key: &str, named: Option<&DottedRef>) -> Option<BindingClaim> {
    if let Some((_, host)) = ACTUATOR_BINDINGS.iter().find(|(k, _)| *k == key) {
        return Some(BindingClaim::Actuator(host));
    }
    named.map(|_| BindingClaim::UnreadKey)
}

/// Collects one `logic` line, or refuses a left-hand side that is not a signal name
/// ([`is_signal_name`]). No actuator could read such a signal, so it is refused before its gate
/// takes a cell and a coordinate.
fn collect_binding<'a>(b: &'a LogicBinding, scope: ScopeRef<'_>, out: &mut ScopeCollected<'a>) {
    if !is_signal_name(&b.lhs) {
        let LhsRefusal { why, fix } = lhs_refusal(&b.lhs);
        out.diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::LogicInvalidSignal,
                b.span.clone(),
                format!(
                    "{label} `logic {lhs} = ...` names `{lhs}`, {why}",
                    label = scope.label(),
                    lhs = b.lhs,
                ),
            )
            .with_footer(format!(
                "Fix: {fix}, or delete the binding if nothing was meant to read it."
            )),
        );
        out.refused_drivers.insert(b.lhs.clone());
        // The right-hand side still reads its signals; warning that they are unconsumed would
        // report this mistake twice.
        let mut refs = HashSet::new();
        collect_refs(&b.rhs, &mut refs);
        out.refused_consumers.extend(refs);
        // Not collected, so an undefined name on the right is reported once the left is fixed.
        return;
    }
    out.bindings.push(PendingBinding {
        lhs: &b.lhs,
        rhs: &b.rhs,
        span: b.span.clone(),
    });
}

/// Why a `logic` left-hand side is not a signal name, and its repair.
struct LhsRefusal {
    /// Completes "names `<lhs>`, ..." in the primary message.
    why: String,
    /// Completes "Fix: ..." in the footer.
    fix: String,
}

/// The refusal for a `logic` left-hand side that fails [`is_signal_name`]: outside the namespace,
/// the namespace alone, or more than one segment after it. Never called on a signal name, whose
/// one segment the last branch would call too many.
fn lhs_refusal(lhs: &DottedRef) -> LhsRefusal {
    debug_assert!(!is_signal_name(lhs), "`{lhs}` is a signal name");
    if lhs.head() != SIGNAL_HEAD {
        return LhsRefusal {
            why: format!(
                "which is outside the `{SIGNAL_HEAD}.` namespace sensors emit into and \
                 actuators read from"
            ),
            fix: format!("rename the left-hand side to `{SIGNAL_HEAD}.<name>`"),
        };
    }
    let segments = lhs.tail();
    match segments.first() {
        None => LhsRefusal {
            why: format!(
                "which is the `{SIGNAL_HEAD}.` namespace itself rather than a signal in it; a \
                 signal name is `{SIGNAL_HEAD}.` and exactly one segment after it"
            ),
            fix: format!("add a name, as in `{SIGNAL_HEAD}.<name>`"),
        },
        Some(first) => LhsRefusal {
            why: format!(
                "which has {count} segments after `{SIGNAL_HEAD}.`; a signal name is \
                 `{SIGNAL_HEAD}.` and exactly one segment after it",
                count = segments.len(),
            ),
            fix: format!(
                "keep one segment after `{SIGNAL_HEAD}.`, as in `{SIGNAL_HEAD}.{first}` or \
                 `{SIGNAL_HEAD}.{joined}`",
                joined = segments.join("_"),
            ),
        },
    }
}

/// The repair for a value that names no signal, when there is one.
///
/// A bare identifier is a name with `sig.` left off, so `opened_by=a` has one reading to offer;
/// any other kind (`opened_by=3`) gets the shape instead. `sig` alone is the namespace without a
/// name, so it gets the shape too.
fn signal_spelling_for(value: &Value) -> Option<String> {
    match &value.kind {
        ValueKind::Ident(name) if name != SIGNAL_HEAD => Some(format!("{SIGNAL_HEAD}.{name}")),
        _ => None,
    }
}

/// The `Fix:` line the two value-side refusals share, so the sensor tail
/// and the actuator argument cannot start offering different repairs for
/// the same mistake.
fn signal_value_fix(value: &Value, drop_advice: &str) -> String {
    match signal_spelling_for(value) {
        Some(spelling) => format!("Fix: write `{spelling}`, or {drop_advice}"),
        None => format!("Fix: name a signal as `{SIGNAL_HEAD}.<name>`, or {drop_advice}"),
    }
}

/// A `-> value` tail on a sensor whose value names no signal, reported at the value, the text to
/// change.
fn diag_tail_names_no_signal(member: &Member, binding: &Value, scope: ScopeRef<'_>) -> Diagnostic {
    diag_names_no_signal(
        member,
        binding,
        binding.span.clone(),
        format!(
            "{label} `{keyword}` emits into a signal, but its `->` tail is \
             {found} rather than a name in the `{SIGNAL_HEAD}.` namespace \
             sensors emit into and actuators read from",
            label = scope.label(),
            keyword = member.role.keyword(),
            found = binding.describe(),
        ),
        "drop the `->` tail if this sensor drives nothing.",
    )
}

/// An actuator key on its own host whose value names no signal.
fn diag_argument_names_no_signal(
    member: &Member,
    key: &str,
    vspan: &ValueWithSpan,
    scope: ScopeRef<'_>,
) -> Diagnostic {
    diag_names_no_signal(
        member,
        &vspan.value,
        vspan.span.clone(),
        format!(
            "{label} `{key}=` wires this `{keyword}` to a signal, but names \
             {found} rather than a name in the `{SIGNAL_HEAD}.` namespace \
             sensors emit into and actuators read from",
            label = scope.label(),
            keyword = member.role.keyword(),
            found = vspan.value.describe(),
        ),
        "drop the argument if this component is not wired.",
    )
}

/// The two value-side refusals: reported at `span`, noted at the member, and repaired by
/// [`signal_value_fix`]. `span` is passed beside `value` because a [`ValueWithSpan`] may cover
/// more than its value's own range.
fn diag_names_no_signal(
    member: &Member,
    value: &Value,
    span: Span,
    primary: String,
    drop_advice: &str,
) -> Diagnostic {
    Diagnostic::new(DiagnosticCode::LogicInvalidSignal, span, primary)
        .with_note(member.span.clone(), "declared here")
        .with_footer(signal_value_fix(value, drop_advice))
}

/// A binding written inside the `[selector]` rather than after it. Only reached for a key this
/// member hosts, so the brackets are the one fault and the `Fix:` can rebuild the line.
fn diag_binding_inside_selector(
    member: &Member,
    key: &str,
    vspan: &ValueWithSpan,
    scope: ScopeRef<'_>,
) -> Diagnostic {
    debug_assert!(
        ACTUATOR_BINDINGS
            .iter()
            .any(|(k, host)| *k == key && *host == member.role.keyword()),
        "the `Fix:` below rebuilds the line with this key on this member, which only \
         compiles for a key `spec/redstone` \"Signal binding\" pairs with it",
    );
    Diagnostic::new(
        DiagnosticCode::LogicMisplacedBinding,
        vspan.span.clone(),
        format!(
            "{label} `{key}=` is inside `{keyword}`'s `[selector]`, where \
             nothing reads it; the brackets pick the member and the binding \
             is written after them",
            label = scope.label(),
            keyword = member.role.keyword(),
        ),
    )
    .with_note(member.span.clone(), "declared here")
    .with_footer(format!(
        "Fix: move `{key}=` out of the brackets, as in \
         `{keyword}[id=<label>] {key}={SIGNAL_HEAD}.<name>`.",
        keyword = member.role.keyword(),
    ))
}

/// An actuator key on a member that is not the component
/// `spec/redstone` "Signal binding" pairs it with.
fn diag_misplaced_actuator(
    member: &Member,
    key: &str,
    host: &str,
    vspan: &ValueWithSpan,
    scope: ScopeRef<'_>,
) -> Diagnostic {
    let known = known_keywords().contains(&host);
    Diagnostic::new(
        DiagnosticCode::LogicMisplacedBinding,
        vspan.span.clone(),
        format!(
            "{label} `{key}=` binds a signal to a `{host}`, and this member is a \
             `{keyword}`",
            label = scope.label(),
            keyword = member.role.keyword(),
        ),
    )
    .with_note(member.span.clone(), "declared here")
    .with_footer(if known {
        format!("Fix: move the `{key}=` argument onto the `{host}` it drives.")
    } else {
        format!(
            "Fix: `{host}` is not a keyword the surface accepts yet, so `{key}=` has no \
             member it can sit on. Drive a `door` with `opened_by=` instead, or drop the \
             argument until `{host}` lands.",
        )
    })
}

/// An actuator patch whose selector picks no single door.
///
/// Anchored on the selector's `id=` when it has one, the text the author corrects. The primary
/// carries the [`PatchTargetError`] reason block-array lowering defers the line with.
fn diag_unresolved_patch(
    member: &Member,
    key: &str,
    reason: &PatchTargetError,
    vspan: &ValueWithSpan,
    scope: ScopeRef<'_>,
) -> Diagnostic {
    let span = member
        .selector
        .as_ref()
        .and_then(|s| s.get("id"))
        .map_or_else(|| member.span.clone(), |id| id.span.clone());
    let keyword = member.role.keyword();
    let fix = match reason {
        PatchTargetError::Ambiguous { count, .. } => format!(
            "Fix: give the {count} `{keyword}` members distinct ids, so the selector names one.",
        ),
        PatchTargetError::NoSuchId {
            id,
            known,
            unlabelled,
            ..
        } if *unlabelled > 0 => {
            if known.is_empty() {
                format!(
                    "Fix: add `id={id}` to the `{keyword}` this patch is meant to bind, or \
                     write `{key}=` on that `{keyword}`'s own line.",
                )
            } else {
                format!(
                    "Fix: set `[id=<label>]` to one of the ids listed, or add `id={id}` to \
                     the `{keyword}` without one that this patch is meant to bind.",
                )
            }
        }
        _ => format!(
            "Fix: set `[id=<label>]` to the id of one `{keyword}` declared in this scope \
             without brackets, or write `{key}=` on that `{keyword}`'s own line.",
        ),
    };
    Diagnostic::new(
        DiagnosticCode::LogicUnresolvedPatch,
        span,
        format!(
            "{label} {reason}, so its `{key}=` binding drives nothing",
            label = scope.label(),
        ),
    )
    .with_note(vspan.span.clone(), "the binding the patch carries")
    .with_footer(fix)
}

/// A second binding under one actuator key on one physical component.
///
/// `kept` is the binding the component keeps — its own line's, when it has
/// one — and `refused` the one this finding is anchored on.
fn diag_duplicate_binding(
    kept: &BoundActuator<'_>,
    refused: &BoundActuator<'_>,
    scope: ScopeRef<'_>,
) -> Diagnostic {
    let keyword = kept.host.role.keyword();
    // One of the two is a patch, which resolves only by its door's `id=`, so `host.id` is set;
    // the fallback keeps this total without a panic.
    let named = kept.host.id.as_deref().map_or_else(
        || format!("this `{keyword}`"),
        |id| format!("`{keyword}` `{id}`"),
    );
    let label = scope.label();
    let key = kept.key;
    let first_driver = kept.driver_name;
    let driver = refused.driver_name;
    let (primary, fix) = if first_driver == driver {
        (
            format!(
                "{label} {named} is already bound by `{key}={first_driver}`, and this line \
                 binds it to the same signal again",
            ),
            format!("Fix: delete the duplicate line; {named} is already bound to `{driver}`."),
        )
    } else {
        (
            format!(
                "{label} {named} is already bound by `{key}={first_driver}`, and a second \
                 `{key}=` would drive it from two wires at once",
            ),
            format!(
                "Fix: combine the signals in the logic layer, as in \
                 `logic {SIGNAL_HEAD}.<name> = {first_driver} or {driver}`, and bind `{key}=` \
                 once to `{SIGNAL_HEAD}.<name>`.",
            ),
        )
    };
    Diagnostic::new(
        DiagnosticCode::LogicDuplicateBinding,
        refused.span.clone(),
        primary,
    )
    .with_note(kept.span.clone(), "first bound here")
    .with_footer(fix)
}

/// An argument whose value is a signal, under a key nothing reads.
fn diag_unknown_binding_key(
    member: &Member,
    key: &str,
    vspan: &ValueWithSpan,
    scope: ScopeRef<'_>,
) -> Diagnostic {
    let keys: Vec<&str> = ACTUATOR_BINDINGS.iter().map(|(k, _)| *k).collect();
    let diag = Diagnostic::new(
        DiagnosticCode::LogicUnknownBindingKey,
        vspan.span.clone(),
        format!(
            "{label} `{key}=` is bound to a signal, but no binding key by that name is \
             read by anything",
            label = scope.label(),
        ),
    )
    .with_note(member.span.clone(), "declared here");
    match nearest_match(key, keys.iter().copied()) {
        Some(near) => diag.with_footer(did_you_mean(&format!("{near}="))),
        None => diag.with_footer(format!(
            "Fix: use one of `{}=`, or drop the argument if this member is not an actuator.",
            keys.join("=`, `"),
        )),
    }
}

/// Finishes one scope: its [`ScopedLogicIrEntry`] goes to `out.scoped` unless a finding is an
/// error, and every finding goes to `out.diagnostics`.
fn finish_scope(scope: ScopeRef<'_>, mut collected: ScopeCollected<'_>, out: &mut SynthOutput) {
    if !collected.has_work() {
        return;
    }

    let ScopeRef { kind, name } = scope;
    let mut ir = LogicIr::new();
    let mut diagnostics = std::mem::take(&mut collected.diagnostics);

    let winners = resolve_drivers(
        &collected.sensors,
        &collected.bindings,
        scope,
        &mut diagnostics,
    );

    let bindings_by_lhs = winners.bindings();

    register_sensors(&collected.sensors, &winners, &mut ir);

    let failed_lhs = lower_all_bindings(
        &collected.bindings,
        &bindings_by_lhs,
        &collected.refused_drivers,
        scope,
        &mut ir,
        &mut diagnostics,
    );

    resolve_actuators(
        &collected.actuators,
        &bindings_by_lhs,
        &failed_lhs,
        &collected.refused_drivers,
        scope,
        &mut ir,
        &mut diagnostics,
    );

    audit_unused_signals(
        &collected,
        &ir,
        &bindings_by_lhs,
        &failed_lhs,
        scope,
        &mut diagnostics,
    );

    check_assert_refs(
        &collected.asserts,
        &ir,
        &bindings_by_lhs,
        &collected.refused_drivers,
        &failed_lhs,
        scope,
        &mut diagnostics,
    );

    // The steps raise findings in their own order, and `SynthOutput::diagnostics` promises
    // source order. Stable, so two findings on one span keep the order they were raised in.
    diagnostics.sort_by_key(|d| (d.span.start, d.span.end));

    let scope_has_error = diagnostics.iter().any(|d| d.severity() == Severity::Error);
    out.diagnostics.extend(diagnostics);
    if !scope_has_error {
        out.scoped.scopes.push(ScopedLogicIrEntry {
            kind,
            name: name.to_owned(),
            ir,
        });
    }
}

/// Which source drives a signal, as an index into the scope's sensors or its bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DriverSource {
    /// Index into the scope's collected sensors — a `-> sig.X` tail.
    Sensor(usize),
    /// Index into the scope's collected bindings — a `logic sig.X = ...`
    /// left-hand side.
    Binding(usize),
}

/// One source that drives one signal.
#[derive(Debug)]
struct Driver<'a> {
    name: &'a DottedRef,
    source: DriverSource,
    span: &'a Span,
}

/// The single driver each signal keeps, once the collisions are reported.
///
/// One map keyed by the signal rather than a sensor set beside a binding map, so no signal can be
/// held by both.
#[derive(Debug, Default)]
struct Winners(HashMap<DottedRef, DriverSource>);

impl Winners {
    /// Whether the sensor at `index` is the one its signal kept.
    fn owns_sensor(&self, index: usize) -> bool {
        self.0
            .values()
            .any(|source| *source == DriverSource::Sensor(index))
    }

    /// Left-hand side → index of the binding that owns it, the shape the lowering steps test
    /// ownership with.
    fn bindings(&self) -> HashMap<DottedRef, usize> {
        self.0
            .iter()
            .filter_map(|(name, source)| match source {
                DriverSource::Binding(index) => Some((name.clone(), *index)),
                DriverSource::Sensor(_) => None,
            })
            .collect()
    }
}

/// Reduces every source that drives a signal to one, reporting the rest.
///
/// Sensor tails and `logic` left-hand sides go into one list ordered by span, so every pairing is
/// caught, two sensors included, and the first in the file is the definition whatever its kind.
/// The second is reported against it and later ones are suppressed, so a signal gets one finding.
fn resolve_drivers(
    sensors: &[PendingSensor],
    bindings: &[PendingBinding<'_>],
    scope: ScopeRef<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Winners {
    let mut drivers: Vec<Driver<'_>> = Vec::with_capacity(sensors.len() + bindings.len());
    drivers.extend(sensors.iter().enumerate().map(|(index, s)| Driver {
        name: &s.name,
        source: DriverSource::Sensor(index),
        span: &s.span,
    }));
    drivers.extend(bindings.iter().enumerate().map(|(index, b)| Driver {
        name: b.lhs,
        source: DriverSource::Binding(index),
        span: &b.span,
    }));
    // By where the author wrote them, which makes "first declared here" true.
    drivers.sort_by_key(|d| (d.span.start, d.span.end));

    let mut winners = Winners::default();
    let mut first: HashMap<&DottedRef, &Driver<'_>> = HashMap::new();
    let mut reported: HashSet<&DottedRef> = HashSet::new();
    for driver in &drivers {
        let Some(incumbent) = first.get(driver.name) else {
            first.insert(driver.name, driver);
            winners.0.insert(driver.name.clone(), driver.source);
            continue;
        };
        // One finding per signal.
        if reported.insert(driver.name) {
            diagnostics.push(diag_multiple_drivers(driver, incumbent, scope));
        }
    }
    winners
}

/// Two sources drive one signal: anchored at the later, with a note at the one that keeps it.
fn diag_multiple_drivers(
    later: &Driver<'_>,
    first: &Driver<'_>,
    scope: ScopeRef<'_>,
) -> Diagnostic {
    use DriverSource::{Binding, Sensor};
    let label = scope.label();
    let name = later.name;
    let primary = match (later.source, first.source) {
        (Binding(_), Binding(_)) => format!(
            "{label} `logic {name} = ...` redefines a signal already driven earlier in this scope",
        ),
        (Binding(_), Sensor(_)) => {
            format!("{label} `logic {name} = ...` conflicts with a sensor already driving `{name}`")
        }
        (Sensor(_), Binding(_)) => {
            format!("{label} this sensor emits `{name}`, which a `logic` line already drives")
        }
        (Sensor(_), Sensor(_)) => {
            format!("{label} this sensor emits `{name}`, which another sensor already emits")
        }
    };
    let note = match first.source {
        Sensor(_) => "sensor emits this signal here",
        Binding(_) => "first declared here",
    };
    let fix = match (later.source, first.source) {
        (Sensor(_), Sensor(_)) => format!(
            "Fix: a signal carries one driver. Emit the two sensors into names of their own and combine them, e.g. `logic {name} = {name}1 or {name}2`.",
        ),
        (Binding(_), Binding(_)) => {
            "Fix: rename this binding, or delete one of the `logic` lines so the signal has a single driver."
                .to_owned()
        }
        _ => "Fix: rename the `logic` LHS, or remove the sensor's `-> sig.<name>` tail."
            .to_owned(),
    };
    Diagnostic::new(
        DiagnosticCode::LogicMultipleDrivers,
        later.span.clone(),
        primary,
    )
    .with_note(first.span.clone(), note)
    .with_footer(fix)
}

/// Checks every signal an `assert` names against the scope's definitions. Nothing evaluates the
/// property, but a name nothing defines is wrong regardless, with the code a `logic` line naming
/// it earns.
fn check_assert_refs(
    asserts: &[&AssertIr],
    ir: &LogicIr,
    bindings_by_lhs: &HashMap<DottedRef, usize>,
    refused_drivers: &HashSet<DottedRef>,
    failed_lhs: &HashSet<DottedRef>,
    scope: ScopeRef<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut reported: HashSet<&DottedRef> = HashSet::new();
    for assertion in asserts {
        let span = assertion.span();
        for name in assertion.signal_refs() {
            // A `logic` line that failed to lower was already reported, and does define its name.
            // One name is one finding however often a property names it.
            if ir.signal_defs.contains_key(name)
                || refused_drivers.contains(name)
                || failed_lhs.contains(name)
                || !reported.insert(name)
            {
                continue;
            }
            diagnostics.push(unbound_signal_diagnostic(
                scope,
                name,
                span.clone(),
                ir,
                bindings_by_lhs,
                failed_lhs,
                "an `assert`",
            ));
        }
    }
}

/// Registers each sensor that kept its signal as an [`InputPort`] with its `signal_defs` entry, in
/// source order.
///
/// A losing sensor means an error, which drops the scope anyway; it is skipped so that a scope
/// allowed to survive an error would not hold two input ports for one signal.
fn register_sensors(sensors: &[PendingSensor], winners: &Winners, ir: &mut LogicIr) {
    for (index, sensor) in sensors.iter().enumerate() {
        if !winners.owns_sensor(index) {
            continue;
        }
        let idx = saturating_index(ir.inputs.len());
        ir.inputs.push(InputPort {
            name: sensor.name.clone(),
            span: sensor.span.clone(),
        });
        ir.signal_defs
            .insert(sensor.name.clone(), SignalRef::Input(idx));
    }
}

/// Lowers into the DAG the right-hand side of each binding that owns its left-hand side
/// ([`resolve_drivers`]' answer).
///
/// Returns the names whose lowering failed, which the later steps consult to suppress cascaded
/// findings.
fn lower_all_bindings<'a>(
    bindings: &'a [PendingBinding<'a>],
    bindings_by_lhs: &HashMap<DottedRef, usize>,
    refused_drivers: &HashSet<DottedRef>,
    scope: ScopeRef<'_>,
    ir: &mut LogicIr,
    diagnostics: &mut Vec<Diagnostic>,
) -> HashSet<DottedRef> {
    let mut ctx = LoweringCtx {
        scope,
        bindings,
        bindings_by_lhs,
        refused_drivers,
        in_progress: HashMap::new(),
        chain: Vec::new(),
        failed_lhs: HashSet::new(),
        cse: HashMap::new(),
        depth: 0,
        depth_reported: HashSet::new(),
    };

    for (idx, b) in bindings.iter().enumerate() {
        if bindings_by_lhs.get(b.lhs).copied() != Some(idx) {
            continue;
        }
        if ir.signal_defs.contains_key(b.lhs) || ctx.failed_lhs.contains(b.lhs) {
            continue;
        }
        if lower_binding(b.lhs, ir, &mut ctx, diagnostics).is_err() {
            // Records every failure, a depth refusal included, though that belongs to the path
            // walked rather than this binding. Harmless only because any error drops the scope:
            // a change that lets a scope survive an error must first split the two uses of
            // `failed_lhs`.
            ctx.failed_lhs.insert(b.lhs.clone());
        }
    }

    ctx.failed_lhs
}

/// Resolves each actuator's driver to an [`OutputPort`]. A missing driver is
/// `E_LOGIC_UNBOUND_SIGNAL`, unless its failure or refusal was already reported.
fn resolve_actuators(
    actuators: &[BoundActuator<'_>],
    bindings_by_lhs: &HashMap<DottedRef, usize>,
    failed_lhs: &HashSet<DottedRef>,
    refused_drivers: &HashSet<DottedRef>,
    scope: ScopeRef<'_>,
    ir: &mut LogicIr,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for act in actuators {
        match ir.signal_defs.get(act.driver_name).copied() {
            Some(driver) => ir.outputs.push(OutputPort {
                name: act.driver_name.clone(),
                driver,
                span: act.span.clone(),
            }),
            None if failed_lhs.contains(act.driver_name)
                || refused_drivers.contains(act.driver_name) => {}
            None => diagnostics.push(unbound_signal_diagnostic(
                scope,
                act.driver_name,
                act.span.clone(),
                ir,
                bindings_by_lhs,
                failed_lhs,
                "actuator argument",
            )),
        }
    }
}

/// Warns about every sensor and owning `logic` binding whose signal nothing consumes. Syntactic,
/// so a bare-ref binding (`logic sig.a = sig.b`), which produces no gate, is covered too.
fn audit_unused_signals(
    collected: &ScopeCollected<'_>,
    ir: &LogicIr,
    bindings_by_lhs: &HashMap<DottedRef, usize>,
    failed_lhs: &HashSet<DottedRef>,
    scope: ScopeRef<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let ScopeCollected {
        bindings,
        actuators,
        asserts,
        refused_consumers,
        ..
    } = collected;
    let mut consumed: HashSet<DottedRef> = HashSet::new();
    for b in bindings {
        collect_refs(b.rhs, &mut consumed);
    }
    for act in actuators {
        consumed.insert(act.driver_name.clone());
    }
    // A property observes wires (`spec/redstone` "Verification"), so a signal an `assert` names
    // is consumed.
    for assertion in asserts {
        consumed.extend(assertion.signal_refs().into_iter().cloned());
    }
    // A refused actuator still says what the author meant to wire.
    consumed.extend(refused_consumers.iter().cloned());

    // Sensors are audited from the IR's inputs, so a scope holding only
    // `pressure_plate ... -> sig.a` is warned about a plate wired to nothing.
    for input in &ir.inputs {
        if !consumed.contains(&input.name) {
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::LogicUnusedSignal,
                    input.span.clone(),
                    format!(
                        "{label} this sensor emits `{name}`, which is never consumed by an \
                         actuator, downstream logic, or an `assert`",
                        label = scope.label(),
                        name = input.name,
                    ),
                )
                .with_footer(
                    "Fix: wire an actuator to this signal, reference it from a `logic` binding \
                     or an `assert`, or remove the sensor's `-> sig.<name>` tail.",
                ),
            );
        }
    }
    for (idx, b) in bindings.iter().enumerate() {
        if bindings_by_lhs.get(b.lhs).copied() != Some(idx) || failed_lhs.contains(b.lhs) {
            continue;
        }
        if consumed.contains(b.lhs) {
            continue;
        }
        let label = scope.label();
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

/// Depth-first lowering state for one scope.
struct LoweringCtx<'a> {
    scope: ScopeRef<'a>,
    bindings: &'a [PendingBinding<'a>],
    bindings_by_lhs: &'a HashMap<DottedRef, usize>,
    /// Signals a refused binding would have driven. A reference to one is
    /// a consequence of that refusal rather than a second mistake, so it
    /// is suppressed the way [`Self::failed_lhs`] suppresses the rest.
    refused_drivers: &'a HashSet<DottedRef>,
    /// Signals being lowered, each with its `logic` line's span; meeting one again is a cycle,
    /// reported at that line.
    in_progress: HashMap<DottedRef, Span>,
    /// The bindings being lowered, as indices into [`Self::bindings`], outermost first: what
    /// `E_LOGIC_NESTING_TOO_DEEP` counts, stands on and notes.
    chain: Vec<usize>,
    /// Signals whose resolution already failed, from a `logic` line or an unbound reference, so
    /// the root cause is reported once rather than once per consumer.
    failed_lhs: HashSet<DottedRef>,
    /// Common-subexpression cache keyed by the fully-populated
    /// [`GateKind`]. Symmetric variants are canonicalised at construction,
    /// so `a and b` and `b and a` share a cache slot.
    cse: HashMap<GateKind, u32>,
    /// How many nested [`lower_expr`] frames are open, bounded by
    /// [`MAX_LOWERING_DEPTH`]. [`lower_binding`] does not touch it directly;
    /// it costs depth only through the [`lower_expr`] it calls.
    depth: usize,
    /// Bindings that already reported the limit, keyed by the span of the deepest binding of the
    /// chain rather than the outermost the finding stands on. Branches of one binding that all hit
    /// the limit share a finding, while two chains from one binding (`logic sig.top = sig.p1 or
    /// sig.q1` over two reverse chains) get one each, as [`lower_binary`] reports every root cause.
    depth_reported: HashSet<(usize, usize)>,
}

/// Deepest `logic` lowering recursion before the pass refuses with `E_LOGIC_NESTING_TOO_DEEP`.
///
/// Counted in `lower_expr` frames: an expression node on the path costs one, and a binding a
/// reference chains through two (the operand and that binding's own expression). A chain of
/// one-operator bindings reaches the bound at about half this many, so the diagnostic counts
/// bindings instead.
///
/// Depth follows declaration order, not graph size: a reference is resolved by lowering the
/// binding it names, so a chain declared in reverse recurses once per binding while the same graph
/// in dependency order stays shallow. Without the bound, about 410 reverse-declared bindings
/// overflow a debug build's stack, an abort with no diagnostic. It sits above `cairn_lang_core`'s
/// expression depth bound, so only a chain of bindings can reach it.
pub const MAX_LOWERING_DEPTH: usize = 256;

// One expression reaching the bound would be told to reorder its bindings. Asserted because the
// two constants live in different crates.
const _: () = assert!(
    cairn_lang_core::MAX_EXPR_DEPTH < MAX_LOWERING_DEPTH,
    "an expression the parser accepts must not be able to exhaust the lowering budget on its own",
);

/// Returned by [`lower_binding`] and [`lower_expr`] once a diagnostic has been queued, so the
/// caller abandons the branch.
struct LoweringFailed;

fn lower_binding<'a>(
    lhs: &'a DottedRef,
    ir: &mut LogicIr,
    ctx: &mut LoweringCtx<'a>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<SignalRef, LoweringFailed> {
    let Some(&binding_idx) = ctx.bindings_by_lhs.get(lhs) else {
        // Both callers check first. `LoweringFailed` here would queue no diagnostic yet put the
        // name in `failed_lhs`, silencing a report a new caller owes.
        unreachable!(
            "lower_binding called for `{lhs}`, which owns no binding — the caller must test \
             `bindings_by_lhs` first"
        )
    };
    let binding_span = ctx.bindings[binding_idx].span.clone();
    let rhs = ctx.bindings[binding_idx].rhs;

    ctx.in_progress.insert(lhs.clone(), binding_span.clone());
    ctx.chain.push(binding_idx);
    let result = lower_expr(rhs, &binding_span, ir, ctx, diagnostics);
    ctx.chain.pop();
    ctx.in_progress.remove(lhs);

    match result {
        Ok(sig) => {
            ir.signal_defs.insert(lhs.clone(), sig);
            Ok(sig)
        }
        Err(err) => Err(err),
    }
}

/// Lowers one boolean expression, one level deeper than the caller.
///
/// Every recursive step, nested operands and the descent into a referenced binding through
/// [`resolve_ref`] alike, goes through here, so this is the one place [`MAX_LOWERING_DEPTH`] is
/// enforced.
fn lower_expr<'a>(
    expr: &'a Expr,
    binding_span: &Span,
    ir: &mut LogicIr,
    ctx: &mut LoweringCtx<'a>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<SignalRef, LoweringFailed> {
    if ctx.depth >= MAX_LOWERING_DEPTH {
        if let Some(diagnostic) = nesting_too_deep(ctx) {
            diagnostics.push(diagnostic);
        }
        return Err(LoweringFailed);
    }
    ctx.depth += 1;
    let result = lower_expr_inner(expr, binding_span, ir, ctx, diagnostics);
    ctx.depth -= 1;
    result
}

/// How many bindings after the first `E_LOGIC_NESTING_TOO_DEEP` notes by line. A chain that
/// reaches the limit is over a hundred lines and the first few show its shape; a footer counts
/// the rest.
const CHAIN_NOTES: usize = 3;

/// `E_LOGIC_NESTING_TOO_DEEP` for the chain `ctx` is inside, or `None`
/// when the binding the lowering stopped in has already reported it.
///
/// Anchored on the outermost binding: bindings lower in declaration order, so every binding it
/// reaches is declared after it, and the reordering starts there. The next [`CHAIN_NOTES`] are
/// noted in the order they were entered, which is the order they reference each other (a chain
/// entered 13 → 15 → 14 notes line 15 before line 14), not source order. Deduplicated on the
/// deepest binding; [`LoweringCtx::depth_reported`] says why.
fn nesting_too_deep(ctx: &mut LoweringCtx<'_>) -> Option<Diagnostic> {
    let (Some(&outermost), Some(&deepest)) = (ctx.chain.first(), ctx.chain.last()) else {
        unreachable!("an expression is lowered only inside `lower_binding`, which opens the chain")
    };
    let deepest = &ctx.bindings[deepest].span;
    if !ctx.depth_reported.insert((deepest.start, deepest.end)) {
        return None;
    }
    let count = ctx.chain.len();
    // One expression cannot reach the bound (see the assertion under `MAX_LOWERING_DEPTH`), so
    // the primary needs no singular.
    debug_assert!(
        count >= 2,
        "one expression cannot exhaust the lowering budget, so a chain that does holds two \
         bindings or more",
    );
    let label = ctx.scope.label();
    let mut diagnostic = Diagnostic::new(
        DiagnosticCode::LogicNestingTooDeep,
        ctx.bindings[outermost].span.clone(),
        format!(
            "{label} `logic` lowering nested past {MAX_LOWERING_DEPTH} levels inside {count} \
             chained bindings, starting with this one",
        ),
    );
    for (position, &index) in ctx.chain.iter().enumerate().skip(1).take(CHAIN_NOTES) {
        diagnostic = diagnostic.with_note(
            ctx.bindings[index].span.clone(),
            format!(
                "chained binding {} of {count}, declared after the first",
                position + 1
            ),
        );
    }
    let unnoted = count.saturating_sub(1 + CHAIN_NOTES);
    if unnoted == 1 {
        diagnostic = diagnostic.with_footer("and 1 more chained binding, declared after the first");
    } else if unnoted > 1 {
        diagnostic = diagnostic.with_footer(format!(
            "and {unnoted} more chained bindings, each declared after the first"
        ));
    }
    Some(diagnostic.with_footer(
        "Fix: declare each `logic` binding after the ones it references. A binding is lowered by \
         descending into whatever it names, so a chain written in reverse nests each binding's \
         expression inside the one that references it, while the same graph in dependency order \
         nests none.",
    ))
}

/// Lowers one boolean expression, lowering every operand even after one fails, so
/// `sig.undef1 or sig.undef2` reports both.
fn lower_expr_inner<'a>(
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
        // A variant with no lowering here is refused at its line rather than lowered into a
        // wrong DAG.
        _ => {
            let label = ctx.scope.label();
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::LogicUnsupportedPrimitive,
                    binding_span.clone(),
                    format!(
                        "{label} logic expression uses a boolean primitive the combinational lowering has no gate for",
                    ),
                )
                .with_footer(
                    "Fix: rewrite the expression with `and`, `or` and `not`",
                ),
            );
            Err(LoweringFailed)
        }
    }
}

/// Lowers both operands of a binary gate before failing, so one run reports every root cause.
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
    if let Some(sig) = ir.signal_defs.get(dr).copied() {
        return Ok(sig);
    }

    // Still being lowered: a cycle.
    if let Some(seed_span) = ctx.in_progress.get(dr).cloned() {
        let label = ctx.scope.label();
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

    // Already reported, here or where a refused binding was written.
    if ctx.failed_lhs.contains(dr) || ctx.refused_drivers.contains(dr) {
        return Err(LoweringFailed);
    }

    if ctx.bindings_by_lhs.contains_key(dr) {
        return match lower_binding(dr, ir, ctx, diagnostics) {
            Ok(sig) => Ok(sig),
            Err(err) => {
                ctx.failed_lhs.insert(dr.clone());
                Err(err)
            }
        };
    }

    // Unbound. Recorded so a later reference does not repeat the finding.
    diagnostics.push(unbound_signal_diagnostic(
        ctx.scope,
        dr,
        binding_span.clone(),
        ir,
        ctx.bindings_by_lhs,
        &ctx.failed_lhs,
        "`logic` binding",
    ));
    ctx.failed_lhs.insert(dr.clone());
    Err(LoweringFailed)
}

/// Reuses the node of an already interned [`GateKind`], or pushes a new one.
fn intern_gate(
    ir: &mut LogicIr,
    ctx: &mut LoweringCtx<'_>,
    kind: GateKind,
    span: Span,
) -> SignalRef {
    if let Some(&idx) = ctx.cse.get(&kind) {
        return SignalRef::Gate(idx);
    }
    let idx = saturating_index(ir.nodes.len());
    ir.nodes.push(GateNode { kind, span });
    ctx.cse.insert(kind, idx);
    SignalRef::Gate(idx)
}

/// Orders a symmetric gate's operands, inputs before gates and then by index, so `and(a, b)` and
/// `and(b, a)` share a CSE slot.
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
    scope: ScopeRef<'_>,
    dr: &DottedRef,
    span: Span,
    ir: &LogicIr,
    bindings_by_lhs: &HashMap<DottedRef, usize>,
    failed_lhs: &HashSet<DottedRef>,
    source_label: &str,
) -> Diagnostic {
    // A name whose `logic` line failed to lower is no valid rename target.
    let mut candidates: Vec<String> = ir
        .inputs
        .iter()
        .map(|p| p.name.to_string())
        .chain(
            bindings_by_lhs
                .keys()
                .filter(|lhs| !failed_lhs.contains(*lhs))
                .map(std::string::ToString::to_string),
        )
        .collect();
    candidates.sort();
    candidates.dedup();
    let label = scope.label();
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
    } else if is_signal_name(dr) {
        let joined = candidates.join(", ");
        diag = diag.with_footer(format!(
            "Valid signals in scope: {joined}. Fix: rename to a defined signal, or drive `{dr}` from a sensor / `logic` line.",
        ));
    } else {
        // Neither a sensor tail nor a `logic` left-hand side accepts a name
        // that is not a signal name, so offering to drive it from one would
        // send the author to a refusal.
        let joined = candidates.join(", ");
        diag = diag.with_footer(format!(
            "Valid signals in scope: {joined}. Fix: rename to a defined signal.",
        ));
    }
    diag
}

/// Adds every signal an expression references to `out`.
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

/// Which scope the pass is working on, as the diagnostics name it.
#[derive(Debug, Clone, Copy)]
struct ScopeRef<'a> {
    kind: ScopeKind,
    name: &'a str,
}

impl ScopeRef<'_> {
    /// `struct=gatehouse:`, the prefix of every finding's primary message, so a multi-scope
    /// module's findings say which scope they are in.
    fn label(self) -> String {
        format!("{}={}:", self.kind.label(), self.name)
    }
}
