//! Lowering records each member's `resolved_state` beside the grid it builds
//! (`spec/blockstate` "`intent_state` and `resolved_state`"): every state the member's cells hold
//! once the body is painted, in the canonical vocabulary.

use cairn_lang_core::block_array::BlockArrayIr;
use serde_json::json;

use crate::common::lowered;

const PRELUDE: &str = "theme t:\n  \
                       slot wall -> @cobblestone\n  \
                       slot eave -> @oak_stairs\n\n";

type Cells = Vec<(u32, u32, u32)>;

/// A recorded member as `(role, id, [(state, cells)])`, the state as the palette prints it.
type Built = (String, Option<String>, Vec<(String, Cells)>);

/// Each member recorded under `key`.
fn built(ir: &BlockArrayIr, key: &str) -> Vec<Built> {
    ir.members[key]
        .iter()
        .map(|member| {
            let blocks = member
                .resolved_state
                .iter()
                .map(|block| {
                    let state = if block.state.properties.is_empty() {
                        block.state.id.clone()
                    } else {
                        format!("{}[{}]", block.state.id, block.state.properties_text())
                    };
                    (state, block.cells.clone())
                })
                .collect();
            (member.role.clone(), member.id.clone(), blocks)
        })
        .collect()
}

fn member(role: &str, id: Option<&str>, blocks: &[(&str, Cells)]) -> Built {
    (
        role.to_owned(),
        id.map(str::to_owned),
        blocks
            .iter()
            .map(|(state, cells)| ((*state).to_owned(), cells.clone()))
            .collect(),
    )
}

#[test]
fn an_eave_stair_records_the_facing_its_out_resolved_to() {
    let ir = lowered(&format!(
        "{PRELUDE}struct s size=3x1\n  \
         walls mat_slot=wall height=1\n  \
         roof kind=flat mat_slot=wall overhang=1\n  \
         stair id=eave kind=stairs mat_slot=eave side=front half=top facing=out shape=outer_left\n"
    ));

    assert_eq!(
        built(&ir, "struct::s"),
        vec![
            member(
                "walls",
                None,
                &[(
                    "minecraft:cobblestone",
                    vec![(1, 1, 1), (2, 1, 1), (3, 1, 1)],
                )],
            ),
            member(
                "roof",
                None,
                &[(
                    "minecraft:cobblestone",
                    (0..3)
                        .flat_map(|z| (0..5).map(move |x| (x, 2, z)))
                        .collect(),
                )],
            ),
            member(
                "stair",
                Some("eave"),
                &[(
                    "minecraft:oak_stairs[facing=south,half=top,shape=outer_left]",
                    vec![(1, 0, 2), (2, 0, 2), (3, 0, 2)],
                )],
            ),
        ],
    );
}

#[test]
fn a_cell_is_recorded_on_the_member_that_painted_it_last() {
    let ir = lowered(&format!(
        "{PRELUDE}struct s size=3x1\n  \
         walls id=shell mat_slot=wall height=2\n  \
         door id=entry side=front at=center\n"
    ));

    assert_eq!(
        built(&ir, "struct::s"),
        vec![
            member(
                "walls",
                Some("shell"),
                &[(
                    "minecraft:cobblestone",
                    vec![(0, 1, 0), (2, 1, 0), (0, 2, 0), (2, 2, 0)],
                )],
            ),
            member(
                "door",
                Some("entry"),
                &[("minecraft:air", vec![(1, 1, 0), (1, 2, 0)])],
            ),
        ],
    );
}

#[test]
fn a_member_that_builds_no_cell_records_nothing() {
    let ir = lowered(&format!(
        "{PRELUDE}struct s size=3x1\n  \
         floor mat_slot=wall\n  \
         door id=entry side=front at=center\n"
    ));

    let roles: Vec<&str> = ir.members["struct::s"]
        .iter()
        .map(|member| member.role.as_str())
        .collect();
    assert_eq!(roles, ["floor"]);
}

#[test]
fn each_place_records_its_own_members_under_its_structure_key() {
    let ir = lowered(&format!(
        "{PRELUDE}def hut size=1x1:\n  \
         walls mat_slot=wall height=1\n\n\
         site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=2\n"
    ));

    let keys: Vec<&str> = ir.members.keys().map(String::as_str).collect();
    assert_eq!(keys, ["site::s::a", "site::s::b"]);
    let expected = vec![member(
        "walls",
        None,
        &[("minecraft:cobblestone", vec![(0, 1, 0)])],
    )];
    assert_eq!(built(&ir, "site::s::a"), expected);
    assert_eq!(built(&ir, "site::s::b"), expected);
}

#[test]
fn the_record_serializes_under_members_keyed_like_structures() {
    let ir = lowered(&format!(
        "{PRELUDE}struct s size=1x1\n  \
         walls id=shell mat_slot=wall height=1\n"
    ));

    let json = serde_json::to_value(&ir).expect("the IR serializes");
    assert_eq!(
        json["members"],
        json!({
            "struct::s": [{
                "role": "walls",
                "id": "shell",
                "resolved_state": [{
                    "state": { "id": "minecraft:cobblestone" },
                    "cells": [[0, 1, 0]],
                }],
            }],
        }),
    );
}
