//! A `sign` hangs on the wall it stands by or stands on the block under it, and its block entity
//! holds the text its line writes (`spec/entities` "Two tiers").

use cairn_lang_core::block_array::{BlockEntity, BlockEntityData, SignText};
use cairn_lang_core::{BlockArrayIr, DiagnosticCode};

use crate::common::{Finding, blocks_where, findings, lowered, only_structure};

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n  slot board -> @spruce_sign\n\n";

/// A 7x5 struct with a floor and 3-row walls, `members` after them; with no overhang its front
/// wall is `z=4` and its left `x=0`.
fn room(members: &str) -> String {
    format!(
        "{THEME}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
         {members}\n"
    )
}

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
    let face = |lines: &[&str]| SignText {
        lines: lines.iter().map(|line| (*line).to_owned()).collect(),
        glowing,
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
        signs(&out),
        [(
            (2, 1, 3),
            "minecraft:oak_wall_sign[facing=north]".to_owned()
        )],
    );
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
