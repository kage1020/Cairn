//! `assert latency` against the legalized circuit: every cell port and buffer repeater on the
//! slowest path counts, per edition (`spec/redstone` "Verification").

use cairn_lang_core::{Edition, lower, parse};
use cairn_lang_redstone::{Diagnostic, DiagnosticCode, verify_latency};

use crate::common::{IDENTITY_WIRE_SOURCE, LONG_WIRE_SOURCE, findings, legalized, synth_source};

/// The `E_SIM_ASSERTION_FAILED` findings the legalized `source` earns on `edition`.
fn verified(source: &str, edition: Edition) -> Vec<Diagnostic> {
    let intent = lower(&parse(source).expect("parse"));
    verify_latency(&intent, &legalized(source, edition))
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

const NO_BUFFER: &str = "the wire carries no buffer repeater, so only fewer cells on the path \
                         take ticks off";

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
    let failed = |edition| -> Vec<String> {
        verified(&src, edition)
            .iter()
            .map(|d| src[d.span.clone()].to_owned())
            .collect()
    };
    assert_eq!(
        failed(Edition::Java),
        ["assert latency(sig.b -> sig.f) <= 1"]
    );
    assert_eq!(
        failed(Edition::Bedrock),
        [
            "assert latency(sig.a -> sig.f) <= 1",
            "assert latency(sig.b -> sig.f) <= 1",
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
                    (
                        None,
                        "the wire carries 1 buffer repeater, one wherever it runs past 15 blocks \
                         of dust, so laying what reads a signal nearer what drives it needs fewer"
                            .to_owned(),
                    ),
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

#[test]
fn an_assertion_counts_only_its_own_scope_and_may_sit_under_a_member() {
    let body = "  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  door id=d side=front at=center\n  \
                lever id=l1 at=inside.back offset=2 y=0 -> sig.a\n  \
                logic sig.n = not sig.a\n  \
                door[id=d] opened_by=sig.n\n  \
                circuit region=floor void=3\n";
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
          from sig.a to sig.n takes 3 ticks, 2 in the torches up to the actuator"
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
