//! `assert latency` against the legalized circuit: every cell port and buffer repeater on the
//! slowest path counts, per edition (`spec/redstone` "Verification").

use cairn_lang_core::{Edition, lower, parse};
use cairn_lang_redstone::{Diagnostic, DiagnosticCode, verify_latency};

use crate::common::{
    IDENTITY_WIRE_SOURCE, LONG_OUTWARD_WIRE_SOURCE, legalized_from_source, shared_bus_source,
    synth_source,
};

/// One finding as `(code, spanned text, primary, notes)`, each note with the text its span covers.
type Finding<'a> = (
    &'static str,
    &'a str,
    String,
    Vec<(Option<&'a str>, String)>,
);

fn findings<'a>(source: &'a str, found: &[Diagnostic]) -> Vec<Finding<'a>> {
    found
        .iter()
        .map(|d| {
            let notes = d
                .notes
                .iter()
                .map(|n| (n.span.clone().map(|s| &source[s]), n.message.clone()))
                .collect();
            (
                d.code.as_str(),
                &source[d.span.clone()],
                d.primary.clone(),
                notes,
            )
        })
        .collect()
}

/// The `E_SIM_ASSERTION_FAILED` findings the legalized `source` earns on `edition`.
fn verified(source: &str, edition: Edition) -> Vec<Diagnostic> {
    let intent = lower(&parse(source).expect("parse"));
    verify_latency(&legalized_from_source(source, edition), &intent)
}

/// Two levers through an `and`, a `not` and an `or` to a door, `asserts` after the region.
fn chain(asserts: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct gate size=9x7\n  floor mat_slot=wall\n  door id=d side=front at=center\n  \
         lever id=l1 at=inside.back offset=1 y=1 -> sig.a\n  \
         lever id=l2 at=inside.back offset=2 y=1 -> sig.b\n  \
         logic sig.f = sig.a and sig.b\n  \
         logic sig.g = not sig.f\n  \
         logic sig.h = sig.g or sig.a\n  \
         door[id=d] opened_by=sig.h\n  \
         circuit region=floor void=2\n{asserts}"
    )
}

const NO_BUFFER: &str = "the path's wires carry no buffer repeater, so only a path through fewer \
                         or faster cells takes ticks off";

/// The footer of a path whose wires carry `n` buffer repeaters.
fn buffered(n: u32) -> String {
    format!(
        "the path's wires carry {n} buffer repeaters, one wherever a wire runs past 15 blocks of \
         dust, so a smaller `circuit` region, which puts the pads and the cells closer together, \
         needs fewer"
    )
}

fn fix(measured: u32) -> String {
    format!(
        "Fix: relax the bound to `<= {measured}`, or shorten the path; the logic is never \
         rewritten to meet a bound"
    )
}

#[test]
fn the_slowest_path_counts_each_cell_by_the_port_it_enters() {
    let line = "assert latency(sig.a -> sig.h) <= 2";
    let src = chain(&format!("  {line}\n"));
    for (edition, primary, cells) in [
        (
            Edition::Java,
            "struct=gate: `assert latency(sig.a -> sig.h) <= 2` fails on java: its slowest path \
             from sig.a to sig.h takes 3 ticks, all in its cells",
            [
                "the path enters cell #0 (`ComparatorAND`) by port `a`: 1 tick through the cell",
                "then cell #1 (`InverterTorch`) by port `a`: 1 tick through the cell",
                "then cell #2 (`RepeaterOR`) by port `b`: 1 tick through the cell",
            ],
        ),
        (
            Edition::Bedrock,
            "struct=gate: `assert latency(sig.a -> sig.h) <= 2` fails on bedrock: its slowest \
             path from sig.a to sig.h takes 3 ticks, all in its cells",
            [
                "the path enters cell #0 (`TorchAND`) by port `a`: 2 ticks through the cell",
                "then cell #1 (`InverterTorch`) by port `a`: 1 tick through the cell",
                "then cell #2 (`TorchOR`) by port `b`: 0 ticks through the cell",
            ],
        ),
    ] {
        let mut notes: Vec<(Option<&str>, String)> = cells
            .iter()
            .zip([
                "logic sig.f = sig.a and sig.b",
                "logic sig.g = not sig.f",
                "logic sig.h = sig.g or sig.a",
            ])
            .map(|(message, at)| (Some(at), (*message).to_owned()))
            .collect();
        notes.extend([
            (
                Some("sig.h"),
                "then the pad of the actuator reading sig.h: no buffer repeater on the wire to it"
                    .to_owned(),
            ),
            (None, NO_BUFFER.to_owned()),
            (None, fix(3)),
        ]);
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [("E_SIM_ASSERTION_FAILED", line, primary.to_owned(), notes)],
            "{edition:?}",
        );
    }
}

#[test]
fn a_bound_the_slowest_path_meets_is_silent() {
    let src = chain("  assert latency(sig.a -> sig.h) <= 3\n");
    for edition in [Edition::Java, Edition::Bedrock] {
        assert_eq!(verified(&src, edition), [], "{edition:?}");
    }
}

#[test]
fn a_comparator_and_takes_its_side_operand_a_tick_later() {
    let src = chain(
        "  assert latency(sig.a -> sig.f) <= 1\n  \
         assert latency(sig.b -> sig.f) <= 1\n",
    );
    // Which operand lands on which port is the netlist's to choose, so only the ports are pinned.
    let entries = |edition| -> Vec<String> {
        verified(&src, edition)
            .iter()
            .map(|d| d.notes[0].message.clone())
            .collect()
    };
    assert_eq!(
        entries(Edition::Java),
        ["the path enters cell #0 (`ComparatorAND`) by port `b`: 2 ticks through the cell"],
    );
    let mut bedrock = entries(Edition::Bedrock);
    bedrock.sort();
    assert_eq!(
        bedrock,
        [
            "the path enters cell #0 (`TorchAND`) by port `a`: 2 ticks through the cell",
            "the path enters cell #0 (`TorchAND`) by port `b`: 2 ticks through the cell",
        ],
    );
}

#[test]
fn a_path_to_a_signal_no_actuator_reads_ends_at_the_cell_driving_it() {
    let line = "assert latency(sig.f -> sig.g) <= 0";
    let src = chain(&format!("  {line}\n"));
    for (edition, name) in [(Edition::Java, "java"), (Edition::Bedrock, "bedrock")] {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "E_SIM_ASSERTION_FAILED",
                line,
                format!(
                    "struct=gate: `{line}` fails on {name}: its slowest path from sig.f to sig.g \
                     takes 1 tick, all in its cells"
                ),
                vec![
                    (
                        Some("logic sig.g = not sig.f"),
                        "the path enters cell #1 (`InverterTorch`) by port `a`: 1 tick through \
                         the cell"
                            .to_owned(),
                    ),
                    (None, NO_BUFFER.to_owned()),
                    (None, fix(1)),
                ],
            )],
            "{edition:?}",
        );
    }
}

#[test]
fn every_buffer_repeater_on_the_routed_wire_counts_a_tick() {
    let line = "assert latency(sig.a -> sig.c) <= 1";
    let src = format!("{LONG_OUTWARD_WIRE_SOURCE}  {line}\n");
    for (edition, name, cell, ticks) in [
        (Edition::Java, "java", "RepeaterOR", 3),
        (Edition::Bedrock, "bedrock", "TorchOR", 2),
    ] {
        let through = if ticks == 3 { "1 tick" } else { "0 ticks" };
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "E_SIM_ASSERTION_FAILED",
                line,
                format!(
                    "struct=reach: `{line}` fails on {name}: its slowest path from sig.a to sig.c \
                     takes {ticks} ticks, 2 in buffer repeaters on the routed wire"
                ),
                vec![
                    (
                        Some("logic sig.c = sig.a or sig.b"),
                        format!(
                            "the path enters cell #0 (`{cell}`) by port `a`: {through} through \
                             the cell"
                        ),
                    ),
                    (
                        Some("sig.c"),
                        "then the pad of the actuator reading sig.c: 2 buffer repeaters on the \
                         wire to it"
                            .to_owned(),
                    ),
                    (None, buffered(2)),
                    (None, fix(ticks)),
                ],
            )],
            "{edition:?}",
        );
    }
}

#[test]
fn a_path_from_a_signal_to_itself_is_the_wire_to_its_actuator() {
    let line = "assert latency(sig.a -> sig.a) <= 1";
    let src = format!("{IDENTITY_WIRE_SOURCE}  {line}\n");
    for (edition, name) in [(Edition::Java, "java"), (Edition::Bedrock, "bedrock")] {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "E_SIM_ASSERTION_FAILED",
                line,
                format!(
                    "struct=pass: `{line}` fails on {name}: its slowest path from sig.a to sig.a \
                     takes 2 ticks, 2 in buffer repeaters on the routed wire"
                ),
                vec![
                    (
                        Some("sig.a"),
                        "the path ends at the pad of the actuator reading sig.a: 2 buffer \
                         repeaters on the wire to it"
                            .to_owned(),
                    ),
                    (None, buffered(2)),
                    (None, fix(2)),
                ],
            )],
            "{edition:?}",
        );
        let met = format!("{IDENTITY_WIRE_SOURCE}  assert latency(sig.a -> sig.a) <= 2\n");
        assert_eq!(verified(&met, edition), [], "{edition:?}");
    }
}

/// [`LONG_OUTWARD_WIRE_SOURCE`] with the door reading `sig.d`, which `logic` gives the net of
/// `sig.c`, and `assert` after the region.
fn second_name(logic: &str, assert: &str) -> String {
    LONG_OUTWARD_WIRE_SOURCE
        .replace(
            "  door[id=front] opened_by=sig.c\n",
            &format!("  {logic}\n  door[id=front] opened_by=sig.d\n"),
        )
        .replace("void=3\n", &format!("void=3\n  {assert}\n"))
}

#[test]
fn a_path_ends_at_the_pad_of_an_actuator_reading_its_end_by_name() {
    for logic in ["logic sig.d = sig.c", "logic sig.d = sig.b or sig.a"] {
        let src = second_name(logic, "assert latency(sig.a -> sig.c) <= 0");
        for edition in [Edition::Java, Edition::Bedrock] {
            let found = verified(&src, edition);
            let expected: Vec<Finding<'_>> = match edition {
                Edition::Bedrock => Vec::new(),
                Edition::Java => vec![(
                    "E_SIM_ASSERTION_FAILED",
                    "assert latency(sig.a -> sig.c) <= 0",
                    "struct=reach: `assert latency(sig.a -> sig.c) <= 0` fails on java: its \
                     slowest path from sig.a to sig.c takes 1 tick, all in its cells"
                        .to_owned(),
                    vec![
                        (
                            Some("logic sig.c = sig.a or sig.b"),
                            "the path enters cell #0 (`RepeaterOR`) by port `a`: 1 tick through \
                             the cell"
                                .to_owned(),
                        ),
                        (None, NO_BUFFER.to_owned()),
                        (None, fix(1)),
                    ],
                )],
            };
            assert_eq!(findings(&src, &found), expected, "{logic} {edition:?}");
        }
    }
}

#[test]
fn buffer_repeaters_on_a_cells_input_wire_count_and_those_on_its_other_ports_do_not() {
    let line = "assert latency(sig.a -> sig.s15) <= 0";
    let src = format!("{}  {line}\n", shared_bus_source());
    for (edition, ticks, buffers) in [(Edition::Java, 35, 4), (Edition::Bedrock, 33, 1)] {
        let found = verified(&src, edition);
        let primaries: Vec<&str> = found.iter().map(|d| d.primary.as_str()).collect();
        let name = edition.as_str();
        assert_eq!(
            primaries,
            [format!(
                "struct=chain: `{line}` fails on {name}: its slowest path from sig.a to sig.s15 \
                 takes {ticks} ticks, {buffers} in buffer repeaters on the routed wire"
            )],
        );
    }
}

#[test]
fn a_gate_end_that_does_not_depend_on_its_start_is_refused_at_its_own_line() {
    let line = "assert latency(sig.g -> sig.f) <= 9";
    let src = chain(&format!("  {line}\n"));
    assert_eq!(
        findings(&src, &synth_source(&src).diagnostics),
        [(
            "E_SIM_ASSERTION_FAILED",
            line,
            "struct=gate: `assert latency(sig.g -> sig.f) <= 9` fails on every edition: sig.f \
             does not depend on sig.g, so no path carries a change at sig.g to it"
                .to_owned(),
            vec![
                (
                    Some("logic sig.f = sig.a and sig.b"),
                    "sig.f is driven here".to_owned(),
                ),
                (
                    None,
                    "Fix: start the path at a signal sig.f is computed from, or delete the \
                     assertion"
                        .to_owned(),
                ),
            ],
        )],
    );
}

#[test]
fn a_refused_end_on_a_shared_net_is_shown_at_the_line_that_names_it() {
    for logic in ["logic sig.d = sig.c", "logic sig.d = sig.b or sig.a"] {
        let src = second_name(
            logic,
            "logic sig.e = sig.d and sig.b\n  assert latency(sig.e -> sig.d) <= 4",
        );
        let refused: Vec<Finding<'_>> = findings(&src, &synth_source(&src).diagnostics)
            .into_iter()
            .filter(|(code, ..)| *code == "E_SIM_ASSERTION_FAILED")
            .collect();
        assert_eq!(
            refused,
            [(
                "E_SIM_ASSERTION_FAILED",
                "assert latency(sig.e -> sig.d) <= 4",
                "struct=reach: `assert latency(sig.e -> sig.d) <= 4` fails on every edition: \
                 sig.d does not depend on sig.e, so no path carries a change at sig.e to it"
                    .to_owned(),
                vec![
                    (Some(logic), "sig.d is driven here".to_owned()),
                    (
                        None,
                        "Fix: start the path at a signal sig.d is computed from, or delete the \
                         assertion"
                            .to_owned(),
                    ),
                ],
            )],
            "{logic}",
        );
    }
}

#[test]
fn an_assertion_counts_only_its_own_scope_and_may_sit_under_a_member() {
    let body = "  floor mat_slot=wall\n  door id=d side=front at=center\n  \
                lever id=l1 at=inside.back offset=1 y=1 -> sig.a\n  \
                logic sig.n = not sig.a\n  \
                door[id=d] opened_by=sig.n\n  \
                circuit region=floor void=2\n";
    let src = format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct quiet size=9x7\n{body}\n\
         struct strict size=9x7\n{body}  level y=0\n    assert latency(sig.a -> sig.n) <= 0\n"
    );
    let primaries: Vec<String> = verified(&src, Edition::Java)
        .into_iter()
        .map(|d| d.primary)
        .collect();
    assert_eq!(
        primaries,
        [
            "struct=strict: `assert latency(sig.a -> sig.n) <= 0` fails on java: its slowest path \
          from sig.a to sig.n takes 1 tick, all in its cells"
        ],
    );
}

#[test]
fn a_bound_whose_end_does_not_depend_on_its_start_is_refused_on_every_edition() {
    let line = "assert latency(sig.h -> sig.b) <= 9";
    let src = chain(&format!("  {line}\n"));
    let out = synth_source(&src);
    assert_eq!(
        findings(&src, &out.diagnostics),
        [(
            "E_SIM_ASSERTION_FAILED",
            line,
            "struct=gate: `assert latency(sig.h -> sig.b) <= 9` fails on every edition: sig.b \
             does not depend on sig.h, so no path carries a change at sig.h to it"
                .to_owned(),
            vec![
                (Some("sig.b"), "sig.b is driven here".to_owned()),
                (
                    None,
                    "Fix: start the path at a signal sig.b is computed from, or delete the \
                     assertion"
                        .to_owned(),
                ),
            ],
        )],
    );
    assert!(out.scoped.scopes.is_empty(), "an error keeps the scope out");
}

#[test]
fn a_bound_naming_a_signal_nothing_drives_is_reported_unbound() {
    let line = "assert latency(sig.a -> sig.nowhere) <= 4";
    let src = chain(&format!("  {line}\n"));
    assert_eq!(
        findings(&src, &synth_source(&src).diagnostics),
        [(
            DiagnosticCode::LogicUnboundSignal.as_str(),
            line,
            "struct=gate: an `assert` references `sig.nowhere`, but no sensor emits it and no \
             `logic` line defines it in this scope"
                .to_owned(),
            vec![(
                None,
                "Valid signals in scope: sig.a, sig.b, sig.f, sig.g, sig.h. Fix: rename to a \
                 defined signal, or drive `sig.nowhere` from a sensor / `logic` line."
                    .to_owned(),
            )],
        )],
    );
}

#[test]
fn a_bound_starting_at_a_signal_nothing_drives_is_reported_unbound() {
    let line = "assert latency(sig.nowhere -> sig.h) <= 4";
    let src = chain(&format!("  {line}\n"));
    let codes: Vec<(&str, &str)> = findings(&src, &synth_source(&src).diagnostics)
        .into_iter()
        .map(|(code, at, ..)| (code, at))
        .collect();
    assert_eq!(codes, [(DiagnosticCode::LogicUnboundSignal.as_str(), line)]);
}

/// A lever through a `not` to a door, the whole body of a scope.
const NOT_BODY: &str = "  floor mat_slot=wall\n  door id=d side=front at=center\n  \
                        lever id=l1 at=inside.back offset=1 y=1 -> sig.a\n  \
                        logic sig.n = not sig.a\n  \
                        door[id=d] opened_by=sig.n\n  \
                        circuit region=floor void=2\n";

#[test]
fn a_def_and_a_struct_of_one_name_each_check_their_own_bounds() {
    let src = format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct twin size=9x7\n{NOT_BODY}  assert latency(sig.a -> sig.n) <= 1\n\n\
         def twin size=9x7:\n{NOT_BODY}  assert latency(sig.a -> sig.n) <= 0\n"
    );
    let primaries: Vec<String> = verified(&src, Edition::Java)
        .into_iter()
        .map(|d| d.primary)
        .collect();
    assert_eq!(
        primaries,
        [
            "def=twin: `assert latency(sig.a -> sig.n) <= 0` fails on java: its slowest path from \
             sig.a to sig.n takes 1 tick, all in its cells"
        ],
    );
}

#[test]
fn a_scopes_failures_are_in_source_order_with_those_under_its_members() {
    let src = format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct s size=9x7\n  level y=0\n    assert latency(sig.a -> sig.n) <= 0\n{NOT_BODY}  \
         assert latency(sig.a -> sig.a) <= 0\n  assert latency(sig.a -> sig.n) <= 0\n"
    );
    let lines: Vec<usize> = verified(&src, Edition::Java)
        .iter()
        .map(|d| src[..d.span.start].lines().count())
        .collect();
    assert_eq!(lines, [6, 14]);
}
