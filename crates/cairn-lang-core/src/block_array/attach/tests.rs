use indexmap::IndexMap;

use super::*;
use crate::block_array::{BlockArray, CellRun, Dims, Entity, PALETTE_CAPACITY, Palette};

const KEY: &str = "struct::hut";

fn stone() -> BlockState {
    BlockState::bare("minecraft:stone")
}

/// A 2x1x2 floor of stone under `struct::hut`, a chest's entity at its far corner.
fn hut() -> BlockArrayIr {
    let mut palette = Palette::new_with_air();
    palette.entries.push(stone());
    let array = BlockArray {
        dims: Dims { x: 2, y: 1, z: 2 },
        palette,
        voxels: vec![PaletteIndex(1); 4],
        block_entities: vec![BlockEntity {
            pos: (1, 0, 1),
            id: "minecraft:chest".to_owned(),
            fields: IndexMap::new(),
        }],
        entities: vec![Entity {
            pos: (0.5, 1.0, 0.5),
            id: "minecraft:item_frame".to_owned(),
        }],
        source_scope: KEY.to_owned(),
    };
    let floor = ResolvedMember {
        role: "floor".to_owned(),
        id: None,
        resolved_state: vec![ResolvedBlock {
            state: stone(),
            runs: vec![
                CellRun {
                    from: (0, 0, 0),
                    len: 2,
                },
                CellRun {
                    from: (0, 0, 1),
                    len: 2,
                },
            ],
        }],
        span: 0..5,
    };
    BlockArrayIr {
        structures: IndexMap::from([(KEY.to_owned(), array)]),
        placements: IndexMap::new(),
        walkways: IndexMap::new(),
        members: IndexMap::from([(KEY.to_owned(), vec![floor])]),
        diagnostics: Vec::new(),
    }
}

fn wire() -> BlockState {
    BlockState::bare("minecraft:redstone_wire")
}

fn comparator() -> BlockState {
    BlockState::bare("minecraft:comparator")
}

/// Stone under the whole floor, dust and a comparator over it.
fn circuit() -> Vec<LaidBlock> {
    let block = |at, state| LaidBlock {
        at,
        state,
        entity: None,
    };
    vec![
        block((1, 1, 0), wire()),
        LaidBlock {
            entity: Some((
                "minecraft:comparator".to_owned(),
                IndexMap::from([("OutputSignal".to_owned(), 3)]),
            )),
            ..block((0, 1, 1), comparator())
        },
        block((0, 0, 0), stone()),
        block((1, 0, 0), stone()),
        block((0, 0, 1), stone()),
        block((1, 0, 1), stone()),
    ]
}

/// A member's block ids, each with its runs.
type Blocks<'a> = Vec<(&'a str, Vec<CellRun>)>;

#[test]
fn a_circuit_lays_under_the_structure_which_rises_over_it() {
    let mut ir = hut();
    assert_eq!(ir.attach_circuit(KEY, 2, &circuit(), 10..20), Ok(()));

    let array = &ir.structures[KEY];
    assert_eq!(array.dims, Dims { x: 2, y: 3, z: 2 });
    assert_eq!(
        array.palette.entries,
        [BlockState::air(), stone(), wire(), comparator()]
    );
    let voxels: Vec<u16> = array.voxels.iter().map(|slot| slot.0).collect();
    assert_eq!(voxels, [1, 1, 1, 1, 0, 2, 3, 0, 1, 1, 1, 1]);
    assert_eq!(
        array.block_entities,
        [
            BlockEntity {
                pos: (1, 2, 1),
                id: "minecraft:chest".to_owned(),
                fields: IndexMap::new(),
            },
            BlockEntity {
                pos: (0, 1, 1),
                id: "minecraft:comparator".to_owned(),
                fields: IndexMap::from([("OutputSignal".to_owned(), 3)]),
            },
        ],
    );
    assert_eq!(array.entities[0].pos, (0.5, 3.0, 0.5));

    let members = &ir.members[KEY];
    let layout: Vec<(&str, Blocks<'_>)> = members
        .iter()
        .map(|member| {
            let blocks = member
                .resolved_state
                .iter()
                .map(|block| (block.state.id.as_str(), block.runs.clone()))
                .collect();
            (member.role.as_str(), blocks)
        })
        .collect();
    assert_eq!(
        layout,
        [
            (
                "circuit",
                vec![
                    (
                        "minecraft:stone",
                        vec![
                            CellRun {
                                from: (0, 0, 0),
                                len: 2
                            },
                            CellRun {
                                from: (0, 0, 1),
                                len: 2
                            },
                        ],
                    ),
                    (
                        "minecraft:redstone_wire",
                        vec![CellRun {
                            from: (1, 1, 0),
                            len: 1
                        }]
                    ),
                    (
                        "minecraft:comparator",
                        vec![CellRun {
                            from: (0, 1, 1),
                            len: 1
                        }]
                    ),
                ],
            ),
            (
                "floor",
                vec![(
                    "minecraft:stone",
                    vec![
                        CellRun {
                            from: (0, 2, 0),
                            len: 2
                        },
                        CellRun {
                            from: (0, 2, 1),
                            len: 2
                        },
                    ],
                )],
            ),
        ],
    );
    assert_eq!(members[0].span, 10..20);
}

#[test]
fn a_refused_circuit_leaves_the_structure_as_it_was() {
    let mut full = hut();
    full.structures[KEY]
        .palette
        .entries
        .extend((2..PALETTE_CAPACITY).map(|i| BlockState::bare(format!("test:b{i}"))));
    for (mut ir, key, void, refusal) in [
        (hut(), "struct::shed", 2, AttachError::NoStructure),
        (hut(), KEY, u32::MAX, AttachError::TooLarge),
        (hut(), KEY, 1 << 22, AttachError::TooLarge),
        (
            full,
            KEY,
            2,
            AttachError::PaletteFull(PaletteFull {
                capacity: PALETTE_CAPACITY,
            }),
        ),
    ] {
        let before = ir.clone();
        assert_eq!(ir.attach_circuit(key, void, &circuit(), 0..0), Err(refusal));
        assert_eq!(ir, before, "{key} void={void}");
    }
}
