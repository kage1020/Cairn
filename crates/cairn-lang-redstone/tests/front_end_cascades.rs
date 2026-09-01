//! What each refusal takes away, and what it must not take away with it.
//!
//! The front end refuses a binding in four ways now — a sensor tail on a
//! member that cannot emit, an actuator key on the wrong component, a
//! signal-valued key nothing reads, and a `logic` left-hand side outside
//! the `sig.` namespace — and a member whose keyword the role table does
//! not know is skipped without a word, because `E_UNKNOWN_KEYWORD` is
//! already its finding.
//!
//! Each of those takes a driver or a consumer out of the scope, and every
//! pass downstream would then report the hole as a mistake of its own. The
//! crate's rule is one finding per root cause, so each refusal records what
//! it removed. This file is the ledger of that: every fixture asserts the
//! *whole* finding list, because "the code I asked about is present" is
//! exactly the assertion a leaked cascade slips past.

use cairn_lang_core::{lower, parse};
use cairn_lang_redstone::{ScopeKind, SynthOutput, synthesize};

fn synth_source(source: &str) -> SynthOutput {
    let module = parse(source).expect("parse");
    let intent = lower(&module);
    synthesize(&intent)
}

fn codes(out: &SynthOutput) -> Vec<&'static str> {
    out.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

const PRELUDE: &str = concat!(
    "@cairn 2026.06\n",
    "\n",
    "theme t:\n",
    "  slot wall -> @cobblestone\n",
    "\n",
    "struct gate size=5x5\n",
    "  walls class=outer mat_slot=wall height=3\n",
);

fn source(body: &str) -> String {
    format!("{PRELUDE}{body}")
}

/// A sensor and a `logic` line over it, so a fixture below can hang a
/// binding off a *derived* signal rather than off the sensor's own — the
/// unused audit only walks `logic` names, so a sensor-driven signal makes
/// every suppression test pass whether the suppression works or not.
const DERIVED: &str = concat!(
    "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
    "  logic sig.x = not sig.a\n",
);

// --- a member the role table does not know ------------------------------

#[test]
fn an_unknown_keyword_that_would_have_consumed_a_signal_leaves_no_unused_warning() {
    // `lamp` is not a keyword, so the member is `E_UNKNOWN_KEYWORD` and
    // this pass says nothing about it. But the author *did* wire `sig.x`,
    // and telling them it is unconsumed on top of telling them `lamp` does
    // not exist is the same mistake reported twice.
    let out = synth_source(&source(&format!("{DERIVED}  lamp id=l1 lit_by=sig.x\n")));
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
}

#[test]
fn an_unknown_keyword_that_would_have_driven_a_signal_leaves_no_unbound_error() {
    // The mirror on the driver side: `lever` is §14.2's sensor and not a
    // keyword, so the tail is skipped — and the `logic` line reading
    // `sig.w` must not be told the signal does not exist.
    let out = synth_source(&source(concat!(
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
    // `opened_by=` on a `window` is refused, and `sig.x` — a `logic` name,
    // so the unused audit does walk it — must not then be reported as
    // unconsumed.
    let out = synth_source(&source(&format!(
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
    // The actuator path has its own unbound check, separate from the one
    // `resolve_ref` walks, and this is the fixture that reaches it: the
    // door is well formed and reads a signal the refused `walls` tail
    // would have emitted.
    let out = synth_source(&source(concat!(
        "  walls class=inner mat_slot=wall height=1 -> sig.w\n",
        "  door id=front side=front at=center\n",
        "  door[id=front] opened_by=sig.w\n",
    )));
    assert_eq!(
        codes(&out),
        ["E_LOGIC_MISPLACED_BINDING"],
        "{:#?}",
        out.diagnostics
    );
}

#[test]
fn a_refused_logic_line_still_counts_as_reading_its_right_hand_side() {
    // The left-hand side is what `foo.bar` gets wrong; `sig.a` on the
    // right is a signal this line reads either way.
    let out = synth_source(&source(concat!(
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
    // The finish step used to return early on a scope with no sensor, no
    // actuator, and no `logic` line — so the one shape where an `assert`
    // is the *only* redstone content was the one shape nothing checked it
    // in.
    let out = synth_source(&source(
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
    let out = synth_source(&source(
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
    // `sig.x` *is* defined by a `logic` line — it failed to lower, which
    // is already reported. Saying "no `logic` line defines it" would be
    // false as well as second, and the candidate list would then offer
    // `sig.x` as a valid signal in the same finding that called it
    // undefined.
    let out = synth_source(&source(concat!(
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
    // `sig.x` fails to lower, and the actuator below then names a signal
    // nothing defines. Its finding offers the signals in scope — and a
    // name whose binding failed is not one: renaming to it lands the
    // author on the finding they already have.
    //
    // The `sig.x` line's *own* finding still lists it, because at that
    // moment `sig.x` is a binding like any other; the contradiction the
    // filter closes is a later reader being offered a name the pass has
    // since refused.
    let source = source(concat!(
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
    // A property observes a wire. Checking the references an `assert`
    // names and then not counting them would warn that a signal is unused
    // in the one place the author wrote down what it is for.
    let out = synth_source(&source(&format!(
        "{DERIVED}  assert always(sig.a -> eventually sig.x within 2)\n",
    )));
    assert!(out.diagnostics.is_empty(), "{:#?}", out.diagnostics);
}

#[test]
fn an_assert_inside_a_level_is_collected_with_the_rest() {
    // `collect_member` gained the nested-`assert` walk in this change; the
    // old code discarded a `level` block's asserts outright.
    let out = synth_source(&source(concat!(
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
    // The result-side sweep. A scope that lowers to one input and no
    // output is a plate in the build wired to nothing, which is the
    // silence this whole pass exists to break — and it is the shape every
    // "the value is not a `sig.` reference" hole ends up in.
    let out = synth_source(&source(
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
    // A `site` body feeds `placements` in as members, which is the one
    // scope kind whose member list is not a geometry body.
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
    // Collection raises its findings before the phases run, so without a
    // sort a refusal on the last line precedes a driver collision on the
    // first.
    let source = source(concat!(
        "  pressure_plate id=p1 at=front.outside offset=0 y=0 -> sig.a\n",
        "  pressure_plate id=p2 at=inside.front offset=0 y=0 -> sig.a\n",
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
