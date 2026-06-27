//! Insta snapshots for every file under `examples/`.
//!
//! The shipped examples are the "source parses" acceptance surface: if any
//! one of them stops parsing, the parser has lost coverage of input the
//! project promises to support.

use cairn_lang_core::parse;

fn parse_example(filename: &str) -> cairn_lang_core::ast::Module {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("examples")
        .join(filename);
    let source =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    parse(&source).unwrap_or_else(|e| panic!("parse {filename}: {e}"))
}

#[test]
fn parses_cottage() {
    insta::assert_yaml_snapshot!(parse_example("cottage.crn"));
}

#[test]
fn parses_themed_tower() {
    insta::assert_yaml_snapshot!(parse_example("themed-tower.crn"));
}

#[test]
fn parses_village() {
    insta::assert_yaml_snapshot!(parse_example("village.crn"));
}

#[test]
fn parses_redstone_door() {
    insta::assert_yaml_snapshot!(parse_example("redstone-door.crn"));
}
