use cairn_lang_core::check::LineStarts;
use cairn_lang_core::parse;
use serde_json::json;

use super::*;
use crate::build::run_level::RunLevelKind;
use crate::data_version::resolve_target;

/// One struct with a painted floor.
const BUILD: &str =
    "theme t:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  floor mat_slot=floor\n";

/// [`BUILD`] and a struct with no size, which lowers to nothing.
const LOST: &str = "theme t:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  floor mat_slot=floor\n\
                    struct bad\n  floor mat_slot=floor\n";

/// Paints `@stonebrick`, which Bedrock declares last at 1.21.0, under a floor above it.
const STONEBRICK: &str = "@requires bedrock version>=1.21.40\ntheme t:\n  slot wall -> @stonebrick\n\n\
                          struct s size=2x2\n  floor mat_slot=wall\n";

fn lowered(source: &str, edition: Edition, target: &str) -> Lowered {
    let module = parse(source).expect("the source parses");
    lower_for_target(&module, edition, resolve_target(edition, target))
}

/// A [`BuildRefusal`], or the version the build writes for, in a form a table compares.
#[derive(Debug, PartialEq, Eq)]
enum Verdict {
    Writes(String),
    Findings,
    Conflicts,
    RunLevel(RunLevelKind),
    Floor(&'static str),
}

fn verdict(source: &str, edition: Edition, target: &str) -> Verdict {
    match lowered(source, edition, target).verdict() {
        Ok(target) => Verdict::Writes(target.mc_version().to_owned()),
        Err(BuildRefusal::Findings) => Verdict::Findings,
        Err(BuildRefusal::Conflicts) => Verdict::Conflicts,
        Err(BuildRefusal::RunLevel(message)) => Verdict::RunLevel(message.kind),
        Err(BuildRefusal::Floor(finding)) => Verdict::Floor(finding.code.as_str()),
    }
}

fn codes(diagnostics: &[Diagnostic]) -> Vec<&'static str> {
    diagnostics.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn the_verdict_is_the_first_refusal_that_holds() {
    let floor = |source: &str| format!("@requires version>=1.21.4\n{source}");
    let rows: [(&str, String, Edition, &str, Verdict); 7] = [
        (
            "a clean build",
            BUILD.to_owned(),
            Edition::Java,
            "1.21.4",
            Verdict::Writes("1.21.4".to_owned()),
        ),
        (
            "a lost scope, ahead of an unshipped target",
            LOST.to_owned(),
            Edition::Java,
            "9.9",
            Verdict::RunLevel(RunLevelKind::PartialBuild),
        ),
        (
            "a lost scope, ahead of the floor",
            floor(LOST),
            Edition::Java,
            "1.21",
            Verdict::RunLevel(RunLevelKind::PartialBuild),
        ),
        (
            "an unshipped target, which no floor weighs",
            floor(BUILD),
            Edition::Java,
            "9.9",
            Verdict::RunLevel(RunLevelKind::UnsupportedTarget),
        ),
        (
            "a floor above the target",
            floor(BUILD),
            Edition::Java,
            "1.21",
            Verdict::Floor("E_VERSION_CAP"),
        ),
        (
            "an error finding, ahead of a conflict",
            STONEBRICK.to_owned(),
            Edition::Bedrock,
            "1.21.40",
            Verdict::Findings,
        ),
        (
            "a conflict, ahead of the floor above the target",
            STONEBRICK.to_owned(),
            Edition::Bedrock,
            "1.21.0",
            Verdict::Conflicts,
        ),
    ];
    for (case, source, edition, target, expected) in rows {
        assert_eq!(verdict(&source, edition, target), expected, "{case}");
    }
}

#[test]
fn a_target_the_floors_refuse_is_lowered_unpinned() {
    let source =
        "theme t:\n  slot floor -> @pale_oak_planks\nstruct s size=2x2\n  floor mat_slot=floor\n";
    let pinned = lowered(source, Edition::Java, "1.21");
    assert_eq!(codes(&pinned.block_ir.diagnostics), ["E_UNKNOWN_ID"]);
    let refused = lowered(
        &format!("@requires version>=1.21.4\n{source}"),
        Edition::Java,
        "1.21",
    );
    assert_eq!(codes(&refused.block_ir.diagnostics), Vec::<&str>::new());
    assert_eq!(
        refused.floor_refusal.as_ref().map(|d| d.code.as_str()),
        Some("E_VERSION_CAP"),
    );
}

#[test]
fn a_conflict_a_theme_declares_says_who_inherits_the_floor() {
    let source = "theme t:\n  requires version>=1.21.40\n  slot wall -> @stonebrick\n\nstruct s \
                  size=2x2\n  floor mat_slot=wall\n";
    let lowered = lowered(source, Edition::Bedrock, "1.21.0");
    let lines = LineStarts::new(source);
    let rendered: Vec<_> = lowered
        .conflicts
        .iter()
        .map(|conflict| serde_json::to_value(conflict.render(source, &lines)).expect("serialises"))
        .collect();
    assert_eq!(
        rendered,
        [json!({
            "code": "E_REQUIRES_CONFLICT",
            "severity": "error",
            "line": 2,
            "col": 3,
            "primary": "`theme t` requires version>=1.21.40, but no bedrock target it admits \
                        declares `minecraft:stonebrick`, which this build paints.",
            "notes": [
                { "message": "`theme t` declares it, and every scope that binds the theme inherits it" },
                {
                    "line": 3,
                    "col": 16,
                    "message": "`minecraft:stonebrick` is last declared at bedrock 1.21.0",
                },
                {
                    "message": "fix: lower the `requires` floor to 1.21.0 or below, or replace \
                                `minecraft:stonebrick` with a block a bedrock target at or above \
                                1.21.40 declares",
                },
            ],
        })],
    );
}

#[test]
fn a_conflict_over_two_ids_names_both_in_source_order() {
    let source = "@requires bedrock version>=1.21.40\n\ntheme t:\n  slot wall -> @monster_egg\n  \
                  slot floor -> @stonebrick\n\nstruct s size=3x3\n  floor mat_slot=floor\n  walls \
                  mat_slot=wall height=2\n";
    let lowered = lowered(source, Edition::Bedrock, "1.21.0");
    let lines = LineStarts::new(source);
    let rendered: Vec<_> = lowered
        .conflicts
        .iter()
        .map(|conflict| serde_json::to_value(conflict.render(source, &lines)).expect("serialises"))
        .collect();
    assert_eq!(
        rendered,
        [json!({
            "code": "E_REQUIRES_CONFLICT",
            "severity": "error",
            "line": 1,
            "col": 1,
            "primary": "this file requires bedrock version>=1.21.40, but no bedrock target it \
                        admits declares `minecraft:monster_egg`, which this build paints.",
            "notes": [
                {
                    "line": 4,
                    "col": 16,
                    "message": "`minecraft:monster_egg` is last declared at bedrock 1.21.0",
                },
                {
                    "line": 5,
                    "col": 17,
                    "message": "`minecraft:stonebrick` is last declared at bedrock 1.21.0",
                },
                {
                    "message": "fix: lower the `@requires` floor to 1.21.0 or below, or replace \
                                `minecraft:monster_egg`, `minecraft:stonebrick` with blocks a \
                                bedrock target at or above 1.21.40 declares",
                },
            ],
        })],
    );
}
