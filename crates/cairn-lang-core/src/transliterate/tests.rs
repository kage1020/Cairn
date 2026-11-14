//! Transliterating hand-built grids.

use indexmap::IndexMap;

use super::{
    BlockGrid, GridEntry, GridUnit, Stamp, TransliterateError, Transliteration, WrittenBody,
    transliterate, unit_names,
};
use crate::CAIRN_VERSION;
use crate::block_array::{
    BlockIdSet, BlockState, Dims, Faces, JoinKind, Joins, PaletteIndex, StateDomain, TargetRegistry,
};
use crate::edition::Edition;

/// A target that declares a few blocks and joins panes to stone, pinning no version.
struct Panes {
    ids: Vec<String>,
}

impl Panes {
    fn new() -> Self {
        let ids = ["air", "glass_pane", "oak_log", "stone"];
        Self {
            ids: ids.iter().map(|id| format!("minecraft:{id}")).collect(),
        }
    }
}

impl TargetRegistry for Panes {
    fn lookup(&self, _token: &str) -> Option<BlockState> {
        None
    }

    fn known_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        Some(BlockIdSet::new("test 1.0", &self.ids))
    }

    fn state_domain(&self, _state: &BlockState) -> StateDomain {
        StateDomain::Checked(Vec::new())
    }

    fn joins(&self, state: &BlockState) -> Option<Joins> {
        Some(match state.id.as_str() {
            "minecraft:glass_pane" => Joins {
                kind: Some(JoinKind::Pane),
                ..Joins::default()
            },
            "minecraft:stone" => Joins {
                sturdy: Faces::ALL,
                ..Joins::default()
            },
            _ => Joins::default(),
        })
    }
}

/// `id[key=value,...]` as a state.
fn state(spelled: &str) -> BlockState {
    let (id, rest) = spelled.split_once('[').unwrap_or((spelled, ""));
    let mut out = BlockState::bare(id);
    for pair in rest
        .trim_end_matches(']')
        .split(',')
        .filter(|p| !p.is_empty())
    {
        let (key, value) = pair.split_once('=').expect("key=value");
        out.properties.insert(key.to_owned(), value.to_owned());
    }
    out
}

/// A grid of `dims` from one character per cell, `x` fastest then `z` then `y`, each character's
/// block given by `legend`; `.` is air.
fn grid(dims: Dims, cells: &str, legend: &[(char, GridEntry)]) -> BlockGrid {
    let mut palette = vec![GridEntry::exact(BlockState::air())];
    palette.extend(legend.iter().map(|(_, entry)| entry.clone()));
    let voxels: Vec<PaletteIndex> = cells
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| {
            let slot = legend
                .iter()
                .position(|(mark, _)| *mark == c)
                .map_or(0, |i| i + 1);
            PaletteIndex(u16::try_from(slot).expect("a small legend"))
        })
        .collect();
    assert_eq!(voxels.len(), dims.volume(), "the picture covers the grid");
    BlockGrid {
        dims,
        palette,
        voxels,
    }
}

fn stone() -> (char, GridEntry) {
    ('s', GridEntry::exact(state("minecraft:stone")))
}

fn pane(mark: char, sides: &str) -> (char, GridEntry) {
    let spelled = format!("minecraft:glass_pane[{sides},waterlogged=false]");
    (mark, GridEntry::exact(state(&spelled)))
}

fn java() -> Stamp {
    Stamp {
        edition: Edition::Java,
        mc_version: "1.21.4".to_owned(),
        comment: vec!["from a test".to_owned()],
    }
}

fn unit(name: &str, grid: BlockGrid) -> GridUnit {
    GridUnit {
        name: name.to_owned(),
        grid,
        comment: Vec::new(),
    }
}

fn written(units: &[GridUnit]) -> Transliteration {
    transliterate(units, &java(), &Panes::new()).unwrap_or_else(|e| panic!("{e}"))
}

fn refused(units: &[GridUnit]) -> TransliterateError {
    match transliterate(units, &java(), &Panes::new()) {
        Ok(out) => panic!("expected a refusal, wrote:\n{}", out.source),
        Err(error) => error,
    }
}

/// The source after its header.
fn body(out: &Transliteration) -> &str {
    let start = out.source.find("\n\n").expect("a header and a body");
    &out.source[start + 2..]
}

#[test]
fn a_box_repeated_at_a_step_folds_into_a_raw_repeat_and_an_air_block_sizes_the_empty_top() {
    let mut row = unit(
        "row",
        grid(Dims { x: 7, y: 2, z: 1 }, "s.s.s.s .......", &[stone()]),
    );
    row.comment = vec!["region `Row`\nat 0,0,0".to_owned()];
    let out = written(&[row]);
    assert_eq!(
        out.source,
        format!(
            "# from a test\n\
             @cairn {CAIRN_VERSION}\n\
             @requires java version>=1.21.4\n\
             @intended_targets [\"1.21.4\"]\n\
             \n\
             # region `Row` at 0,0,0\n\
             struct row size=7x1\n  \
               raw_repeat count=4 step=2,0,0\n    \
                 raw_block mat=minecraft:stone at=0,0,0 origin=imported\n  \
               raw_block mat=minecraft:air at=0,1,0 origin=imported\n"
        )
    );
    assert_eq!(
        out.bodies,
        [WrittenBody {
            name: "row".to_owned(),
            chunks: Vec::new(),
            lines: 3,
        }]
    );
    assert_eq!(out.warnings, []);
}

#[test]
fn a_box_grows_along_x_then_z_then_y() {
    let out = written(&[unit(
        "slab",
        grid(Dims { x: 2, y: 2, z: 2 }, "ss ss s. ..", &[stone()]),
    )]);
    assert_eq!(
        body(&out),
        "struct slab size=2x2\n  \
           raw_fill mat=minecraft:stone from=0,0,0 to=1,0,1 origin=imported\n  \
           raw_block mat=minecraft:stone at=0,1,0 origin=imported\n"
    );
}

#[test]
fn a_connection_state_the_compile_derives_back_is_left_out() {
    let out = written(&[unit(
        "pane",
        grid(
            Dims { x: 3, y: 1, z: 2 },
            "sps s.s",
            &[
                stone(),
                pane('p', "east=true,north=false,south=false,west=true"),
            ],
        ),
    )]);
    assert_eq!(
        body(&out),
        "struct pane size=3x2\n  \
           raw_repeat count=2 step=2,0,0\n    \
             raw_fill mat=minecraft:stone from=0,0,0 to=0,0,1 origin=imported\n  \
           raw_block mat=minecraft:glass_pane[waterlogged=false] at=1,0,0 origin=imported\n"
    );
}

#[test]
fn a_cell_whose_stored_connections_the_compile_would_not_derive_writes_them_all() {
    let out = written(&[unit(
        "panes",
        grid(
            Dims { x: 4, y: 1, z: 1 },
            "sp.p",
            &[
                stone(),
                pane('p', "east=false,north=false,south=false,west=true"),
            ],
        ),
    )]);
    assert_eq!(
        body(&out),
        "struct panes size=4x1\n  \
           raw_block mat=minecraft:stone at=0,0,0 origin=imported\n  \
           raw_block mat=minecraft:glass_pane[waterlogged=false] at=1,0,0 origin=imported\n  \
           raw_block mat=minecraft:glass_pane[east=false,north=false,south=false,\
           waterlogged=false,west=true] at=3,0,0 origin=imported\n"
    );
}

#[test]
fn a_state_at_its_default_or_one_the_target_does_not_read_is_left_out() {
    let log = GridEntry {
        state: state("minecraft:oak_log[axis=y,lit=false]"),
        defaults: IndexMap::from([("axis".to_owned(), "y".to_owned())]),
        unread: vec!["lit".to_owned()],
    };
    let tilted = GridEntry {
        state: state("minecraft:oak_log[axis=x,lit=true]"),
        ..log.clone()
    };
    let out = written(&[unit(
        "logs",
        grid(
            Dims { x: 2, y: 1, z: 1 },
            "lt",
            &[('l', log), ('t', tilted)],
        ),
    )]);
    assert_eq!(
        body(&out),
        "struct logs size=2x1\n  \
           raw_block mat=minecraft:oak_log at=0,0,0 origin=imported\n  \
           raw_block mat=minecraft:oak_log[axis=x] at=1,0,0 origin=imported\n"
    );
}

#[test]
fn a_grid_past_the_chunk_footprint_is_written_as_a_site_of_full_height_columns() {
    let cells = format!("{}{}", "s".repeat(49 * 49), ".".repeat(49 * 49));
    let out = written(&[unit(
        "big",
        grid(Dims { x: 49, y: 2, z: 49 }, &cells, &[stone()]),
    )]);
    assert_eq!(
        body(&out),
        "theme imported\n\
         \n\
         def big_x0_z0 size=48x48\n  \
           raw_fill mat=minecraft:stone from=0,0,0 to=47,0,47 origin=imported\n  \
           raw_block mat=minecraft:air at=0,1,0 origin=imported\n\
         \n\
         def big_x0_z1 size=48x1\n  \
           raw_fill mat=minecraft:stone from=0,0,0 to=47,0,0 origin=imported\n  \
           raw_block mat=minecraft:air at=0,1,0 origin=imported\n\
         \n\
         def big_x1_z0 size=1x48\n  \
           raw_fill mat=minecraft:stone from=0,0,0 to=0,0,47 origin=imported\n  \
           raw_block mat=minecraft:air at=0,1,0 origin=imported\n\
         \n\
         def big_x1_z1 size=1x1\n  \
           raw_block mat=minecraft:stone at=0,0,0 origin=imported\n  \
           raw_block mat=minecraft:air at=0,1,0 origin=imported\n\
         \n\
         site big\n  \
           place id=big_x0_z1 use=big_x0_z1 theme=imported at=origin\n  \
           place id=big_x0_z0 use=big_x0_z0 theme=imported north_of=big_x0_z1 gap=0\n  \
           place id=big_x1_z0 use=big_x1_z0 theme=imported east_of=big_x0_z0 gap=0\n  \
           place id=big_x1_z1 use=big_x1_z1 theme=imported east_of=big_x0_z1 gap=0\n"
    );
    let chunks = ["big_x0_z0", "big_x0_z1", "big_x1_z0", "big_x1_z1"];
    assert_eq!(
        out.bodies,
        [WrittenBody {
            name: "big".to_owned(),
            chunks: chunks.map(str::to_owned).to_vec(),
            lines: 8,
        }]
    );
}

#[test]
fn each_unit_is_written_as_its_own_body_in_the_order_given() {
    let out = written(&[
        unit("first", grid(Dims { x: 1, y: 1, z: 1 }, "s", &[stone()])),
        unit(
            "second",
            grid(
                Dims { x: 1, y: 1, z: 1 },
                "p",
                &[pane('p', "east=false,north=false,south=false,west=false")],
            ),
        ),
    ]);
    assert_eq!(
        body(&out),
        "struct first size=1x1\n  \
           raw_block mat=minecraft:stone at=0,0,0 origin=imported\n\
         \n\
         struct second size=1x1\n  \
           raw_block mat=minecraft:glass_pane[waterlogged=false] at=0,0,0 origin=imported\n"
    );
}

#[test]
fn a_grid_of_only_air_is_refused() {
    let error = refused(&[unit("hollow", grid(Dims { x: 2, y: 1, z: 1 }, "..", &[]))]);
    assert_eq!(
        error,
        TransliterateError::Empty {
            unit: "hollow".to_owned()
        }
    );
    assert_eq!(
        error.to_string(),
        "`hollow` holds no block but air, so there is nothing to write"
    );
}

#[test]
fn a_grid_whose_voxels_disagree_with_its_extents_or_palette_is_refused() {
    let stone = || vec![GridEntry::exact(state("minecraft:stone"))];
    let cases = [
        (
            Dims { x: 2, y: 1, z: 1 },
            vec![0, 0, 0],
            "it lists 3 voxels for 2x1x1 cells",
        ),
        (
            Dims { x: 2, y: 1, z: 1 },
            vec![0, 5],
            "a voxel names palette entry 5 of 1",
        ),
        (
            Dims { x: 0, y: 1, z: 1 },
            vec![],
            "its extents 0x1x1 hold no cell",
        ),
    ];
    for (dims, voxels, reason) in cases {
        let grid = BlockGrid {
            dims,
            palette: stone(),
            voxels: voxels.into_iter().map(PaletteIndex).collect(),
        };
        assert_eq!(
            refused(&[unit("bad", grid)]),
            TransliterateError::Malformed {
                unit: "bad".to_owned(),
                reason: reason.to_owned(),
            }
        );
    }
}

#[test]
fn a_block_the_target_does_not_declare_is_refused_with_the_compile_errors() {
    let error = refused(&[unit(
        "odd",
        grid(
            Dims { x: 1, y: 1, z: 1 },
            "d",
            &[('d', GridEntry::exact(state("minecraft:dirt")))],
        ),
    )]);
    let TransliterateError::Refused { source, errors, .. } = &error else {
        panic!("expected the compile's refusal, got {error:?}");
    };
    let found: Vec<(&str, &str)> = errors
        .iter()
        .map(|e| (e.code.as_str(), &source[e.span.clone()]))
        .collect();
    assert_eq!(found, [("E_UNKNOWN_ID", "minecraft:dirt")]);
    assert_eq!(
        error.to_string(),
        "the stamped target, java 1.21.4, refuses blocks of the file:\n  error[E_UNKNOWN_ID]: \
         `minecraft:dirt` is not a block in `test 1.0` (at `raw_block mat=minecraft:dirt \
         at=0,0,0 origin=imported`)\nFix: open and save the file in java 1.21.4, which renames \
         a block an older release spelled otherwise, or replace each block no java 1.21.4 \
         declares"
    );
}

#[test]
fn unit_names_are_identifiers_no_body_name_rule_refuses_and_no_two_alike() {
    assert_eq!(
        unit_names([
            "Main Region",
            "main-region",
            "CON",
            "3d",
            "",
            "struct",
            "--a--b--"
        ]),
        [
            "main_region",
            "main_region_2",
            "con_structure",
            "structure_3d",
            "structure",
            "struct_structure",
            "a_b",
        ]
    );
}
