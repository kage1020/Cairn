//! A `bed` lays its head in the cell its anchor names and its foot behind it, its head toward the
//! wall it stands by unless its line turns it (`spec/blockstate` "Derive by default, promote on
//! override").

use cairn_lang_core::block_array::{
    BlockEntity, BlockEntityData, BlockIdSet, BlockState, TargetRegistry,
};
use cairn_lang_core::{BlockArrayIr, DiagnosticCode};

use crate::common::{Finding, blocks_where, findings, lowered, lowered_with, only_structure};

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n  slot sheet -> @blue_bed\n\n";

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

/// A bed half's block entity as its cell and the colour it keeps.
type Colored = ((u32, u32, u32), Option<String>);

/// The beds `out` holds.
fn beds(out: &BlockArrayIr) -> Vec<Placed> {
    blocks_where(only_structure(out), |id| id.ends_with("bed"))
}

/// The block entities `out` holds, each as its cell and colour.
fn colors(out: &BlockArrayIr) -> Vec<Colored> {
    only_structure(out)
        .block_entities
        .iter()
        .map(|entity| match &entity.data {
            BlockEntityData::Bed { color } => (entity.pos, color.clone()),
            other => panic!("a bed's block entity, not {other:?}"),
        })
        .collect()
}

/// The beds `members` lays in [`room`], which it must lay without a finding, with the colour each
/// half's block entity keeps.
fn laid(members: &str) -> (Vec<Placed>, Vec<Colored>) {
    let src = room(members);
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), Vec::<Finding<'_>>::new(), "{members}");
    (beds(&out), colors(&out))
}

/// A bed of `id` with its head at `head` and its foot at `foot`, pointing `facing`.
fn bed(id: &str, facing: &str, head: (u32, u32, u32), foot: (u32, u32, u32)) -> Vec<Placed> {
    let mut halves = vec![
        (head, format!("{id}[facing={facing},part=head]")),
        (foot, format!("{id}[facing={facing},part=foot]")),
    ];
    halves.sort();
    halves
}

/// Both halves' block entities, each keeping `color`, head first.
fn kept(color: &str, head: (u32, u32, u32), foot: (u32, u32, u32)) -> Vec<Colored> {
    vec![
        (head, Some(color.to_owned())),
        (foot, Some(color.to_owned())),
    ]
}

#[test]
fn a_bed_by_a_wall_lies_with_its_head_to_the_wall() {
    assert_eq!(
        laid("bed at=inside.back offset=4 y=1"),
        (
            bed("minecraft:red_bed", "north", (2, 1, 1), (2, 1, 2)),
            kept("red", (2, 1, 1), (2, 1, 2)),
        ),
    );
    assert_eq!(
        laid("bed at=inside.left offset=2 y=1"),
        (
            bed("minecraft:red_bed", "west", (1, 1, 2), (2, 1, 2)),
            kept("red", (1, 1, 2), (2, 1, 2)),
        ),
    );
}

#[test]
fn a_bed_line_turns_it_along_the_wall() {
    // Along the front wall `offset=` grows toward the east, so `right` points the head east.
    assert_eq!(
        laid("bed at=inside.front offset=3 y=1 facing=right"),
        (
            bed("minecraft:red_bed", "east", (3, 1, 3), (2, 1, 3)),
            kept("red", (3, 1, 3), (2, 1, 3)),
        ),
    );
    assert_eq!(
        laid("bed at=inside.front offset=3 y=1 facing=out"),
        (
            bed("minecraft:red_bed", "south", (3, 1, 3), (3, 1, 2)),
            kept("red", (3, 1, 3), (3, 1, 2)),
        ),
    );
}

#[test]
fn a_bed_at_a_triple_points_its_head_the_compass_way_its_line_writes() {
    assert_eq!(
        laid("bed at=3,1,2 facing=east"),
        (
            bed("minecraft:red_bed", "east", (3, 1, 2), (2, 1, 2)),
            kept("red", (3, 1, 2), (2, 1, 2)),
        ),
    );
}

#[test]
fn a_bed_takes_its_block_and_colour_from_its_slot() {
    assert_eq!(
        laid("bed at=inside.back offset=2 y=1 mat_slot=sheet"),
        (
            bed("minecraft:blue_bed", "north", (4, 1, 1), (4, 1, 2)),
            kept("blue", (4, 1, 1), (4, 1, 2)),
        ),
    );
}

#[test]
fn a_bed_bound_to_no_bed_is_built_from_the_default_one() {
    let src = room("bed at=inside.back offset=4 y=1 mat_slot=wall");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::IncompatibleMaterial,
            "@cobblestone",
            "bed derives `facing` / `part` from the wall it lies against and the way its head \
             points and `minecraft:cobblestone` is not a bed, so those states have nowhere to go",
            vec![
                "bind the slot to a bed material — the registry pack's `bed.default` and \
                 `bed.<colour>` tokens resolve to one",
                "reached through `mat_slot=wall`, so every member reading that slot has it too",
            ],
        )],
    );
    assert_eq!(
        beds(&out),
        bed("minecraft:red_bed", "north", (2, 1, 1), (2, 1, 2)),
    );
}

#[test]
fn a_bed_whose_foot_has_no_cell_is_refused_and_not_built() {
    const COMPASS: &str = "`facing=` takes one of north, south, east, west, the way the bed's \
                           head points: north toward the `back` wall and east toward the `right` \
                           one";
    for (line, primary, notes) in [
        (
            "bed at=inside.front offset=1 y=1 facing=right",
            "bed `facing=right` puts its foot, the cell behind its head, in the struct's walls",
            vec!["turn the bed with `facing=`, or move it, so the cell behind its head is free"],
        ),
        (
            "bed at=0,1,2 facing=east",
            "bed `facing=east` puts its foot, the cell behind its head, outside the struct",
            vec!["turn the bed with `facing=`, or move it, so the cell behind its head is free"],
        ),
        (
            "bed at=inside.back offset=2 y=1 facing=in",
            "bed `facing=in` points its head away from the wall it stands by, which puts its foot \
             in that wall",
            vec![
                "a bed by a wall lies with its head to the wall, `facing=out`, which it takes when \
                 its line writes none, or along it, `facing=left` or `facing=right`",
            ],
        ),
        (
            "bed at=3,1,2",
            "bed at a coordinate triple has no `facing=`, so nothing says which way it lies",
            vec![
                COMPASS,
                "no wall stands by a bed at a triple for its head to lie against",
            ],
        ),
        (
            "bed at=3,1,2 facing=left",
            "unknown bed `facing=left` at a coordinate triple",
            vec![
                COMPASS,
                "`left` names a way from a wall, and a bed at a coordinate triple stands by none; \
                 anchor it by a wall with `at=<side>.outside` or `at=inside.<side>` to turn it \
                 that way",
            ],
        ),
        (
            "bed at=3,1,2 facing=wets",
            "unknown bed `facing=wets` at a coordinate triple",
            vec![COMPASS, "did you mean `facing=west`?"],
        ),
    ] {
        let src = room(line);
        let out = lowered(&src);
        assert_eq!(
            findings(&src, &out),
            [(DiagnosticCode::InvalidMember, line, primary, notes)],
            "{line}",
        );
        assert_eq!((beds(&out), colors(&out)), (vec![], vec![]), "{line}");
    }
}

#[test]
fn a_bed_outside_a_wall_lies_outside_the_footprint() {
    let src = "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=5x4\n  \
               walls mat_slot=wall height=2\n  roof kind=flat mat_slot=wall overhang=2\n  \
               bed at=front.outside offset=1 y=0 facing=left\n";
    let out = lowered(src);
    assert_eq!(findings(src, &out), Vec::<Finding<'_>>::new());
    assert_eq!(
        beds(&out),
        bed("minecraft:red_bed", "west", (3, 0, 6), (4, 0, 6)),
    );
    // Its head to the wall, its foot one voxel past an overhang of one.
    let src = src
        .replace(" facing=left", "")
        .replace("overhang=2", "overhang=1");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::InvalidMember,
            "bed at=front.outside offset=1 y=0",
            "bed puts its foot, the cell behind its head, in the wall or past the struct's \
             overhang",
            vec!["turn the bed with `facing=`, or move it, so the cell behind its head is free"],
        )],
    );
}

#[test]
fn a_facing_of_the_wrong_kind_is_ignored_and_the_bed_laid_to_the_wall() {
    let line = "bed at=inside.back offset=4 y=1 facing=3";
    let src = room(line);
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::IgnoredArgument,
            "3",
            "`facing=` must be the way the bed's head points, `in`, `out`, `left` or `right` by a \
             wall and `north`, `south`, `east` or `west` at a triple, not integer `3`; the value \
             was ignored",
            vec!["the bed lies with its head to the wall it stands by"],
        )],
    );
    assert_eq!(
        beds(&out),
        bed("minecraft:red_bed", "north", (2, 1, 1), (2, 1, 2)),
    );
}

/// A target that spells every dyed bed `minecraft:bed`, as Bedrock does.
struct OneBed;

impl TargetRegistry for OneBed {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        match token {
            "cobblestone" => Some(BlockState::bare("minecraft:cobblestone")),
            "bed" | "bed.default" | "bed.blue" => Some(BlockState::bare("minecraft:bed")),
            _ => None,
        }
    }

    fn known_tokens(&self) -> Vec<String> {
        ["cobblestone", "bed", "bed.default", "bed.blue"]
            .map(str::to_owned)
            .to_vec()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        None
    }

    fn canonical_block(&self, token: &str) -> Option<String> {
        match token {
            "bed.default" => Some("minecraft:red_bed".to_owned()),
            "bed.blue" => Some("minecraft:blue_bed".to_owned()),
            _ => None,
        }
    }
}

#[test]
fn a_bed_the_target_spells_as_every_dyed_bed_keeps_the_colour_its_token_names() {
    let src = "theme t:\n  slot wall -> @cobblestone\n  slot sheet -> @bed.blue\n  \
               slot plain -> @bed\n\nstruct s size=7x5\n  floor mat_slot=wall\n  \
               walls mat_slot=wall height=3\n  bed at=inside.back offset=5 y=1\n  \
               bed at=inside.back offset=3 y=1 mat_slot=sheet\n  \
               bed at=inside.back offset=1 y=1 mat_slot=plain\n";
    let out = lowered_with(src, &OneBed);
    assert_eq!(findings(src, &out), Vec::<Finding<'_>>::new());
    let at = |x: u32| [(x, 1, 1), (x, 1, 2)];
    assert_eq!(
        only_structure(&out).block_entities,
        [(at(1), Some("red")), (at(3), Some("blue")), (at(5), None),]
            .into_iter()
            .flat_map(|(cells, color)| {
                cells.map(|pos| BlockEntity {
                    pos,
                    data: BlockEntityData::Bed {
                        color: color.map(str::to_owned),
                    },
                })
            })
            .collect::<Vec<_>>(),
    );
}
