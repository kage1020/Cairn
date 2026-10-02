//! An actuator binding drives one physical component, and the netlist
//! carries a port only for a binding that does.
//!
//! `spec/redstone` "Signal binding": a binding written where no component
//! reads it "would put a port in the netlist with no component behind
//! it". The front end used to register an actuator from the line it was
//! written on and nothing else, so two shapes got a port and a placed pad
//! with nothing behind them, both exiting 0:
//!
//! - a selector-form patch whose `[id=]` names no physical door — which
//!   block-array lowering already deferred, from a lookup of its own that
//!   the front end never asked; and
//! - a second binding on one door, which became a second output port
//!   into it: a wired OR the logic layer never states.
//!
//! The third shape the same gap let through, a `->` tail on a sensor
//! keyword in the selector form, is `check`'s `E_MISPLACED_BINDING` now
//! (`cairn-lang-core/tests/check_binding.rs`); what this pass owes such a
//! line is the input it no longer registers.

use cairn_lang_redstone::{Diagnostic, DiagnosticCode, SynthOutput};

mod common;
use common::synth_source;

const PRELUDE: &str = concat!(
    "theme t:\n",
    "  slot wall -> @oak_planks\n",
    "\n",
    "struct s size=7x5\n",
    "  floor mat_slot=wall\n",
    "  walls mat_slot=wall height=3\n",
    "  door id=front side=front at=center\n",
    "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
);

fn source(body: &str) -> String {
    format!("{PRELUDE}{body}  circuit region=floor void=2\n")
}

fn codes(out: &SynthOutput) -> Vec<&'static str> {
    out.diagnostics.iter().map(|d| d.code.as_str()).collect()
}

fn only(out: &SynthOutput, code: DiagnosticCode) -> &Diagnostic {
    let found: Vec<_> = out.diagnostics.iter().filter(|d| d.code == code).collect();
    assert_eq!(
        found.len(),
        1,
        "expected one {code:?}: {:#?}",
        out.diagnostics
    );
    found[0]
}

fn slice<'s>(src: &'s str, d: &Diagnostic) -> &'s str {
    &src[d.span.start..d.span.end]
}

/// Output port names of scope `s`, or `None` when the scope was refused.
fn outputs(out: &SynthOutput) -> Option<Vec<String>> {
    out.scoped
        .scopes
        .iter()
        .find(|e| e.name == "s")
        .map(|e| e.ir.outputs.iter().map(|p| p.name.to_string()).collect())
}

fn inputs(out: &SynthOutput) -> Option<Vec<String>> {
    out.scoped
        .scopes
        .iter()
        .find(|e| e.name == "s")
        .map(|e| e.ir.inputs.iter().map(|p| p.name.to_string()).collect())
}

// --- a patch that picks no door ---------------------------------------

/// The shape the gap was found with: a patch on a door id nobody
/// declared. It was an output port with a placed pad.
#[test]
fn a_patch_on_an_id_no_door_carries_is_refused_and_gets_no_port() {
    let src = source("  door[id=nope] opened_by=sig.a\n");
    let out = synth_source(&src);
    assert_eq!(codes(&out), ["E_LOGIC_UNRESOLVED_PATCH"]);
    let d = only(&out, DiagnosticCode::LogicUnresolvedPatch);
    assert!(
        d.primary.contains(
            "door actuator patch selects `id=nope` but no physical door with that id exists \
             (known door ids: front)"
        ),
        "got: {}",
        d.primary,
    );
    // The id is what the author corrects, so the finding underlines it.
    assert_eq!(slice(&src, d), "nope");
    assert_eq!(
        outputs(&out),
        None,
        "an error keeps the scope out of the IR"
    );
}

/// Block-array lowering's `W_DEFERRED_MEMBER` primaries for `src`.
fn lowering_deferrals(src: &str) -> Vec<String> {
    let module = cairn_lang_core::parse(src).expect("parse");
    let ir = cairn_lang_core::lower(&module);
    let resolution = cairn_lang_core::resolve(&ir, None);
    let lowered = cairn_lang_core::block_array::lower_to_block_array(&ir, &resolution, None);
    lowered
        .diagnostics
        .iter()
        .filter(|d| d.code.as_str() == "W_DEFERRED_MEMBER")
        .map(|d| d.primary.clone())
        .collect()
}

/// Each reason a patch picks no door is the sentence block-array lowering
/// defers the same line with, read from the same lookup — so the two
/// passes cannot give one line two different accounts. One row per
/// reason, so an edit to any of them on one side alone fails here.
#[test]
fn every_reason_is_the_one_lowering_gives() {
    let cases = [
        (
            "no such id",
            "  door[id=nope] opened_by=sig.a\n",
            "no physical door with that id exists (known door ids: front)",
        ),
        (
            "no id",
            "  door[] opened_by=sig.a\n",
            "has no `[id=<label>]` selector",
        ),
        (
            "id not a label",
            "  door[id=3] opened_by=sig.a\n",
            "must be an identifier or string label, got integer",
        ),
        (
            // The second `front` sits under a `level`, which lowering's
            // flattened view merges with the top level and
            // `check::duplicate` (per scope body) does not flag.
            "ambiguous",
            concat!(
                "  level y=1\n",
                "    door id=front side=back at=center\n",
                "  door[id=front] opened_by=sig.a\n",
            ),
            "the same id is declared on 2 physical doors in this scope",
        ),
    ];
    for (name, body, reason) in cases {
        let src = source(body);
        let deferred = lowering_deferrals(&src);
        assert_eq!(deferred.len(), 1, "{name}: lowering gave {deferred:#?}");
        assert!(
            deferred[0].contains(reason),
            "{name}: lowering: {}",
            deferred[0]
        );
        let out = synth_source(&src);
        assert_eq!(codes(&out), ["E_LOGIC_UNRESOLVED_PATCH"], "{name}");
        let synth = &only(&out, DiagnosticCode::LogicUnresolvedPatch).primary;
        assert_eq!(
            synth,
            &format!(
                "struct=s: {}, so its `opened_by=` binding drives nothing",
                deferred[0]
            ),
            "{name}",
        );
    }
}

/// The one line the two passes describe differently, on purpose: a
/// selector key a `door` carries but a patch does not read. Lowering
/// refuses the key before it looks for a door, while this pass, which
/// leaves the selector's other keys to lowering, finds only that there is
/// no `id=` to look up by.
#[test]
fn a_selector_key_other_than_id_is_where_the_passes_diverge() {
    let src = source("  door[side=front] opened_by=sig.a\n");
    let deferred = lowering_deferrals(&src);
    assert_eq!(
        deferred,
        ["door actuator patch `[selector]` accepts only `id=<label>`; unknown attribute(s): side"],
    );
    let out = synth_source(&src);
    assert_eq!(codes(&out), ["E_LOGIC_UNRESOLVED_PATCH"]);
    let d = only(&out, DiagnosticCode::LogicUnresolvedPatch);
    assert!(
        d.primary.contains(
            "door actuator patch has no `[id=<label>]` selector naming the physical door"
        ),
        "got: {}",
        d.primary,
    );
    assert!(!d.primary.contains(&deferred[0]), "got: {}", d.primary);
}

/// An id two doors carry already names a declared door, so the repair is
/// to tell the two apart, not to name one.
#[test]
fn an_ambiguous_id_is_told_to_give_the_doors_distinct_ids() {
    let src = source(concat!(
        "  level y=1\n",
        "    door id=front side=back at=center\n",
        "  door[id=front] opened_by=sig.a\n",
    ));
    let out = synth_source(&src);
    let d = only(&out, DiagnosticCode::LogicUnresolvedPatch);
    assert!(
        !d.primary.contains("disambiguate"),
        "the reason states the fault and stops: {}",
        d.primary,
    );
    assert_eq!(
        d.notes.last().expect("a fix").message,
        "Fix: give the 2 `door` members distinct ids, so the selector names one.",
    );
}

/// A door declared without an `id=` is still a door. The finding counts
/// it rather than saying none is declared, and the repair is to label it.
#[test]
fn a_patch_beside_an_unlabelled_door_is_told_to_add_the_id() {
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @oak_planks\n",
        "\n",
        "struct s size=7x5\n",
        "  floor mat_slot=wall\n",
        "  walls mat_slot=wall height=3\n",
        "  door side=front at=center\n",
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  door[id=front] opened_by=sig.a\n",
        "  circuit region=floor void=2\n",
    );
    let out = synth_source(src);
    assert_eq!(codes(&out), ["E_LOGIC_UNRESOLVED_PATCH"]);
    let d = only(&out, DiagnosticCode::LogicUnresolvedPatch);
    assert!(
        d.primary
            .contains("(1 physical door is declared in this scope, without an `id=`)"),
        "got: {}",
        d.primary,
    );
    assert_eq!(
        d.notes.last().expect("a fix").message,
        "Fix: add `id=front` to the `door` this patch is meant to bind, or write `opened_by=` \
         on that `door`'s own line.",
    );
}

/// Beside a labelled door, the unlabelled one is named after the known ids
/// and the repair offers both edits.
#[test]
fn a_patch_beside_labelled_and_unlabelled_doors_mentions_both() {
    let src = source(concat!(
        "  door side=back at=center\n",
        "  door[id=nope] opened_by=sig.a\n",
    ));
    let out = synth_source(&src);
    let d = only(&out, DiagnosticCode::LogicUnresolvedPatch);
    assert!(
        d.primary
            .contains("(known door ids: front; 1 more without an `id=`)"),
        "got: {}",
        d.primary,
    );
    assert_eq!(
        d.notes.last().expect("a fix").message,
        "Fix: set `[id=<label>]` to one of the ids listed, or add `id=nope` to the `door` \
         without one that this patch is meant to bind.",
    );
}

/// A door written under a `level` is a door the patch can pick, so the
/// lookup is not the top level alone.
#[test]
fn a_patch_picks_a_door_declared_under_a_level() {
    let src = source(concat!(
        "  level y=1\n",
        "    door id=upper side=back at=center\n",
        "  door[id=upper] opened_by=sig.a\n",
    ));
    let out = synth_source(&src);
    assert_eq!(codes(&out), Vec::<&str>::new());
    assert_eq!(outputs(&out), Some(vec!["sig.a".to_owned()]));
}

/// The refused binding still read its signal, so the sensor feeding it
/// is not also told that nothing consumes it — the author did wire it.
#[test]
fn a_refused_patch_still_counts_its_signal_as_consumed() {
    let out = synth_source(&source("  door[id=nope] opened_by=sig.a\n"));
    assert!(
        !codes(&out).contains(&"W_LOGIC_UNUSED_SIGNAL"),
        "got: {:#?}",
        codes(&out),
    );
}

// --- one binding per door ---------------------------------------------

/// The issue's third shape: two patches on one door were two output
/// ports into it.
#[test]
fn a_second_patch_on_one_door_is_refused_with_a_note_at_the_first() {
    let src = source(concat!(
        "  pressure_plate id=q at=inside.front offset=1 y=0 -> sig.b\n",
        "  logic sig.x = not sig.b\n",
        "  door[id=front] opened_by=sig.a\n",
        "  door[id=front] opened_by=sig.x\n",
    ));
    let out = synth_source(&src);
    assert_eq!(codes(&out), ["E_LOGIC_DUPLICATE_BINDING"]);
    let d = only(&out, DiagnosticCode::LogicDuplicateBinding);
    assert_eq!(slice(&src, d), "sig.x");
    assert!(
        d.primary
            .contains("`door` `front` is already bound by `opened_by=sig.a`"),
        "got: {}",
        d.primary,
    );
    let first = d.notes[0]
        .span
        .clone()
        .expect("the note points at the first");
    assert_eq!(&src[first.start..first.end], "sig.a");
    let footer = &d.notes.last().expect("a fix").message;
    assert!(
        footer.contains("`logic sig.<name> = sig.a or sig.x`"),
        "got: {footer}",
    );
}

/// The door's own line is a binding too, so a patch after it is the
/// second one, whichever form each is written in.
#[test]
fn a_patch_on_a_door_bound_on_its_own_line_is_refused() {
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @oak_planks\n",
        "\n",
        "struct s size=7x5\n",
        "  floor mat_slot=wall\n",
        "  walls mat_slot=wall height=3\n",
        "  door id=front side=front at=center opened_by=sig.a\n",
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  pressure_plate id=q at=inside.front offset=1 y=0 -> sig.b\n",
        "  door[id=front] opened_by=sig.b\n",
        "  circuit region=floor void=2\n",
    );
    let out = synth_source(src);
    assert_eq!(codes(&out), ["E_LOGIC_DUPLICATE_BINDING"]);
    assert_eq!(
        slice(src, only(&out, DiagnosticCode::LogicDuplicateBinding)),
        "sig.b"
    );
}

/// The door's own line is the first binding wherever the patch is
/// written. A patch above it used to be filed first, so the finding
/// underlined the declaring line and pointed its note at the patch.
#[test]
fn a_patch_above_a_bound_door_is_the_binding_refused() {
    let src = concat!(
        "theme t:\n",
        "  slot wall -> @oak_planks\n",
        "\n",
        "struct s size=7x5\n",
        "  floor mat_slot=wall\n",
        "  walls mat_slot=wall height=3\n",
        "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n",
        "  pressure_plate id=q at=inside.front offset=1 y=0 -> sig.b\n",
        "  door[id=front] opened_by=sig.b\n",
        "  door id=front side=front at=center opened_by=sig.a\n",
        "  circuit region=floor void=2\n",
    );
    let out = synth_source(src);
    assert_eq!(codes(&out), ["E_LOGIC_DUPLICATE_BINDING"]);
    let d = only(&out, DiagnosticCode::LogicDuplicateBinding);
    assert_eq!(slice(src, d), "sig.b");
    let line_start = src[..d.span.start].rfind('\n').map_or(0, |i| i + 1);
    assert!(
        src[line_start..].starts_with("  door[id=front]"),
        "the patch line is the one blamed"
    );
    assert!(
        d.primary
            .contains("`door` `front` is already bound by `opened_by=sig.a`"),
        "got: {}",
        d.primary,
    );
    let first = d.notes[0]
        .span
        .clone()
        .expect("the note points at the first");
    let first_line = src[..first.start].rfind('\n').map_or(0, |i| i + 1);
    assert!(
        src[first_line..].starts_with("  door id=front"),
        "the note points at the declaration"
    );
    assert_eq!(&src[first.start..first.end], "sig.a");
    assert!(
        d.notes
            .last()
            .expect("a fix")
            .message
            .contains("`logic sig.<name> = sig.a or sig.b`"),
    );
    // The refused patch's signal was still wired by the author.
    assert!(!codes(&out).contains(&"W_LOGIC_UNUSED_SIGNAL"));
}

/// Two bindings to the same signal are one wire written twice. Combining
/// them in the logic layer would be `sig.a or sig.a`, a gate that does
/// nothing, so the repair is to delete the second line.
#[test]
fn a_second_binding_to_the_same_signal_is_told_to_delete_the_line() {
    let src = source("  door[id=front] opened_by=sig.a\n");
    let src = src.replace(
        "  door id=front side=front at=center\n",
        "  door id=front side=front at=center opened_by=sig.a\n",
    );
    let out = synth_source(&src);
    assert_eq!(codes(&out), ["E_LOGIC_DUPLICATE_BINDING"]);
    let d = only(&out, DiagnosticCode::LogicDuplicateBinding);
    assert_eq!(
        d.primary,
        "struct=s: `door` `front` is already bound by `opened_by=sig.a`, and this line binds it \
         to the same signal again",
    );
    let footer = &d.notes.last().expect("a fix").message;
    assert_eq!(
        footer,
        "Fix: delete the duplicate line; `door` `front` is already bound to `sig.a`.",
    );
    assert!(!footer.contains(" or "), "got: {footer}");
}

/// One binding on each of two doors is two ports — the duplicate is
/// per door, not per key.
#[test]
fn one_binding_on_each_of_two_doors_is_two_ports() {
    let src = source(concat!(
        "  door id=back side=back at=center\n",
        "  door[id=front] opened_by=sig.a\n",
        "  door[id=back] opened_by=sig.a\n",
    ));
    let out = synth_source(&src);
    assert_eq!(codes(&out), Vec::<&str>::new());
    assert_eq!(
        outputs(&out),
        Some(vec!["sig.a".to_owned(), "sig.a".to_owned()])
    );
}

// --- a sensor tail on a selector line ---------------------------------

/// `pressure_plate[id=nope] -> sig.b` is refused by `check`, which
/// `cairn synth` runs first. Here that means no input port — the plate
/// it would stand for was never declared — and no second finding about
/// `sig.b` on the door reading it.
#[test]
fn a_tail_on_a_selector_line_registers_no_input() {
    let src = source(concat!(
        "  pressure_plate[id=nope] -> sig.b\n",
        "  door[id=front] opened_by=sig.b\n",
    ));
    let out = synth_source(&src);
    assert_eq!(inputs(&out), Some(vec!["sig.a".to_owned()]));
    assert_eq!(outputs(&out), Some(Vec::new()));
    assert!(
        !codes(&out).contains(&"E_LOGIC_UNBOUND_SIGNAL"),
        "got: {:#?}",
        codes(&out),
    );
}
