//! `E_MISPLACED_BINDING`: a `-> value` tail on a member that cannot emit a signal is an error;
//! what the tail names is the redstone crate's question.

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::intent::{SENSOR_HOSTS, known_keywords, role_of};

use crate::common::{codes, diagnose, exactly_one, notes, slice};

#[test]
fn a_tail_on_a_member_that_cannot_emit_is_refused() {
    let d = exactly_one(diagnose(
        "struct s size=5x5\n  walls class=outer mat_slot=wall height=3 -> sig.w\n",
    ));
    assert_eq!(d.code.as_str(), "E_MISPLACED_BINDING");
    assert!(
        d.primary.contains("`walls` cannot emit a signal"),
        "got: {}",
        d.primary,
    );
}

#[test]
fn the_finding_lands_on_the_tail_and_not_on_the_line() {
    let src = "struct s size=5x5\n  walls class=outer mat_slot=wall height=3 -> sig.w\n";
    let d = exactly_one(diagnose(src));
    assert_eq!(slice(src, &d), "sig.w");
}

#[test]
fn the_repair_names_the_hosts_that_exist_and_the_ones_reserved() {
    let d = exactly_one(diagnose(
        "struct s size=5x5\n  walls mat_slot=wall height=3 -> sig.w\n",
    ));
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

/// `-> a` is `E_LOGIC_INVALID_SIGNAL`'s, which needs the Logic IR that `check` does not build.
#[test]
fn a_tail_on_a_sensor_naming_no_signal_is_left_to_the_redstone_pipeline() {
    assert_eq!(
        codes("struct s size=5x5\n  pressure_plate id=p at=front.outside offset=0 y=0 -> a\n"),
        Vec::<&str>::new(),
    );
}

#[test]
fn a_tail_on_the_wrong_host_naming_no_signal_is_still_one_finding() {
    let d = exactly_one(diagnose(
        "struct s size=5x5\n  walls mat_slot=wall height=3 -> a\n",
    ));
    assert_eq!(d.code.as_str(), "E_MISPLACED_BINDING");
}

/// `spec/redstone` "Signal binding" lists `lever` as a sensor the surface has not reached.
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

#[test]
fn a_tail_under_a_level_is_reached() {
    let d = exactly_one(diagnose(concat!(
        "struct s size=5x5\n",
        "  level y=3\n",
        "    walls mat_slot=wall height=2 -> sig.w\n",
    )));
    assert_eq!(d.code.as_str(), "E_MISPLACED_BINDING");
}

#[test]
fn a_tail_in_a_def_or_a_site_body_is_reached() {
    // An unplaced `def` also earns `W_UNUSED_DEF`.
    let in_def = diagnose(concat!(
        "def hut size=3x3:\n",
        "  walls mat_slot=wall height=2 -> sig.w\n",
    ));
    assert!(
        in_def
            .iter()
            .any(|d| d.code == DiagnosticCode::MisplacedBinding),
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
            .any(|d| d.code == DiagnosticCode::MisplacedBinding),
        "{in_site:#?}",
    );
}

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

#[test]
fn a_tail_on_a_sensor_keyword_in_the_selector_form_is_refused() {
    let src = "struct s size=5x5\n  pressure_plate[id=nope] -> sig.b\n";
    let d = exactly_one(diagnose(src));
    assert_eq!(d.code.as_str(), "E_MISPLACED_BINDING");
    assert!(
        d.primary.contains(
            "`pressure_plate[...]` is the selector form, which carries no sensor binding"
        ),
        "got: {}",
        d.primary,
    );
    assert_eq!(slice(src, &d), "sig.b");
    let note = notes(&d).join("\n");
    assert!(
        note.contains("`door[id=<label>] opened_by=`"),
        "got: {note}"
    );
}

#[test]
fn a_selector_tail_is_refused_even_when_the_selector_names_a_real_plate() {
    let src = concat!(
        "struct s size=5x5\n",
        "  pressure_plate id=p at=front.outside offset=0 y=0\n",
        "  pressure_plate[id=p] -> sig.b\n",
    );
    assert_eq!(codes(src), ["E_MISPLACED_BINDING"]);
}

#[test]
fn a_selector_tail_on_a_non_sensor_is_told_about_the_host() {
    let d = exactly_one(diagnose(
        "struct s size=5x5\n  walls[class=outer] mat_slot=wall height=3 -> sig.w\n",
    ));
    assert!(
        d.primary.contains("`walls` cannot emit a signal"),
        "got: {}",
        d.primary,
    );
}
