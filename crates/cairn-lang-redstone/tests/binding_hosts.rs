//! A signal binding sits on the component that carries it.
//!
//! `spec/redstone` §14.2 does not offer `-> sig.X` and the four actuator
//! keys as free-floating attributes: it writes each one on one component —
//! a sensor emits, `door ... opened_by=`, `lamp ... lit_by=`,
//! `piston ... powered_by=`, `dispenser ... fired_by=`. The front end read
//! only the *value*, so any member carrying a `sig.`-valued argument became
//! an output port and any member with a `sig.` tail became an input port.
//! `walls ... powered_by=sig.x` and `walls ... -> sig.w` both reached
//! placement as live ports on members with no component behind them.
//!
//! The same read is what let a typo vanish: `oepend_by=sig.x` matched no
//! key, so the actuator silently disappeared and the only trace was a
//! warning that the signal it would have driven was unconsumed.

use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{DiagnosticCode, SynthOutput, synthesize};

fn synth_source(source: &str) -> SynthOutput {
    let module = parse(source).expect("parse");
    let intent = lower(&module);
    synthesize(&intent)
}

fn codes(out: &SynthOutput) -> Vec<&'static str> {
    out.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn only(out: &SynthOutput, code: DiagnosticCode) -> &cairn_lang_redstone::Diagnostic {
    let found: Vec<_> = out.diagnostics.iter().filter(|d| d.code == code).collect();
    assert_eq!(found.len(), 1, "expected one {code:?}: {:#?}", codes(out));
    found[0]
}

const PRELUDE: &str = concat!(
    "@cairn 2026.06\n",
    "\n",
    "theme t:\n",
    "  slot wall -> @cobblestone\n",
    "\n",
    "struct gate size=5x5\n",
    "  walls class=outer mat_slot=wall height=3\n",
);

fn source(body: &str) -> String {
    format!("{PRELUDE}{body}")
}

// --- sensor tails --------------------------------------------------------

#[test]
fn a_sensor_tail_on_a_wall_is_refused_and_registers_no_input() {
    let out = synth_source(&source(concat!(
        "  walls class=inner mat_slot=wall height=1 -> sig.w\n",
        "  logic sig.x = not sig.w\n",
    )));
    let d = only(&out, DiagnosticCode::LogicMisplacedBinding);
    assert!(
        d.primary.contains("`walls` cannot emit a signal"),
        "{}",
        d.primary,
    );
    // Root cause once: the `logic` line below names `sig.w`, and that it
    // is now undefined is this finding's consequence, not a second one.
    assert_eq!(codes(&out), ["E_LOGIC_MISPLACED_BINDING"]);
}

#[test]
fn a_sensor_tail_on_a_pressure_plate_still_lowers() {
    let out = synth_source(&source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  logic sig.x = not sig.a\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.x\n",
    )));
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
    assert_eq!(out.scoped.scopes[0].ir.inputs.len(), 1);
}

// --- actuator keys -------------------------------------------------------

#[test]
fn opened_by_on_a_window_names_the_component_it_belongs_to() {
    let out = synth_source(&source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  window side=front y=1 offset=1 size=1x1 mat_slot=wall opened_by=sig.a\n",
    )));
    let d = only(&out, DiagnosticCode::LogicMisplacedBinding);
    assert!(
        d.primary
            .contains("`opened_by=` binds a signal to a `door`, and this member is a `window`"),
        "{}",
        d.primary,
    );
    assert!(
        d.notes.iter().any(|n| n
            .message
            .contains("move the `opened_by=` argument onto the `door`")),
        "{:#?}",
        d.notes,
    );
}

#[test]
fn the_three_keys_with_no_keyword_to_host_them_say_so() {
    // `lamp`, `piston`, and `dispenser` are not keywords the surface
    // accepts, so `lit_by=` / `powered_by=` / `fired_by=` have no legal
    // member at all today. Telling the author to "move it onto the piston"
    // would send them to write a line that is `E_UNKNOWN_KEYWORD`.
    for (key, host) in [
        ("powered_by", "piston"),
        ("lit_by", "lamp"),
        ("fired_by", "dispenser"),
    ] {
        let out = synth_source(&source(&format!(
            "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n  \
             door id=front side=front at=center {key}=sig.a\n",
        )));
        let d = only(&out, DiagnosticCode::LogicMisplacedBinding);
        assert!(
            d.primary.contains(&format!("binds a signal to a `{host}`")),
            "{}",
            d.primary,
        );
        assert!(
            d.notes.iter().any(|n| n.message.contains(&format!(
                "`{host}` is not a keyword the surface accepts yet"
            ))),
            "{:#?}",
            d.notes,
        );
    }
}

#[test]
fn opened_by_on_a_door_still_lowers() {
    let out = synth_source(&source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.a\n",
    )));
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
    assert_eq!(out.scoped.scopes[0].ir.outputs.len(), 1);
}

#[test]
fn a_binding_on_an_unknown_keyword_is_left_to_the_keyword_finding() {
    // `widget` is not in the role table, so `check` already reports the
    // line as `E_UNKNOWN_KEYWORD`. A second finding here would be about a
    // component that does not exist — and `widget` is deliberately not one
    // of the four hosts, so what keeps this quiet is the role guard rather
    // than the keyword happening to match the key's host.
    let out = synth_source(&source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  widget id=w1 lit_by=sig.a\n",
    )));
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
}

#[test]
fn an_unknown_keyword_that_matches_a_future_host_is_still_skipped() {
    // `lamp` is the component §14.2 pairs with `lit_by=`, so its keyword
    // string satisfies the host check on its own — but `lamp` is not a
    // keyword the surface accepts, the member is `E_UNKNOWN_KEYWORD`, and
    // the front end must not build a port on it. Without the role guard
    // this line registers a live output on a member that does not exist.
    let out = synth_source(&source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  lamp id=l1 lit_by=sig.a\n",
    )));
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
    assert!(
        out.scoped.scopes[0].ir.outputs.is_empty(),
        "a member the role table does not know carries no port: {:#?}",
        out.scoped.scopes[0].ir.outputs,
    );
}

// --- keys that read like a binding and are not one ----------------------

#[test]
fn a_typoed_actuator_key_is_refused_with_the_key_it_meant() {
    let out = synth_source(&source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  logic sig.x = not sig.a\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] oepend_by=sig.x\n",
    )));
    let d = only(&out, DiagnosticCode::LogicUnknownBindingKey);
    assert!(
        d.notes
            .iter()
            .any(|n| n.message == "did you mean `opened_by=`?"),
        "{:#?}",
        d.notes,
    );
    // And the signal the actuator would have consumed is not separately
    // reported as unused — the author did wire it.
    assert_eq!(codes(&out), ["E_LOGIC_UNKNOWN_BINDING_KEY"]);
}

#[test]
fn a_key_from_no_vocabulary_at_all_gets_the_list_instead() {
    // Too far from any actuator key for a suggestion; the finding still
    // has to say what the legal keys are.
    let out = synth_source(&source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  logic sig.x = not sig.a\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] zzz=sig.x\n",
    )));
    let d = only(&out, DiagnosticCode::LogicUnknownBindingKey);
    assert!(
        d.notes
            .iter()
            .any(|n| n.message.contains("`opened_by=`") && n.message.contains("`fired_by=`")),
        "{:#?}",
        d.notes,
    );
}

#[test]
fn a_non_signal_value_under_an_unknown_key_is_not_this_pass_business() {
    // The check is keyed on the value being a `sig.` reference. Every
    // ordinary argument — `mat_slot=`, `height=`, an unknown one holding a
    // number — is somebody else's to validate, and a schema for that does
    // not exist yet.
    let out = synth_source(&source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  logic sig.x = not sig.a\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.x zzz=3\n",
    )));
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
}

// --- the `logic` left-hand side -----------------------------------------

#[test]
fn a_logic_lhs_outside_the_sig_namespace_is_refused_and_lowers_no_gate() {
    let out = synth_source(&source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  logic foo.bar = not sig.a\n",
    )));
    let d = only(&out, DiagnosticCode::LogicInvalidSignal);
    assert!(
        d.primary.contains("which is outside the `sig.` namespace"),
        "{}",
        d.primary,
    );
    assert!(
        out.scoped.scopes.is_empty(),
        "the scope is dropped, so no cell takes a coordinate for it",
    );
    assert_eq!(codes(&out), ["E_LOGIC_INVALID_SIGNAL"]);
}

// --- `assert` references -------------------------------------------------

#[test]
fn an_assert_over_signals_nothing_defines_is_reported() {
    let out = synth_source(&source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  logic sig.x = not sig.a\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.x\n",
        "  assert always(sig.zzz -> eventually sig.qqq within 3)\n",
    )));
    let found: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::LogicUnboundSignal)
        .collect();
    assert_eq!(found.len(), 2, "both references: {:#?}", out.diagnostics);
    assert!(
        found[0]
            .primary
            .contains("an `assert` references `sig.zzz`"),
        "{}",
        found[0].primary,
    );
}

#[test]
fn an_assert_truth_row_checks_every_reference_it_names() {
    // The other assert form, and the input list rather than the single
    // output — a walk that only read the output would pass the test above.
    let out = synth_source(&source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  logic sig.x = not sig.a\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.x\n",
        "  assert truth(sig.a, sig.nope -> sig.x) { 00->0; 01->1; 10->1; 11->1 }\n",
    )));
    let found: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::LogicUnboundSignal)
        .collect();
    assert_eq!(found.len(), 1, "{:#?}", out.diagnostics);
    assert!(
        found[0].primary.contains("`sig.nope`"),
        "{}",
        found[0].primary,
    );
}

#[test]
fn an_assert_over_defined_signals_stays_silent() {
    // `redstone-door.crn` ships both assert forms over signals that exist;
    // this is the same shape, so the check cannot be firing on those.
    let out = synth_source(&source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  logic sig.x = not sig.a\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.x\n",
        "  assert always(sig.a -> eventually sig.x within 2)\n",
        "  assert truth(sig.a -> sig.x) { 0->1; 1->0 }\n",
    )));
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
}

#[test]
fn an_assert_naming_a_signal_a_refused_binding_would_have_driven_is_not_a_second_finding() {
    let out = synth_source(&source(concat!(
        "  walls class=inner mat_slot=wall height=1 -> sig.w\n",
        "  assert always(sig.w -> eventually sig.w within 1)\n",
    )));
    assert_eq!(
        codes(&out),
        ["E_LOGIC_MISPLACED_BINDING"],
        "{:#?}",
        out.diagnostics
    );
}
