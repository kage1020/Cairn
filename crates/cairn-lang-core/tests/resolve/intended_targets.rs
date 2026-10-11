//! `@intended_targets` weighed against the floors the same file declares, with labels ordered by
//! `DataVersion` in one edition's table, never as text.

use cairn_lang_core::check::weigh_intended_targets;
use cairn_lang_core::resolve::VersionOrder;
use cairn_lang_core::{Edition, parse};

/// Java's release table, cut to the rows these fixtures name. The top row is past `1.21.4` so
/// that Bedrock's `1.21.40` lands inside the table's span.
fn java_order() -> VersionOrder {
    VersionOrder::new([
        ("1.19".to_owned(), 3105),
        ("1.20".to_owned(), 3463),
        ("1.20.4".to_owned(), 3700),
        ("1.21".to_owned(), 3953),
        ("1.21.4".to_owned(), 4189),
        ("1.21.11".to_owned(), 4536),
        ("26.2".to_owned(), 4700),
    ])
}

/// The three versions the built-in Java pack ships block data for.
fn java_targetable() -> Vec<String> {
    ["1.20.4", "1.21", "1.21.4"]
        .iter()
        .map(|v| (*v).to_owned())
        .collect()
}

/// One structure, so the fixtures are files a build would accept.
const BUILD: &str = "\
theme t:
  slot wall -> @oak_planks

struct hut size=5x5
  walls mat_slot=wall height=3
";

fn findings(headers: &str) -> Vec<(&'static str, String)> {
    let module = parse(&format!("{headers}{BUILD}")).expect("the fixtures all parse");
    weigh_intended_targets(&module, Edition::Java, &java_order(), &java_targetable())
        .into_iter()
        .map(|d| (d.code.as_str(), d.primary))
        .collect()
}

fn codes(headers: &str) -> Vec<&'static str> {
    findings(headers)
        .into_iter()
        .map(|(code, _)| code)
        .collect()
}

// -- the contradiction ----------------------------------------------------

#[test]
fn a_floor_above_every_intended_target_is_an_error() {
    assert_eq!(
        codes("@requires version>=1.21\n@intended_targets [\"1.20.4\"]\n"),
        ["E_INTENDED_TARGET_CAP"],
    );
}

#[test]
fn the_finding_names_the_version_and_the_floor_that_refuses_it() {
    let found = findings("@requires version>=1.21\n@intended_targets [\"1.20.4\"]\n");
    let (_, primary) = found.first().expect("one finding");
    assert!(primary.contains("1.20.4"), "got: {primary}");
    let module = parse(&format!(
        "@requires version>=1.21\n@intended_targets [\"1.20.4\"]\n{BUILD}"
    ))
    .expect("parse");
    let notes =
        &weigh_intended_targets(&module, Edition::Java, &java_order(), &java_targetable())[0].notes;
    assert!(
        notes.iter().any(|n| n.message.contains("version>=1.21")),
        "the floor has to be quoted back: {notes:?}",
    );
    assert!(
        notes
            .iter()
            .any(|n| n.message.contains("valid java targets: 1.21, 1.21.4")),
        "and the closed set of candidates offered: {notes:?}",
    );
}

/// `spec/syntax` "Headers" calls the list a hint, and one half right is not an error.
#[test]
fn a_floor_above_part_of_the_list_is_a_warning() {
    assert_eq!(
        codes("@requires version>=1.21\n@intended_targets [\"1.20.4\",\"1.21.4\"]\n"),
        ["W_INTENDED_TARGET_CAP"],
    );
}

#[test]
fn a_list_at_or_above_the_floor_is_silent() {
    assert!(codes("@requires version>=1.21\n@intended_targets [\"1.21\",\"1.21.4\"]\n").is_empty(),);
}

#[test]
fn a_file_without_a_floor_raises_nothing() {
    assert!(codes("@intended_targets [\"1.21.4\"]\n").is_empty());
}

/// A `def`'s floor is a floor on every `place` naming it, per `spec/versioning-editions`
/// "Fail-loud and minimum-version inference".
#[test]
fn a_floor_a_placed_def_declares_reaches_the_comparison() {
    let source = "\
@intended_targets [\"1.20.4\"]

theme t:
  slot wall -> @oak_planks

def cottage size=5x5:
  requires version>=1.21.4
  walls mat_slot=wall height=3

site hamlet:
  place id=a use=cottage theme=t at=origin
";
    let module = parse(source).expect("parse");
    let found = weigh_intended_targets(&module, Edition::Java, &java_order(), &java_targetable());
    assert_eq!(
        found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
        ["E_INTENDED_TARGET_CAP"],
    );
    assert!(
        found[0]
            .notes
            .iter()
            .any(|n| n.message.contains("`def cottage`")),
        "the part that imposed the floor is where the repair is: {:?}",
        found[0].notes,
    );
}

// -- versions no target names ---------------------------------------------

#[test]
fn a_version_the_pack_cannot_build_is_its_own_finding() {
    assert_eq!(
        codes("@intended_targets [\"1.19\"]\n"),
        ["W_INTENDED_TARGET_UNSUPPORTED"],
    );
}

#[test]
fn a_label_this_edition_cannot_place_says_so() {
    let found = findings("@intended_targets [\"1.21.40\"]\n");
    assert_eq!(
        found.iter().map(|(code, _)| *code).collect::<Vec<_>>(),
        ["W_INTENDED_TARGET_UNSUPPORTED"],
    );
}

#[test]
fn an_unbuildable_version_below_the_floor_is_not_also_a_cap() {
    assert_eq!(
        codes("@requires version>=1.21\n@intended_targets [\"1.19\"]\n"),
        ["W_INTENDED_TARGET_UNSUPPORTED"],
    );
}

#[test]
fn a_capped_version_beside_an_unbuildable_one_is_two_findings() {
    assert_eq!(
        codes("@requires version>=1.21\n@intended_targets [\"1.20.4\",\"1.19\"]\n"),
        ["E_INTENDED_TARGET_CAP", "W_INTENDED_TARGET_UNSUPPORTED"],
    );
}

#[test]
fn an_unbuildable_name_does_not_demote_the_cap() {
    let capped = "@requires version>=1.21\n@intended_targets [\"1.20.4\"]\n";
    let with_a_name_nothing_builds =
        "@requires version>=1.21\n@intended_targets [\"1.20.4\",\"1.19\",\"1.21.40\"]\n";
    assert_eq!(codes(capped)[0], "E_INTENDED_TARGET_CAP");
    assert_eq!(
        codes(with_a_name_nothing_builds)[0],
        "E_INTENDED_TARGET_CAP",
        "two unbuildable names beside one capped version leave the cap \
         exactly as far-reaching as it was",
    );
}

#[test]
fn a_buildable_version_above_the_floor_keeps_the_finding_a_warning() {
    assert_eq!(
        codes("@requires version>=1.21\n@intended_targets [\"1.20.4\",\"1.21.4\",\"1.19\"]\n"),
        ["W_INTENDED_TARGET_CAP", "W_INTENDED_TARGET_UNSUPPORTED"],
    );
}

// -- the edges ------------------------------------------------------------

#[test]
fn an_empty_list_is_not_a_contradiction() {
    assert!(codes("@requires version>=1.21\n@intended_targets []\n").is_empty());
}

/// Such a floor is its own `E_REQUIRES_UNORDERABLE`, raised where a target is pinned.
#[test]
fn a_floor_this_table_cannot_place_refuses_no_intended_target() {
    assert!(codes("@requires version>=1.21.40\n@intended_targets [\"1.21\"]\n").is_empty());
}

#[test]
fn a_floor_scoped_to_the_other_edition_is_inert() {
    assert!(
        codes("@requires bedrock version>=1.21.60\n@intended_targets [\"1.20.4\"]\n").is_empty()
    );
}

#[test]
fn a_label_spelled_with_trailing_zeros_finds_its_row() {
    assert!(codes("@requires version>=1.21\n@intended_targets [\"1.21.0\"]\n").is_empty());
}

#[test]
fn each_edition_answers_for_itself() {
    let module = parse(&format!("@intended_targets [\"1.20.4\"]\n{BUILD}")).expect("parse");
    let bedrock = VersionOrder::new([
        ("1.21.0".to_owned(), 649),
        ("1.21.40".to_owned(), 686),
        ("1.21.60".to_owned(), 729),
    ]);
    let targetable: Vec<String> = ["1.21.0", "1.21.40", "1.21.60"]
        .iter()
        .map(|v| (*v).to_owned())
        .collect();
    assert!(
        weigh_intended_targets(&module, Edition::Java, &java_order(), &java_targetable())
            .is_empty(),
        "1.20.4 is an ordinary Java target",
    );
    assert_eq!(
        weigh_intended_targets(&module, Edition::Bedrock, &bedrock, &targetable)
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
        ["W_INTENDED_TARGET_UNSUPPORTED"],
    );
}
