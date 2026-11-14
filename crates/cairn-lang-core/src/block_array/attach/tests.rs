use indexmap::IndexMap;

use super::*;
use crate::block_array::{
    BlockArray, BlockEntityData, CellRun, Dims, Entity, EntityData, Face, PALETTE_CAPACITY,
    Palette,
};

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
            data: BlockEntityData::Fields {
                id: "minecraft:chest".to_owned(),
                fields: IndexMap::new(),
            },
        }],
        entities: vec![Entity {
            id: "minecraft:armor_stand".to_owned(),
            pos: (0.5, 1.0, 0.5),
            block_pos: (0, 1, 0),
            facing: Face::South,
            data: EntityData::ArmorStand,
            nbt: None,
        }],
        source_scope: KEY.to_owned(),
        derived: Vec::new(),
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
        geometry: MemberGeometry {
            anchor: (0, 0, 0),
            declared_bbox: Bbox::spanning((0, 0, 0), (1, 0, 1)),
            actual_bbox: Bbox::spanning((0, 0, 0), (1, 0, 1)),
            host_face: None,
        },
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
            entity: Some(LaidEntity {
                id: "minecraft:comparator".to_owned(),
                fields: IndexMap::from([("OutputSignal".to_owned(), 3)]),
            }),
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
                data: BlockEntityData::Fields {
                    id: "minecraft:chest".to_owned(),
                    fields: IndexMap::new(),
                },
            },
            BlockEntity {
                pos: (0, 1, 1),
                data: BlockEntityData::Fields {
                    id: "minecraft:comparator".to_owned(),
                    fields: IndexMap::from([("OutputSignal".to_owned(), 3)]),
                },
            },
        ],
    );
    assert_eq!(array.entities[0].pos, (0.5, 3.0, 0.5));
    assert_eq!(array.entities[0].block_pos, (0, 3, 0));

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
fn a_circuit_raises_every_member_geometry_over_the_layers_it_reserves() {
    let mut ir = hut();
    assert_eq!(ir.attach_circuit(KEY, 2, &circuit(), 10..20), Ok(()));
    let geometry: Vec<MemberGeometry> = ir.members[KEY]
        .iter()
        .map(|member| member.geometry)
        .collect();
    assert_eq!(
        geometry,
        [
            MemberGeometry {
                anchor: (0, 0, 0),
                declared_bbox: Bbox::spanning((0, 0, 0), (1, 1, 1)),
                actual_bbox: Bbox::spanning((0, 0, 0), (1, 1, 1)),
                host_face: None,
            },
            MemberGeometry {
                anchor: (0, 2, 0),
                declared_bbox: Bbox::spanning((0, 2, 0), (1, 2, 1)),
                actual_bbox: Bbox::spanning((0, 2, 0), (1, 2, 1)),
                host_face: None,
            },
        ],
    );
}

#[test]
fn a_circuit_declares_every_layer_it_reserves_and_holds_the_box_around_its_blocks() {
    let mut ir = hut();
    let lone = [LaidBlock {
        at: (1, 0, 0),
        state: wire(),
        entity: None,
    }];
    assert_eq!(ir.attach_circuit(KEY, 2, &lone, 10..20), Ok(()));
    assert_eq!(
        ir.members[KEY][0].geometry,
        MemberGeometry {
            anchor: (0, 0, 0),
            declared_bbox: Bbox::spanning((0, 0, 0), (1, 1, 1)),
            actual_bbox: Bbox::cell((1, 0, 0)),
            host_face: None,
        },
    );
}

#[test]
fn a_refused_circuit_leaves_the_structure_as_it_was() {
    let mut full = hut();
    full.structures[KEY]
        .palette
        .entries
        .extend((2..PALETTE_CAPACITY).map(|i| BlockState::bare(format!("test:b{i}"))));
    let with = |at: (u32, u32, u32), field: &str| {
        let mut blocks = circuit();
        blocks.push(LaidBlock {
            at,
            state: comparator(),
            entity: Some(LaidEntity {
                id: "minecraft:comparator".to_owned(),
                fields: IndexMap::from([(field.to_owned(), 1)]),
            }),
        });
        blocks
    };
    for (mut ir, key, void, blocks, refusal) in [
        (
            hut(),
            "struct::shed",
            2,
            circuit(),
            AttachError::NoStructure,
        ),
        (hut(), KEY, u32::MAX, circuit(), AttachError::TooLarge),
        (hut(), KEY, 1 << 22, circuit(), AttachError::TooLarge),
        (
            full,
            KEY,
            2,
            circuit(),
            AttachError::PaletteFull(PaletteFull {
                capacity: PALETTE_CAPACITY,
            }),
        ),
        (
            hut(),
            KEY,
            2,
            with((1, 2, 0), "OutputSignal"),
            AttachError::OutOfBounds { at: (1, 2, 0) },
        ),
        (
            hut(),
            KEY,
            2,
            with((2, 0, 0), "OutputSignal"),
            AttachError::OutOfBounds { at: (2, 0, 0) },
        ),
        (
            hut(),
            KEY,
            2,
            with((1, 1, 0), "OutputSignal"),
            AttachError::Overlap { at: (1, 1, 0) },
        ),
        (
            hut(),
            KEY,
            2,
            with((1, 1, 1), "x"),
            AttachError::PositionedField {
                at: (1, 1, 1),
                field: "x".to_owned(),
            },
        ),
    ] {
        let before = ir.clone();
        assert_eq!(ir.attach_circuit(key, void, &blocks, 0..0), Err(refusal));
        assert_eq!(ir, before, "{key} void={void}");
    }
}

#[test]
fn a_slot_only_the_circuit_holds_records_every_key_of_its_state_as_derived() {
    let mut ir = hut();
    let dot = BlockState {
        id: "minecraft:redstone_wire".to_owned(),
        properties: ["east", "north", "power", "south", "west"]
            .into_iter()
            .map(|key| (key.to_owned(), "none".to_owned()))
            .collect(),
    };
    let blocks = [
        LaidBlock {
            at: (0, 0, 0),
            state: stone(),
            entity: None,
        },
        LaidBlock {
            at: (1, 0, 0),
            state: dot,
            entity: None,
        },
    ];
    assert_eq!(ir.attach_circuit(KEY, 1, &blocks, 10..20), Ok(()));
    let keys = ["east", "north", "power", "south", "west"].map(str::to_owned);
    assert_eq!(
        ir.structures[KEY].derived,
        [Vec::new(), Vec::new(), keys.to_vec()]
    );
}
