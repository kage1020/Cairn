//! A signal binding sits on the component that carries it (`spec/redstone` "Signal binding"),
//! and a misspelt key or a value that names no signal is reported rather than dropped.

use cairn_lang_redstone::DiagnosticCode;

use crate::common::{codes, gate_source, only, synth_source};

// --- the value axis ------------------------------------------------------

/// One row per `ValueKind`, and `sig.a.b`, since a signal name has exactly two segments.
#[test]
fn a_sensor_tail_that_names_no_signal_is_refused_whatever_it_names() {
    for (tail, found) in [
        ("foo.bar", "reference `foo.bar`"),
        ("sig.a.b", "reference `sig.a.b`"),
        ("a", "identifier `a`"),
        ("3", "integer `3`"),
        ("\"sig.a\"", "string `\"sig.a\"`"),
        // Cairn strings have no escapes: the message shows the backslash as typed, not `Debug`'s.
        ("\"a\\b\"", "string `\"a\\b\"`"),
        ("@tok", "token `@tok`"),
        ("true", "boolean `true`"),
        ("2x2", "size `2x2`"),
        ("[a,b]", "a list"),
    ] {
        let out = synth_source(&gate_source(&format!(
            "  pressure_plate id=p at=front.outside offset=0 y=0 -> {tail}\n",
        )));
        assert_eq!(
            codes(&out),
            ["E_LOGIC_INVALID_SIGNAL"],
            "tail `-> {tail}` must be refused, once",
        );
        let d = only(&out, DiagnosticCode::LogicInvalidSignal);
        assert!(
            d.primary.contains(found),
            "the message says what it found; expected {found:?} in: {}",
            d.primary,
        );
    }
}

#[test]
fn a_bare_name_is_offered_its_namespace_and_a_number_is_not() {
    let named = synth_source(&gate_source(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> a\n",
    ));
    let d = only(&named, DiagnosticCode::LogicInvalidSignal);
    assert!(
        d.notes.iter().any(|n| n.message.contains("write `sig.a`")),
        "{:#?}",
        d.notes,
    );

    for (tail, absent) in [("3", "sig.3"), ("sig", "sig.sig")] {
        let unnamed = synth_source(&gate_source(&format!(
            "  pressure_plate id=p at=front.outside offset=0 y=0 -> {tail}\n",
        )));
        let d = only(&unnamed, DiagnosticCode::LogicInvalidSignal);
        assert!(
            d.notes
                .iter()
                .any(|n| n.message.contains("name a signal as `sig.<name>`")),
            "`-> {tail}`: {:#?}",
            d.notes,
        );
        assert!(
            !d.notes.iter().any(|n| n.message.contains(absent)),
            "there is no name here to offer, least of all {absent:?}: {:#?}",
            d.notes,
        );
    }
}

#[test]
fn a_tail_on_the_wrong_host_draws_no_value_side_finding() {
    let out = synth_source(&gate_source(
        "  walls class=inner mat_slot=wall height=1 -> a\n",
    ));
    assert_eq!(codes(&out), Vec::<&str>::new(), "{:#?}", out.diagnostics);
}

#[test]
fn an_actuator_argument_that_names_no_signal_is_refused() {
    for (value, found) in [
        ("foo.bar", "reference `foo.bar`"),
        ("sig.a.b", "reference `sig.a.b`"),
        ("a", "identifier `a`"),
        ("3", "integer `3`"),
        ("true", "boolean `true`"),
        ("2x2", "size `2x2`"),
        ("[a,b]", "a list"),
    ] {
        let out = synth_source(&gate_source(&format!(
            "  door id=d side=front at=center mat_slot=wall opened_by={value}\n",
        )));
        assert_eq!(
            codes(&out),
            ["E_LOGIC_INVALID_SIGNAL"],
            "`opened_by={value}` must be refused, once",
        );
        let d = only(&out, DiagnosticCode::LogicInvalidSignal);
        assert!(
            d.primary.contains("`opened_by=` wires this `door`") && d.primary.contains(found),
            "expected the key, the host, and {found:?} in: {}",
            d.primary,
        );
    }
}

#[test]
fn an_actuator_argument_on_the_wrong_host_is_a_host_fault_even_when_it_names_no_signal() {
    let out = synth_source(&gate_source(
        "  window side=front y=1 offset=1 size=2x2 mat_slot=wall opened_by=a\n",
    ));
    assert_eq!(codes(&out), ["E_LOGIC_MISPLACED_BINDING"]);
}

#[test]
fn a_binding_inside_the_selector_is_refused_and_counts_as_wired() {
    let out = synth_source(&gate_source(concat!(
        "  door id=front side=front at=center mat_slot=wall\n",
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  door[id=front,opened_by=sig.a]\n",
    )));
    assert_eq!(
        codes(&out),
        ["E_LOGIC_MISPLACED_BINDING"],
        "the author did wire `sig.a`, so it is not also unconsumed",
    );
    let d = only(&out, DiagnosticCode::LogicMisplacedBinding);
    assert!(
        d.primary
            .contains("`opened_by=` is inside `door`'s `[selector]`"),
        "{}",
        d.primary,
    );
}

#[test]
fn a_bracketed_pair_is_answered_by_the_fault_that_survives_unbracketing() {
    let wrong_host = synth_source(&gate_source(concat!(
        "  door id=front side=front at=center mat_slot=wall\n",
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  door[id=front,lit_by=sig.a]\n",
    )));
    assert_eq!(codes(&wrong_host), ["E_LOGIC_MISPLACED_BINDING"]);
    assert!(
        only(&wrong_host, DiagnosticCode::LogicMisplacedBinding)
            .primary
            .contains("binds a signal to a `lamp`"),
        "the host fault, not the brackets: {}",
        only(&wrong_host, DiagnosticCode::LogicMisplacedBinding).primary,
    );

    let typo = synth_source(&gate_source(concat!(
        "  door id=front side=front at=center mat_slot=wall\n",
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  door[id=front,oepend_by=sig.a]\n",
    )));
    assert_eq!(codes(&typo), ["E_LOGIC_UNKNOWN_BINDING_KEY"]);
    assert!(
        only(&typo, DiagnosticCode::LogicUnknownBindingKey)
            .notes
            .iter()
            .any(|n| n.message.contains("did you mean `opened_by=`")),
        "{:#?}",
        only(&typo, DiagnosticCode::LogicUnknownBindingKey).notes,
    );
}

#[test]
fn a_signal_under_the_one_key_the_brackets_read_is_not_blamed_on_the_brackets() {
    let out = synth_source(&gate_source(concat!(
        "  door id=front side=front at=center mat_slot=wall\n",
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  door[id=sig.a]\n",
    )));
    assert_eq!(codes(&out), ["E_LOGIC_UNKNOWN_BINDING_KEY"]);
    assert!(
        !only(&out, DiagnosticCode::LogicUnknownBindingKey)
            .primary
            .contains("[selector]"),
        "`id=` is read where it is written: {}",
        only(&out, DiagnosticCode::LogicUnknownBindingKey).primary,
    );
}

#[test]
fn a_selector_binding_with_no_name_leaves_the_signal_unconsumed() {
    let out = synth_source(&gate_source(concat!(
        "  door id=front side=front at=center mat_slot=wall\n",
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  door[id=front,opened_by=a]\n",
    )));
    assert_eq!(
        codes(&out),
        ["W_LOGIC_UNUSED_SIGNAL", "E_LOGIC_MISPLACED_BINDING"],
        "source order again: the sensor is written above the patch",
    );
}

#[test]
fn a_selector_binding_on_an_unknown_keyword_is_left_to_the_keyword_finding() {
    let out = synth_source(&gate_source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  widget[id=w1,opened_by=sig.a]\n",
    )));
    assert_eq!(
        codes(&out),
        Vec::<&str>::new(),
        "the selector walk says nothing about a member that is not a component",
    );
    assert!(
        !codes(&out).contains(&"W_LOGIC_UNUSED_SIGNAL"),
        "{:#?}",
        out.diagnostics,
    );
}

#[test]
fn a_selector_that_binds_nothing_is_left_alone() {
    let out = synth_source(&gate_source(concat!(
        "  door id=front side=front at=center mat_slot=wall\n",
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  door[id=front] opened_by=sig.a\n",
    )));
    assert!(out.diagnostics.is_empty(), "{:#?}", codes(&out));
    let scope = out.scoped.scopes.first().expect("one scope");
    assert_eq!(scope.ir.outputs.len(), 1, "the door is wired");
}

#[test]
fn a_malformed_value_and_the_signal_it_leaves_unconsumed_are_both_reported() {
    let out = synth_source(&gate_source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  door id=d side=front at=center mat_slot=wall opened_by=a\n",
    )));
    assert_eq!(
        codes(&out),
        ["W_LOGIC_UNUSED_SIGNAL", "E_LOGIC_INVALID_SIGNAL"],
        "source order: the sensor is written first",
    );
}

// --- sensor tails --------------------------------------------------------

#[test]
fn a_sensor_tail_on_a_wall_registers_no_input_and_no_cascade() {
    let out = synth_source(&gate_source(concat!(
        "  walls class=inner mat_slot=wall height=1 -> sig.w\n",
        "  logic sig.x = not sig.w\n",
    )));
    // The `walls` tail is `check`'s `E_MISPLACED_BINDING`, so synth itself adds nothing.
    assert_eq!(codes(&out), Vec::<&str>::new(), "{:#?}", out.diagnostics);
    let scope = out.scoped.scopes.first().expect("one scope");
    assert!(
        scope.ir.inputs.is_empty(),
        "the tail is not a sensor's, so it is no input port: {:#?}",
        scope.ir.inputs,
    );
}

#[test]
fn a_sensor_tail_on_a_pressure_plate_still_lowers() {
    let out = synth_source(&gate_source(concat!(
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
    let out = synth_source(&gate_source(concat!(
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
    for (key, host) in [
        ("powered_by", "piston"),
        ("lit_by", "lamp"),
        ("fired_by", "dispenser"),
    ] {
        let out = synth_source(&gate_source(&format!(
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
    let out = synth_source(&gate_source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.a\n",
    )));
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
    assert_eq!(out.scoped.scopes[0].ir.outputs.len(), 1);
}

#[test]
fn a_binding_on_an_unknown_keyword_is_left_to_the_keyword_finding() {
    // `widget` is none of the hosts, so only the role guard keeps this quiet.
    let out = synth_source(&gate_source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  widget id=w1 lit_by=sig.a\n",
    )));
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
}

#[test]
fn an_unknown_keyword_that_matches_a_future_host_is_still_skipped() {
    // `lamp` names `lit_by=`'s host but is not a keyword, so only the role guard skips it.
    let out = synth_source(&gate_source(concat!(
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
    let out = synth_source(&gate_source(concat!(
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
    assert_eq!(codes(&out), ["E_LOGIC_UNKNOWN_BINDING_KEY"]);
}

#[test]
fn a_key_from_no_vocabulary_at_all_gets_the_list_instead() {
    let out = synth_source(&gate_source(concat!(
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
    let out = synth_source(&gate_source(concat!(
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
    let out = synth_source(&gate_source(concat!(
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

/// `spec/redstone` "Signal binding": a signal name is `sig.` and exactly one segment after it.
/// `foo`, with no segment after the head, keeps the head test ahead of the segment count.
#[test]
fn a_logic_lhs_that_is_not_one_segment_after_sig_is_refused() {
    for (body, lhs, why, fix) in [
        (
            concat!(
                "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
                "  logic foo = not sig.a\n",
            ),
            "foo",
            "which is outside the `sig.` namespace",
            "rename the left-hand side to `sig.<name>`",
        ),
        (
            concat!(
                "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
                "  logic sig = not sig.a\n",
            ),
            "sig",
            "which is the `sig.` namespace itself rather than a signal in it",
            "add a name, as in `sig.<name>`",
        ),
        (
            concat!(
                "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
                "  logic sig = not sig.a\n",
                "  logic sig.o = not sig\n",
                "  door id=front side=front at=center\n",
                "  door[id=front] opened_by=sig.o\n",
            ),
            "sig",
            "which is the `sig.` namespace itself rather than a signal in it",
            "add a name, as in `sig.<name>`",
        ),
        (
            concat!(
                "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
                "  logic sig.x.y = not sig.a\n",
            ),
            "sig.x.y",
            "which has 2 segments after `sig.`",
            "keep one segment after `sig.`, as in `sig.x` or `sig.x_y`",
        ),
        (
            concat!(
                "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
                "  logic sig.x.y = not sig.a\n",
                "  logic sig.o = not sig.x.y\n",
                "  door id=front side=front at=center\n",
                "  door[id=front] opened_by=sig.o\n",
            ),
            "sig.x.y",
            "which has 2 segments after `sig.`",
            "keep one segment after `sig.`, as in `sig.x` or `sig.x_y`",
        ),
        (
            concat!(
                "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
                "  logic sig.x.y.z = not sig.a\n",
            ),
            "sig.x.y.z",
            "which has 3 segments after `sig.`",
            "keep one segment after `sig.`, as in `sig.x` or `sig.x_y_z`",
        ),
    ] {
        let out = synth_source(&gate_source(body));
        assert_eq!(codes(&out), ["E_LOGIC_INVALID_SIGNAL"], "{body}");
        let d = only(&out, DiagnosticCode::LogicInvalidSignal);
        assert!(
            d.primary
                .contains(&format!("`logic {lhs} = ...` names `{lhs}`, {why}")),
            "{}",
            d.primary,
        );
        let footer = format!("Fix: {fix}, or delete the binding if nothing was meant to read it.");
        assert!(
            d.notes.iter().any(|n| n.message == footer),
            "{footer}\n{:#?}",
            d.notes,
        );
    }
}

#[test]
fn an_actuator_reading_a_refused_lhs_is_refused_by_the_same_rule() {
    let out = synth_source(&gate_source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  logic sig.x.y = not sig.a\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.x.y\n",
    )));
    assert_eq!(
        codes(&out),
        ["E_LOGIC_INVALID_SIGNAL", "E_LOGIC_INVALID_SIGNAL"],
        "{:#?}",
        out.diagnostics,
    );
}

#[test]
fn two_lines_with_the_same_refused_lhs_are_two_refusals() {
    let out = synth_source(&gate_source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  logic sig.x.y = not sig.a\n",
        "  logic sig.x.y = sig.a\n",
    )));
    assert_eq!(
        codes(&out),
        ["E_LOGIC_INVALID_SIGNAL", "E_LOGIC_INVALID_SIGNAL"],
        "{:#?}",
        out.diagnostics,
    );
}

#[test]
fn an_unbound_reference_is_offered_only_the_repairs_that_exist() {
    for (reference, offers_drive) in [
        ("sig.x.y", false),
        ("sig", false),
        ("foo.bar", false),
        ("sig.nowhere", true),
    ] {
        for (line, label) in [
            (
                format!("  logic sig.o = not {reference}\n"),
                "`logic` binding",
            ),
            (
                format!("  assert always(sig.a -> eventually {reference} within 3)\n"),
                "an `assert`",
            ),
        ] {
            let body = format!(
                concat!(
                    "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
                    "  logic sig.k = not sig.a\n",
                    "  door id=front side=front at=center\n",
                    "  door[id=front] opened_by=sig.k\n",
                    "{line}",
                ),
                line = line,
            );
            let out = synth_source(&gate_source(&body));
            let found: Vec<_> = out
                .diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::LogicUnboundSignal)
                .collect();
            assert_eq!(found.len(), 1, "{body}{:#?}", out.diagnostics);
            assert!(found[0].primary.contains(label), "{}", found[0].primary);
            let footer = found[0]
                .notes
                .iter()
                .find_map(|n| n.message.strip_prefix("Valid signals in scope: "))
                .unwrap_or_else(|| panic!("{body}{:#?}", found[0].notes));
            assert_eq!(
                footer.contains(&format!("or drive `{reference}` from a sensor")),
                offers_drive,
                "{body}{footer}",
            );
            assert!(
                footer.contains("Fix: rename to a defined signal"),
                "{footer}",
            );
        }
    }
}

// --- `assert` references -------------------------------------------------

#[test]
fn an_assert_over_signals_nothing_defines_is_reported() {
    let out = synth_source(&gate_source(concat!(
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
    // The other assert form, whose signals are inputs rather than the one output.
    let out = synth_source(&gate_source(concat!(
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
    let out = synth_source(&gate_source(concat!(
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
    let out = synth_source(&gate_source(concat!(
        "  walls class=inner mat_slot=wall height=1 -> sig.w\n",
        "  assert always(sig.w -> eventually sig.w within 1)\n",
    )));
    assert_eq!(
        codes(&out),
        Vec::<&str>::new(),
        "the tail is `check`'s `E_MISPLACED_BINDING`, and the `assert` over \
         the signal it would have driven is not a second finding: {:#?}",
        out.diagnostics
    );
}
