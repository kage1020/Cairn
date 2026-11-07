use cairn_lang_core::resolve::DroppedIntent;

use super::*;
use crate::pipeline::lowered_probe;

#[test]
fn the_notes_answer_the_figure_they_sit_under() {
    let entries = vec![
        UnsupportedEntry {
            id: "minecraft:oak_sign".to_owned(),
            reason: UnsupportedReason::AbsentFromEdition {
                suggestion: Some("minecraft:oak_log".to_owned()),
                aliases: Vec::new(),
            },
        },
        UnsupportedEntry {
            id: "minecraft:frame".to_owned(),
            reason: UnsupportedReason::StatesUnmapped {
                states: "facing=north".to_owned(),
                detail: "no Java block maps onto `minecraft:frame` there".to_owned(),
            },
        },
    ];
    assert_eq!(
        unsupported_notes(Edition::Bedrock, &entries),
        [
            "note: what `unsupported: 2` counts on bedrock:".to_owned(),
            format!(
                "  note: `minecraft:oak_sign` — {}",
                unsupported_reason(&entries[0].reason)
            ),
            format!(
                "  note: `minecraft:frame` — {}",
                unsupported_reason(&entries[1].reason)
            ),
        ],
    );
    assert_eq!(unsupported_notes(Edition::Java, &[]), [] as [String; 0]);
}

#[test]
fn the_degraded_notes_answer_the_figure_they_sit_under() {
    let shape = |value: &str, mc_versions: &[&str]| DroppedIntent {
        key: "shape".to_owned(),
        value: value.to_owned(),
        mc_versions: mc_versions.iter().map(|v| (*v).to_owned()).collect(),
        reason: "stairs render straight".to_owned(),
    };
    let entries = vec![
        DegradedEntry {
            id: "minecraft:spruce_stairs".to_owned(),
            states: "facing=north,shape=outer_left".to_owned(),
            dropped: vec![shape("outer_left", &["1.21.0", "1.21.60"])],
        },
        // The same id again: one entry per state combination.
        DegradedEntry {
            id: "minecraft:spruce_stairs".to_owned(),
            states: "facing=south,shape=inner_right".to_owned(),
            dropped: vec![shape("inner_right", &["1.21.60"])],
        },
    ];
    assert_eq!(
        degraded_notes(Edition::Bedrock, &entries),
        [
            "note: what `degraded: 2` counts on bedrock:",
            "  note: `minecraft:spruce_stairs[facing=north,shape=outer_left]` — stairs render \
             straight (1.21.0, 1.21.60)",
            "  note: `minecraft:spruce_stairs[facing=south,shape=inner_right]` — stairs render \
             straight (1.21.60)",
        ],
    );
    assert_eq!(degraded_notes(Edition::Java, &[]), [] as [String; 0]);
}

#[test]
fn a_refused_palette_says_which_edition_lost_its_row_and_whose_bug_it_is() {
    use cairn_lang_core::block_array::BlockState;

    let pack = builtin_pack(Edition::Bedrock);
    let mut block_ir = lowered_probe(Edition::Bedrock);
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

    let invalid = portability_for_bedrock(&block_ir, pack.blocks(), pack.aliases(), pack.states())
        .expect_err("a value outside the Java domain is not a portability figure");

    let lines = invalid_palette_report(Edition::Bedrock, &invalid);
    assert!(
        portability_figure(
            Edition::Bedrock,
            portability_for_bedrock(&block_ir, pack.blocks(), pack.aliases(), pack.states()),
        )
        .is_none(),
        "a refused palette costs the edition its row rather than printing counts over it",
    );
    assert_eq!(lines.len(), 3, "header, one leak, closing note: {lines:?}");
    assert!(
        lines[0].starts_with("error: the bedrock ") && lines[0].contains("no portability"),
        "the header names the edition and what it is not getting, got: {}",
        lines[0],
    );
    assert_eq!(
        lines[1],
        format!("  error: {}", invalid.leaks()[0]),
        "the leak is quoted, not reworded",
    );
    // Both places such a blockstate can come from, since the leak's own `Fix:` names only one.
    assert_eq!(
        lines[2],
        "  note: a validated pack cannot produce these, so each is either a state literal in the \
         source — a property the Java block does not have, or a value outside its domain, which \
         `E_STATE_DOMAIN` refuses at a pinned target but these figures' range-wide lowering does \
         not check — or a leak in the pack or this compiler. The figure is withheld rather than \
         counting a validation gap as ordinary portability",
    );
}

#[test]
fn every_unsupported_reason_renders_the_repair_it_names() {
    let bare = unsupported_reason(&UnsupportedReason::AbsentFromEdition {
        suggestion: None,
        aliases: Vec::new(),
    });
    assert!(
        bare.contains("declares the block") && !bare.contains("did you mean"),
        "no suggestion means no dangling clause, got: {bare}",
    );
    let suggested = unsupported_reason(&UnsupportedReason::AbsentFromEdition {
        suggestion: Some("minecraft:oak_slab".to_owned()),
        aliases: Vec::new(),
    });
    assert!(
        suggested.contains("did you mean `minecraft:oak_slab`?"),
        "got: {suggested}",
    );
    // A known alias outranks a typo guess.
    let renamed = unsupported_reason(&UnsupportedReason::AbsentFromEdition {
        suggestion: Some("minecraft:oak_slab".to_owned()),
        aliases: vec!["minecraft:standing_sign".to_owned()],
    });
    assert!(
        renamed.contains("it spells the block `minecraft:standing_sign`")
            && !renamed.contains("did you mean"),
        "got: {renamed}",
    );
    let unmapped = unsupported_reason(&UnsupportedReason::StatesUnmapped {
        states: "facing=north".to_owned(),
        detail: "no Java block maps onto `minecraft:frame` there".to_owned(),
    });
    assert_eq!(
        unmapped,
        "the edition has the block, but no version's state map has a form for `facing=north`: no \
         Java block maps onto `minecraft:frame` there — bind the slot to another block, or build \
         for the other edition",
    );
}
