//! A pinned target weighed against the version floors a build of its edition is held to:
//! `E_VERSION_CAP` below one, `E_REQUIRES_UNORDERABLE` for one the edition's table cannot place.

use cairn_lang_core::check::weigh_target;
use cairn_lang_core::resolve::{VersionOrder, declared_version_floors};
use cairn_lang_core::{Diagnostic, Edition, parse};
use serde_json::json;

use crate::common::slice;

fn java_order() -> VersionOrder {
    VersionOrder::new([
        ("1.20.4".to_owned(), 3700),
        ("1.21".to_owned(), 3953),
        ("1.21.4".to_owned(), 4189),
        ("26.3".to_owned(), 4800),
    ])
}

fn bedrock_order() -> VersionOrder {
    VersionOrder::new([
        ("1.21.0".to_owned(), 18_153_475),
        ("1.21.40".to_owned(), 18_163_713),
        ("1.21.60".to_owned(), 18_168_865),
    ])
}

fn order_of(edition: Edition) -> VersionOrder {
    match edition {
        Edition::Java => java_order(),
        Edition::Bedrock => bedrock_order(),
    }
}

fn java_targetable() -> Vec<String> {
    ["1.20.4", "1.21", "1.21.4", "26.3"]
        .iter()
        .map(|v| (*v).to_owned())
        .collect()
}

const BUILD: &str = "\
theme t:
  slot wall -> @oak_planks

struct hut size=5x5
  walls mat_slot=wall height=3
";

/// `source` weighed for a Java build at `target`.
fn weighed(source: &str, target: &str) -> Option<Diagnostic> {
    let module = parse(source).expect("the fixtures all parse");
    weigh_target(
        &declared_version_floors(&module, Edition::Java),
        Edition::Java,
        target,
        &java_targetable(),
        &order_of,
    )
}

fn notes(d: &Diagnostic) -> Vec<&str> {
    d.notes.iter().map(|n| n.message.as_str()).collect()
}

#[test]
fn a_target_every_floor_admits_is_not_refused() {
    let source = format!("@requires version>=1.21\n{BUILD}");
    assert_eq!(weighed(&source, "1.21"), None);
    assert_eq!(weighed(&source, "26.3"), None);
}

#[test]
fn a_source_with_no_floor_reads_no_version_table() {
    let module = parse(BUILD).expect("parses");
    let refusal = weigh_target(
        &declared_version_floors(&module, Edition::Java),
        Edition::Java,
        "1.20.4",
        &java_targetable(),
        &|_| panic!("no floor to place"),
    );
    assert_eq!(refusal, None);
}

#[test]
fn a_target_below_a_floor_is_refused_at_the_floor_with_the_targets_that_meet_it() {
    let source = format!("@cairn 2026.7\n@requires version>=1.21.4\n{BUILD}");
    let d = weighed(&source, "1.21").expect("1.21 is below the floor");
    assert_eq!(d.code.as_str(), "E_VERSION_CAP");
    assert_eq!(slice(&source, &d), "@requires version>=1.21.4");
    assert_eq!(
        d.primary,
        "this file requires version>=1.21.4 (target 1.21)"
    );
    assert_eq!(
        notes(&d),
        [
            "valid java targets: 1.21.4, 26.3",
            "fix: --target 1.21.4, or lower the `@requires` floor",
        ],
    );
    assert_eq!(
        serde_json::to_value(&d.data).expect("serialises"),
        json!({
            "kind": "version_cap",
            "edition": "java",
            "target": "1.21",
            "floor": "version>=1.21.4",
            "valid": ["1.21.4", "26.3"],
        }),
    );
}

#[test]
fn a_floor_a_def_declares_names_the_part_and_the_line_to_lower() {
    let source = "\
def hut size=3x3:
  requires java version>=1.21.4
  walls mat_slot=wall height=3

theme t:
  slot wall -> @oak_planks

site v:
  place id=a use=hut theme=t at=origin
";
    let d = weighed(source, "1.20.4").expect("1.20.4 is below the def's floor");
    assert_eq!(d.code.as_str(), "E_VERSION_CAP");
    assert_eq!(slice(source, &d), "requires java version>=1.21.4");
    assert_eq!(
        d.primary,
        "`def hut` requires java version>=1.21.4 (target 1.20.4)",
    );
    assert_eq!(
        notes(&d),
        [
            "`def hut` declares it, and every `place use=hut` inherits it",
            "valid java targets: 1.21.4, 26.3",
            "fix: --target 1.21.4, or lower the `requires` floor",
        ],
    );
}

#[test]
fn a_floor_no_target_meets_says_so_and_offers_no_target() {
    let source = format!("@requires version>=99.0\n{BUILD}");
    let d = weighed(&source, "26.3").expect("no target meets 99.0");
    assert_eq!(
        notes(&d),
        [
            "no supported java target satisfies it: 1.20.4, 1.21, 1.21.4, 26.3, latest",
            "fix: lower the `@requires` floor, or build against another edition",
        ],
    );
    assert_eq!(
        serde_json::to_value(&d.data).expect("serialises")["valid"],
        json!([]),
    );
}

#[test]
fn the_first_floor_in_source_order_that_refuses_the_target_is_named() {
    let source = format!("@requires version>=1.21\n@requires version>=1.21.4\n{BUILD}");
    let d = weighed(&source, "1.20.4").expect("both floors refuse 1.20.4");
    assert_eq!(slice(&source, &d), "@requires version>=1.21");
    assert_eq!(notes(&d)[0], "valid java targets: 1.21.4, 26.3");
}

#[test]
fn a_floor_the_table_cannot_place_is_named_ahead_of_one_the_target_is_below() {
    let source = format!("@requires version>=1.21.4\n@requires version>=1.21.40\n{BUILD}");
    let d = weighed(&source, "1.20.4").expect("1.21.40 names no Java release");
    assert_eq!(d.code.as_str(), "E_REQUIRES_UNORDERABLE");
    assert_eq!(slice(&source, &d), "@requires version>=1.21.40");
    assert_eq!(
        d.primary,
        "this file requires version>=1.21.40, which names no java release",
    );
    assert_eq!(
        notes(&d),
        [
            "java ships 1.21.4 and then 26.3, and nothing between them; Cairn orders versions \
             by DataVersion and a label that names no release has none",
            "`1.21.40` is a bedrock release; if that is the numbering this floor is written in, \
             say so",
            "fix: `@requires bedrock version>=1.21.40`, or name a java release",
        ],
    );
    assert_eq!(d.data, None);
}

#[test]
fn an_unplaceable_floor_scoped_to_the_edition_is_not_offered_the_other_one() {
    let source = format!("@requires java version>=1.21.40\n{BUILD}");
    let d = weighed(&source, "26.3").expect("1.21.40 names no Java release");
    assert_eq!(d.code.as_str(), "E_REQUIRES_UNORDERABLE");
    assert_eq!(
        notes(&d)[1..],
        ["fix: name a java release"],
        "got: {:?}",
        notes(&d),
    );
}
