//! A `sign` hangs on the wall it stands by or stands on the block under it, and its block entity
//! holds the text its line writes (`spec/entities` "Block entities").

use cairn_lang_core::block_array::{
    BlockEntity, BlockEntityData, BlockIdSet, BlockState, SignText, TargetRegistry,
};
use cairn_lang_core::{BlockArrayIr, DiagnosticCode};
use serde_json::json;

use crate::common::{Finding, blocks_where, findings, lowered, lowered_with, only_structure};

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n  slot board -> @spruce_sign\n\n";

/// A 7x5 struct with a floor and 3-row walls, `members` after them; with no overhang its front
/// wall is `z=4` and its left `x=0`.
fn room(members: &str) -> String {
    format!(
        "{THEME}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         {members}\n"
    )
}

/// The notes of a `W_PHASE_CONFLICT` after the one naming the overwritten member.
const PHASE_NOTES: [&str; 2] = [
    "If the override is deliberate this is the local-override rule of spec/compilation \
     \"Phase evaluation\" and the later line wins as written. If it is not, move one of the two \
     so their footprints do not meet.",
    "Members in different phases may overlap freely — a `door` cut through a `walls` is the \
     evaluation model working — so this is only reported for two members the same phase \
     evaluates.",
];

/// A block as `id[properties]`, with its cell.
type Placed = ((u32, u32, u32), String);

/// The signs `out` holds.
fn signs(out: &BlockArrayIr) -> Vec<Placed> {
    blocks_where(only_structure(out), |id| id.ends_with("sign"))
}

/// The block entities `out` holds.
fn entities(out: &BlockArrayIr) -> Vec<BlockEntity> {
    only_structure(out).block_entities.clone()
}

/// The signs `members` sets in [`room`], which it must set without a finding, with the block
/// entities they keep.
fn set(members: &str) -> (Vec<Placed>, Vec<BlockEntity>) {
    let src = room(members);
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), Vec::<Finding<'_>>::new(), "{members}");
    (signs(&out), entities(&out))
}

/// A sign's block entity at `pos`, its faces written as `front` and `back`.
fn sign_entity(pos: (u32, u32, u32), front: &[&str], back: &[&str], glowing: bool) -> BlockEntity {
    let face = |lines: &[&str]| {
        SignText::new(
            lines.iter().map(|line| (*line).to_owned()).collect(),
            glowing,
        )
        .expect("a face of at most four lines")
    };
    BlockEntity {
        pos,
        data: BlockEntityData::Sign {
            front: face(front),
            back: face(back),
        },
    }
}

#[test]
fn a_sign_by_a_wall_hangs_on_it_facing_away_and_blank() {
    assert_eq!(
        set("sign at=inside.front offset=2 y=1"),
        (
            vec![(
                (2, 1, 3),
                "minecraft:oak_wall_sign[facing=north]".to_owned()
            )],
            vec![sign_entity((2, 1, 3), &[], &[], false)],
        ),
    );
}

#[test]
fn the_text_a_sign_line_writes_is_kept_in_its_block_entity() {
    assert_eq!(
        set(r#"sign at=inside.back offset=3 y=1 text=["The", "Inn"] back="Closed" glowing=true"#),
        (
            vec![(
                (3, 1, 1),
                "minecraft:oak_wall_sign[facing=south]".to_owned()
            )],
            vec![sign_entity((3, 1, 1), &["The", "Inn"], &["Closed"], true)],
        ),
    );
}

#[test]
fn a_sign_at_a_triple_stands_toward_the_front_or_hangs_the_way_it_faces() {
    assert_eq!(
        set(r#"sign at=3,1,2 text="Well""#),
        (
            vec![((3, 1, 2), "minecraft:oak_sign[rotation=0]".to_owned())],
            vec![sign_entity((3, 1, 2), &["Well"], &[], false)],
        ),
    );
    assert_eq!(
        set("sign at=3,1,2 facing=east"),
        (
            vec![((3, 1, 2), "minecraft:oak_wall_sign[facing=east]".to_owned())],
            vec![sign_entity((3, 1, 2), &[], &[], false)],
        ),
    );
}

#[test]
fn a_sign_is_built_from_the_standing_sign_its_slot_binds() {
    assert_eq!(
        set("sign at=inside.left offset=1 y=2 mat_slot=board\n  sign at=2,1,2 mat_slot=board"),
        (
            vec![
                (
                    (1, 2, 1),
                    "minecraft:spruce_wall_sign[facing=east]".to_owned()
                ),
                ((2, 1, 2), "minecraft:spruce_sign[rotation=0]".to_owned()),
            ],
            vec![
                sign_entity((1, 2, 1), &[], &[], false),
                sign_entity((2, 1, 2), &[], &[], false),
            ],
        ),
    );
}

#[test]
fn a_sign_bound_to_no_sign_is_built_from_the_default_one() {
    let src = room("sign at=inside.front offset=2 y=1 mat_slot=wall");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::IncompatibleMaterial,
            "@cobblestone",
            "sign derives `facing` / `rotation` from the wall it hangs on or the block it stands \
             on and `minecraft:cobblestone` is not a sign, so those states have nowhere to go",
            vec![
                "bind the slot to a standing sign whose wall sign the target has — the registry \
                 pack's `sign.oak` resolves to one",
                "reached through `mat_slot=wall`, so every member reading that slot has it too",
            ],
        )],
    );
    assert_eq!(
        serde_json::to_value(&out.diagnostics[0].data).expect("serialises"),
        json!({
            "kind": "incompatible_material",
            "id": "minecraft:cobblestone",
            "required": "sign",
            "slot": "wall",
            "token": "cobblestone",
        }),
    );
    assert_eq!(
        signs(&out),
        [(
            (2, 1, 3),
            "minecraft:oak_wall_sign[facing=north]".to_owned()
        )],
    );
}

#[test]
fn a_wall_sign_or_a_hanging_sign_is_no_standing_sign_and_the_default_is_built() {
    let src = "theme t:\n  slot wall -> @cobblestone\n  slot hung -> @oak_wall_sign\n  \
               slot chained -> @oak_hanging_sign\n\nstruct s size=7x5\n  floor mat_slot=wall\n  \
               walls mat_slot=wall height=3\n  sign at=inside.front offset=2 y=1 mat_slot=chained\n  \
               sign at=3,1,2 mat_slot=hung\n";
    let declared = pinned(
        &[
            "minecraft:cobblestone",
            "minecraft:oak_hanging_sign",
            "minecraft:oak_sign",
            "minecraft:oak_wall_hanging_sign",
            "minecraft:oak_wall_sign",
        ],
        &[],
    );
    let no_sign = |id: &str| {
        format!(
            "sign derives `facing` / `rotation` from the wall it hangs on or the block it stands \
             on and `{id}` is not a sign, so those states have nowhere to go"
        )
    };
    let remedy = "bind the slot to a standing sign whose wall sign the target has — the registry \
                  pack's `sign.oak` resolves to one";
    let reached = |slot: &str| {
        format!("reached through `mat_slot={slot}`, so every member reading that slot has it too")
    };
    for out in [lowered(src), lowered_with(src, &declared)] {
        assert_eq!(
            findings(src, &out),
            [
                (
                    DiagnosticCode::IncompatibleMaterial,
                    "@oak_wall_sign",
                    no_sign("minecraft:oak_wall_sign").as_str(),
                    vec![remedy, reached("hung").as_str()],
                ),
                (
                    DiagnosticCode::IncompatibleMaterial,
                    "@oak_hanging_sign",
                    no_sign("minecraft:oak_hanging_sign").as_str(),
                    vec![remedy, reached("chained").as_str()],
                ),
            ],
        );
        assert_eq!(
            signs(&out),
            [
                (
                    (2, 1, 3),
                    "minecraft:oak_wall_sign[facing=north]".to_owned()
                ),
                ((3, 1, 2), "minecraft:oak_sign[rotation=0]".to_owned()),
            ],
        );
    }
}

#[test]
fn a_sign_line_the_sign_cannot_take_is_refused_and_it_is_not_built() {
    const COMPASS: &str = "`facing=` takes one of north, south, east, west: north toward the \
                           `back` wall and east toward the `right` one";
    for (line, primary, notes) in [
        (
            "sign at=inside.front offset=2 y=1 facing=north",
            "sign `facing=north` by a wall: the sign hangs on the wall's face, so `facing=` has \
             nothing to choose",
            vec!["drop `facing=`; the sign faces away from the wall it hangs on"],
        ),
        (
            "sign at=3,1,2 facing=in",
            "unknown sign `facing=in` at a coordinate triple",
            vec![
                COMPASS,
                "`in` names a way from a wall, which a sign does not take: by a wall it hangs on \
                 the wall and faces away from it, with no `facing=`",
            ],
        ),
        (
            "sign at=3,1,2 facing=eats",
            "unknown sign `facing=eats` at a coordinate triple",
            vec![COMPASS, "did you mean `facing=east`?"],
        ),
        (
            r#"sign at=3,1,2 text=["a", "b", "c", "d", "e"]"#,
            "sign `text=` writes 5 lines, more than a face of a sign holds",
            vec!["a face holds at most four lines; write `text=` as a list of up to four strings"],
        ),
        (
            r#"sign at=3,1,2 back=["a", "b", "c", "d", "e"]"#,
            "sign `back=` writes 5 lines, more than a face of a sign holds",
            vec!["a face holds at most four lines; write `back=` as a list of up to four strings"],
        ),
        (
            "sign at=3,1,2 offset=1",
            "sign `offset=` beside a coordinate triple, which names the cell whole",
            vec![
                "drop `offset=`, or set the sign by a wall with `at=<side>.outside` or \
                 `at=inside.<side>`, which reads them",
            ],
        ),
        (
            "sign",
            "sign has no `at=`, so nothing says where it stands",
            vec![
                "write `at=` as the coordinate triple `x,y,z` of the cell the sign stands in, as \
                 in `at=1,1,1`, or as a wall anchor, `at=<side>.outside` or `at=inside.<side>`",
            ],
        ),
    ] {
        let src = room(line);
        let out = lowered(&src);
        assert_eq!(
            findings(&src, &out),
            [(DiagnosticCode::InvalidMember, line, primary, notes)],
            "{line}",
        );
        assert_eq!((signs(&out), entities(&out)), (vec![], vec![]), "{line}");
    }
}

#[test]
fn a_value_of_the_wrong_kind_is_ignored_and_the_sign_built_without_it() {
    let line = r#"sign at=inside.front offset=2 y=1 text=3 back=["a", 4] glowing=yes"#;
    let src = room(line);
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::IgnoredArgument,
                "3",
                "`text=` must be a string, or a list of up to four strings, not integer `3`; the \
                 value was ignored",
                vec!["the sign's front is left blank"],
            ),
            (
                DiagnosticCode::IgnoredArgument,
                "4",
                "`back=` must be a string, or a list of up to four strings, not integer `4`; the \
                 value was ignored",
                vec!["the sign's back is left blank"],
            ),
            (
                DiagnosticCode::IgnoredArgument,
                "yes",
                "`glowing=` must be `true` or `false`, not identifier `yes`; the value was ignored",
                vec!["the sign's text does not glow"],
            ),
        ],
    );
    assert_eq!(
        (signs(&out), entities(&out)),
        (
            vec![(
                (2, 1, 3),
                "minecraft:oak_wall_sign[facing=north]".to_owned()
            )],
            vec![sign_entity((2, 1, 3), &[], &[], false)],
        ),
    );
}

#[test]
fn a_sign_a_later_member_builds_over_keeps_no_block_entity() {
    let (placed, kept) = set("sign at=3,1,2\n  raw_block mat=minecraft:stone at=3,1,2");
    assert_eq!((placed, kept), (vec![], vec![]));
}

#[test]
fn a_face_holds_four_lines() {
    assert_eq!(
        set(r#"sign at=3,1,2 text=["a", "b", "c", "d"] back=["e", "f", "g", "h"]"#),
        (
            vec![((3, 1, 2), "minecraft:oak_sign[rotation=0]".to_owned())],
            vec![sign_entity(
                (3, 1, 2),
                &["a", "b", "c", "d"],
                &["e", "f", "g", "h"],
                false
            )],
        ),
    );
}

#[test]
fn a_face_of_too_many_lines_is_refused_whatever_its_lines_hold() {
    let line = r#"sign at=3,1,2 text=["a", "b", "c", "d", "e", 1]"#;
    let src = room(line);
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::InvalidMember,
                line,
                "sign `text=` writes 6 lines, more than a face of a sign holds",
                vec![
                    "a face holds at most four lines; write `text=` as a list of up to four \
                     strings",
                ],
            ),
            (
                DiagnosticCode::IgnoredArgument,
                "1",
                "`text=` must be a string, or a list of up to four strings, not integer `1`; the \
                 value was ignored",
                vec!["this sign is not built either way — see the finding on the same line"],
            ),
        ],
    );
    assert_eq!((signs(&out), entities(&out)), (vec![], vec![]));
}

#[test]
fn a_sign_outside_a_wall_hangs_on_its_outer_face() {
    let src = "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=5x4\n  \
               walls mat_slot=wall height=2\n  roof kind=flat mat_slot=wall overhang=1\n  \
               sign at=front.outside offset=1 y=0 text=\"Inn\"\n";
    let out = lowered(src);
    assert_eq!(findings(src, &out), Vec::<Finding<'_>>::new());
    assert_eq!(
        (signs(&out), entities(&out)),
        (
            vec![(
                (2, 0, 5),
                "minecraft:oak_wall_sign[facing=south]".to_owned()
            )],
            vec![sign_entity((2, 0, 5), &["Inn"], &[], false)],
        ),
    );
}

#[test]
fn two_signs_of_different_text_on_one_cell_contest_its_block_entity() {
    let line = r#"sign at=3,1,2 text="Open""#;
    let src = room(&format!("sign at=3,1,2 text=\"Shut\"\n  {line}"));
    let out = lowered(&src);
    let mut notes = vec!["overwritten member declared here"];
    notes.extend(PHASE_NOTES);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::PhaseConflict,
            line,
            "`sign` overwrites 1 voxel that `sign` painted in the same phase, so which block \
             entity the build keeps is decided by the order the two lines are written in",
            notes,
        )],
    );
    assert_eq!(
        (signs(&out), entities(&out)),
        (
            vec![((3, 1, 2), "minecraft:oak_sign[rotation=0]".to_owned())],
            vec![sign_entity((3, 1, 2), &["Open"], &[], false)],
        ),
    );
    assert_eq!(
        set("sign at=3,1,2 text=\"Open\"\n  sign at=3,1,2 text=\"Open\""),
        (
            vec![((3, 1, 2), "minecraft:oak_sign[rotation=0]".to_owned())],
            vec![sign_entity((3, 1, 2), &["Open"], &[], false)],
        ),
    );
}

#[test]
fn a_sign_whose_slot_binds_no_block_falls_back_to_the_default_sign_for_its_stand() {
    let src = "theme t:\n  slot wall -> @cobblestone\n  slot odd -> 1\n\nstruct s size=7x5\n  \
               floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
               sign at=inside.front offset=2 y=1 mat_slot=odd\n  sign at=3,1,2 mat_slot=odd\n";
    let out = lowered(src);
    // The notes list what lowering builds, which every deferred member carries.
    assert_eq!(
        findings(src, &out)
            .into_iter()
            .map(|(code, span, primary, _)| (code, span, primary))
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::DeferredMember,
                "sign at=inside.front offset=2 y=1 mat_slot=odd",
                "sign's `mat_slot=` did not resolve to a block id under theme `t`; it falls back \
                 to `minecraft:oak_wall_sign`",
            ),
            (
                DiagnosticCode::DeferredMember,
                "sign at=3,1,2 mat_slot=odd",
                "sign's `mat_slot=` did not resolve to a block id under theme `t`; it falls back \
                 to `minecraft:oak_sign`",
            ),
        ],
    );
    assert_eq!(
        signs(&out),
        [
            (
                (2, 1, 3),
                "minecraft:oak_wall_sign[facing=north]".to_owned()
            ),
            ((3, 1, 2), "minecraft:oak_sign[rotation=0]".to_owned()),
        ],
    );
}

/// A target that declares `ids` and writes each canonical token `respelled` lists as the id beside
/// it, or, where it pins no version, may declare every id but `denied`.
struct Signs {
    ids: Option<Vec<String>>,
    respelled: &'static [(&'static str, &'static str)],
    denied: &'static str,
}

impl TargetRegistry for Signs {
    fn lookup(&self, _token: &str) -> Option<BlockState> {
        None
    }

    fn known_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        self.ids
            .as_deref()
            .map(|ids| BlockIdSet::new("test 2.0", ids))
    }

    fn respelled(&self, state: &BlockState) -> Option<BlockState> {
        self.respelled
            .iter()
            .find(|(token, _)| *token == state.id)
            .map(|(_, written)| BlockState {
                id: (*written).to_owned(),
                properties: state.properties.clone(),
            })
    }

    fn may_declare(&self, id: &str) -> bool {
        id != self.denied
    }
}

/// [`Signs`] pinned to `ids`, writing the tokens `respelled` lists.
fn pinned(ids: &[&str], respelled: &'static [(&'static str, &'static str)]) -> Signs {
    Signs {
        ids: Some(ids.iter().map(|id| (*id).to_owned()).collect()),
        respelled,
        denied: "",
    }
}

/// The payload of the refusal of a sign `mat_slot=board` binds to `@birch_sign`.
fn birch_sign_payload() -> serde_json::Value {
    json!({
        "kind": "incompatible_material",
        "id": "minecraft:birch_sign",
        "required": "sign",
        "slot": "board",
        "token": "birch_sign",
    })
}

/// A room hanging a sign bound to `@birch_sign` on its front wall and one bound to `@spruce_sign`
/// on its back wall.
const BIRCH_AND_SPRUCE: &str = "theme t:\n  slot wall -> @cobblestone\n  slot board -> @birch_sign\n  \
                                slot pine -> @spruce_sign\n\nstruct s size=7x5\n  \
                                floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
                                sign at=inside.front offset=2 y=1 mat_slot=board\n  \
                                sign at=inside.back offset=2 y=1 mat_slot=pine\n";

#[test]
fn a_standing_sign_whose_wall_sign_the_target_lacks_is_refused_naming_the_wall_sign() {
    let registry = pinned(
        &[
            "minecraft:birch_sign",
            "minecraft:cobblestone",
            "minecraft:oak_sign",
            "minecraft:oak_wall_sign",
            "minecraft:spruce_sign",
            "minecraft:spruce_wall_sign",
        ],
        &[],
    );
    let out = lowered_with(BIRCH_AND_SPRUCE, &registry);
    assert_eq!(
        findings(BIRCH_AND_SPRUCE, &out),
        [(
            DiagnosticCode::IncompatibleMaterial,
            "@birch_sign",
            "sign is built from a standing sign whose wall sign the target has, and \
             `minecraft:birch_wall_sign`, the wall sign of `minecraft:birch_sign`, is not a block \
             in `test 2.0`",
            vec![
                "bind the slot to a standing sign whose wall sign the target has — the registry \
                 pack's `sign.oak` resolves to one",
                "reached through `mat_slot=board`, so every member reading that slot has it too",
            ],
        )],
    );
    assert_eq!(
        serde_json::to_value(&out.diagnostics[0].data).expect("serialises"),
        birch_sign_payload(),
    );
    assert_eq!(
        signs(&out),
        [
            (
                (2, 1, 3),
                "minecraft:oak_wall_sign[facing=north]".to_owned()
            ),
            (
                (4, 1, 1),
                "minecraft:spruce_wall_sign[facing=south]".to_owned()
            ),
        ],
    );
}

#[test]
fn a_standing_sign_whose_wall_sign_the_target_lacks_stands_as_the_default_standing_sign() {
    let src = "theme t:\n  slot wall -> @cobblestone\n  slot board -> @birch_sign\n\n\
               struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
               sign at=3,1,2 mat_slot=board\n";
    let registry = pinned(
        &[
            "minecraft:birch_sign",
            "minecraft:cobblestone",
            "minecraft:oak_sign",
        ],
        &[],
    );
    let out = lowered_with(src, &registry);
    assert_eq!(
        findings(src, &out),
        [(
            DiagnosticCode::IncompatibleMaterial,
            "@birch_sign",
            "sign is built from a standing sign whose wall sign the target has, and \
             `minecraft:birch_wall_sign`, the wall sign of `minecraft:birch_sign`, is not a block \
             in `test 2.0`",
            vec![
                "bind the slot to a standing sign whose wall sign the target has — the registry \
                 pack's `sign.oak` resolves to one",
                "reached through `mat_slot=board`, so every member reading that slot has it too",
            ],
        )],
    );
    assert_eq!(
        serde_json::to_value(&out.diagnostics[0].data).expect("serialises"),
        birch_sign_payload(),
    );
    assert_eq!(
        signs(&out),
        [((3, 1, 2), "minecraft:oak_sign[rotation=0]".to_owned())],
    );
}

#[test]
fn a_standing_sign_whose_wall_sign_the_target_writes_under_another_id_stands_and_hangs() {
    let src = "theme t:\n  slot wall -> @cobblestone\n  slot board -> @oak_sign\n  \
               slot pine -> @spruce_sign\n\nstruct s size=7x5\n  floor mat_slot=wall\n  \
               walls mat_slot=wall height=3\n  sign at=inside.front offset=2 y=1 mat_slot=board\n  \
               sign at=3,1,2 mat_slot=board\n  sign at=inside.back offset=2 y=1 mat_slot=pine\n";
    // Bedrock's state map keeps the undeclared oak wall sign for the writer to map; the spruce one
    // is renamed.
    let registry = pinned(
        &[
            "minecraft:cobblestone",
            "minecraft:oak_sign",
            "minecraft:spruce_sign",
        ],
        &[
            ("minecraft:oak_wall_sign", "minecraft:oak_wall_sign"),
            ("minecraft:spruce_wall_sign", "minecraft:spruce_hung_sign"),
        ],
    );
    let out = lowered_with(src, &registry);
    assert_eq!(findings(src, &out), Vec::<Finding<'_>>::new());
    assert_eq!(
        signs(&out),
        [
            (
                (2, 1, 3),
                "minecraft:oak_wall_sign[facing=north]".to_owned()
            ),
            ((3, 1, 2), "minecraft:oak_sign[rotation=0]".to_owned()),
            (
                (4, 1, 1),
                "minecraft:spruce_hung_sign[facing=south]".to_owned()
            ),
        ],
    );
}

#[test]
fn with_no_version_pinned_a_wall_sign_no_version_writes_stands_in_for_its_block() {
    let registry = Signs {
        ids: None,
        respelled: &[],
        denied: "minecraft:birch_wall_sign",
    };
    let out = lowered_with(BIRCH_AND_SPRUCE, &registry);
    assert_eq!(findings(BIRCH_AND_SPRUCE, &out), Vec::<Finding<'_>>::new());
    assert_eq!(
        signs(&out),
        [
            (
                (2, 1, 3),
                "@minecraft:birch_wall_sign[facing=north]".to_owned()
            ),
            (
                (4, 1, 1),
                "minecraft:spruce_wall_sign[facing=south]".to_owned()
            ),
        ],
    );
}
