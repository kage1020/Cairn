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
//! Detection contract. `spec/redstone` §14.2 writes each binding on one
//! component, and a binding elsewhere describes no circuit, so what a
//! line is *for* is read from the line and then held to that:
//! - **Sensor**: a `-> value` tail on a member whose keyword is in
//!   [`SENSOR_HOSTS`]. A follow-up recognizer for `lever` / `button` /
//!   `daylight` / `observer` costs one entry in that table.
//! - **Actuator**: one of the argument keys in [`ACTUATOR_BINDINGS`]
//!   (`opened_by` / `powered_by` / `lit_by` / `fired_by`) on the
//!   component that table pairs the key with.
//!
//! The *claim* comes from either side of a pair and the value is then
//! checked against it. A tail claims a sensor whatever the value is; an
//! actuator key claims a wire whatever the value is; and a `sig.X` value
//! still claims one under a key nothing reads, which is
//! `E_LOGIC_UNKNOWN_BINDING_KEY`. Reading the claim off the value alone
//! is what let a well-spelled key with a malformed value fall through
//! every branch and vanish.
//!
//! Three faults, and each is reported by the one finding the other
//! repairs would not answer:
//! - the **host** cannot carry this binding — `E_LOGIC_MISPLACED_BINDING`,
//!   asked first, because no edit to the value makes `walls` carry a tail;
//! - the **key** is one nothing reads — `E_LOGIC_UNKNOWN_BINDING_KEY`,
//!   with the nearest key it might be a typo for;
//! - the **value** names no signal — `E_LOGIC_INVALID_SIGNAL`, the code
//!   the `logic` left-hand side has always taken, because all three
//!   positions are the same rule about the same namespace.
//!
//! A pair that claims nothing on either side is an argument like any
//! other. What its *key* means is the per-keyword schema question, which
//! no pass answers yet, so `hieght=3` is nobody's finding here.
//!
//! The `[selector]` carries the same pairs and is never a binding site:
//! the brackets pick a member that already exists and the binding is
//! written after them. It is walked, and answered by the same three
//! faults — a bracketed pair whose only fault *is* the brackets earns
//! `E_LOGIC_MISPLACED_BINDING` saying so, and one that would still be
//! wrong outside them is told that instead.
//!
//! Cascade suppression: any signal name that has already produced an
//! `E_LOGIC_UNBOUND_SIGNAL` (either because a `logic` binding for it
//! failed to lower or because a raw reference to it was unresolved) is
//! recorded in `failed_lhs`; a name a refused binding would have driven
//! or consumed is recorded in `ScopeCollected`'s two `refused_*` sets.
//! Every downstream reference — RHS lookups, actuator resolution, the
//! `assert` walk, the unused-signal audit — checks those before emitting
//! another diagnostic, so a single root cause produces exactly one
//! finding.

use std::collections::{HashMap, HashSet};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::logic_ir::{
    GateKind, GateNode, InputPort, LogicIr, OutputPort, ScopeKind, ScopedLogicIr,
    ScopedLogicIrEntry, SignalRef,
};
use cairn_lang_core::ast::{DottedRef, Expr, SIGNAL_HEAD, Value, ValueKind};
use cairn_lang_core::check::Severity;
use cairn_lang_core::error::Span;
use cairn_lang_core::intent::{
    AssertIr, DefIr, IntentModule, LogicBinding, Member, MemberBody, MemberRole, SiteIr, StructIr,
    ValueWithSpan, known_keywords,
};
use cairn_lang_core::suggest::nearest_match;

/// Each actuator argument key from `spec/redstone` §14.2, paired with the
/// component keyword that carries it.
///
/// §14.2 does not offer these keys as free-floating attributes: it writes
/// each one on one component (`lamp ... lit_by=`, `piston ... powered_by=`,
/// `door ... opened_by=`, `dispenser ... fired_by=`). Reading only the
/// value made every member an actuator, so `walls ... powered_by=sig.x`
/// became a live output port on a member with no component behind it.
///
/// Of the four hosts, only `door` is a keyword the surface accepts today —
/// `lamp`, `piston`, and `dispenser` are not in the role table, so the
/// other three keys have no legal host yet and are refused wherever they
/// are written. The table is what changes when those keywords land; the
/// check does not.
/// Public so `tests/actuator_schema.rs` can compare this array against
/// `MemberRole::arguments` outright rather than against a copy of it.
pub const ACTUATOR_BINDINGS: &[(&str, &str)] = &[
    ("opened_by", "door"),
    ("powered_by", "piston"),
    ("lit_by", "lamp"),
    ("fired_by", "dispenser"),
];

/// The component keywords that may carry a `->` sensor tail.
///
/// §14.2's sensor set is `lever` / `button` / `daylight` / `observer`, none
/// of which the surface accepts yet; `pressure_plate` is the one sensor the
/// role table knows, and it is the only member a tail may sit on. Without
/// the check a `walls ... -> sig.w` registered an input port and reached
/// placement as a pad for a signal no component emits.
/// Public for the same reason as [`ACTUATOR_BINDINGS`].
pub const SENSOR_HOSTS: &[&str] = &["pressure_plate"];

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
/// One [`LogicIr`] entry per struct / def / site whose body carries any
/// redstone content — a sensor, an actuator, a `logic` binding, or an
/// `assert`. Scopes with none of those are omitted so the JSON dump stays
/// proportional to redstone content.
///
/// Expects the caller to have run `cairn_lang_core::check` and stopped on
/// its Errors, which `cairn synth` does. This pass reads a member's role
/// to decide which bindings it may carry, so a member whose keyword the
/// role table does not know is skipped rather than reported: the finding
/// that matters there is `E_UNKNOWN_KEYWORD`, and it is not this pass's
/// to raise.
pub fn synthesize(module: &IntentModule) -> SynthOutput {
    // `IntentModule` files the three scope kinds into three vectors, each
    // one internally in source order, so walking them one after another
    // dumps every `struct` before every `def` however the module was
    // written. Both readers of this walk are documented as source-ordered
    // — the IR dump reads top-to-bottom the way the file does, and the
    // findings come out in the order a reader meets the lines — so the
    // three are merged back by span here instead.
    let mut scopes: Vec<ModuleScope<'_>> =
        Vec::with_capacity(module.structs.len() + module.defs.len() + module.sites.len());
    scopes.extend(module.structs.iter().map(ModuleScope::Struct));
    scopes.extend(module.defs.iter().map(ModuleScope::Def));
    scopes.extend(module.sites.iter().map(ModuleScope::Site));
    scopes.sort_by_key(ModuleScope::span);

    let mut out = SynthOutput::default();
    for scope in scopes {
        match scope {
            ModuleScope::Struct(s) => lower_struct(s, &mut out),
            ModuleScope::Def(d) => lower_def(d, &mut out),
            ModuleScope::Site(s) => lower_site(s, &mut out),
        }
    }
    out
}

/// One top-level scope of a module, whichever of the three kinds it is.
///
/// Exists so [`synthesize`] can hold all three in one list long enough to
/// order them; every other pass reads the kind-specific vectors directly.
enum ModuleScope<'a> {
    Struct(&'a StructIr),
    Def(&'a DefIr),
    Site(&'a SiteIr),
}

impl ModuleScope<'_> {
    /// Byte range of the `struct` / `def` / `site` block, as the sort
    /// key. Two scopes cannot begin at the same byte, so the start alone
    /// would order them — the end is carried anyway so this key matches
    /// the one the diagnostic sort uses, and so a tie can never fall back
    /// on the by-kind order this function exists to undo.
    fn span(&self) -> (usize, usize) {
        match self {
            Self::Struct(s) => (s.span.start, s.span.end),
            Self::Def(d) => (d.span.start, d.span.end),
            Self::Site(s) => (s.span.start, s.span.end),
        }
    }
}

fn lower_struct(s: &StructIr, out: &mut SynthOutput) {
    let scope = ScopeRef {
        kind: ScopeKind::Struct,
        name: &s.name,
    };
    let mut collected = ScopeCollected::default();
    collect_body(&s.members, &s.logic, &s.asserts, scope, &mut collected);
    finish_scope(scope, collected, out);
}

fn lower_def(d: &DefIr, out: &mut SynthOutput) {
    let scope = ScopeRef {
        kind: ScopeKind::Def,
        name: &d.name,
    };
    let mut collected = ScopeCollected::default();
    collect_body(&d.members, &d.logic, &d.asserts, scope, &mut collected);
    finish_scope(scope, collected, out);
}

fn lower_site(s: &SiteIr, out: &mut SynthOutput) {
    let mut collected = ScopeCollected::default();
    // A site body carries `place` / `connect` rows, which never emit sensor
    // bindings themselves, so in practice only `s.logic` contributes here.
    // The walk still descends because `collect_body` is shared with the two
    // scopes that do have members — not because a site may host a fixture:
    // `check::member_scope` reports `pressure_plate` / `door` written among
    // a site's rows as `E_MISPLACED_MEMBER`, since nothing lowers them into
    // a block and a sensor with no block behind it senses nothing. Letting
    // a site host fixtures for real is a lowering change, and this walk
    // would already be ready for it.
    let scope = ScopeRef {
        kind: ScopeKind::Site,
        name: &s.name,
    };
    collect_body(&s.placements, &s.logic, &s.asserts, scope, &mut collected);
    finish_scope(scope, collected, out);
}

/// Per-scope working set built during the collection pass. Kept separate
/// from [`LogicIr`] because a mid-collection error must never surface a
/// half-built IR to the caller — the finish step decides whether the
/// working set becomes an IR or an error report.
impl ScopeCollected<'_> {
    /// Whether this scope has anything for the finish step to do.
    ///
    /// Destructured rather than written as a list of `is_empty()` calls:
    /// the list form is a hand-written predicate over a subset of the
    /// fields, and a field added later lands in the gap silently. That is
    /// what happened to `asserts` — a scope whose only redstone content
    /// was an `assert` returned here before anything checked the signals
    /// it names. Written this way the next field is a compile error.
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

#[derive(Debug, Default)]
struct ScopeCollected<'a> {
    /// Findings raised while walking the body. Collection is where a
    /// binding's *host* is known — the finish step sees a flat list of
    /// pending sensors and actuators and no longer knows which member each
    /// came from — so these are raised here and carried along.
    diagnostics: Vec<Diagnostic>,
    /// `assert` properties, in source order. Their evaluation waits on the
    /// simulator, but the signals they name are checked as soon as the
    /// scope's `signal_defs` is complete.
    asserts: Vec<&'a AssertIr>,
    /// Signals a refused binding would have driven — a `-> sig.X` tail on
    /// a member that is not a sensor, or a `logic` LHS outside the `sig.`
    /// namespace. A later line referencing one of these is not a second
    /// mistake, so the unbound-signal report skips it: the crate reports
    /// the root cause once.
    refused_drivers: HashSet<DottedRef>,
    /// Signals a refused binding would have consumed — an actuator key on
    /// the wrong member, or a signal-valued key nothing reads. The author
    /// did wire the actuator, so the unused-signal audit counts these as
    /// consumed rather than adding a warning about the wire they meant to
    /// draw.
    refused_consumers: HashSet<DottedRef>,
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
    asserts: &'a [AssertIr],
    scope: ScopeRef<'_>,
    out: &mut ScopeCollected<'a>,
) {
    for m in members {
        collect_member(m, scope, out);
    }
    for b in logic {
        collect_binding(b, scope, out);
    }
    // The walk above reaches every `logic` line nested under a member
    // before it reaches the scope's own, so a binding written later inside
    // a `level` would otherwise be numbered — and reported — ahead of one
    // written earlier at the top level. Source order is what both readers
    // want: the node numbering is documented as reading top-to-bottom the
    // way the file does, and `E_LOGIC_MULTIPLE_DRIVERS` names the *first*
    // declaration in its note, which is only true if "first" means first
    // in the file. `MemberBody` splits a body into a members vector and a
    // logic vector, so the interleaving the author wrote survives nowhere
    // but in the spans — sorting by them is the reconstruction, not a
    // convenience.
    //
    // `resolve_drivers` sorts a list of its own for the same reason, and
    // does not replace this one: that list decides which driver keeps a
    // signal, while this one is the order the surviving bindings lower in,
    // which is the order the IR's nodes are numbered in.
    out.bindings.sort_by_key(|b| (b.span.start, b.span.end));
    out.asserts.extend(asserts);
}

fn collect_member<'a>(m: &'a Member, scope: ScopeRef<'_>, out: &mut ScopeCollected<'a>) {
    // A member whose keyword is not in the role table is already
    // `E_UNKNOWN_KEYWORD` from the `check` pass. Reading its bindings
    // could only add a second finding about a component that does not
    // exist, so the whole member is left to that one — but what it would
    // have driven or consumed is still recorded, or the author of
    // `lamp id=l1 lit_by=sig.x` is told that `sig.x` is unconsumed on top
    // of being told that `lamp` is not a keyword.
    let unknown_keyword = matches!(m.role, MemberRole::Other(_));

    // Sensor: a `-> value` tail claims to emit a signal, whatever the
    // value turns out to be. Reading the claim off the *tail* and not off
    // the value is what makes `-> a` a mistake rather than a line nobody
    // looks at: the author who writes one has written a sensor.
    //
    // The host is asked first. No edit to the value makes `walls` carry a
    // tail, so telling that author about the `sig.` namespace would send
    // them round the loop to be told about the host next time.
    if let Some(binding) = &m.binding {
        let named = signal_named_by(binding);
        if unknown_keyword || !SENSOR_HOSTS.contains(&m.role.keyword()) {
            if !unknown_keyword {
                out.diagnostics
                    .push(diag_misplaced_sensor(m, binding, scope));
            }
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

    // Actuator: an `opened_by=` / `powered_by=` / ... argument on the
    // component §14.2 pairs that key with. The key claims the binding on
    // its own — the value used to have to be a `sig.X` dotted ref for this
    // loop to look at the field at all, which is how a well-spelled key
    // with a malformed value went straight past it.
    for (key, vspan) in &m.intent_state.fields {
        let named = signal_named_by(&vspan.value);
        let Some(claim) = binding_claim(key, named) else {
            // Neither side says a binding was meant. What the key *does*
            // mean is the argument-schema question, which no pass answers
            // yet.
            continue;
        };
        match claim {
            BindingClaim::UnreadKey => {
                // The value is a signal, so the author meant to wire
                // something; the key is not a binding anyone reads.
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
                    out.actuators.push(PendingActuator {
                        driver_name: dr.clone(),
                        span: vspan.span.clone(),
                    });
                    continue;
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

    // The `[selector]` carries the same `key=value` pairs and is not a
    // binding site: §14.2 writes the actuator patch as
    // `door[id=front] opened_by=sig.x`, with the binding after the
    // brackets, and `block_array`'s patch recogniser already refuses any
    // selector attribute but `id=`. Walked here for the same answer in
    // this pass's vocabulary, and for every host rather than the door
    // patch alone.
    for (key, vspan) in m.selector.iter().flatten() {
        let named = signal_named_by(&vspan.value);
        let Some(claim) = binding_claim(key, named) else {
            continue;
        };
        if !unknown_keyword {
            // Report whichever fault moving the pair out of the brackets
            // would not fix, which is the same rule the argument list
            // follows one level in. `door[id=front,lit_by=sig.a]` moved
            // out is still a `lit_by=` on a door, and
            // `door[id=front,oepend_by=sig.a]` moved out is still a key
            // nothing reads — so telling either author about the
            // brackets first would be advice that does not work. Only a
            // key this member does host has the brackets as its one
            // fault.
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

    // Nested body (e.g. `level y=0` block) — same triple of members /
    // logic / asserts as the top-level, so recurse.
    let MemberBody {
        members: children,
        logic,
        asserts,
    } = &m.children;
    for child in children {
        collect_member(child, scope, out);
    }
    for b in logic {
        collect_binding(b, scope, out);
    }
    out.asserts.extend(asserts);
}

/// The signal reference a value names, or `None` for a value that names
/// no signal at all.
///
/// One reading of "is this a signal reference" for all three positions
/// that hold one — the sensor tail, an argument value, and a selector
/// attribute — so the three cannot start disagreeing about what counts.
///
/// Two segments, not just a `sig.` head. `sig.a.b` used to pass here and
/// register a port whose name the block-array pass then refused
/// (`must be a two-segment signal reference`), so the front end and the
/// lowering disagreed about what a signal name is and the front end was
/// the lenient one.
fn signal_named_by(value: &Value) -> Option<&DottedRef> {
    match &value.kind {
        ValueKind::DotRef(dr) if dr.head() == SIGNAL_HEAD && dr.tail().len() == 1 => Some(dr),
        _ => None,
    }
}

/// Why a `key=value` pair is read as a binding the author meant to write.
#[derive(Debug, Clone, Copy)]
enum BindingClaim {
    /// The key is one of [`ACTUATOR_BINDINGS`], which is a binding whatever
    /// the value says. Carries the component §14.2 pairs the key with —
    /// `'static` because it comes from that table and from nowhere else,
    /// which is what the borrow says and a lifetime parameter would not.
    Actuator(&'static str),
    /// The key is not a binding anyone reads, and the value is a signal —
    /// so the author meant to wire something and put it under a key that
    /// nothing consumes. Only built when a signal *was* named, which is
    /// what every caller reads back off `named` rather than off here.
    UnreadKey,
}

/// Whether a pair claims to be a binding, from either side.
///
/// Either side is the point. Keying only on the value is what let a
/// well-spelled key with a malformed value fall through; keying only on
/// the key would lose [`DiagnosticCode::LogicUnknownBindingKey`], which
/// exists because a `sig.` value says a binding was meant even where the
/// key does not. A pair that says nothing on either side is an argument
/// like any other, and what its key means is a question no pass answers
/// yet.
fn binding_claim(key: &str, named: Option<&DottedRef>) -> Option<BindingClaim> {
    if let Some((_, host)) = ACTUATOR_BINDINGS.iter().find(|(k, _)| *k == key) {
        return Some(BindingClaim::Actuator(host));
    }
    named.map(|_| BindingClaim::UnreadKey)
}

/// Take one `logic` line, or refuse its left-hand side.
///
/// Sensors emit into the `sig.` namespace and actuators consume from it, so
/// a binding named outside it can never be read. Refusing at collection is
/// what keeps the gate out of the DAG: lowered, it took a cell and a
/// placement coordinate for a signal with no consumer, and said so only as
/// `W_LOGIC_UNUSED_SIGNAL`.
fn collect_binding<'a>(b: &'a LogicBinding, scope: ScopeRef<'_>, out: &mut ScopeCollected<'a>) {
    if b.lhs.head() != SIGNAL_HEAD {
        out.diagnostics.push(
            Diagnostic::new(
                DiagnosticCode::LogicInvalidSignal,
                b.span.clone(),
                format!(
                    "{label} `logic {lhs} = ...` names `{lhs}`, which is outside the \
                     `{SIGNAL_HEAD}.` namespace sensors emit into and actuators read from",
                    label = scope.label(),
                    lhs = b.lhs,
                ),
            )
            .with_footer(format!(
                "Fix: rename the left-hand side to `{SIGNAL_HEAD}.<name>`, or delete the \
                 binding if nothing was meant to read it.",
            )),
        );
        out.refused_drivers.insert(b.lhs.clone());
        // The left-hand side is what was refused; the right-hand side is
        // still a list of signals this line reads, and warning that they
        // are unconsumed would be the same mistake reported twice.
        let mut refs = HashSet::new();
        collect_refs(&b.rhs, &mut refs);
        out.refused_consumers.extend(refs);
        return;
    }
    out.bindings.push(PendingBinding {
        lhs: &b.lhs,
        rhs: &b.rhs,
        span: b.span.clone(),
    });
}

/// The repair for a value that names no signal, when there is one.
///
/// A bare identifier is a name with the namespace left off, so
/// `opened_by=a` has exactly one reading and the message can offer it.
/// Every other kind is a value of some other sort standing where a name
/// belongs — `opened_by=3` names nothing that adding `sig.` would fix —
/// so the message asks for the shape instead of guessing at a name.
///
/// `sig` is the identifier that is not a name: the author wrote the
/// namespace and stopped, and `sig.sig` is not what they meant. It takes
/// the shape-only advice with every other kind.
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

/// A `-> value` tail on a sensor whose value names no signal.
///
/// The tail is what says a signal was meant; the value is what fails to
/// name one. Reported at the value, because that is the text to change.
fn diag_tail_names_no_signal(m: &Member, binding: &Value, scope: ScopeRef<'_>) -> Diagnostic {
    Diagnostic::new(
        DiagnosticCode::LogicInvalidSignal,
        binding.span.clone(),
        format!(
            "{label} `{keyword}` emits into a signal, but its `->` tail is \
             {found} rather than a name in the `{SIGNAL_HEAD}.` namespace \
             sensors emit into and actuators read from",
            label = scope.label(),
            keyword = m.role.keyword(),
            found = binding.describe(),
        ),
    )
    .with_note(m.span.clone(), "declared here")
    .with_footer(signal_value_fix(
        binding,
        "drop the `->` tail if this sensor drives nothing.",
    ))
}

/// An actuator key on its own host whose value names no signal.
fn diag_argument_names_no_signal(
    m: &Member,
    key: &str,
    vspan: &ValueWithSpan,
    scope: ScopeRef<'_>,
) -> Diagnostic {
    Diagnostic::new(
        DiagnosticCode::LogicInvalidSignal,
        vspan.span.clone(),
        format!(
            "{label} `{key}=` wires this `{keyword}` to a signal, but names \
             {found} rather than a name in the `{SIGNAL_HEAD}.` namespace \
             sensors emit into and actuators read from",
            label = scope.label(),
            keyword = m.role.keyword(),
            found = vspan.value.describe(),
        ),
    )
    .with_note(m.span.clone(), "declared here")
    .with_footer(signal_value_fix(
        &vspan.value,
        "drop the argument if this component is not wired.",
    ))
}

/// A binding written inside the `[selector]` rather than after it.
///
/// The brackets select a member that already exists; what the line does
/// to it is written after them. Only reached for a key this member does
/// host, so the brackets are the one thing wrong with the line and the
/// `Fix:` can rebuild it verbatim — a key the member cannot host, or one
/// nothing reads, goes to the finding that owns *that* fault, because
/// moving it out of the brackets would not answer it.
fn diag_binding_inside_selector(
    m: &Member,
    key: &str,
    vspan: &ValueWithSpan,
    scope: ScopeRef<'_>,
) -> Diagnostic {
    debug_assert!(
        ACTUATOR_BINDINGS
            .iter()
            .any(|(k, host)| *k == key && *host == m.role.keyword()),
        "the `Fix:` below rebuilds the line with this key on this member, \
         which only compiles for a key §14.2 pairs with it",
    );
    Diagnostic::new(
        DiagnosticCode::LogicMisplacedBinding,
        vspan.span.clone(),
        format!(
            "{label} `{key}=` is inside `{keyword}`'s `[selector]`, where \
             nothing reads it; the brackets pick the member and the binding \
             is written after them",
            label = scope.label(),
            keyword = m.role.keyword(),
        ),
    )
    .with_note(m.span.clone(), "declared here")
    .with_footer(format!(
        "Fix: move `{key}=` out of the brackets, as in \
         `{keyword}[id=<label>] {key}={SIGNAL_HEAD}.<name>`.",
        keyword = m.role.keyword(),
    ))
}

/// A `->` tail on a member that is not a sensor.
///
/// Whatever the tail names: the host is asked before the value, so
/// `walls -> a` reaches here rather than the value-side refusal.
fn diag_misplaced_sensor(m: &Member, binding: &Value, scope: ScopeRef<'_>) -> Diagnostic {
    Diagnostic::new(
        DiagnosticCode::LogicMisplacedBinding,
        binding.span.clone(),
        format!(
            "{label} `{keyword}` cannot emit a signal; only a sensor carries a \
             `-> {SIGNAL_HEAD}.<name>` tail",
            label = scope.label(),
            keyword = m.role.keyword(),
        ),
    )
    .with_note(m.span.clone(), "declared here")
    .with_footer(format!(
        "Fix: move the tail onto a sensor. `{hosts}` {verb} the sensor {noun} the surface \
         accepts today; `spec/redstone` §14.2 also lists `lever`, `button`, `daylight`, \
         and `observer`.",
        hosts = SENSOR_HOSTS.join("`, `"),
        verb = if SENSOR_HOSTS.len() == 1 { "is" } else { "are" },
        noun = if SENSOR_HOSTS.len() == 1 {
            "keyword"
        } else {
            "keywords"
        },
    ))
}

/// An actuator key on a member that is not the component §14.2 pairs it
/// with.
fn diag_misplaced_actuator(
    m: &Member,
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
            keyword = m.role.keyword(),
        ),
    )
    .with_note(m.span.clone(), "declared here")
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

/// An argument whose value is a signal, under a key nothing reads.
fn diag_unknown_binding_key(
    m: &Member,
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
    .with_note(m.span.clone(), "declared here");
    match nearest_match(key, keys.iter().copied()) {
        Some(near) => diag.with_footer(format!("did you mean `{near}=`?")),
        None => diag.with_footer(format!(
            "Fix: use one of `{}=`, or drop the argument if this member is not an actuator.",
            keys.join("=`, `"),
        )),
    }
}

/// Finalise one scope's collected state. Success writes a
/// [`ScopedLogicIrEntry`] onto `out.scoped`; every finding — including
/// warnings that survive alongside a well-formed IR — appends to
/// `out.diagnostics`. Each step below owns one specific transformation of
/// the working set so the top-level flow reads as a linear pipeline.
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

    // Collection raises its findings before the phases run, and each
    // phase raises in its own order, so which finding comes first has
    // never tracked which line comes first. `SynthOutput::diagnostics`
    // promises source order and `cairn synth` prints them in the order it
    // is handed, so the sort belongs here. Stable, so two findings on one
    // span keep the order the passes raised them in.
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

/// Which source drives a signal, and where in its list it sits.
///
/// The kind and the index travel together because they are one fact: the
/// index means nothing without knowing which list it points into. Carried
/// as two fields they read as independent, and the correlation lives in
/// whoever remembers to check the tag first.
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
/// One map keyed by the signal, rather than a sensor set beside a binding
/// map: this PR's whole claim is that a signal carries one driver, and two
/// containers can hold a name in both at once. Here that state cannot be
/// written down.
#[derive(Debug, Default)]
struct Winners(HashMap<DottedRef, DriverSource>);

impl Winners {
    /// Whether the sensor at `index` is the one its signal kept.
    fn owns_sensor(&self, index: usize) -> bool {
        self.0
            .values()
            .any(|source| *source == DriverSource::Sensor(index))
    }

    /// LHS name to the index of the binding that owns it — the shape the
    /// lowering passes take, since they test ownership by comparing an
    /// index against the one the name maps to.
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

/// Reduce every source that drives a signal to one, reporting the rest.
///
/// A signal is driven by a sensor's `-> sig.X` tail or by a `logic` line's
/// left-hand side, and the two used to be checked by different passes in a
/// fixed order — sensors registered first, `logic` duplicates second,
/// sensor-versus-`logic` third, with a shared "already reported" set. That
/// arrangement had three consequences, all of them here:
///
/// - two sensors on one signal were the pairing nobody checked, so the
///   second was dropped in silence and its plate wired to nothing;
/// - a signal driven by a sensor *and* two `logic` lines reported only the
///   `logic` pair, because the earlier pass had already claimed the name in
///   the shared set, so the root cause was the one thing not mentioned;
/// - "first" meant "sensor", not "first in the file", so a `logic` line
///   written above the sensor it collides with was the line accused.
///
/// One list of drivers ordered by span answers all three: the first is the
/// definition, the second is reported against it, and the rest are
/// suppressed so the finding still lands once per signal.
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
    // Ordered by where the author wrote them, which is what makes "first
    // declared here" true. `sort_by_key` is stable, so a sensor tail and a
    // `logic` line that somehow shared a start byte would keep the order
    // they were collected in rather than swapping between runs.
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
        // One finding per signal. A third driver adds nothing an author
        // has not already been told to go and look at.
        if reported.insert(driver.name) {
            diagnostics.push(diag_multiple_drivers(driver, incumbent, scope));
        }
    }
    winners
}

/// Two sources drive one signal.
///
/// Anchored at the later of the two, with a note at the one that keeps the
/// signal — the shape `E_LOGIC_MULTIPLE_DRIVERS` has always had, now with
/// both sides named whichever kind they are.
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

/// Check every signal an `assert` names against the scope's definitions.
///
/// The simulator that would evaluate these is unbuilt — `roadmap.md`
/// schedules it for M6 — but a property over a name nothing emits and
/// nothing defines is not waiting on the simulator to be wrong. It is the
/// same finding a `logic` line naming that signal earns, so it is the
/// same code.
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
            // `failed_lhs` joins the two sets the rest of the pass
            // consults: a `logic` line that failed to lower has already
            // been reported, and its LHS *is* defined by a `logic` line —
            // saying otherwise here would be false as well as second.
            // A property may also name one signal twice
            // (`always(sig.x -> eventually sig.x ...)`), and one name is
            // one finding.
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

/// Register each sensor that won its signal as an [`InputPort`] and its
/// `signal_defs` entry, in source order.
///
/// A sensor that lost is skipped rather than overwriting the winner's
/// entry. Nothing observable rests on that today — a losing sensor means
/// [`resolve_drivers`] raised an error, and `finish_scope` drops the whole
/// scope on one, so the IR reaches nobody and every finding the later
/// passes raise names the same signals either way. It is here for the
/// reason the `failed_lhs` comment in [`lower_all_bindings`] gives: a
/// change that lets a scope survive an Error would otherwise inherit an
/// IR where one signal has two input ports and `signal_defs` points at the
/// one the author was told to remove.
fn register_sensors(sensors: &[PendingSensor], winners: &Winners, ir: &mut LogicIr) {
    for (index, sensor) in sensors.iter().enumerate() {
        if !winners.owns_sensor(index) {
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

/// Lower the RHS of each binding that owns its LHS into the DAG.
///
/// Ownership is [`resolve_drivers`]' answer, so a binding that lost its
/// name — to a sensor or to another `logic` line, whichever came first in
/// the file — is not in `bindings_by_lhs` and never reaches here. "First"
/// means first by span rather than first collected, which differ whenever
/// a `level` block is involved.
///
/// Returns the set of LHS names whose lowering failed — the actuator
/// resolution, the `assert` walk, and the unused audit consult it to
/// suppress cascaded diagnostics.
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
        failed_lhs: HashSet::new(),
        cse: HashMap::new(),
        depth: 0,
        depth_reported: HashSet::new(),
    };

    for (idx, b) in bindings.iter().enumerate() {
        // Only the binding that owns its LHS is lowered; a binding that
        // lost the name to a sensor or to an earlier line is not in
        // `bindings_by_lhs` at all, and `resolve_drivers` has already said
        // so.
        if bindings_by_lhs.get(b.lhs).copied() != Some(idx) {
            continue;
        }
        if ir.signal_defs.contains_key(b.lhs) || ctx.failed_lhs.contains(b.lhs) {
            continue;
        }
        if lower_binding(b.lhs, ir, &mut ctx, diagnostics).is_err() {
            // Records every failure kind, including a depth refusal, which is
            // a property of the path walked rather than of this binding. That
            // is only harmless because `finish_scope` drops the whole scope on
            // any Error, so the cascade suppression `failed_lhs` also drives
            // (`resolve_actuators`, `audit_unused_signals`) never reports on an
            // IR anyone sees. A future change that lets a scope survive an
            // Error has to split the two uses apart first.
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
    refused_drivers: &HashSet<DottedRef>,
    scope: ScopeRef<'_>,
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
            None if failed_lhs.contains(&act.driver_name)
                || refused_drivers.contains(&act.driver_name) => {}
            None => diagnostics.push(unbound_signal_diagnostic(
                scope,
                &act.driver_name,
                act.span.clone(),
                ir,
                bindings_by_lhs,
                failed_lhs,
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
    // A property observes wires — that is what §14.7 is for — so a signal
    // an `assert` names is read even when no actuator reads it. Checking
    // an assert's references and then not counting them would warn that a
    // signal is unused in the one place the author wrote down what it is
    // for.
    for assertion in asserts {
        consumed.extend(assertion.signal_refs().into_iter().cloned());
    }
    // An actuator the front end refused still says what the author meant
    // to wire. Warning that the signal is unconsumed on top of the finding
    // that took the actuator away is the same mistake reported twice.
    consumed.extend(refused_consumers.iter().cloned());

    // Sensors are audited from the result side rather than by shape: a
    // scope whose only content is `pressure_plate ... -> sig.a` lowers to
    // an IR with one input and no output, which is a build with a plate
    // wired to nothing — the same silence this pass exists to break.
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

/// DFS lowering context for one scope. Held on a separate struct so the
/// recursive helpers can pass a single `&mut` instead of threading five
/// arguments.
struct LoweringCtx<'a> {
    scope: ScopeRef<'a>,
    bindings: &'a [PendingBinding<'a>],
    bindings_by_lhs: &'a HashMap<DottedRef, usize>,
    /// Signals a refused binding would have driven. A reference to one is
    /// a consequence of that refusal rather than a second mistake, so it
    /// is suppressed the way [`Self::failed_lhs`] suppresses the rest.
    refused_drivers: &'a HashSet<DottedRef>,
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
    /// How many nested [`lower_expr`] frames are open, bounded by
    /// [`MAX_LOWERING_DEPTH`]. [`lower_binding`] does not touch it directly;
    /// it costs depth only through the [`lower_expr`] it calls.
    depth: usize,
    /// Bindings that already reported the limit, keyed by span. Two
    /// independent chains in one scope each get their diagnostic, while the
    /// several branches of one binding that all hit the wall share one —
    /// `lower_binary` deliberately keeps every root cause on a single pass,
    /// and a bare flag broke that for the second chain.
    depth_reported: HashSet<(usize, usize)>,
}

/// Deepest `logic` lowering recursion before the pass refuses.
///
/// Depth is counted in [`lower_expr`] frames, which is not the unit an
/// author writes in:
///
/// - one expression node on the path costs **one** level
/// - one binding a reference chains through costs **two** — the operand
///   descent plus the referenced binding's own expression
///
/// so this bound is reached by roughly `MAX_LOWERING_DEPTH / 2` chained
/// bindings. The diagnostic states both, because "nested past 256 levels"
/// on a file with 130 `logic` lines is not something an author can act on.
///
/// The depth comes from declaration order, not graph size: [`lower_binding`]
/// resolves a reference by lowering the binding it names, so a chain
/// declared in reverse recurses once per binding while the same graph in
/// dependency order stays shallow at several thousand. Measured on a debug
/// build, roughly 410 reverse-declared bindings overflowed the native stack
/// — an uncatchable abort, with no diagnostic.
///
/// Sits above `cairn_lang_core`'s expression-tree bound so a single
/// well-formed expression can never reach it; what remains is the chained
/// case, which is what the diagnostic's footer addresses.
pub const MAX_LOWERING_DEPTH: usize = 256;

// A single well-formed expression must never be able to reach the bound: it
// would earn the footer below, which tells the author to reorder bindings —
// useless advice for a scope holding one. `cairn_lang_core` caps the tree it
// hands over, and one node costs one level here, so keeping this above that
// cap is what makes the chained case the only reachable one. Asserted rather
// than assumed, because the two constants live in different crates.
const _: () = assert!(
    cairn_lang_core::MAX_EXPR_DEPTH < MAX_LOWERING_DEPTH,
    "an expression the parser accepts must not be able to exhaust the lowering budget on its own",
);

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
        // `LoweringFailed` is defined as "a diagnostic has been queued",
        // and there is none to queue here — the caller is asking to lower
        // a name no binding owns. Both callers check first, so this is
        // unreachable; returning the error silently instead would put the
        // name into `failed_lhs` and suppress the report a third caller
        // owes the user, which is the class this whole pass exists to
        // close.
        unreachable!(
            "lower_binding called for `{lhs}`, which owns no binding — the caller must test \
             `bindings_by_lhs` first"
        )
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

/// Lower one boolean expression, one level deeper than the caller.
///
/// Every recursive step in the pass — nested operands, and the descent into
/// a referenced binding via [`resolve_ref`] — funnels through here, so this
/// is the single place [`MAX_LOWERING_DEPTH`] is enforced. A new recursive
/// site cannot reopen the hole by forgetting to count.
fn lower_expr<'a>(
    expr: &'a Expr,
    binding_span: &Span,
    ir: &mut LogicIr,
    ctx: &mut LoweringCtx<'a>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<SignalRef, LoweringFailed> {
    if ctx.depth >= MAX_LOWERING_DEPTH {
        let key = (binding_span.start, binding_span.end);
        if ctx.depth_reported.insert(key) {
            let label = ctx.scope.label();
            diagnostics.push(
                Diagnostic::new(
                    DiagnosticCode::LogicNestingTooDeep,
                    binding_span.clone(),
                    format!(
                        "{label} `logic` lowering nested past {MAX_LOWERING_DEPTH} levels, about \
                         {} chained bindings",
                        MAX_LOWERING_DEPTH / 2,
                    ),
                )
                .with_footer(
                    "Fix: declare each `logic` binding after the ones it references. A binding is \
                     lowered by descending into whatever it names, so a chain written in reverse \
                     costs two levels per binding, while the same graph in dependency order costs \
                     none.",
                ),
            );
        }
        return Err(LoweringFailed);
    }
    ctx.depth += 1;
    let result = lower_expr_inner(expr, binding_span, ir, ctx, diagnostics);
    ctx.depth -= 1;
    result
}

/// Lower one boolean expression. Every operand is lowered independently
/// before the enclosing gate is built, and every recursive failure is
/// collected — a `logic sig.x = sig.undef1 or sig.undef2` reports both
/// unbound refs rather than shortcircuiting at the first.
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
        // `Expr` is `#[non_exhaustive]`; the current parser produces only
        // the four variants above. A future extension (`xor(a, b)`,
        // `mux(sel=..., a=..., b=...)`, ...) will land as new variants —
        // reject them fail-loud with a dedicated code so the missing
        // lowering site is signalled at the offending line rather than
        // hidden inside a wrong DAG.
        _ => {
            let label = ctx.scope.label();
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

    // Root cause already reported once — silent skip. `refused_drivers`
    // joins it: a `-> sig.X` tail on a member that cannot emit, or a
    // `logic` LHS outside the `sig.` namespace, was already refused where
    // it was written, and the signal it would have defined going missing
    // is that finding's consequence rather than a second one.
    if ctx.failed_lhs.contains(dr) || ctx.refused_drivers.contains(dr) {
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
    scope: ScopeRef<'_>,
    dr: &DottedRef,
    span: Span,
    ir: &LogicIr,
    bindings_by_lhs: &HashMap<DottedRef, usize>,
    failed_lhs: &HashSet<DottedRef>,
    source_label: &str,
) -> Diagnostic {
    // A name whose `logic` line failed to lower is a key in
    // `bindings_by_lhs` and is not a signal anyone can rename to. Offering
    // it made the finding contradict itself: "no `logic` line defines
    // `sig.x`" followed by "valid signals in scope: sig.x".
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

/// Which scope a pass is working on, as the diagnostics need it.
///
/// Carried as one value rather than the `(kind, name)` pair every function
/// used to take, because the pair is threaded through the whole file and
/// every finding renders it the same way.
#[derive(Debug, Clone, Copy)]
struct ScopeRef<'a> {
    kind: ScopeKind,
    name: &'a str,
}

impl ScopeRef<'_> {
    /// `struct=gatehouse:` — the prefix every finding in this pass opens
    /// with.
    fn label(self) -> String {
        scope_label(self.kind, self.name)
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
