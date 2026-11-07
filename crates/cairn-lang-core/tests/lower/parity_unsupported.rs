//! A semantic member bound to a block only the other edition has is `E_PARITY_UNSUPPORTED`, and a
//! raw member naming one is `E_UNKNOWN_ID` with the guard that would keep it
//! (`spec/versioning-editions` "A block only the other edition has").

use cairn_lang_core::block_array::{
    AbsentConcept, BlockArrayIr, BlockIdSet, BlockState, TargetRegistry,
};
use cairn_lang_core::{DiagnosticCode, Edition};
use serde_json::json;

use crate::common::{findings, lowered_with};

const BORDER_REASON: &str =
    "the border block, which Bedrock uses to wall players in; Java has no such block";

/// A pinned `java 2.0` that lacks Bedrock's border block, which its pack maps `@wall.border` onto.
struct JavaWithoutBorder {
    ids: Vec<String>,
}

impl JavaWithoutBorder {
    fn new() -> Self {
        let mut ids: Vec<String> = ["border", "oak_door", "stone"]
            .iter()
            .map(|id| format!("minecraft:{id}"))
            .collect();
        ids.sort();
        Self { ids }
    }
}

impl TargetRegistry for JavaWithoutBorder {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        (token == "wall.border").then(|| BlockState::bare("minecraft:border_block"))
    }

    fn known_tokens(&self) -> Vec<String> {
        vec!["wall.border".to_owned()]
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        Some(BlockIdSet::new("java 2.0", &self.ids))
    }

    fn absent_concept(&self, id: &str) -> Option<AbsentConcept> {
        (id == "minecraft:border_block").then(|| AbsentConcept {
            declared_by: Edition::Bedrock,
            reason: BORDER_REASON.to_owned(),
        })
    }
}

fn lowered(source: &str) -> BlockArrayIr {
    lowered_with(source, &JavaWithoutBorder::new())
}

fn floored(token: &str) -> String {
    format!("theme t:\n  slot f -> @{token}\n\nstruct s size=3x3\n  floor mat_slot=f\n")
}

/// Both notes of `E_PARITY_UNSUPPORTED` on the border block at `java 2.0`.
fn parity_notes() -> Vec<&'static str> {
    vec![
        BORDER_REASON,
        "the semantic layer is edition-neutral: bind this slot from a `theme NAME_bedrock` and \
         give `theme NAME_java` a block Java has, or place the block with a `raw_block` under \
         `@edition bedrock`, which only a Bedrock build keeps",
    ]
}

#[test]
fn a_slot_bound_to_the_other_editions_block_names_the_theme_and_the_guard_that_build_it() {
    let source = floored("border_block");
    let out = lowered(&source);
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::ParityUnsupported,
            "@border_block",
            "`minecraft:border_block` is a block only Bedrock has, so `java 2.0` cannot build it",
            parity_notes(),
        )],
        "the block is no typo, so the nearby `minecraft:border` is not offered",
    );
    assert_eq!(
        serde_json::to_value(&out.diagnostics[0].data).expect("the payload serialises"),
        json!({
            "kind": "parity_unsupported",
            "id": "minecraft:border_block",
            "registry": "java 2.0",
            "declared_by": "bedrock",
            "reason": BORDER_REASON,
        }),
    );
}

#[test]
fn a_walkway_path_of_the_other_editions_block_is_refused_the_same_way() {
    let source = "theme t:\n  slot wall -> @stone\n\n\
                  def hut size=5x5:\n  \
                  walls id=w mat_slot=wall height=3\n  \
                  door id=e side=front at=center\n\n\
                  site s:\n  \
                  place id=a use=hut theme=t at=origin\n  \
                  place id=b use=hut theme=t east_of=a gap=4\n  \
                  connect a.e to b.e path=@border_block\n";
    let out = lowered(source);
    assert_eq!(
        findings(source, &out),
        [(
            DiagnosticCode::ParityUnsupported,
            "@border_block",
            "`minecraft:border_block` is a block only Bedrock has, so `java 2.0` cannot build it",
            parity_notes(),
        )],
    );
}

#[test]
fn a_raw_block_of_the_other_editions_block_is_offered_the_guard_that_keeps_it() {
    let source = "struct s size=3x3\n  raw_block mat=minecraft:border_block at=1,0,1\n";
    let out = lowered(source);
    assert_eq!(
        findings(source, &out),
        [(
            DiagnosticCode::UnknownId,
            "minecraft:border_block",
            "`minecraft:border_block` is not a block in `java 2.0`",
            vec![
                "`minecraft:border_block` is a block only Bedrock has: the border block, which \
                 Bedrock uses to wall players in; Java has no such block; put this line under \
                 `@edition bedrock`, so that only a Bedrock build keeps it",
            ],
        )],
    );
    assert_eq!(
        serde_json::to_value(&out.diagnostics[0].data).expect("the payload serialises"),
        json!({
            "kind": "unknown_id",
            "id": "minecraft:border_block",
            "registry": "java 2.0",
            "origin": "authored",
            "declared_by": "bedrock",
        }),
    );
}

#[test]
fn a_pack_mapping_onto_the_other_editions_block_stays_the_packs_fault() {
    let source = floored("wall.border");
    let out = lowered(&source);
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::UnknownId,
            "@wall.border",
            "`minecraft:border_block` is not a block in `java 2.0`",
            vec![
                "the registry pack maps `@wall.border` onto it; the token is fine and the pack's \
                 mapping is not, so this is not a fix you make here",
            ],
        )],
    );
}
