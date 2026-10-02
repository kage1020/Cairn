//! Insta snapshots for every file under `examples/`.
//!
//! The shipped examples are the "source parses" acceptance surface: if any
//! one of them stops parsing, the parser has lost coverage of input the
//! project promises to support. One snapshot per example, named
//! `parses_<stem>` so the fixture a diff belongs to is in its file name.

use cairn_lang_core::parse;

mod common;
use common::examples;

#[test]
fn every_shipped_example_parses_to_its_snapshot() {
    for (name, source) in examples() {
        let module = parse(&source).unwrap_or_else(|e| panic!("parse {name}: {e}"));
        let stem = name.trim_end_matches(".crn").replace('-', "_");
        insta::assert_yaml_snapshot!(format!("parses_{stem}"), module);
    }
}
