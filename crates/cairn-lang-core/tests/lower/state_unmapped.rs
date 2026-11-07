//! `E_STATE_UNMAPPED`: a block state the pinned target's state map has no form for
//! (`spec/lint` "Diagnostic codes").

use cairn_lang_core::block_array::{
    BlockArrayIr, BlockIdSet, BlockState, StateDomain, TargetRegistry, lower_to_block_array,
};
use cairn_lang_core::{DiagnosticCode, lower, parse, resolve};
use serde_json::json;

use crate::common::{PRELUDE, findings};

/// A pinned target whose state map has no form for a log lying along `x`, and whose tables take
/// every state.
struct NoSidewaysLog {
    ids: Vec<String>,
    with_state_map: bool,
}

impl NoSidewaysLog {
    fn pinned() -> Self {
        let mut ids: Vec<String> = ["cobblestone", "gravel", "oak_door", "oak_log", "oak_planks"]
            .iter()
            .map(|id| format!("minecraft:{id}"))
            .collect();
        ids.sort();
        Self {
            ids,
            with_state_map: true,
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

    fn unmapped_state(&self, state: &BlockState) -> Option<String> {
        let sideways = state.properties.get("axis").is_some_and(|axis| axis == "x");
        (self.with_state_map && sideways).then(|| "its map has no sideways log".to_owned())
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

const NOTE: &str = "pass a `--target` whose state map carries these states, bind the material \
                    to a block test 1.0 has a form for, or build for the other edition";

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
    let unmapped: Vec<(&str, &str)> = findings(&source, &out)
        .into_iter()
        .filter(|(code, ..)| *code == DiagnosticCode::StateUnmapped)
        .map(|(_, at, primary, _)| (at, primary))
        .collect();
    assert_eq!(
        unmapped,
        [(
            "@oak_log[axis=x]",
            "`minecraft:oak_log[axis=x]` has no form at test 1.0: its map has no sideways log",
        )],
    );
    assert!(out.walkways.is_empty(), "{:?}", out.walkways.keys());

    let laid = lowered_at(&duo("z"), &NoSidewaysLog::pinned());
    assert_eq!(laid.walkways.len(), 1, "{:?}", laid.diagnostics);
}
