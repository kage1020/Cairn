//! `assert latency` against the legalized circuit: every cell port and buffer repeater on the
//! slowest path counts, per edition (`spec/redstone` "Verification").

use cairn_lang_core::{Edition, lower, parse};
use cairn_lang_redstone::{Diagnostic, DiagnosticCode, verify_latency};

use crate::common::{IDENTITY_WIRE_SOURCE, LONG_WIRE_SOURCE, findings, legalized, synth_source};

/// The `E_SIM_ASSERTION_FAILED` findings the legalized `source` earns on `edition`.
fn verified(source: &str, edition: Edition) -> Vec<Diagnostic> {
    let intent = lower(&parse(source).expect("parse"));
    verify_latency(&legalized(source, edition), &intent)
}

/// Two levers through an `and`, a `not` and an `or` to a door, `asserts` after the region.
fn chain(asserts: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct gate size=15x11\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  door id=d side=front at=center\n  \
         lever id=l1 at=inside.back offset=2 y=0 -> sig.a\n  \
         lever id=l2 at=inside.back offset=6 y=0 -> sig.b\n  \
         logic sig.f = sig.a and sig.b\n  \
         logic sig.g = not sig.f\n  \
         logic sig.h = sig.g or sig.a\n  \
         door[id=d] opened_by=sig.h\n  \
         circuit region=floor void=4\n{asserts}"
    )
}

const NO_BUFFER: &str = "the path's wires carry no buffer repeater, so only a path through fewer \
                         or faster cells takes ticks off";

/// The footer of a path whose wires carry `n` buffer repeaters.
fn buffered(n: u32) -> String {
    let repeaters = if n == 1 { "repeater" } else { "repeaters" };
    format!(
        "the path's wires carry {n} buffer {repeaters}, one wherever a wire runs past 15 blocks \
         of dust, so laying what reads a signal nearer what drives it needs fewer"
    )
}

fn fix(measured: u32) -> String {
    format!(
        "Fix: relax the bound to `<= {measured}`, or shorten the path; the logic is never \
         rewritten to meet a bound"
    )
}

/// The note on the riser a path ends at, `buffers` the repeaters on the wire to it.
fn riser_note(signal: &str, buffers: &str) -> String {
    format!(
        "then the actuator reading {signal}: {buffers} on the wire to the torches under it, then 2 \
         ticks up to it"
    )
}

#[test]
fn the_slowest_path_counts_each_cell_by_the_port_it_enters_and_the_torches_up_to_the_actuator() {
    let line = "assert latency(sig.a -> sig.h) <= 2";
    let src = chain(&format!("  {line}\n"));
    for (edition, ticks, cells) in [
        (
            Edition::Java,
            5,
            [
                "the path enters cell #0 (`ComparatorAND`) by port `a`: 1 tick through the cell",
                "then cell #1 (`InverterTorch`) by port `a`: 1 tick through the cell",
                "then cell #2 (`RepeaterOR`) by port `b`: 1 tick through the cell",
            ],
        ),
        (
            Edition::Bedrock,
            6,
            [
                "the path enters cell #0 (`TorchAND`) by port `a`: 2 ticks through the cell",
                "then cell #1 (`InverterTorch`) by port `a`: 1 tick through the cell",
                "then cell #2 (`RepeaterOR`) by port `b`: 1 tick through the cell",
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
                Some("door id=d side=front at=center"),
                riser_note("sig.h", "no buffer repeater"),
            ),
            (None, NO_BUFFER.to_owned()),
            (None, fix(ticks)),
        ]);
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "E_SIM_ASSERTION_FAILED",
                line,
                format!(
                    "struct=gate: `{line}` fails on {edition}: its slowest path from sig.a to \
                     sig.h takes {ticks} ticks, 2 in the torches up to the actuator",
                    edition = edition.as_str(),
                ),
                notes,
            )],
            "{edition:?}",
        );
    }
}

#[test]
fn a_bound_the_slowest_path_meets_is_silent() {
    let src = chain("  assert latency(sig.a -> sig.h) <= 6\n");
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
    let src = format!("{LONG_WIRE_SOURCE}  {line}\n");
    for edition in [Edition::Java, Edition::Bedrock] {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "E_SIM_ASSERTION_FAILED",
                line,
                format!(
                    "struct=reach: `{line}` fails on {edition}: its slowest path from sig.a to \
                     sig.c takes 4 ticks, 1 in buffer repeaters on the routed wire and 2 in the \
                     torches up to the actuator",
                    edition = edition.as_str(),
                ),
                vec![
                    (
                        Some("logic sig.c = sig.a or sig.b"),
                        "the path enters cell #0 (`RepeaterOR`) by port `a`: 1 buffer repeater \
                         on the wire, then 1 tick through the cell"
                            .to_owned(),
                    ),
                    (
                        Some("door id=front side=front at=center"),
                        riser_note("sig.c", "no buffer repeater"),
                    ),
                    (None, buffered(1)),
                    (None, fix(4)),
                ],
            )],
            "{edition:?}",
        );
    }
}

#[test]
fn a_path_from_a_signal_to_itself_is_the_wire_to_its_actuator() {
    for (bound, failed) in [(2, true), (3, false)] {
        let line = format!("assert latency(sig.a -> sig.a) <= {bound}");
        let src = format!("{IDENTITY_WIRE_SOURCE}  {line}\n");
        for edition in [Edition::Java, Edition::Bedrock] {
            let found = verified(&src, edition);
            assert_eq!(found.is_empty(), !failed, "{line} {edition:?}: {found:#?}");
            if let Some(failure) = found.first() {
                let notes: Vec<&str> = failure.notes.iter().map(|n| n.message.as_str()).collect();
                assert_eq!(
                    notes[0],
                    "the path ends at the actuator reading sig.a: 1 buffer repeater on the wire \
                     to the torches under it, then 2 ticks up to it",
                );
            }
        }
    }
}

/// A lever through a `not` to a door, the whole body of a scope.
const NOT_BODY: &str = "  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
                        door id=d side=front at=center\n  \
                        lever id=l1 at=inside.back offset=2 y=0 -> sig.a\n  \
                        logic sig.n = not sig.a\n  \
                        door[id=d] opened_by=sig.n\n  \
                        circuit region=floor void=3\n";

#[test]
fn an_assertion_counts_only_its_own_scope_and_may_sit_under_a_member() {
    let src = format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct quiet size=9x7\n{NOT_BODY}\n\
         struct strict size=9x7\n{NOT_BODY}  level y=0\n    assert latency(sig.a -> sig.n) <= 0\n"
    );
    let primaries: Vec<String> = verified(&src, Edition::Java)
        .into_iter()
        .map(|d| d.primary)
        .collect();
    assert_eq!(
        primaries,
        [
            "struct=strict: `assert latency(sig.a -> sig.n) <= 0` fails on java: its slowest path \
          from sig.a to sig.n takes 3 ticks, 2 in the torches up to the actuator"
        ],
    );
}

#[test]
fn a_def_and_a_struct_of_one_name_each_check_their_own_bounds_once() {
    let src = format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct twin size=9x7\n{NOT_BODY}  assert latency(sig.a -> sig.n) <= 3\n\n\
         def twin size=9x7:\n{NOT_BODY}  assert latency(sig.a -> sig.n) <= 2\n\n\
         site row:\n  place id=one use=twin theme=t at=origin\n  \
         place id=two use=twin theme=t east_of=one gap=3\n"
    );
    let primaries: Vec<String> = verified(&src, Edition::Java)
        .into_iter()
        .map(|d| d.primary)
        .collect();
    assert_eq!(
        primaries,
        [
            "def=twin: `assert latency(sig.a -> sig.n) <= 2` fails on java: its slowest path from \
             sig.a to sig.n takes 3 ticks, 2 in the torches up to the actuator"
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
    assert_eq!(lines, [6, 15]);
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

/// [`LONG_WIRE_SOURCE`] with the door reading `sig.d`, which `logic` gives the net of `sig.c`,
/// and `assert` after the region.
fn second_name(logic: &str, assert: &str) -> String {
    LONG_WIRE_SOURCE
        .replace(
            "  door[id=front] opened_by=sig.c\n",
            &format!("  {logic}\n  door[id=front] opened_by=sig.d\n"),
        )
        .replace("void=4\n", &format!("void=4\n  {assert}\n"))
}

#[test]
fn a_path_ends_at_the_riser_of_an_actuator_reading_its_end_by_name() {
    for logic in ["logic sig.d = sig.c", "logic sig.d = sig.b or sig.a"] {
        let line = "assert latency(sig.a -> sig.c) <= 1";
        let src = second_name(logic, line);
        for edition in [Edition::Java, Edition::Bedrock] {
            assert_eq!(
                findings(&src, &verified(&src, edition)),
                [(
                    "E_SIM_ASSERTION_FAILED",
                    line,
                    format!(
                        "struct=reach: `{line}` fails on {edition}: its slowest path from sig.a \
                         to sig.c takes 2 ticks, 1 in buffer repeaters on the routed wire",
                        edition = edition.as_str(),
                    ),
                    vec![
                        (
                            Some("logic sig.c = sig.a or sig.b"),
                            "the path enters cell #0 (`RepeaterOR`) by port `a`: 1 buffer \
                             repeater on the wire, then 1 tick through the cell"
                                .to_owned(),
                        ),
                        (None, buffered(1)),
                        (None, fix(2)),
                    ],
                )],
                "{logic} {edition:?}",
            );
        }
    }
}

#[test]
fn a_refused_end_on_a_shared_net_is_shown_at_the_line_that_names_it() {
    for logic in ["logic sig.d = sig.c", "logic sig.d = sig.b or sig.a"] {
        let src = second_name(
            logic,
            "logic sig.e = sig.d and sig.b\n  assert latency(sig.e -> sig.d) <= 4",
        );
        let refused: Vec<_> = findings(&src, &synth_source(&src).diagnostics)
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

/// A lever whose signal a lamp beside it reads through a `not` and a door across the floor reads
/// straight, `asserts` after the region.
fn fork(asserts: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @oak_planks\n\n\
         struct fork size=40x6\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         door id=far side=front at=center\n  \
         lever id=l at=inside.back offset=1 y=0 -> sig.a\n  \
         lamp id=near at=inside.back offset=3 y=0 lit_by=sig.n\n  \
         logic sig.n = not sig.a\n  \
         door[id=far] opened_by=sig.a\n  \
         circuit region=floor void=4\n{asserts}"
    )
}

#[test]
fn buffer_repeaters_on_one_branch_of_a_net_count_only_on_paths_along_it() {
    let src =
        fork("  assert latency(sig.a -> sig.n) <= 0\n  assert latency(sig.a -> sig.a) <= 0\n");
    for edition in [Edition::Java, Edition::Bedrock] {
        let primaries: Vec<String> = verified(&src, edition)
            .into_iter()
            .map(|d| d.primary)
            .collect();
        let name = edition.as_str();
        assert_eq!(
            primaries,
            [
                format!(
                    "struct=fork: `assert latency(sig.a -> sig.n) <= 0` fails on {name}: its \
                     slowest path from sig.a to sig.n takes 3 ticks, 2 in the torches up to the \
                     actuator"
                ),
                format!(
                    "struct=fork: `assert latency(sig.a -> sig.a) <= 0` fails on {name}: its \
                     slowest path from sig.a to sig.a takes 3 ticks, 1 in buffer repeaters on the \
                     routed wire and 2 in the torches up to the actuator"
                ),
            ],
        );
    }
}

#[test]
fn a_signal_two_actuators_read_ends_at_the_one_with_more_buffer_repeaters() {
    let line = "assert latency(sig.a -> sig.a) <= 0";
    let src = fork(&format!("  {line}\n")).replace(
        "  door[id=far] opened_by=sig.a\n",
        "  lamp id=close at=inside.back offset=5 y=0 lit_by=sig.a\n  door[id=far] opened_by=sig.a\n",
    );
    for edition in [Edition::Java, Edition::Bedrock] {
        assert_eq!(
            findings(&src, &verified(&src, edition)),
            [(
                "E_SIM_ASSERTION_FAILED",
                line,
                format!(
                    "struct=fork: `{line}` fails on {edition}: its slowest path from sig.a to \
                     sig.a takes 3 ticks, 1 in buffer repeaters on the routed wire and 2 in the \
                     torches up to the actuator",
                    edition = edition.as_str(),
                ),
                vec![
                    (
                        Some("door id=far side=front at=center"),
                        "the path ends at the actuator reading sig.a: 1 buffer repeater on the \
                         wire to the torches under it, then 2 ticks up to it"
                            .to_owned(),
                    ),
                    (None, buffered(1)),
                    (None, fix(3)),
                ],
            )],
            "{edition:?}",
        );
    }
}
