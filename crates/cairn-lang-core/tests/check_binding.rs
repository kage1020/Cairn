//! `E_MISPLACED_BINDING` — a `-> value` tail on a member that cannot emit
//! a signal.
//!
//! The last of a member line's four fields to get a check. A tail is read
//! by exactly one thing, the sensor set of `spec/redstone` "Signal
//! binding", and until this pass landed the only code that said so lived
//! in `cairn-lang-redstone` behind `cairn synth
//! --experimental-logic-synth`. So `walls ... -> sig.w` built a wall,
//! exited 0 through `check` and `compile`, and left the signal it named
//! emitted by nothing.
//!
//! What is asked here is the host and only the host, because the host is
//! what can be asked without a Logic IR. The value-side questions — does
//! the tail name a signal, is that signal driven twice or by nobody —
//! stay in the redstone crate, and the fixtures below pin the boundary
//! from this side: a tail on a `pressure_plate` passes here whatever it
//! names.

use cairn_lang_core::Diagnostic;
use cairn_lang_core::intent::{SENSOR_HOSTS, known_keywords, role_of};

mod common;
use common::{codes, diagnose, notes, slice};

/// The one finding `source` is expected to raise, or a panic naming what
/// it raised instead.
fn only(source: &str) -> Diagnostic {
    let diags = diagnose(source);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly one finding, got {diags:#?}"
    );
    diags.into_iter().next().expect("length checked above")
}

#[test]
fn a_tail_on_a_member_that_cannot_emit_is_refused() {
    let d = only("struct s size=5x5\n  walls class=outer mat_slot=wall height=3 -> sig.w\n");
    assert_eq!(d.code.as_str(), "E_MISPLACED_BINDING");
    assert!(
        d.primary.contains("`walls` cannot emit a signal"),
        "got: {}",
        d.primary,
    );
}

#[test]
fn the_finding_lands_on_the_tail_and_not_on_the_line() {
    // The member carries four other fields and none of them is wrong; a
    // span covering the statement would make the reader find the tail
    // again. The value is what the author deletes or moves, so it is what
    // the finding underlines.
    let src = "struct s size=5x5\n  walls class=outer mat_slot=wall height=3 -> sig.w\n";
    let d = only(src);
    assert_eq!(slice(src, &d), "sig.w");
}

#[test]
fn the_repair_names_the_hosts_that_exist_and_the_ones_reserved() {
    // Either may be what the author meant: to move the tail onto the
    // plate they already have, or to write the `lever` the surface has
    // not reached. A note naming only the first would read as a refusal
    // of the second.
    let d = only("struct s size=5x5\n  walls mat_slot=wall height=3 -> sig.w\n");
    let note = notes(&d).join("\n");
    assert!(note.contains("`pressure_plate`"), "got: {note}");
    for reserved in ["`lever`", "`button`", "`daylight`", "`observer`"] {
        assert!(note.contains(reserved), "{reserved} missing from: {note}");
    }
}

#[test]
fn a_tail_on_a_sensor_is_accepted() {
    assert_eq!(
        codes("struct s size=5x5\n  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.a\n"),
        Vec::<&str>::new(),
    );
}

/// A tail on a sensor whose value names no signal is not this pass's.
///
/// `-> a` is a real mistake and `E_LOGIC_INVALID_SIGNAL` is its code; the
/// `sig.` namespace is a question about the Logic IR, which `check` does
/// not build. Pinned from this side so a later widening of this pass into
/// the value is a decision rather than a slip — the two would then both
/// fire on one line.
#[test]
fn a_tail_on_a_sensor_naming_no_signal_is_left_to_the_redstone_pipeline() {
    assert_eq!(
        codes("struct s size=5x5\n  pressure_plate id=p at=front.outside offset=0 y=0 -> a\n"),
        Vec::<&str>::new(),
    );
}

/// The host is asked before the value, and on a member that cannot emit
/// the value is not asked at all.
#[test]
fn a_tail_on_the_wrong_host_naming_no_signal_is_still_one_finding() {
    let d = only("struct s size=5x5\n  walls mat_slot=wall height=3 -> a\n");
    assert_eq!(d.code.as_str(), "E_MISPLACED_BINDING");
}

/// A keyword the role table does not know is left to `E_UNKNOWN_KEYWORD`.
///
/// `lever` is the case that matters: `spec/redstone` "Signal binding"
/// lists it as a sensor and the surface has not reached it, so telling
/// its author that the host cannot emit would be telling them the wrong
/// thing. The keyword's own finding carries the repair.
#[test]
fn a_tail_on_an_unknown_keyword_is_left_to_the_keyword_finding() {
    assert!(
        !known_keywords().contains(&"lever"),
        "`lever` became a keyword; this fixture needs another one",
    );
    assert_eq!(
        codes("struct s size=5x5\n  lever id=v side=front offset=1 y=1 -> sig.w\n"),
        ["E_UNKNOWN_KEYWORD"],
    );
}

/// The walk reaches a member nested under a grouping keyword.
///
/// `level y=N` is the one construct that nests today, and a pass that
/// only read the top level would let the same tail through one indent
/// deeper.
#[test]
fn a_tail_under_a_level_is_reached() {
    let d = only(concat!(
        "struct s size=5x5\n",
        "  level y=3\n",
        "    walls mat_slot=wall height=2 -> sig.w\n",
    ));
    assert_eq!(d.code.as_str(), "E_MISPLACED_BINDING");
}

/// A `def` body and a `site` body are walked too.
///
/// The three member lists are separate fields of the IR, so covering one
/// says nothing about the other two.
#[test]
fn a_tail_in_a_def_or_a_site_body_is_reached() {
    // `W_UNUSED_DEF` rides along on any `def` nothing places, so this
    // half asserts the presence of the code rather than the whole list.
    let in_def = diagnose(concat!(
        "def hut size=3x3:\n",
        "  walls mat_slot=wall height=2 -> sig.w\n",
    ));
    assert!(
        in_def
            .iter()
            .any(|d| d.code.as_str() == "E_MISPLACED_BINDING"),
        "{in_def:#?}",
    );

    let in_site = diagnose(concat!(
        "def hut size=3x3:\n",
        "  walls mat_slot=wall height=2\n",
        "\n",
        "site s:\n",
        "  place id=a use=hut at=origin -> sig.w\n",
    ));
    assert!(
        in_site
            .iter()
            .any(|d| d.code.as_str() == "E_MISPLACED_BINDING"),
        "{in_site:#?}",
    );
}

/// Every sensor host is a keyword the role table knows.
///
/// A tail sits on the member rather than on an argument, so there is no
/// vocabulary entry to hold the table against. What must hold is that
/// each host is a real keyword: the pass refuses a `->` on everything
/// else, and a host the role table has never heard of would refuse every
/// source that used it — the `Other` arm returns before the host is even
/// consulted.
#[test]
fn every_sensor_host_is_a_keyword_the_role_table_knows() {
    assert!(
        !SENSOR_HOSTS.is_empty(),
        "an empty host list refuses every tail",
    );
    for host in SENSOR_HOSTS {
        assert!(
            role_of(host).arguments().is_some(),
            "`{host}` carries a `->` tail per `spec/redstone` \"Signal binding\" and is \
             not a known keyword",
        );
    }
}
