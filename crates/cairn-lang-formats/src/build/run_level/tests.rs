use cairn_lang_core::block_array::{BlockState, lower_to_block_array};
use cairn_lang_core::resolve::resolve;
use cairn_lang_core::{lower, parse};

use super::*;
use crate::portability::portability_for_bedrock;

#[test]
fn a_message_prints_gcc_style_at_the_place_it_names() {
    let source = "struct a size=3x3\n@requires version>=1.21\n";
    let lines = LineStarts::new(source);
    let message = |at| RunLevelMessage {
        severity: Severity::Error,
        code: Some("E_VERSION_CAP"),
        at,
        primary: "this file requires version>=1.21 (target 1.20.4).".to_owned(),
        notes: vec!["fix: --target 1.21, or lower the `@requires` floor".to_owned()],
    };
    assert_eq!(
        message(RunLevelAt::Offset(18)).lines("hut.crn", source, &lines),
        [
            "error[E_VERSION_CAP]: hut.crn:2:1: this file requires version>=1.21 (target 1.20.4).",
            "  fix: --target 1.21, or lower the `@requires` floor",
        ],
    );
    assert_eq!(
        message(RunLevelAt::Source).lines("hut.crn", source, &lines)[0],
        "error[E_VERSION_CAP]: hut.crn: this file requires version>=1.21 (target 1.20.4).",
    );
    assert_eq!(
        unsupported_target(&UnsupportedTarget {
            edition: Edition::Java,
            requested: "9.9".to_owned(),
            suggestion: None,
            supported: "1.21".to_owned(),
        })
        .lines("hut.crn", source, &lines),
        ["error: unsupported java target `9.9`. supported targets: 1.21"],
    );
}

#[test]
fn a_message_renders_as_a_finding_does_with_a_position_only_at_an_offset() {
    let source = "struct a size=3x3\n@requires version>=1.21\n";
    let lines = LineStarts::new(source);
    let at_line = RunLevelMessage {
        severity: Severity::Error,
        code: Some("E_VERSION_CAP"),
        at: RunLevelAt::Offset(18),
        primary: "floor".to_owned(),
        notes: vec!["fix: raise it".to_owned()],
    };
    assert_eq!(
        serde_json::to_value(at_line.render(source, &lines)).expect("serialises"),
        serde_json::json!({
            "code": "E_VERSION_CAP",
            "severity": "error",
            "line": 2,
            "col": 1,
            "primary": "floor",
            "notes": [{ "message": "fix: raise it" }],
        }),
    );
    let nowhere = backend_refusal("struct::a", "too tall");
    assert_eq!(
        serde_json::to_value(nowhere.render(source, &lines)).expect("serialises"),
        serde_json::json!({
            "severity": "error",
            "primary": "checking `struct::a`: too tall",
        }),
    );
}

#[test]
fn a_partial_build_counts_what_was_asked_for_and_names_each_lost_scope() {
    assert_eq!(
        partial_build(
            &["struct::shed".to_owned()],
            1,
            "refusing to certify a partial build"
        ),
        RunLevelMessage {
            severity: Severity::Error,
            code: Some("E_PARTIAL_BUILD"),
            at: RunLevelAt::Source,
            primary: "1 of 2 requested scopes did not lower; refusing to certify a partial build"
                .to_owned(),
            notes: vec!["note: `struct::shed` produced no voxels".to_owned()],
        },
    );
}

#[test]
fn a_refused_palette_says_which_edition_lost_its_row_and_whose_bug_it_is() {
    let module = parse("struct probe size=3x3\n  walls height=2\n").expect("the probe parses");
    let ir = lower(&module);
    let pack = builtin_pack(Edition::Bedrock);
    let mut block_ir = lower_to_block_array(
        &ir,
        &resolve(&ir, Some(Edition::Bedrock)),
        Some(&pack.view(None)),
    );
    let mut leaked = BlockState::bare("minecraft:oak_stairs");
    leaked
        .properties
        .insert("facing".to_owned(), "up".to_owned());
    block_ir
        .structures
        .values_mut()
        .next()
        .expect("the probe lowers to one structure")
        .palette
        .intern(leaked);
    let invalid = portability_for_bedrock(
        &block_ir,
        pack.blocks(),
        pack.aliases(),
        pack.absent(),
        pack.states(),
    )
    .expect_err("a value outside the Java domain is not a portability figure");

    assert_eq!(
        invalid_palette(Edition::Bedrock, &invalid),
        RunLevelMessage {
            severity: Severity::Error,
            code: None,
            at: RunLevelAt::Nothing,
            primary: "the bedrock palette carries blockstates a registry pack is expected to \
                      refuse, so this edition gets no portability figure:"
                .to_owned(),
            notes: vec![
                // The leak is quoted, not reworded.
                format!("error: {}", invalid.leaks()[0]),
                // Both places such a blockstate can come from, since the leak's own `Fix:` names
                // only one.
                "note: a validated pack cannot produce these, so each is either a state literal \
                 in the source — a property the Java block does not have, or a value outside its \
                 domain, which `E_STATE_DOMAIN` refuses at a pinned target but these figures' \
                 range-wide lowering does not check — or a leak in the pack or this compiler. The \
                 figure is withheld rather than counting a validation gap as ordinary portability"
                    .to_owned(),
            ],
        },
    );
}
