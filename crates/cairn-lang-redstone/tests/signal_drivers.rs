//! A signal carries one driver, and the finding names both of them.
//!
//! A signal is driven by a sensor's `-> sig.X` tail or by a `logic` line's
//! left-hand side. The front end used to check the two with different
//! passes in a fixed order — sensors registered first, `logic` duplicates
//! second, sensor-versus-`logic` third, sharing one "already reported" set
//! — and each of the three defects below fell out of that arrangement:
//!
//! - sensor-versus-sensor was the pairing nobody checked, so the second
//!   plate was dropped in silence while the block stayed in the build;
//! - a signal driven by a sensor *and* two `logic` lines reported only the
//!   `logic` pair, because the earlier pass had claimed the name in the
//!   shared set — the root cause was the one thing not mentioned;
//! - "first" meant "sensor", so a `logic` line written above the sensor it
//!   collides with was the line accused of the collision.
//!
//! One driver list ordered by span answers all three, and these tests
//! come in mirrored pairs because a rule that always blames the sensor,
//! or always blames the `logic` line, would satisfy either half alone.

use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{Diagnostic, DiagnosticCode, SynthOutput, synthesize};

fn synth_source(source: &str) -> SynthOutput {
    let module = parse(source).expect("parse");
    let intent = lower(&module);
    synthesize(&intent)
}

/// 1-based line of a byte offset, counted the way the source reads.
fn line_of(source: &str, offset: usize) -> usize {
    source[..offset].bytes().filter(|b| *b == b'\n').count() + 1
}

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
    "  pressure_plate id=plate2 at=inside.front offset=0 y=0 -> sig.a\n",
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
    // The fix is not "delete one" — two plates opening one door is a
    // circuit, and the note says how to write it.
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
    // The scope is dropped on the error, so what this pins is that the
    // *silent* path is gone: before, both plates lowered, one input port
    // existed, and `--stage placement` laid a single pad for a build that
    // contains two plates.
    let out = synth_source(TWO_SENSORS);
    assert!(
        out.scoped.scopes.is_empty(),
        "a refused driver drops its scope: {:#?}",
        out.scoped,
    );
    // And the refusal is the whole of what this source earns. A losing
    // sensor that reached `signal_defs` anyway would take the name from
    // the winner, and the passes downstream would go on to report against
    // the wrong definition — those findings reach the user even though the
    // scope does not.
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
    // The masking case. Two `logic` lines make the `logic`-versus-`logic`
    // pair fire first, and it used to claim the name in the shared set, so
    // the sensor — the thing the author actually has to look at — was
    // never mentioned.
    let source = concat!(
        "@cairn 2026.06\n",
        "\n",
        "struct gate size=5x5\n",
        "  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a\n",
        "  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b\n",
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
        "the first `logic` line is reported against the sensor above it, \
         and the second is suppressed",
    );
    assert!(
        found[0].primary.contains("conflicts with a sensor"),
        "{}",
        found[0].primary,
    );
}

#[test]
fn a_logic_line_above_the_sensor_is_the_first_driver() {
    // The mirror of the pair above, and the collection-order defect: the
    // sensor used to win the name whatever the file said, so this line was
    // the one accused.
    let source = concat!(
        "@cairn 2026.06\n",
        "\n",
        "struct gate size=5x5\n",
        "  pressure_plate id=q at=inside.front offset=0 y=0 -> sig.b\n",
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
        "the `logic` line on line 5 is written first, so the sensor below \
         it is the redefinition",
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
    // "Report the root cause once" is the crate's own rule, and it has to
    // survive a signal with three sources rather than two.
    let source = concat!(
        "@cairn 2026.06\n",
        "\n",
        "struct gate size=5x5\n",
        "  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a\n",
        "  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.a\n",
        "  pressure_plate id=p3 at=inside.back offset=0 y=0 -> sig.a\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.a\n",
    );
    let out = synth_source(source);
    let found = drivers_findings(&out);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(anchor_and_note(source, found[0]), (5, 4));
}

#[test]
fn two_logic_lines_on_one_signal_keep_the_wording_they_had() {
    // The one pairing that already worked. Its message is the one authors
    // have seen, so the unified pass must not have moved it.
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
    assert!(
        found[0]
            .primary
            .contains("redefines a signal already driven earlier in this scope"),
        "{}",
        found[0].primary,
    );
}

#[test]
fn one_sensor_per_signal_still_lowers_untouched() {
    // The guard every test above needs: the rule refuses a second driver,
    // not a second sensor.
    let source = concat!(
        "@cairn 2026.06\n",
        "\n",
        "struct gate size=5x5\n",
        "  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a\n",
        "  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.b\n",
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
    // The strongest available evidence for "first means first in the
    // file": collection is depth-first, so a `logic` line inside a `level`
    // is collected *after* the members below it, and the span sort is the
    // only thing that puts it back where the author wrote it.
    let source = concat!(
        "@cairn 2026.06
",
        "
",
        "struct gate size=5x5
",
        "  pressure_plate id=q at=inside.front offset=0 y=0 -> sig.b
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
        "the nested `logic` line on line 6 is written above the sensor on \
         line 7, so the sensor is the redefinition",
    );
}

#[test]
fn a_binding_that_lost_its_name_is_not_lowered() {
    // Losing the name has to mean losing the lowering, or the pass goes on
    // to report the RHS of a line it has already told the author to
    // delete. The sensor and the binding are each the first of their own
    // list, so a `Winners` that mixed the two index spaces would map
    // `sig.a` to index 0 and match this binding by accident.
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
        "`sig.undef` belongs to the line that lost, so it is never looked \
         at: {:#?}",
        out.diagnostics,
    );
}
