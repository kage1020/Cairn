//! At a pinned target, an entity whose box reaches into a full block, a door's cell or another
//! entity's box is `W_ENTITY_CLIPPING`, and a frame or painting with nothing behind it is
//! `W_NO_ATTACHMENT_FACE`, each reported once at the member that places it (`spec/lint` "Entity
//! AABB").

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::block_array::{
    BlockIdSet, BlockState, EntityBox, PaintingLookup, PaintingVariant, StateDomain, TargetRegistry,
};

use crate::common::{Finding, findings, lowered, lowered_with};

/// A pinned target whose catalog boxes the structured entities, whose cobblestone is the one full
/// block, and whose painting table has `kebab` 1x1 and `bust` 2x2.
struct Boxes;

impl TargetRegistry for Boxes {
    fn lookup(&self, _token: &str) -> Option<BlockState> {
        None
    }

    fn known_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        None
    }

    fn state_domain(&self, _state: &BlockState) -> StateDomain {
        StateDomain::Checked(Vec::new())
    }

    fn solid(&self, state: &BlockState) -> Option<bool> {
        Some(state.id == "minecraft:cobblestone")
    }

    fn entity_box(&self, id: &str) -> Option<EntityBox> {
        let (width, height, depth) = match id.strip_prefix("minecraft:")? {
            "armor_stand" => (0.5, 1.975, None),
            "villager" => (0.6, 1.95, None),
            "item_frame" => (0.75, 0.75, Some(0.0625)),
            "painting" => (1.0, 1.0, Some(0.0625)),
            "block_display" => (0.0, 0.0, None),
            _ => return None,
        };
        Some(EntityBox {
            width,
            height,
            depth,
        })
    }

    fn painting(&self, variant: &str) -> PaintingLookup {
        let side = if variant == "bust" { 2 } else { 1 };
        PaintingLookup::Known(PaintingVariant {
            spelling: format!("minecraft:{variant}"),
            width: side,
            height: side,
        })
    }
}

const CLEAR: &str = "an entity's box stays clear of a full block, a door's cells and another \
                     entity's box, though it may meet one at a face";
const OUT_OF_BLOCKS: &str =
    "move the member to cells its box fits in, or take the block out of them";
const HUNG: &str = "an item frame or a painting hangs only on blocks the game counts as solid, \
                    one behind each cell it covers; air, water and lava hold none";
const HANG_FIX: &str = "put a block behind it, or move the member onto a wall";

/// A 7x5 struct of cobblestone with a floor and 3-row walls, `members` after them; with no
/// overhang its front wall is `z=4` and its left `x=0`.
fn room(members: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=7x5\n  floor mat_slot=wall\n  \
         walls mat_slot=wall height=3\n  {members}\n"
    )
}

/// The findings `members` raises in [`room`] at the [`Boxes`] target.
fn judged(members: &str, expected: &[Finding<'_>]) {
    let src = room(members);
    let out = lowered_with(&src, &Boxes);
    assert_eq!(findings(&src, &out), expected, "{members}");
}

#[test]
fn an_entity_whose_box_reaches_into_a_full_block_is_reported_with_the_block() {
    judged(
        "raw_block mat=minecraft:cobblestone at=2,2,2\n  armor_stand at=2,1,2",
        &[(
            DiagnosticCode::EntityClipping,
            "armor_stand at=2,1,2",
            "`armor_stand` places `minecraft:armor_stand` at 2,1,2, and its box reaches into the \
             full block `minecraft:cobblestone` at 2,2,2",
            vec![CLEAR, OUT_OF_BLOCKS],
        )],
    );
}

#[test]
fn one_report_counts_everything_a_box_reaches_into_and_names_the_first() {
    judged(
        "raw_block mat=minecraft:cobblestone at=2,1,2\n  \
         raw_block mat=minecraft:cobblestone at=2,2,2\n  villager at=2,1,2",
        &[(
            DiagnosticCode::EntityClipping,
            "villager at=2,1,2",
            "`villager` places `minecraft:villager` at 2,1,2, and its box reaches into 2 places \
             it must stay clear of: the first, the full block `minecraft:cobblestone` at 2,1,2,",
            vec![CLEAR, OUT_OF_BLOCKS],
        )],
    );
}

#[test]
fn a_box_that_meets_a_block_its_wall_or_another_box_at_a_face_only_touches_it() {
    judged(
        "raw_block mat=minecraft:cobblestone at=3,1,2\n  armor_stand at=2,1,2\n  \
         painting at=inside.front offset=2 y=1 variant=bust\n  \
         item_frame at=inside.back offset=3 y=2\n  \
         painting at=inside.back offset=1 y=1 variant=kebab\n  \
         painting at=inside.back offset=2 y=1 variant=kebab",
        &[],
    );
}

#[test]
fn an_entity_in_a_door_s_cell_is_reported_with_the_door() {
    judged(
        "door side=front at=center\n  armor_stand at=3,1,4",
        &[(
            DiagnosticCode::EntityClipping,
            "armor_stand at=3,1,4",
            "`armor_stand` places `minecraft:armor_stand` at 3,1,4, and its box reaches into 2 \
             places it must stay clear of: the first, the door at 3,1,4, whose swing passes \
             through it,",
            vec![CLEAR, "move the member out of the door's cells"],
        )],
    );
}

#[test]
fn an_entity_whose_box_overlaps_an_earlier_line_s_is_reported_with_it() {
    judged(
        "villager at=2,1,2\n  armor_stand at=2,1,2",
        &[(
            DiagnosticCode::EntityClipping,
            "armor_stand at=2,1,2",
            "`armor_stand` places `minecraft:armor_stand` at 2,1,2, and its box reaches into the \
             box of `minecraft:villager` at 2,1,2, which the `villager` line before it places",
            vec![
                CLEAR,
                "move one of the two members so that their boxes do not overlap",
            ],
        )],
    );
    judged(
        "armor_stand at=2,2,2\n  villager at=2,1,2",
        &[(
            DiagnosticCode::EntityClipping,
            "villager at=2,1,2",
            "`villager` places `minecraft:villager` at 2,1,2, and its box reaches into the box of \
             `minecraft:armor_stand` at 2,2,2, which the `armor_stand` line before it places",
            vec![
                CLEAR,
                "move one of the two members so that their boxes do not overlap",
            ],
        )],
    );
}

#[test]
fn a_display_s_empty_box_clips_nothing_and_nothing_clips_it() {
    judged(
        "raw_block mat=minecraft:cobblestone at=2,1,2\n  block_display at=2,1,2 mat_slot=wall\n  \
         block_display at=2,1,2 mat_slot=wall",
        &[],
    );
}

#[test]
fn a_hanging_entity_with_air_behind_a_cell_it_covers_is_dropped() {
    judged(
        "painting at=3,1,2 facing=north variant=kebab\n  \
         raw_block mat=minecraft:cobblestone at=2,1,3\n  \
         painting at=3,1,1 facing=north variant=bust",
        &[
            (
                DiagnosticCode::NoAttachmentFace,
                "painting at=3,1,2 facing=north variant=kebab",
                "`painting` places `minecraft:painting` at 3,1,2, where the game drops it: the \
                 cell behind it, 3,1,3, holds air",
                vec![HUNG, HANG_FIX],
            ),
            (
                DiagnosticCode::NoAttachmentFace,
                "painting at=3,1,1 facing=north variant=bust",
                "`painting` places `minecraft:painting` at 3,1,1, where the game drops it: 4 \
                 cells behind it hold nothing it hangs on: the first, 2,1,2, holds air",
                vec![HUNG, HANG_FIX],
            ),
        ],
    );
}

#[test]
fn a_frame_on_the_floor_hangs_on_the_block_under_it() {
    judged(
        "item_frame at=2,1,2 facing=up\n  item_frame at=2,2,2 facing=up",
        &[(
            DiagnosticCode::NoAttachmentFace,
            "item_frame at=2,2,2 facing=up",
            "`item_frame` places `minecraft:item_frame` at 2,2,2, where the game drops it: the cell \
         behind it, 2,1,2, holds air",
            vec![HUNG, HANG_FIX],
        )],
    );
}

#[test]
fn nothing_is_judged_without_a_pinned_target_or_in_a_body_an_error_refuses_a_member_of() {
    let clipping = "raw_block mat=minecraft:cobblestone at=2,2,2\n  armor_stand at=2,1,2";
    let src = room(clipping);
    assert_eq!(findings(&src, &lowered(&src)), Vec::<Finding<'_>>::new());
    judged(
        &format!("{clipping}\n  painting at=3,1,2 variant=kebab"),
        &[(
            DiagnosticCode::InvalidMember,
            "painting at=3,1,2 variant=kebab",
            "painting at a coordinate triple has no `facing=`, so nothing says which block it \
             hangs on",
            vec![
                "write `facing=` with the way it faces, one of north, south, east, west: it \
                 hangs on the block behind it",
            ],
        )],
    );
}
