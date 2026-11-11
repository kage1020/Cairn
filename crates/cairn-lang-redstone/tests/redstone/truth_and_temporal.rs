//! `assert truth` and `assert always` against the legalized circuit, run from rest through every
//! combination of the sensors they depend on, per edition (`spec/redstone` "Verification").

use std::fmt::Write as _;

use cairn_lang_core::{Edition, lower, parse};
use cairn_lang_redstone::{Diagnostic, verify_truth_and_temporal};

use crate::common::{EXAMPLES, LONG_WIRE_SOURCE, built, findings, legalized, load_example};

const EDITIONS: [Edition; 2] = [Edition::Java, Edition::Bedrock];

/// The findings of running the legalized `source`'s circuits on `edition`.
fn verified(source: &str, edition: Edition) -> Vec<Diagnostic> {
    let intent = lower(&parse(source).expect("parse"));
    verify_truth_and_temporal(
        &legalized(source, edition),
        &intent,
        &built(&intent, edition),
    )
}

/// A struct with a floor, walls and a door, `body` its members after them.
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

/// `sig.g` is `not sig.f`, and `sig.d` another name of `sig.a`, so no run rests with either pair
/// reading two values apart, or alike.
#[test]
fn the_rows_of_a_met_truth_table_no_run_reaches_are_left_unchecked_with_a_warning() {
    let fix = "Fix: remove the rows no run reaches, or assert over inputs the runs switch to them";
    let cells = "assert truth(sig.f, sig.g -> sig.h) { 00 -> 0; 01 -> 1; 10 -> 1; 11 -> 0 }";
    let names = "assert truth(sig.a, sig.d -> sig.a) { 00 -> 0; 01 -> 1; 10 -> 1; 11 -> 1 }";
    for (src, line, signals, rows) in [
        (
            chain(&format!("  {cells}\n")),
            cells,
            "sig.f, sig.g -> sig.h",
            ["00", "11"],
        ),
        (
            or_and(&format!("  logic sig.d = sig.a\n  {names}\n")),
            names,
            "sig.a, sig.d -> sig.a",
            ["01", "10"],
        ),
    ] {
        for edition in EDITIONS {
            let mut notes: Vec<(Option<&str>, String)> = rows
                .iter()
                .map(|row| {
                    let at = &line[line.find(&format!("{row} ->")).expect("the row")..][..7];
                    (
                        Some(at),
                        format!("no run rests with the inputs reading what `{row}` matches"),
                    )
                })
                .collect();
            notes.push((None, fix.to_owned()));
            assert_eq!(
                findings(&src, &verified(&src, edition)),
                [(
                    "W_SIM_ASSERTION_UNCHECKED",
                    line,
                    format!(
                        "struct=gate: `assert truth({signals})` has 2 rows no run reaches, which \
                         are not checked"
                    ),
                    notes,
                )],
                "{line} on {edition:?}",
            );
        }
    }
}

#[test]
fn a_row_with_an_observers_signal_on_is_never_reached_since_its_pulse_is_over_at_rest() {
    let line = "assert truth(sig.w -> sig.n) { 1 -> 1; 0 -> 1 }";
    let src = gate(
        "15x11",
        &format!(
            "  observer id=eye at=inside.back offset=2 y=0 facing=up -> sig.w\n  \
             lamp id=lit at=inside.left offset=3 y=0\n  \
             logic sig.n = not sig.w\n  \
             lamp[id=lit] lit_by=sig.n\n  \
             circuit region=floor void=4\n  {line}\n"
        ),
    );
    for edition in EDITIONS {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "W_SIM_ASSERTION_UNCHECKED",
                line,
                "struct=gate: `assert truth(sig.w -> sig.n)` has a row no run reaches, which is not \
                 checked"
                    .to_owned(),
                vec![
                    (
                        Some("1 -> 1"),
                        "no run rests with the inputs reading what `1` matches".to_owned(),
                    ),
                    (
                        Some("observer id=eye at=inside.back offset=2 y=0 facing=up -> sig.w"),
                        "sig.w is on only for the tick of this observer's pulse, so no run rests \
                         with it on"
                            .to_owned(),
                    ),
                    (
                        None,
                        "Fix: remove the rows no run reaches, or assert over inputs the runs \
                         switch to them"
                            .to_owned(),
                    ),
                ],
            )],
            "{edition:?}",
        );
    }
}

#[test]
fn a_dont_care_input_matches_either_reading() {
    let line = "assert truth(sig.a, sig.b -> sig.both) { 1- -> 1 }";
    let src = or_and(&format!("  {line}\n"));
    for edition in EDITIONS {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "E_SIM_ASSERTION_FAILED",
                line,
                format!(
                    "struct=gate: `assert truth(sig.a, sig.b -> sig.both)` fails on {}: with \
                     sig.a switched on, the circuit rests with its inputs reading `10` and \
                     sig.both reading 0",
                    edition.as_str(),
                ),
                vec![
                    (Some("1- -> 1"), "this row says sig.both reads 1".to_owned()),
                    (
                        None,
                        "Fix: correct the row, or what drives sig.both; the logic is never \
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
fn the_first_run_to_break_a_truth_table_counts_up_with_the_first_sensor_the_highest_bit() {
    let line = "assert truth(sig.a, sig.b -> sig.o) { 00 -> 0; 01 -> 0; 10 -> 0; 11 -> 1 }";
    let src = or_and(&format!("  {line}\n"));
    for edition in EDITIONS {
        let found = findings(&src, &verified(&src, edition));
        assert_eq!(
            found
                .iter()
                .map(|(code, _, primary, notes)| (*code, primary.as_str(), notes[0].0))
                .collect::<Vec<_>>(),
            [(
                "E_SIM_ASSERTION_FAILED",
                format!(
                    "struct=gate: `assert truth(sig.a, sig.b -> sig.o)` fails on {}: with sig.b \
                     switched on, the circuit rests with its inputs reading `01` and sig.o \
                     reading 1",
                    edition.as_str(),
                )
                .as_str(),
                Some("01 -> 0"),
            )],
            "{edition:?}",
        );
    }
}

/// `sig.z` drives nothing in the circuit, so it has no pad.
#[test]
fn a_sensor_nothing_in_the_circuit_reads_reads_as_the_run_switches_it() {
    let line = "assert truth(sig.z -> sig.a) { 1 -> 0 }";
    let src = or_and(&format!(
        "  lever id=l3 at=inside.back offset=12 y=0 -> sig.z\n  {line}\n"
    ));
    for edition in EDITIONS {
        let found = findings(&src, &verified(&src, edition));
        assert_eq!(
            found
                .iter()
                .map(|(code, _, primary, _)| (*code, primary.as_str()))
                .collect::<Vec<_>>(),
            [(
                "E_SIM_ASSERTION_FAILED",
                format!(
                    "struct=gate: `assert truth(sig.z -> sig.a)` fails on {}: with sig.a and \
                     sig.z switched on, the circuit rests with its inputs reading `1` and sig.a \
                     reading 1",
                    edition.as_str(),
                )
                .as_str(),
            )],
            "{edition:?}",
        );
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

/// `sensors` levers whose signals only `assertion` reads, given the text of a truth table over
/// them all, and a door a plate opens.
fn levers(sensors: usize, assertion: impl Fn(&str) -> String) -> (String, String) {
    let levers = (0..sensors).fold(String::new(), |mut out, i| {
        let offset = 2 * i + 1;
        let _ = writeln!(
            out,
            "  lever id=l{i} at=inside.back offset={offset} y=0 -> sig.s{i}"
        );
        out
    });
    let inputs: Vec<String> = (0..sensors).map(|i| format!("sig.s{i}")).collect();
    let text = format!("assert truth({} -> sig.s0)", inputs.join(", "));
    let line = assertion(&text);
    let src = gate(
        "30x10",
        &format!(
            "  pressure_plate id=p at=inside.front offset=1 y=0 -> sig.p\n{levers}  \
             door[id=d] opened_by=sig.p\n  circuit region=floor void=3\n  {line}\n"
        ),
    );
    (src, text)
}

#[test]
fn an_assertion_over_as_many_sensors_as_a_check_runs_is_checked() {
    let (src, text) = levers(10, |text| format!("{text} {{ 1--------- -> 0 }}"));
    for edition in EDITIONS {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "E_SIM_ASSERTION_FAILED",
                format!("{text} {{ 1--------- -> 0 }}").as_str(),
                format!(
                    "struct=gate: `{text}` fails on {}: with sig.s0 switched on, the circuit rests \
                     with its inputs reading `1000000000` and sig.s0 reading 1",
                    edition.as_str(),
                ),
                vec![
                    (
                        Some("1--------- -> 0"),
                        "this row says sig.s0 reads 0".to_owned()
                    ),
                    (
                        None,
                        "Fix: correct the row, or what drives sig.s0; the logic is never \
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
    let (src, text) = levers(11, |text| format!("{text} {{ 0---------- -> 1 }}"));
    let line = format!("{text} {{ 0---------- -> 1 }}");
    for edition in EDITIONS {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "W_SIM_ASSERTION_UNCHECKED",
                line.as_str(),
                format!(
                    "struct=gate: `{text}` is not checked: its signals depend on 11 sensors, and a \
                     check runs every combination of at most 10"
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

#[test]
fn an_always_consequent_is_read_at_the_actuators_that_name_it() {
    let at_cell = "assert always(sig.a -> eventually sig.c within 0)";
    let at_door = "assert always(sig.a -> eventually sig.d within 0)";
    let src = LONG_WIRE_SOURCE
        .replace(
            "  door[id=front] opened_by=sig.c\n",
            "  logic sig.d = sig.c\n  door[id=front] opened_by=sig.d\n",
        )
        .replace("void=4\n", &format!("void=4\n  {at_cell}\n  {at_door}\n"));
    let fix = |ticks: u32| {
        format!(
            "Fix: relax the bound to `within {ticks}`, or shorten the path; the logic is never \
             rewritten to meet an assertion"
        )
    };
    for edition in EDITIONS {
        let failed = |line: &str, signal: &str, ticks: u32| {
            format!(
                "struct=reach: `{line}` fails on {}: with sig.a switched on, sig.a reads on at \
                 tick 0 and {signal} not until tick {ticks}, {ticks} ticks later",
                edition.as_str(),
            )
        };
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [
                (
                    "E_SIM_ASSERTION_FAILED",
                    at_cell,
                    failed(at_cell, "sig.c", 2),
                    vec![(None, fix(2))],
                ),
                (
                    "E_SIM_ASSERTION_FAILED",
                    at_door,
                    failed(at_door, "sig.d", 4),
                    vec![
                        (
                            Some("door id=front side=front at=center"),
                            "sig.d is read at the torches under this actuator".to_owned(),
                        ),
                        (None, fix(4)),
                    ],
                ),
            ],
            "{edition:?}",
        );
    }
}

/// `sig.x` reads on a tick after either lever, and `sig.o` follows `sig.b` at once and `sig.a`
/// only through two stages of delay.
fn slow_a(asserts: &str) -> String {
    gate(
        "17x13",
        &format!(
            "  lever id=l1 at=inside.back offset=2 y=0 -> sig.a\n  \
             lever id=l2 at=inside.back offset=6 y=0 -> sig.b\n  \
             lamp id=lit at=inside.back offset=10 y=0\n  \
             logic sig.x = sig.a or sig.b\n  \
             logic sig.slow = delay(sig.a, 2)\n  \
             logic sig.o = sig.b or sig.slow\n  \
             door[id=d] opened_by=sig.o\n  \
             lamp[id=lit] lit_by=sig.x\n  \
             circuit region=floor void=4\n{asserts}"
        ),
    )
}

#[test]
fn an_always_is_refused_with_the_run_its_consequent_follows_latest_in() {
    let line = "assert always(sig.x -> eventually sig.o within 0)";
    let src = slow_a(&format!("  {line}\n"));
    for edition in EDITIONS {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "E_SIM_ASSERTION_FAILED",
                line,
                format!(
                    "struct=gate: `{line}` fails on {}: with sig.a switched on, sig.x reads on at \
                     tick 1 and sig.o not until tick 7, 6 ticks later",
                    edition.as_str(),
                ),
                vec![
                    (
                        Some("door id=d side=front at=center"),
                        "sig.o is read at the torches under this actuator".to_owned(),
                    ),
                    (
                        None,
                        "Fix: relax the bound to `within 6`, or shorten the path; the logic is \
                         never rewritten to meet an assertion"
                            .to_owned(),
                    ),
                ],
            )],
            "{edition:?}",
        );
    }
    let relaxed = slow_a("  assert always(sig.x -> eventually sig.o within 6)\n");
    for edition in EDITIONS {
        assert_eq!(verified(&relaxed, edition), [], "{edition:?}");
    }
}

#[test]
fn an_antecedent_that_reads_on_late_counts_the_ticks_from_when_it_does() {
    let late = "assert always(sig.o -> eventually sig.o within 0)";
    let never = "assert always(sig.o -> eventually sig.both within 0)";
    let src = or_and(&format!("  {late}\n  {never}\n"));
    for edition in EDITIONS {
        let failed = |line: &str, then: &str| {
            format!(
                "struct=gate: `{line}` fails on {}: with sig.b switched on, sig.o reads on at \
                 tick 1 and {then}",
                edition.as_str(),
            )
        };
        assert_eq!(
            findings(&src, &verified(&src, edition))
                .into_iter()
                .map(|(code, line, primary, notes)| (code, line, primary, notes.len()))
                .collect::<Vec<_>>(),
            [
                (
                    "E_SIM_ASSERTION_FAILED",
                    late,
                    failed(late, "sig.o not until tick 3, 2 ticks later"),
                    2,
                ),
                (
                    "E_SIM_ASSERTION_FAILED",
                    never,
                    failed(never, "sig.both never does"),
                    2,
                ),
            ],
            "{edition:?}",
        );
    }
}

/// The lamp across the floor reads the pulse after the one beside the lever does.
#[test]
fn an_always_two_actuators_read_counts_the_one_its_consequent_reaches_last() {
    let pulse = |within: u32| {
        format!(
            "theme t:\n  slot wall -> @oak_planks\n\n\
             struct far size=60x6\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
             lever id=l at=inside.back offset=1 y=0 -> sig.a\n  \
             lamp id=near at=inside.back offset=3 y=0 lit_by=sig.o\n  \
             lamp id=far at=inside.back offset=57 y=0 lit_by=sig.o\n  \
             logic sig.e = edge_rising(sig.a)\n  \
             logic sig.o = delay(sig.e, 1)\n  \
             circuit region=floor void=4\n  \
             assert always(sig.e -> eventually sig.o within {within})\n"
        )
    };
    let line = "assert always(sig.e -> eventually sig.o within 5)";
    let src = pulse(5);
    for edition in EDITIONS {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "E_SIM_ASSERTION_FAILED",
                line,
                format!(
                    "struct=far: `{line}` fails on {}: with sig.a switched on, sig.e reads on at \
                     tick 2 and sig.o not until tick 8, 6 ticks later",
                    edition.as_str(),
                ),
                vec![
                    (
                        Some("lamp id=far at=inside.back offset=57 y=0 lit_by=sig.o"),
                        "sig.o is read at the torches under this actuator, the slowest of the 2 \
                         that read it"
                            .to_owned(),
                    ),
                    (
                        None,
                        "Fix: relax the bound to `within 6`, or shorten the path; the logic is \
                         never rewritten to meet an assertion"
                            .to_owned(),
                    ),
                ],
            )],
            "{edition:?}",
        );
        assert_eq!(verified(&pulse(6), edition), [], "{edition:?}");
    }
}

/// `sig.m` follows `sig.f` a tick later on Java than on Bedrock.
#[test]
fn each_edition_runs_its_own_legalized_circuit() {
    let line = "assert always(sig.f -> eventually sig.m within 4)";
    let src = gate(
        "15x11",
        &format!(
            "  lever id=l1 at=inside.back offset=2 y=0 -> sig.a\n  \
             lever id=l2 at=inside.back offset=6 y=0 -> sig.b\n  \
             lamp id=lit at=inside.left offset=3 y=0\n  \
             logic sig.f = sig.a and sig.b\n  \
             logic sig.m = sig.f and sig.a\n  \
             lamp[id=lit] lit_by=sig.m\n  \
             circuit region=floor void=4\n  {line}\n"
        ),
    );
    assert_eq!(
        findings(&src, &verified(&src, Edition::Java))
            .into_iter()
            .map(|(code, _, primary, _)| (code, primary))
            .collect::<Vec<_>>(),
        [(
            "E_SIM_ASSERTION_FAILED",
            format!(
                "struct=gate: `{line}` fails on java: with sig.a and sig.b switched on, sig.f \
                 reads on at tick 2 and sig.m not until tick 7, 5 ticks later"
            ),
        )],
    );
    assert_eq!(verified(&src, Edition::Bedrock), []);
}

/// The delay alone takes 264 ticks.
#[test]
fn a_run_lasts_until_the_circuit_rests_however_long_its_path() {
    let late = "assert always(sig.a -> eventually sig.d within 273)";
    let wrong = "assert truth(sig.a -> sig.d) { 1 -> 0 }";
    let src = format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct slow size=272x6\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         door id=d side=front at=center\n  \
         lever id=l at=inside.back offset=1 y=0 -> sig.a\n  \
         logic sig.d = delay(sig.a, 132)\n  \
         door[id=d] opened_by=sig.d\n  \
         circuit region=floor void=3\n  \
         assert truth(sig.a -> sig.d) {{ 0 -> 0; 1 -> 1 }}\n  \
         assert always(sig.a -> eventually sig.d within 274)\n  {late}\n  {wrong}\n"
    );
    for edition in EDITIONS {
        let name = edition.as_str();
        assert_eq!(
            findings(&src, &verified(&src, edition))
                .into_iter()
                .map(|(code, line, primary, _)| (code, line, primary))
                .collect::<Vec<_>>(),
            [
                (
                    "E_SIM_ASSERTION_FAILED",
                    late,
                    format!(
                        "struct=slow: `{late}` fails on {name}: with sig.a switched on, sig.a \
                         reads on at tick 0 and sig.d not until tick 274, 274 ticks later"
                    ),
                ),
                (
                    "E_SIM_ASSERTION_FAILED",
                    wrong,
                    format!(
                        "struct=slow: `assert truth(sig.a -> sig.d)` fails on {name}: with sig.a \
                         switched on, the circuit rests with its inputs reading `1` and sig.d \
                         reading 1"
                    ),
                ),
            ],
            "{edition:?}",
        );
    }
}

#[test]
fn an_assertion_in_a_scope_that_lays_no_circuit_is_left_unchecked_with_a_warning() {
    let body = "  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
                lever id=l1 at=inside.back offset=2 y=0 -> sig.a\n  \
                lever id=l2 at=inside.back offset=6 y=0 -> sig.b\n";
    let always = "assert always(sig.a -> eventually sig.b within 0)";
    let truth = "assert truth(sig.a -> sig.c) { 1 -> 0 }";
    let src = format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct pair size=15x11\n{body}  logic sig.c = sig.a\n  {always}\n\n\
         def bare size=15x11:\n{body}  logic sig.c = sig.a\n  {truth}\n\n\
         site row:\n  place id=one use=bare theme=t at=origin\n"
    );
    let unchecked = |line, scope: &str, text: &str| {
        (
            "W_SIM_ASSERTION_UNCHECKED",
            line,
            format!(
                "{scope} `{text}` is not checked: the scope lays no circuit to run, its logic \
                 having no cell to build and no actuator to drive"
            ),
            vec![(
                None,
                "Fix: have an actuator read a signal the assertion names, so that the scope lays a \
                 circuit, or remove the assertion"
                    .to_owned(),
            )],
        )
    };
    for edition in EDITIONS {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [
                unchecked(always, "struct=pair:", always),
                unchecked(truth, "def=bare:", "assert truth(sig.a -> sig.c)"),
            ],
            "{edition:?}",
        );
    }
}

/// `W_UNUSED_DEF` says the def is built nowhere.
#[test]
fn a_def_no_place_builds_is_not_run() {
    let src = format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         def twin size=9x7:\n{NOT_BODY}  assert truth(sig.a -> sig.n) {{ 0 -> 0 }}\n"
    );
    for edition in EDITIONS {
        assert_eq!(verified(&src, edition), [], "{edition:?}");
    }
}

#[test]
fn a_def_placed_twice_reports_the_row_its_circuit_breaks_once() {
    let src = format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         def twin size=9x7:\n{NOT_BODY}  assert truth(sig.a -> sig.n) {{ 0 -> 0 }}\n\n\
         site row:\n  place id=one use=twin theme=t at=origin\n  \
         place id=two use=twin theme=t east_of=one gap=3\n"
    );
    for edition in EDITIONS {
        assert_eq!(
            verified(&src, edition)
                .into_iter()
                .map(|d| d.primary)
                .collect::<Vec<_>>(),
            [format!(
                "def=twin: `assert truth(sig.a -> sig.n)` fails on {}: with no sensor switched \
                 on, the circuit rests with its inputs reading `0` and sig.n reading 1",
                edition.as_str(),
            )],
            "{edition:?}",
        );
    }
}

/// A lever through a `not` to a door.
const NOT_BODY: &str = "  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
                        door id=d side=front at=center\n  \
                        lever id=l1 at=inside.back offset=2 y=0 -> sig.a\n  \
                        logic sig.n = not sig.a\n  \
                        door[id=d] opened_by=sig.n\n  \
                        circuit region=floor void=3\n";
