//! `assert truth` and `assert always` against the legalized circuit, run from rest through every
//! combination of the sensors they depend on, per edition (`spec/redstone` "Verification").

use std::fmt::Write as _;

use cairn_lang_core::{Edition, lower, parse};
use cairn_lang_redstone::{Diagnostic, verify_behaviour};

use crate::common::{EXAMPLES, findings, legalized, load_example};

const EDITIONS: [Edition; 2] = [Edition::Java, Edition::Bedrock];

/// What the legalized `source` earns on `edition`.
fn verified(source: &str, edition: Edition) -> Vec<Diagnostic> {
    let intent = lower(&parse(source).expect("parse"));
    verify_behaviour(&legalized(source, edition), &intent)
}

/// A struct of the prelude's walls and door, `body` its members after them.
fn gate(size: &str, body: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct gate size={size}\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         door id=d side=front at=center\n{body}"
    )
}

/// Two levers through an `and`, a `not` and an `or` to the door, which `sig.h` holds open
/// whatever the levers say; `asserts` after the region.
fn chain(asserts: &str) -> String {
    gate(
        "15x11",
        &format!(
            "  lever id=l1 at=inside.back offset=2 y=0 -> sig.a\n  \
             lever id=l2 at=inside.back offset=6 y=0 -> sig.b\n  \
             logic sig.f = sig.a and sig.b\n  \
             logic sig.g = not sig.f\n  \
             logic sig.h = sig.g or sig.a\n  \
             door[id=d] opened_by=sig.h\n  \
             circuit region=floor void=4\n{asserts}"
        ),
    )
}

/// Two levers whose `or` opens the door and whose `and` lights a lamp, `asserts` after them.
fn or_and(asserts: &str) -> String {
    gate(
        "17x13",
        &format!(
            "  lever id=l1 at=inside.back offset=2 y=0 -> sig.a\n  \
             lever id=l2 at=inside.back offset=6 y=0 -> sig.b\n  \
             lamp id=lit at=inside.back offset=10 y=0\n  \
             logic sig.o = sig.a or sig.b\n  \
             logic sig.both = sig.a and sig.b\n  \
             door[id=d] opened_by=sig.o\n  \
             lamp[id=lit] lit_by=sig.both\n  \
             circuit region=floor void=4\n{asserts}"
        ),
    )
}

#[test]
fn every_example_meets_its_truth_and_always_assertions_on_both_editions() {
    for (name, _) in EXAMPLES {
        let source = load_example(name);
        for edition in EDITIONS {
            assert_eq!(verified(&source, edition), [], "{name} on {edition:?}");
        }
    }
}

#[test]
fn a_truth_row_the_circuit_contradicts_is_refused_with_the_run_that_breaks_it() {
    let line = "assert truth(sig.a, sig.b -> sig.h) { 00 -> 0; 01 -> 1; 10 -> 1; 11 -> 1 }";
    let src = chain(&format!("  {line}\n"));
    for edition in EDITIONS {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "E_SIM_ASSERTION_FAILED",
                line,
                format!(
                    "struct=gate: `assert truth(sig.a, sig.b -> sig.h)` fails on {}: with no \
                     sensor switched on, the circuit rests with its inputs reading `00` and sig.h \
                     reading 1",
                    edition.as_str(),
                ),
                vec![
                    (Some("00 -> 0"), "this row says sig.h reads 0".to_owned()),
                    (
                        None,
                        "Fix: correct the row, or what drives sig.h; the logic is never \
                         rewritten to meet an assertion"
                            .to_owned(),
                    ),
                ],
            )],
            "{edition:?}",
        );
    }
}

#[test]
fn a_truth_table_the_circuit_meets_is_silent_and_a_dont_care_output_asserts_nothing() {
    let src = chain("  assert truth(sig.a, sig.b -> sig.h) { 0- -> 1; 10 -> 1; 11 -> - }\n");
    for edition in EDITIONS {
        assert_eq!(verified(&src, edition), [], "{edition:?}");
    }
}

/// `sig.g` is `not sig.f`, so no run reads the two alike, and the rows that say so go unchecked.
#[test]
fn a_truth_table_over_cell_outputs_checks_only_the_combinations_a_run_reaches() {
    let src =
        chain("  assert truth(sig.f, sig.g -> sig.h) { 00 -> 0; 01 -> 1; 10 -> 1; 11 -> 0 }\n");
    for edition in EDITIONS {
        assert_eq!(verified(&src, edition), [], "{edition:?}");
    }
}

#[test]
fn an_always_its_consequent_meets_late_names_the_ticks_it_takes_at_the_actuator() {
    let line = "assert always(sig.a -> eventually sig.o within 2)";
    let src = or_and(&format!("  {line}\n"));
    for edition in EDITIONS {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "E_SIM_ASSERTION_FAILED",
                line,
                format!(
                    "struct=gate: `{line}` fails on {}: with sig.a switched on, sig.a reads on at \
                     tick 0 and sig.o not until tick 3, 3 ticks later",
                    edition.as_str(),
                ),
                vec![
                    (
                        Some("door id=d side=front at=center"),
                        "sig.o is read at the torches under this actuator".to_owned(),
                    ),
                    (
                        None,
                        "Fix: relax the bound to `within 3`, or shorten the path; the logic is \
                         never rewritten to meet an assertion"
                            .to_owned(),
                    ),
                ],
            )],
            "{edition:?}",
        );
    }
    let met = or_and("  assert always(sig.a -> eventually sig.o within 3)\n");
    for edition in EDITIONS {
        assert_eq!(verified(&met, edition), [], "{edition:?}");
    }
}

#[test]
fn an_always_its_consequent_never_follows_is_refused_with_the_first_run_that_shows_it() {
    let line = "assert always(sig.a -> eventually sig.both within 20)";
    let src = or_and(&format!("  {line}\n"));
    for edition in EDITIONS {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "E_SIM_ASSERTION_FAILED",
                line,
                format!(
                    "struct=gate: `{line}` fails on {}: with sig.a switched on, sig.a reads on at \
                     tick 0 and sig.both never does",
                    edition.as_str(),
                ),
                vec![
                    (
                        Some("lamp id=lit at=inside.back offset=10 y=0"),
                        "sig.both is read at the torches under this actuator".to_owned(),
                    ),
                    (
                        None,
                        "Fix: correct the assertion, or what drives sig.both; the logic is never \
                         rewritten to meet an assertion"
                            .to_owned(),
                    ),
                ],
            )],
            "{edition:?}",
        );
    }
}

/// The latch keeps the observer's pulse, which is over by the time the circuit rests.
#[test]
fn an_observer_is_switched_on_for_the_one_tick_of_its_pulse() {
    let line = "assert truth(sig.w, sig.r -> sig.held) { 00 -> 0 }";
    let src = gate(
        "15x11",
        &format!(
            "  observer id=eye at=inside.back offset=2 y=0 facing=up -> sig.w\n  \
             lever id=off at=inside.back offset=6 y=0 -> sig.r\n  \
             lamp id=lit at=inside.left offset=3 y=0\n  \
             logic sig.held = latch(set=sig.w, reset=sig.r)\n  \
             lamp[id=lit] lit_by=sig.held\n  \
             circuit region=floor void=4\n  {line}\n"
        ),
    );
    for edition in EDITIONS {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "E_SIM_ASSERTION_FAILED",
                line,
                format!(
                    "struct=gate: `assert truth(sig.w, sig.r -> sig.held)` fails on {}: with \
                     sig.w switched on, the circuit rests with its inputs reading `00` and \
                     sig.held reading 1",
                    edition.as_str(),
                ),
                vec![
                    (Some("00 -> 0"), "this row says sig.held reads 0".to_owned()),
                    (
                        None,
                        "Fix: correct the row, or what drives sig.held; the logic is never \
                         rewritten to meet an assertion"
                            .to_owned(),
                    ),
                ],
            )],
            "{edition:?}",
        );
    }
}

#[test]
fn an_assertion_over_more_sensors_than_a_check_runs_is_left_unchecked_with_a_warning() {
    let levers = (0..11).fold(String::new(), |mut out, i| {
        let offset = 2 * i + 1;
        let _ = writeln!(
            out,
            "  lever id=l{i} at=inside.back offset={offset} y=0 -> sig.s{i}"
        );
        out
    });
    let inputs: Vec<String> = (0..11).map(|i| format!("sig.s{i}")).collect();
    let text = format!("assert truth({} -> sig.s0)", inputs.join(", "));
    let line = format!("{text} {{ 0---------- -> 1 }}");
    let src = gate(
        "30x10",
        &format!(
            "  pressure_plate id=p at=inside.front offset=1 y=0 -> sig.p\n{levers}  \
             door[id=d] opened_by=sig.p\n  circuit region=floor void=3\n  {line}\n"
        ),
    );
    for edition in EDITIONS {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "W_SIM_ASSERTION_UNCHECKED",
                line.as_str(),
                format!(
                    "struct=gate: `{text}` is not checked: its signals depend on 11 sensors, more \
                     than the 10 a check runs the circuit through every combination of"
                ),
                vec![(
                    None,
                    "Fix: split the assertion into several whose signals each depend on fewer \
                     sensors"
                        .to_owned(),
                )],
            )],
            "{edition:?}",
        );
    }
}
