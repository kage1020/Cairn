use cairn_lang_core::resolve::{VersionOrder, declared_version_floors};
use cairn_lang_core::{Edition, parse};

use super::*;

/// No shipped pack ends two painted ids at different versions, so these bounds are made up.
fn order() -> VersionOrder {
    VersionOrder::new([
        ("1.0".to_owned(), 1),
        ("2.0".to_owned(), 2),
        ("3.0".to_owned(), 3),
    ])
}

fn declared_at(id: &str, start: usize, version: &str) -> Bound {
    bound(
        &order(),
        Painted {
            id: id.to_owned(),
            span: start..start + 1,
        },
        version.to_owned(),
    )
}

fn named<'a>(ids: impl IntoIterator<Item = &'a Bound>) -> Vec<(&'a str, &'a str)> {
    ids.into_iter()
        .map(|b| (b.painted.id.as_str(), b.version.as_str()))
        .collect()
}

#[test]
fn the_earliest_end_bounds_the_range() {
    let inference = Inference {
        edition: Edition::Bedrock,
        first_declared: Vec::new(),
        last_declared: vec![
            declared_at("minecraft:a", 10, "2.0"),
            declared_at("minecraft:b", 20, "1.0"),
        ],
        undeclared: Vec::new(),
    };
    let (version, ids) = inference.max_edge().expect("two ids end the range");
    assert_eq!(version, "1.0");
    assert_eq!(named(ids), [("minecraft:b", "1.0")]);
}

#[test]
fn a_conflict_names_the_earliest_end_first_then_in_source_order() {
    let module = parse("@requires bedrock version>=3.0\n").expect("a header alone parses");
    let floors = declared_version_floors(&module, Edition::Bedrock);
    let found = conflicts(
        &[
            declared_at("minecraft:a", 10, "2.0"),
            declared_at("minecraft:c", 30, "1.0"),
            declared_at("minecraft:b", 20, "1.0"),
        ],
        &floors,
        &order(),
    );
    let [conflict] = found.as_slice() else {
        panic!("one floor, one conflict");
    };
    assert_eq!(
        named(&conflict.ids),
        [
            ("minecraft:b", "1.0"),
            ("minecraft:c", "1.0"),
            ("minecraft:a", "2.0"),
        ],
    );
}

#[test]
fn the_edges_cross_where_the_latest_start_follows_the_earliest_end() {
    let inference = Inference {
        edition: Edition::Bedrock,
        first_declared: vec![
            declared_at("minecraft:a", 10, "2.0"),
            declared_at("minecraft:b", 20, "3.0"),
        ],
        last_declared: vec![declared_at("minecraft:c", 30, "2.0")],
        undeclared: Vec::new(),
    };
    assert!(inference.crossed());
}
