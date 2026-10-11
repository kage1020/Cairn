//! A refused binding or unknown member takes its driver or consumer with it, and no later pass
//! reports the hole: each fixture asserts the whole finding list, one finding per root cause.

use cairn_lang_redstone::ScopeKind;

use crate::common::{codes, gate_source, synth_source};

/// A `logic`-derived `sig.x`: the unused audit walks only `logic` names, so a sensor's own signal
/// would let every suppression test pass whether it works or not.
const DERIVED: &str = concat!(
    "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
    "  logic sig.x = not sig.a\n",
);

// --- a member the role table does not know ------------------------------

#[test]
fn an_unknown_keyword_that_would_have_consumed_a_signal_leaves_no_unused_warning() {
    let out = synth_source(&gate_source(&format!(
        "{DERIVED}  lamp id=l1 lit_by=sig.x\n"
    )));
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
}

#[test]
fn an_unknown_keyword_that_would_have_driven_a_signal_leaves_no_unbound_error() {
    // `lever` is a sensor in `spec/redstone` "Signal binding" but not a keyword.
    let out = synth_source(&gate_source(concat!(
        "  lever id=v side=front offset=1 y=1 -> sig.w\n",
        "  logic sig.x = not sig.w\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.x\n",
    )));
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
}

// --- a refusal on a known member ----------------------------------------

#[test]
fn a_misplaced_actuator_on_a_derived_signal_leaves_no_unused_warning() {
    let out = synth_source(&gate_source(&format!(
        "{DERIVED}  window side=front y=1 offset=1 size=1x1 mat_slot=wall opened_by=sig.x\n",
    )));
    assert_eq!(
        codes(&out),
        ["E_LOGIC_MISPLACED_BINDING"],
        "{:#?}",
        out.diagnostics
    );
}

#[test]
fn an_actuator_reading_a_signal_a_refused_tail_would_have_driven_is_not_reported() {
    // The `walls` tail is `check`'s `E_MISPLACED_BINDING`, so synth itself adds nothing.
    let out = synth_source(&gate_source(concat!(
        "  walls class=inner mat_slot=wall height=1 -> sig.w\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.w\n",
    )));
    assert_eq!(codes(&out), Vec::<&str>::new(), "{:#?}", out.diagnostics);
}

#[test]
fn a_refused_logic_line_still_counts_as_reading_its_right_hand_side() {
    let out = synth_source(&gate_source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  logic foo.bar = not sig.a\n",
    )));
    assert_eq!(
        codes(&out),
        ["E_LOGIC_INVALID_SIGNAL"],
        "{:#?}",
        out.diagnostics
    );
}

// --- `assert` ------------------------------------------------------------

#[test]
fn a_scope_whose_only_content_is_an_assert_is_still_checked() {
    let out = synth_source(&gate_source(
        "  assert always(sig.zzz -> eventually sig.qqq within 3)\n",
    ));
    assert_eq!(
        codes(&out),
        ["E_LOGIC_UNBOUND_SIGNAL", "E_LOGIC_UNBOUND_SIGNAL"],
        "{:#?}",
        out.diagnostics,
    );
}

#[test]
fn an_assert_naming_one_signal_twice_reports_it_once() {
    let out = synth_source(&gate_source(
        "  assert always(sig.zzz -> eventually sig.zzz within 3)\n",
    ));
    assert_eq!(
        codes(&out),
        ["E_LOGIC_UNBOUND_SIGNAL"],
        "{:#?}",
        out.diagnostics
    );
}

#[test]
fn an_assert_over_a_binding_that_failed_to_lower_adds_nothing() {
    let out = synth_source(&gate_source(concat!(
        "  logic sig.x = not sig.undef\n",
        "  assert always(sig.x -> eventually sig.x within 3)\n",
    )));
    assert_eq!(
        codes(&out),
        ["E_LOGIC_UNBOUND_SIGNAL"],
        "{:#?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics[0].primary.contains("`sig.undef`"),
        "the one finding is about the name that is genuinely undefined: {}",
        out.diagnostics[0].primary,
    );
}

#[test]
fn a_candidate_list_never_offers_a_signal_the_pass_has_refused() {
    // `sig.x`'s own finding still lists it; only later findings drop it.
    let source = gate_source(concat!(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  logic sig.x = not sig.undef\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.nope\n",
    ));
    let out = synth_source(&source);
    let actuator = out
        .diagnostics
        .iter()
        .find(|d| d.primary.contains("`sig.nope`"))
        .expect("the actuator's unbound finding");
    let footer = actuator
        .notes
        .iter()
        .find_map(|n| n.message.strip_prefix("Valid signals in scope: "))
        .expect("the unbound finding lists the signals in scope");
    assert!(
        !footer.contains("sig.x"),
        "`sig.x` failed to lower, so it is not a signal to rename to: {footer}",
    );
    assert!(footer.contains("sig.a"), "{footer}");
}

#[test]
fn an_assert_counts_as_a_consumer() {
    let out = synth_source(&gate_source(&format!(
        "{DERIVED}  assert always(sig.a -> eventually sig.x within 2)\n",
    )));
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
}

#[test]
fn an_assert_inside_a_level_is_collected_with_the_rest() {
    let out = synth_source(&gate_source(concat!(
        "  level y=0\n",
        "    assert always(sig.zzz -> eventually sig.qqq within 3)\n",
    )));
    assert_eq!(
        codes(&out),
        ["E_LOGIC_UNBOUND_SIGNAL", "E_LOGIC_UNBOUND_SIGNAL"],
        "{:#?}",
        out.diagnostics,
    );
}

// --- a sensor nothing reads ---------------------------------------------

#[test]
fn a_sensor_whose_signal_nothing_reads_is_reported() {
    let out = synth_source(&gate_source(
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
    ));
    assert_eq!(
        codes(&out),
        ["W_LOGIC_UNUSED_SIGNAL"],
        "{:#?}",
        out.diagnostics
    );
}

// --- the scopes that are not a `struct` ---------------------------------

#[test]
fn a_def_scope_carries_its_own_label_through_synth() {
    let out = synth_source(concat!(
        "@cairn 2026.06\n",
        "\n",
        "def hut size=5x5:\n",
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  logic sig.x = not sig.a\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.x\n",
        "  assert always(sig.zzz -> eventually sig.zzz within 1)\n",
    ));
    assert_eq!(
        codes(&out),
        ["E_LOGIC_UNBOUND_SIGNAL"],
        "{:#?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics[0].primary.starts_with("def=hut:"),
        "{}",
        out.diagnostics[0].primary,
    );
}

#[test]
fn a_site_scope_carries_its_own_label_through_synth() {
    let out = synth_source(concat!(
        "@cairn 2026.06\n",
        "\n",
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "\n",
        "def hut size=5x5:\n",
        "  walls class=outer mat_slot=wall height=3\n",
        "\n",
        "site plaza:\n",
        "  place id=one use=hut theme=t at=origin\n",
        "  logic sig.s = sig.q and sig.q\n",
    ));
    let entries: Vec<(ScopeKind, &str)> = out
        .scoped
        .scopes
        .iter()
        .map(|s| (s.kind, s.name.as_str()))
        .collect();
    assert!(entries.is_empty(), "the scope has an error: {entries:?}");
    assert_eq!(
        codes(&out),
        ["E_LOGIC_UNBOUND_SIGNAL"],
        "{:#?}",
        out.diagnostics
    );
    assert!(
        out.diagnostics[0].primary.starts_with("site=plaza:"),
        "{}",
        out.diagnostics[0].primary,
    );
}

// --- findings come out in the order the file reads ----------------------

#[test]
fn findings_from_two_collection_phases_come_out_in_line_order() {
    let source = gate_source(concat!(
        "  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a\n",
        "  pressure_plate id=p2 at=inside.front offset=1 y=0 -> sig.a\n",
        "  walls class=inner mat_slot=wall height=1 -> sig.w\n",
    ));
    let out = synth_source(&source);
    let lines: Vec<usize> = out
        .diagnostics
        .iter()
        .map(|d| {
            source[..d.span.start]
                .bytes()
                .filter(|b| *b == b'\n')
                .count()
                + 1
        })
        .collect();
    assert!(
        lines.windows(2).all(|w| w[0] <= w[1]),
        "findings are not in source order: {lines:?} for {:#?}",
        codes(&out),
    );
}
