//! `E_STATE_UNMAPPED`: a block state the pinned target's state map has no form for
//! (`spec/lint` "Diagnostic codes").

use cairn_lang_core::block_array::{
    BlockArrayIr, BlockIdSet, BlockState, StateDomain, TargetRegistry, UnmappedState,
    lower_to_block_array,
};
use cairn_lang_core::{DiagnosticCode, lower, parse, resolve};
use serde_json::json;

use crate::common::{PRELUDE, findings};

/// A pinned target whose state map has no form for a log lying along `x`, or for a jigsaw at all,
/// which the maps of `mapped_at` have, and whose tables take every state.
struct NoSidewaysLog {
    ids: Vec<String>,
    with_state_map: bool,
    mapped_at: Vec<String>,
}

impl NoSidewaysLog {
    fn pinned() -> Self {
        let mut ids: Vec<String> = [
            "cobblestone",
            "gravel",
            "jigsaw",
            "oak_door",
            "oak_log",
            "oak_planks",
        ]
        .iter()
        .map(|id| format!("minecraft:{id}"))
        .collect();
        ids.sort();
        Self {
            ids,
            with_state_map: true,
            mapped_at: vec!["0.9".to_owned(), "1.1".to_owned()],
        }
    }
}

impl TargetRegistry for NoSidewaysLog {
    fn lookup(&self, _token: &str) -> Option<BlockState> {
        None
    }

    fn known_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        Some(BlockIdSet::new("test 1.0", &self.ids))
    }

    fn unmapped_state(&self, state: &BlockState) -> Option<UnmappedState> {
        let sideways = state.properties.get("axis").is_some_and(|axis| axis == "x");
        let reason = match state.id.as_str() {
            "minecraft:jigsaw" => "its palette refuses a jigsaw",
            _ if sideways => "its map has no sideways log",
            _ => return None,
        };
        self.with_state_map.then(|| UnmappedState {
            registry: "test 1.0".to_owned(),
            reason: reason.to_owned(),
            mapped_at: self.mapped_at.clone(),
        })
    }

    fn state_domain(&self, _state: &BlockState) -> StateDomain {
        StateDomain::Checked(Vec::new())
    }
}

fn lowered_at(source: &str, registry: &NoSidewaysLog) -> BlockArrayIr {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    lower_to_block_array(&ir, &resolve(&ir, None), Some(registry))
}

fn log_floor(axis: &str) -> String {
    format!(
        "theme t:\n  slot f -> @oak_log[axis={axis}]\n\nstruct s size=3x3\n  floor mat_slot=f\n"
    )
}

const NOTE: &str = "pass `--target` 0.9 or 1.1, whose state maps carry these states, bind the \
                    material to a block test 1.0 has a form for, or build for the other edition";

#[test]
fn a_member_building_a_state_the_map_lacks_is_refused_at_the_member() {
    let source = log_floor("x");
    let out = lowered_at(&source, &NoSidewaysLog::pinned());
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::StateUnmapped,
            "floor mat_slot=f",
            "`minecraft:oak_log[axis=x]` has no form at test 1.0: its map has no sideways log",
            vec![NOTE],
        )],
    );
    let unmapped = out
        .diagnostics
        .iter()
        .find(|d| d.code == DiagnosticCode::StateUnmapped)
        .expect("the refusal");
    // `spec/lint` "Machine-readable payload".
    assert_eq!(
        serde_json::to_value(&unmapped.data).expect("the payload serialises"),
        json!({
            "kind": "state_unmapped",
            "id": "minecraft:oak_log",
            "states": "axis=x",
            "registry": "test 1.0",
        }),
    );
}

#[test]
fn a_bare_block_the_map_refuses_is_named_without_states() {
    let source = "theme t:\n  slot f -> @jigsaw\n\nstruct s size=3x3\n  floor mat_slot=f\n";
    let out = lowered_at(source, &NoSidewaysLog::pinned());
    assert_eq!(
        findings(source, &out),
        [(
            DiagnosticCode::StateUnmapped,
            "floor mat_slot=f",
            "`minecraft:jigsaw` has no form at test 1.0: its palette refuses a jigsaw",
            vec![
                "pass `--target` 0.9 or 1.1, whose state maps carry the block, bind the \
                 material to a block test 1.0 has a form for, or build for the other edition",
            ],
        )],
    );
}

#[test]
fn the_note_offers_each_version_whose_map_carries_the_state() {
    let note = |mapped_at: &[&str]| {
        let registry = NoSidewaysLog {
            mapped_at: mapped_at.iter().map(|v| (*v).to_owned()).collect(),
            ..NoSidewaysLog::pinned()
        };
        let source = log_floor("x");
        let out = lowered_at(&source, &registry);
        let unmapped = out
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::StateUnmapped)
            .expect("the refusal");
        let notes: Vec<&str> = unmapped.notes.iter().map(|n| n.message.as_str()).collect();
        notes.join("\n")
    };
    assert_eq!(
        note(&[]),
        "bind the material to a block test 1.0 has a form for, or build for the other edition",
    );
    assert_eq!(
        note(&["1.1"]),
        "pass `--target 1.1`, whose state map carries these states, bind the material to a block \
         test 1.0 has a form for, or build for the other edition",
    );
    assert_eq!(
        note(&["0.8", "0.9", "1.1"]),
        "pass `--target` 0.8, 0.9 or 1.1, whose state maps carry these states, bind the material \
         to a block test 1.0 has a form for, or build for the other edition",
    );
}

#[test]
fn a_state_the_map_carries_is_not_refused() {
    let source = log_floor("y");
    let out = lowered_at(&source, &NoSidewaysLog::pinned());
    assert_eq!(out.diagnostics, []);
}

#[test]
fn a_target_without_a_state_map_refuses_no_state() {
    let source = log_floor("x");
    let registry = NoSidewaysLog {
        with_state_map: false,
        ..NoSidewaysLog::pinned()
    };
    let out = lowered_at(&source, &registry);
    assert_eq!(out.diagnostics, []);
}

#[test]
fn a_walkway_whose_path_the_map_lacks_is_not_laid() {
    let duo = |axis: &str| {
        format!(
            "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
             place id=peer use=hut theme=plain east_of=anchor gap=4\n  \
             connect anchor.entry to peer.entry path=@oak_log[axis={axis}]\n"
        )
    };
    let source = duo("x");
    let out = lowered_at(&source, &NoSidewaysLog::pinned());
    let found: Vec<(DiagnosticCode, &str, &str)> = findings(&source, &out)
        .into_iter()
        .map(|(code, at, primary, _)| (code, at, primary))
        .collect();
    assert_eq!(
        found,
        [(
            DiagnosticCode::StateUnmapped,
            "@oak_log[axis=x]",
            "`minecraft:oak_log[axis=x]` has no form at test 1.0: its map has no sideways log",
        )],
    );
    assert!(out.walkways.is_empty(), "{:?}", out.walkways.keys());

    let laid = lowered_at(&duo("z"), &NoSidewaysLog::pinned());
    assert_eq!(laid.walkways.len(), 1, "{:?}", laid.diagnostics);
}

#[test]
fn a_raw_block_whose_state_the_map_lacks_is_refused_at_its_line() {
    let source = "struct s size=3x3\n  raw_block mat=minecraft:oak_log[axis=x] at=1,0,1\n";
    let out = lowered_at(source, &NoSidewaysLog::pinned());
    assert_eq!(
        findings(source, &out),
        [(
            DiagnosticCode::StateUnmapped,
            "raw_block mat=minecraft:oak_log[axis=x] at=1,0,1",
            "`minecraft:oak_log[axis=x]` has no form at test 1.0: its map has no sideways log",
            vec![
                "pass `--target` 0.9 or 1.1, whose state maps carry these states, set `mat=` to \
                 a block test 1.0 has a form for, or build for the other edition",
            ],
        )],
    );
}
