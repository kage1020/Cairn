use cairn_lang_core::resolve::DroppedIntent;

use super::*;

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
    let foreign = unsupported_reason(&UnsupportedReason::NoCounterpart {
        detail: "the border block, which Bedrock uses to wall players in; Java has no such block"
            .to_owned(),
    });
    assert_eq!(
        foreign,
        "only the other edition has the block: the border block, which Bedrock uses to wall \
         players in; Java has no such block — bind the slot to a block this edition has, or build \
         for the other edition",
    );
}
