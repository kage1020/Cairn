//! `E_UNKNOWN_ID` on an id other versions of the edition declare names the nearest of them, so
//! the fix is a target or a floor (`spec/versioning-editions` "Which registry an unknown ID is
//! unknown to").

use cairn_lang_core::block_array::{
    BlockArrayIr, BlockIdSet, BlockState, DeclaredElsewhere, TargetRegistry, lower_to_block_array,
};
use cairn_lang_core::{DiagnosticCode, Edition, lower, parse, resolve};
use serde_json::json;

use crate::common::findings;

/// A pinned `java 2.0` whose neighbours `1.0` and `3.0` declare what it lacks.
struct Neighbours {
    ids: Vec<String>,
    /// Ids `1.0` declares and `2.0` does not.
    older: &'static [&'static str],
    /// Ids `3.0` declares and `2.0` does not.
    newer: &'static [&'static str],
    /// One alias group, filtered to what `2.0` declares.
    group: &'static [&'static str],
}

impl Neighbours {
    fn new() -> Self {
        let mut ids: Vec<String> = ["dark_oak_planks", "iron_chain", "oak_planks"]
            .iter()
            .map(|id| format!("minecraft:{id}"))
            .collect();
        ids.sort();
        Self {
            ids,
            older: &["minecraft:chain", "minecraft:hollow_block"],
            newer: &["minecraft:pale_oak_planks", "minecraft:hollow_block"],
            group: &["minecraft:chain", "minecraft:iron_chain"],
        }
    }
}

impl TargetRegistry for Neighbours {
    fn lookup(&self, _token: &str) -> Option<BlockState> {
        None
    }

    fn known_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        Some(BlockIdSet::new("java 2.0", &self.ids))
    }

    fn aliases_for(&self, id: &str) -> Vec<String> {
        if !self.group.contains(&id) {
            return Vec::new();
        }
        self.group
            .iter()
            .filter(|spelling| self.ids.iter().any(|known| known == *spelling))
            .map(|spelling| (*spelling).to_owned())
            .collect()
    }

    fn declared_elsewhere(&self, id: &str) -> Option<DeclaredElsewhere> {
        let until = self.older.contains(&id).then(|| "1.0".to_owned());
        let since = self.newer.contains(&id).then(|| "3.0".to_owned());
        (until.is_some() || since.is_some()).then_some(DeclaredElsewhere {
            edition: Edition::Java,
            until,
            since,
        })
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

#[test]
fn an_id_a_later_version_adds_names_it_as_a_target_and_a_floor() {
    let source = floored("pale_oak_planks");
    let out = lowered_at(&source);
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::UnknownId,
            "@pale_oak_planks",
            "`minecraft:pale_oak_planks` is not a block in `java 2.0`",
            vec![
                "`minecraft:pale_oak_planks` first appears in java 3.0: build with `--target \
                 3.0` or a later version, or declare `@requires java version>=3.0` so a target \
                 without it is refused at the floor",
            ],
        )],
        "a real block is no typo, so `dark_oak_planks` is not offered",
    );
    assert_eq!(
        serde_json::to_value(&out.diagnostics[0].data).expect("the payload serialises"),
        json!({
            "kind": "unknown_id",
            "id": "minecraft:pale_oak_planks",
            "registry": "java 2.0",
            "origin": "authored",
            "since": "3.0",
        }),
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
                "`minecraft:chain` was last declared in java 1.0: build with `--target 1.0` or \
                 an earlier version, or pick a block `java 2.0` has",
            ],
        )],
    );
    assert_eq!(
        serde_json::to_value(&out.diagnostics[0].data).expect("the payload serialises"),
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
fn an_id_declared_on_both_sides_names_both() {
    let source = floored("hollow_block");
    let out = lowered_at(&source);
    let notes: Vec<Vec<&str>> = findings(&source, &out)
        .into_iter()
        .map(|(.., notes)| notes)
        .collect();
    assert_eq!(
        notes,
        [vec![
            "`minecraft:hollow_block` is declared in java 1.0 and again from 3.0: build with \
             one of them as `--target`, or pick a block `java 2.0` has",
        ]],
    );
}

#[test]
fn an_id_no_version_declares_is_still_answered_by_distance() {
    let source = floored("oak_plank");
    let out = lowered_at(&source);
    let notes: Vec<Vec<&str>> = findings(&source, &out)
        .into_iter()
        .map(|(.., notes)| notes)
        .collect();
    assert_eq!(
        notes,
        [vec![
            "`java 2.0` spells the nearest block `minecraft:oak_planks`"
        ]],
    );
}
