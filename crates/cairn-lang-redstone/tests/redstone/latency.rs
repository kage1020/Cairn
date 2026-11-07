//! `assert latency` against the legalized circuit: every cell port and buffer repeater on the
//! slowest path counts, per edition (`spec/redstone` "Verification").

use cairn_lang_core::{Edition, lower, parse};
use cairn_lang_redstone::{Diagnostic, DiagnosticCode, verify_latency};

use crate::common::{
    IDENTITY_WIRE_SOURCE, LONG_OUTWARD_WIRE_SOURCE, legalized_from_source, synth_source,
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
    verify_latency(&intent, &legalized_from_source(source, edition))
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

const NO_BUFFER: &str = "the wire carries no buffer repeater, so only fewer cells on the path \
                         take ticks off";

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
                    (
                        None,
                        "the wire carries 2 buffer repeaters, one wherever it runs past 15 blocks \
                         of dust, so a smaller `circuit` region, which puts the pads nearer what \
                         drives them, needs fewer"
                            .to_owned(),
                    ),
                    (None, fix(ticks)),
                ],
            )],
            "{edition:?}",
        );
    }
}

#[test]
fn a_path_from_a_signal_to_itself_is_the_wire_to_its_actuator() {
    for (bound, failed) in [(1, true), (2, false)] {
        let line = format!("assert latency(sig.a -> sig.a) <= {bound}");
        let src = format!("{IDENTITY_WIRE_SOURCE}  {line}\n");
        for edition in [Edition::Java, Edition::Bedrock] {
            let found = verified(&src, edition);
            assert_eq!(found.is_empty(), !failed, "{line} {edition:?}: {found:#?}");
            if let Some(failure) = found.first() {
                let notes: Vec<&str> = failure.notes.iter().map(|n| n.message.as_str()).collect();
                assert_eq!(
                    notes[0],
                    "the path ends at the pad of the actuator reading sig.a: 2 buffer repeaters \
                     on the wire to it",
                );
            }
        }
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
