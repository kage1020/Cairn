//! A signal carries one driver, and the finding names both, the one written first as the original.
//! The tests come in mirrored pairs: a rule that always blames one kind of driver passes one half.

use cairn_lang_redstone::{Diagnostic, DiagnosticCode, SynthOutput};

use crate::common::{line_of, synth_source};

fn drivers_findings(out: &SynthOutput) -> Vec<&Diagnostic> {
    out.diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::LogicMultipleDrivers)
        .collect()
}

/// `(line the finding anchors at, line its first note points at)`.
fn anchor_and_note(source: &str, d: &Diagnostic) -> (usize, usize) {
    let note = d.notes[0]
        .span
        .as_ref()
        .expect("the finding points at the driver that keeps the signal");
    (line_of(source, d.span.start), line_of(source, note.start))
}

const TWO_SENSORS: &str = concat!(
    "@cairn 2026.06\n",
    "\n",
    "struct gate size=5x5\n",
    "  pressure_plate id=plate1 at=front.outside offset=0 y=0 -> sig.a\n",
    "  pressure_plate id=plate2 at=inside.front offset=1 y=0 -> sig.a\n",
    "  logic sig.x = not sig.a\n",
    "  door id=front side=front at=center\n",
    "  door[id=front] opened_by=sig.x\n",
);

#[test]
fn a_second_sensor_on_one_signal_is_refused_rather_than_dropped() {
    let out = synth_source(TWO_SENSORS);
    let found = drivers_findings(&out);
    assert_eq!(found.len(), 1, "{:#?}", out.diagnostics);
    assert_eq!(anchor_and_note(TWO_SENSORS, found[0]), (5, 4));
    assert!(
        found[0]
            .primary
            .contains("this sensor emits `sig.a`, which another sensor already emits"),
        "{}",
        found[0].primary,
    );
    assert!(
        found[0]
            .notes
            .iter()
            .any(|n| n.message.contains("logic sig.a = sig.a1 or sig.a2")),
        "{:#?}",
        found[0].notes,
    );
}

#[test]
fn the_refused_sensor_leaves_no_input_port_behind() {
    // The scope is dropped on the error, so no input port survives either.
    let out = synth_source(TWO_SENSORS);
    assert!(
        out.scoped.scopes.is_empty(),
        "a refused driver drops its scope: {:#?}",
        out.scoped,
    );
    assert_eq!(
        out.diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
        ["E_LOGIC_MULTIPLE_DRIVERS"],
        "{:#?}",
        out.diagnostics,
    );
}

#[test]
fn a_sensor_and_two_logic_lines_report_the_sensor_as_the_first_driver() {
    // The `logic`-versus-`logic` pair must not mask the sensor.
    let source = concat!(
        "@cairn 2026.06\n",
        "\n",
        "struct gate size=5x5\n",
        "  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a\n",
        "  pressure_plate id=p2 at=inside.front offset=1 y=0 -> sig.b\n",
        "  logic sig.a = sig.b and sig.b\n",
        "  logic sig.a = sig.b or sig.b\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.a\n",
    );
    let out = synth_source(source);
    let found = drivers_findings(&out);
    assert_eq!(found.len(), 1, "one finding per signal: {found:#?}");
    assert_eq!(
        anchor_and_note(source, found[0]),
        (6, 4),
        "the first `logic` line is reported against the sensor above it",
    );
    assert!(
        found[0].primary.contains("conflicts with a sensor"),
        "{}",
        found[0].primary,
    );
}

#[test]
fn a_logic_line_above_the_sensor_is_the_first_driver() {
    let source = concat!(
        "@cairn 2026.06\n",
        "\n",
        "struct gate size=5x5\n",
        "  pressure_plate id=q at=inside.front offset=1 y=0 -> sig.b\n",
        "  logic sig.a = sig.b and sig.b\n",
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.a\n",
    );
    let out = synth_source(source);
    let found = drivers_findings(&out);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        anchor_and_note(source, found[0]),
        (6, 5),
        "the `logic` line is written first, so the sensor is the redefinition",
    );
    assert!(
        found[0]
            .primary
            .contains("this sensor emits `sig.a`, which a `logic` line already drives"),
        "{}",
        found[0].primary,
    );
    assert!(
        found[0]
            .notes
            .iter()
            .any(|n| n.message == "first declared here"),
        "{:#?}",
        found[0].notes,
    );
}

#[test]
fn a_third_driver_adds_no_second_finding() {
    let source = concat!(
        "@cairn 2026.06\n",
        "\n",
        "struct gate size=5x5\n",
        "  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a\n",
        "  pressure_plate id=p2 at=inside.front offset=1 y=0 -> sig.a\n",
        "  pressure_plate id=p3 at=inside.back offset=1 y=0 -> sig.a\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.a\n",
    );
    let out = synth_source(source);
    let found = drivers_findings(&out);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(anchor_and_note(source, found[0]), (5, 4));
}

#[test]
fn two_logic_lines_on_one_signal_say_the_signal_is_redefined() {
    let source = concat!(
        "@cairn 2026.06\n",
        "\n",
        "struct gate size=5x5\n",
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.b\n",
        "  logic sig.a = sig.b and sig.b\n",
        "  logic sig.a = sig.b or sig.b\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.a\n",
    );
    let out = synth_source(source);
    let found = drivers_findings(&out);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(anchor_and_note(source, found[0]), (6, 5));
    assert_eq!(
        found[0].primary,
        "struct=gate: `logic sig.a = ...` redefines a signal already driven earlier in this scope",
    );
}

#[test]
fn one_sensor_per_signal_still_lowers_untouched() {
    let source = concat!(
        "@cairn 2026.06\n",
        "\n",
        "struct gate size=5x5\n",
        "  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a\n",
        "  pressure_plate id=p2 at=inside.front offset=1 y=0 -> sig.b\n",
        "  logic sig.x = sig.a or sig.b\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.x\n",
    );
    let out = synth_source(source);
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
    let entry = out.scoped.scopes.first().expect("one scope");
    assert_eq!(entry.ir.inputs.len(), 2);
    assert_eq!(entry.ir.outputs.len(), 1);
}

#[test]
fn a_nested_binding_above_a_sensor_is_still_the_first_driver() {
    // Collection is depth-first, so only the span sort puts the nested line first.
    let source = concat!(
        "@cairn 2026.06
",
        "
",
        "struct gate size=5x5
",
        "  pressure_plate id=q at=inside.front offset=1 y=0 -> sig.b
",
        "  level y=0
",
        "    logic sig.a = sig.b and sig.b
",
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
",
        "  door id=front side=front at=center
",
        "  door[id=front] opened_by=sig.a
",
    );
    let out = synth_source(source);
    let found = drivers_findings(&out);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(
        anchor_and_note(source, found[0]),
        (7, 6),
        "the nested `logic` line is written first, so the sensor is the redefinition",
    );
}

#[test]
fn a_binding_that_lost_its_name_is_not_lowered() {
    // The sensor and the binding are both index 0 of their own lists.
    let source = concat!(
        "@cairn 2026.06
",
        "
",
        "struct gate size=5x5
",
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a
",
        "  logic sig.a = not sig.undef
",
        "  door id=front side=front at=center
",
        "  door[id=front] opened_by=sig.a
",
    );
    let out = synth_source(source);
    assert_eq!(
        out.diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
        ["E_LOGIC_MULTIPLE_DRIVERS"],
        "the losing line's right-hand side is never checked: {:#?}",
        out.diagnostics,
    );
}
