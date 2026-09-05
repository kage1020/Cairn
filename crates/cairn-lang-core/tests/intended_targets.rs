//! `@intended_targets` weighed against the floors the same file declares.
//!
//! The two headers used to be two inert statements. A file could say
//! `@requires version>=1.21` on one line and `@intended_targets ["1.20.4"]`
//! on the next, and `cairn check` exited 0 — while `cairn compile --target
//! 1.20.4` refused with `E_VERSION_CAP`, because one of the two
//! declarations decides a build and the other decided nothing. The header
//! that reads like an instruction was the ignored one.
//!
//! The comparison needs a version table, so it is a pass of its own rather
//! than one of `check`'s: every question below is answered by ordering two
//! labels by `DataVersion` in one edition's table, and comparing them as
//! text is the defect `VersionOrder` exists to remove.

use cairn_lang_core::check::weigh_intended_targets;
use cairn_lang_core::resolve::VersionOrder;
use cairn_lang_core::{Edition, parse};

/// Java's release table, cut down to the rows these fixtures name.
///
/// Hand-built rather than read from the registry pack: this crate does not
/// depend on `cairn-lang-formats`, and the pass takes the table as an
/// argument precisely so it does not have to.
///
/// The top row is well past `1.21.4` on purpose. Bedrock's `1.21.40` has
/// to land *inside* the table's span to be the "names no release of this
/// edition" case rather than the "newer than anything here" one, which is
/// the shape the shipped table has and the shape the cross-edition
/// fixtures below are about.
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

/// The case the pass exists for: the file's own floor refuses every
/// version the file says it is for.
#[test]
fn a_floor_above_every_intended_target_is_an_error() {
    assert_eq!(
        codes("@requires version>=1.21\n@intended_targets [\"1.20.4\"]\n"),
        ["E_INTENDED_TARGET_CAP"],
    );
}

/// An error, and it says which line refuses which version — a finding that
/// names neither is one the author has to reconstruct from two headers.
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

/// Some, not all: the versions above the floor still build, so the header
/// is a wish stated too widely rather than a file nothing can be made of.
/// `spec/syntax.md` §5.3 calls it a hint, and a hint that is half right is
/// not an error.
#[test]
fn a_floor_above_part_of_the_list_is_a_warning() {
    assert_eq!(
        codes("@requires version>=1.21\n@intended_targets [\"1.20.4\",\"1.21.4\"]\n"),
        ["W_INTENDED_TARGET_CAP"],
    );
}

/// The ordinary file raises nothing, which is the case every other test
/// here is a departure from.
#[test]
fn a_list_at_or_above_the_floor_is_silent() {
    assert!(codes("@requires version>=1.21\n@intended_targets [\"1.21\",\"1.21.4\"]\n").is_empty(),);
}

/// No floor, nothing to contradict. The header alone is not a claim this
/// pass has anything to say about.
#[test]
fn a_file_without_a_floor_raises_nothing() {
    assert!(codes("@intended_targets [\"1.21.4\"]\n").is_empty());
}

/// A floor a `def` declares is a floor on every `place use=` that names it
/// (`spec/versioning-editions.md` §10.4), so the intent it contradicts is
/// the file's just as much as a header's would be.
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

/// A release the pack ships no block data for. Not a cap: no floor is
/// involved and raising one would not make `--target 1.19` exist.
#[test]
fn a_version_the_pack_cannot_build_is_its_own_finding() {
    assert_eq!(
        codes("@intended_targets [\"1.19\"]\n"),
        ["W_INTENDED_TARGET_UNSUPPORTED"],
    );
}

/// A label written in the other edition's numbering. Java ships `1.21.4`
/// and Bedrock `1.21.40`, and the two sets are disjoint, so a Java build
/// can say what this is rather than only that it is not one of three.
#[test]
fn a_label_this_edition_cannot_place_says_so() {
    let found = findings("@intended_targets [\"1.21.40\"]\n");
    assert_eq!(
        found.iter().map(|(code, _)| *code).collect::<Vec<_>>(),
        ["W_INTENDED_TARGET_UNSUPPORTED"],
    );
}

/// The two questions are asked in one order and it matters: a version this
/// compiler cannot build is reported as that whatever the floors say,
/// because "`--target 1.19` does not exist here" is what the author acts
/// on and a cap beside it would send them to edit a floor that is not what
/// stops the build.
#[test]
fn an_unbuildable_version_below_the_floor_is_not_also_a_cap() {
    assert_eq!(
        codes("@requires version>=1.21\n@intended_targets [\"1.19\"]\n"),
        ["W_INTENDED_TARGET_UNSUPPORTED"],
    );
}

/// One header can earn both: the versions it names are not all wrong in
/// the same way, and the two have different repairs.
#[test]
fn a_capped_version_beside_an_unbuildable_one_is_two_findings() {
    assert_eq!(
        codes("@requires version>=1.21\n@intended_targets [\"1.20.4\",\"1.19\"]\n"),
        ["W_INTENDED_TARGET_CAP", "W_INTENDED_TARGET_UNSUPPORTED"],
    );
}

// -- the edges ------------------------------------------------------------

/// `@intended_targets []` states no intention, so no floor contradicts it.
/// Reading the empty list as "names no buildable version" would be true
/// and useless.
#[test]
fn an_empty_list_is_not_a_contradiction() {
    assert!(codes("@requires version>=1.21\n@intended_targets []\n").is_empty());
}

/// A floor this edition's table cannot place refuses nothing here. It is
/// its own refusal (`E_REQUIRES_UNORDERABLE`, raised where a target is
/// pinned) against the `requires` line, and folding it in would refuse
/// every intended target of a file whose repair is on a different line.
#[test]
fn a_floor_this_table_cannot_place_refuses_no_intended_target() {
    assert!(codes("@requires version>=1.21.40\n@intended_targets [\"1.21\"]\n").is_empty());
}

/// A floor scoped to the other edition is inert here, exactly as it is for
/// `E_VERSION_CAP` — inert, not violated, which is what lets one file
/// declare a floor per edition.
#[test]
fn a_floor_scoped_to_the_other_edition_is_inert() {
    assert!(
        codes("@requires bedrock version>=1.21.60\n@intended_targets [\"1.20.4\"]\n").is_empty()
    );
}

/// Trailing zeros are not a different version: the lookup matches the way
/// the rest of the compiler does, so `1.21.0` finds Java's `1.21` row and
/// is weighed rather than reported as a version nobody ships.
#[test]
fn a_label_spelled_with_trailing_zeros_finds_its_row() {
    assert!(codes("@requires version>=1.21\n@intended_targets [\"1.21.0\"]\n").is_empty());
}

/// The same file answers differently per edition, which is why the finding
/// names the edition that asked: Bedrock ships none of Java's labels, so
/// what is an ordinary list there is a list of versions it cannot build.
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
