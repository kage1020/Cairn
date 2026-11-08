//! `E_UNKNOWN_ID` on an id other versions of the edition declare names the nearest of them, so
//! the fix is a target or a floor (`spec/versioning-editions` "Which registry an unknown ID is
//! unknown to").

use cairn_lang_core::block_array::{
    BlockArrayIr, BlockIdSet, BlockState, DeclaredElsewhere, TargetRegistry, lower_to_block_array,
};
use cairn_lang_core::{DiagnosticCode, Edition, lower, parse, resolve};
use serde_json::{Value, json};

use crate::common::findings;

/// A pinned `java 2.0` whose neighbours `1.0` and `3.0` declare what it lacks.
struct Neighbours {
    ids: Vec<String>,
    /// Ids `1.0` declares and `2.0` does not.
    older: &'static [&'static str],
    /// Ids `3.0` declares and `2.0` does not.
    newer: &'static [&'static str],
    /// Alias groups across the edition; `aliases_for` keeps the spellings `2.0` declares.
    groups: &'static [&'static [&'static str]],
}

impl Neighbours {
    fn new() -> Self {
        let mut ids: Vec<String> = ["dark_oak_planks", "iron_chain", "oak_planks", "stonebrick"]
            .iter()
            .map(|id| format!("minecraft:{id}"))
            .collect();
        ids.sort();
        Self {
            ids,
            older: &["minecraft:chain", "minecraft:hollow_block"],
            // `dark_oak_plank` is one edit from a block `2.0` declares.
            newer: &[
                "minecraft:dark_oak_plank",
                "minecraft:hollow_block",
                "minecraft:stone_bricks",
            ],
            groups: &[
                &["minecraft:chain", "minecraft:iron_chain"],
                &["minecraft:stonebrick", "minecraft:stone_bricks"],
            ],
        }
    }
}

impl TargetRegistry for Neighbours {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        (token == "trim.dark").then(|| BlockState::bare("minecraft:dark_oak_plank"))
    }

    fn known_tokens(&self) -> Vec<String> {
        vec!["trim.dark".to_owned()]
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        Some(BlockIdSet::new("java 2.0", &self.ids))
    }

    fn aliases_for(&self, id: &str) -> Vec<String> {
        let Some(group) = self.groups.iter().find(|group| group.contains(&id)) else {
            return Vec::new();
        };
        group
            .iter()
            .filter(|spelling| self.ids.iter().any(|known| known == *spelling))
            .map(|spelling| (*spelling).to_owned())
            .collect()
    }

    fn declared_elsewhere(&self, id: &str) -> Option<DeclaredElsewhere> {
        let until = self.older.contains(&id).then(|| "1.0".to_owned());
        let since = self.newer.contains(&id).then(|| "3.0".to_owned());
        DeclaredElsewhere::new(Edition::Java, until, since)
    }
}

fn floored(token: &str) -> String {
    format!("theme t:\n  slot f -> @{token}\n\nstruct s size=3x3\n  floor mat_slot=f\n")
}

fn lowered_at(source: &str) -> BlockArrayIr {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    lower_to_block_array(&ir, &resolve(&ir, None), Some(&Neighbours::new()))
}

/// The payload of `out`'s one finding.
fn payload(out: &BlockArrayIr) -> Value {
    assert_eq!(out.diagnostics.len(), 1, "{:#?}", out.diagnostics);
    serde_json::to_value(&out.diagnostics[0].data).expect("the payload serialises")
}

#[test]
fn an_id_a_later_version_adds_names_it_as_a_target_and_a_floor() {
    let source = floored("dark_oak_plank");
    let out = lowered_at(&source);
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::UnknownId,
            "@dark_oak_plank",
            "`minecraft:dark_oak_plank` is not a block in `java 2.0`",
            vec![
                "the first target that declares `minecraft:dark_oak_plank` is java 3.0: build \
                 with `--target 3.0` or another target that declares it, or declare `@requires \
                 java version>=V`, with V the release that added it (3.0 at the latest), so a \
                 target without it is refused at the floor",
            ],
        )],
        "a real block is no typo, so `dark_oak_planks` is not offered",
    );
    assert_eq!(
        payload(&out),
        json!({
            "kind": "unknown_id",
            "id": "minecraft:dark_oak_plank",
            "registry": "java 2.0",
            "origin": "authored",
            "since": "3.0",
        }),
    );
}

#[test]
fn an_id_the_pack_chose_is_offered_no_floor_in_the_source() {
    let source = floored("trim.dark");
    let out = lowered_at(&source);
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::UnknownId,
            "@trim.dark",
            "`minecraft:dark_oak_plank` is not a block in `java 2.0`",
            vec![
                "the registry pack maps `@trim.dark` onto it; the token is fine and the pack's \
                 mapping is not, so this is not a fix you make here",
                "the first target that declares `minecraft:dark_oak_plank` is java 3.0: build \
                 with `--target 3.0` or another target that declares it",
            ],
        )],
    );
}

#[test]
fn an_id_an_earlier_version_had_names_the_last_one_beside_the_rename() {
    let source = floored("chain");
    let out = lowered_at(&source);
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::UnknownId,
            "@chain",
            "`minecraft:chain` is not a block in `java 2.0`",
            vec![
                "`java 2.0` spells this block `minecraft:iron_chain`, per the registry pack's \
                 alias table",
                "the last target that declares `minecraft:chain` is java 1.0: build with \
                 `--target 1.0` or another target that declares it, or pick a block `java 2.0` \
                 has",
            ],
        )],
    );
    assert_eq!(
        payload(&out),
        json!({
            "kind": "unknown_id",
            "id": "minecraft:chain",
            "registry": "java 2.0",
            "origin": "authored",
            "aliases": ["minecraft:iron_chain"],
            "until": "1.0",
        }),
    );
}

#[test]
fn an_id_a_later_version_spells_apart_names_the_spelling_then_the_version() {
    let source = floored("stone_bricks");
    let out = lowered_at(&source);
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::UnknownId,
            "@stone_bricks",
            "`minecraft:stone_bricks` is not a block in `java 2.0`",
            vec![
                "`java 2.0` spells this block `minecraft:stonebrick`, per the registry pack's \
                 alias table",
                "the first target that declares `minecraft:stone_bricks` is java 3.0: build \
                 with `--target 3.0` or another target that declares it, or declare `@requires \
                 java version>=V`, with V the release that added it (3.0 at the latest), so a \
                 target without it is refused at the floor",
            ],
        )],
    );
    assert_eq!(
        payload(&out),
        json!({
            "kind": "unknown_id",
            "id": "minecraft:stone_bricks",
            "registry": "java 2.0",
            "origin": "authored",
            "aliases": ["minecraft:stonebrick"],
            "since": "3.0",
        }),
    );
}

#[test]
fn an_id_declared_on_both_sides_names_both() {
    let source = floored("hollow_block");
    let out = lowered_at(&source);
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::UnknownId,
            "@hollow_block",
            "`minecraft:hollow_block` is not a block in `java 2.0`",
            vec![
                "java 1.0 declares `minecraft:hollow_block` before this target and 3.0 after \
                 it: build with one of them as `--target`, or pick a block `java 2.0` has",
            ],
        )],
    );
    assert_eq!(
        payload(&out),
        json!({
            "kind": "unknown_id",
            "id": "minecraft:hollow_block",
            "registry": "java 2.0",
            "origin": "authored",
            "since": "3.0",
            "until": "1.0",
        }),
    );
}

#[test]
fn an_id_no_version_declares_is_still_answered_by_distance() {
    let source = floored("oak_plank");
    let out = lowered_at(&source);
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::UnknownId,
            "@oak_plank",
            "`minecraft:oak_plank` is not a block in `java 2.0`",
            vec!["`java 2.0` spells the nearest block `minecraft:oak_planks`"],
        )],
    );
    assert_eq!(
        payload(&out),
        json!({
            "kind": "unknown_id",
            "id": "minecraft:oak_plank",
            "registry": "java 2.0",
            "origin": "authored",
            "suggestion": "minecraft:oak_planks",
        }),
    );
}
