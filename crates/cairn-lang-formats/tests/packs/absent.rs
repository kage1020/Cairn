//! A block one built-in pack lists as absent is refused at the other edition's targets with the
//! pack's reason, and built by its own edition's, guarded or not (`spec/versioning-editions`
//! "A block only the other edition has").

use cairn_lang_core::{DiagnosticCode, Edition};

use crate::common::{latest, lower_source};

/// Each finding's code, the source text it points at, its primary message and its notes.
fn findings(source: &str, edition: Edition) -> Vec<(DiagnosticCode, &str, String, Vec<String>)> {
    lower_source(source, edition, Some(latest(edition)))
        .diagnostics
        .into_iter()
        .map(|d| {
            let notes = d.notes.into_iter().map(|note| note.message).collect();
            (d.code, &source[d.span], d.primary, notes)
        })
        .collect()
}

fn floored(token: &str) -> String {
    format!("theme t:\n  slot f -> @{token}\n\nstruct s size=3x3\n  floor mat_slot=f\n")
}

#[test]
fn a_slot_bound_to_a_block_only_one_edition_has_is_refused_at_the_others_target() {
    let cases = [
        (
            "border_block",
            Edition::Java,
            "`minecraft:border_block` is a block only Bedrock has, so `java 26.3` cannot build it",
            "the border block, which Bedrock uses to wall players in; Java has no such block",
            "the semantic layer is edition-neutral: bind this slot from a `theme NAME_bedrock` and \
             give `theme NAME_java` a block Java has, or place the block with a `raw_block` under \
             `@edition bedrock`, which only a Bedrock build keeps",
        ),
        (
            "test_block",
            Edition::Bedrock,
            "`minecraft:test_block` is a block only Java has, so `bedrock 1.26.60` cannot build it",
            "a block of Java's test framework, which runs and reports the tests a structure \
             holds; Bedrock has no such block",
            "the semantic layer is edition-neutral: bind this slot from a `theme NAME_java` and \
             give `theme NAME_bedrock` a block Bedrock has, or place the block with a `raw_block` \
             under `@edition java`, which only a Java build keeps",
        ),
    ];
    for (block, lacks, primary, reason, fix) in cases {
        let source = floored(block);
        let token = format!("@{block}");
        assert_eq!(
            findings(&source, lacks),
            [(
                DiagnosticCode::ParityUnsupported,
                token.as_str(),
                primary.to_owned(),
                vec![reason.to_owned(), fix.to_owned()],
            )],
            "{block} on {lacks}",
        );
        assert_eq!(
            findings(&source, lacks.other()),
            [],
            "{block} on its own edition"
        );
    }
}

#[test]
fn a_guarded_raw_block_builds_only_on_its_edition_and_an_unguarded_one_is_offered_the_guard() {
    let guarded = "struct s size=3x3\n  \
                   @edition bedrock\n    \
                   raw_block mat=minecraft:border_block at=1,0,1\n";
    assert_eq!(findings(guarded, Edition::Java), []);
    let bedrock = lower_source(guarded, Edition::Bedrock, Some(latest(Edition::Bedrock)));
    assert_eq!(bedrock.diagnostics, []);
    let palette: Vec<&str> = bedrock.structures["struct::s"]
        .palette
        .entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect();
    assert_eq!(
        palette,
        ["minecraft:air", "minecraft:border_block"],
        "the Bedrock build keeps the guarded block",
    );

    let unguarded = "struct s size=3x3\n  raw_block mat=minecraft:border_block at=1,0,1\n";
    assert_eq!(
        findings(unguarded, Edition::Java),
        [(
            DiagnosticCode::UnknownId,
            "minecraft:border_block",
            "`minecraft:border_block` is not a block in `java 26.3`".to_owned(),
            vec![
                "`minecraft:border_block` is a block only Bedrock has: the border block, which \
                 Bedrock uses to wall players in; Java has no such block; put this line under \
                 `@edition bedrock`, so that only a Bedrock build keeps it"
                    .to_owned(),
            ],
        )],
    );
}
