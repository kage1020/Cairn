//! Transliterating hand-built grids.

use indexmap::IndexMap;

use super::{
    BlockGrid, BlockGridError, GridEntry, GridUnit, Stamp, TransliterateError, Transliteration,
    UnspellablePart, WrittenBody, transliterate, unit_names,
};
use crate::CAIRN_VERSION;
use crate::block_array::{
    BlockIdSet, BlockState, Dims, DomainFault, Faces, JoinKind, Joins, OutOfDomain, PaletteIndex,
    StateDomain, TargetRegistry,
};
use crate::edition::Edition;

/// A target that declares a few blocks, joins panes to stone and builds no log on a `w` axis.
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

    fn state_domain(&self, state: &BlockState) -> StateDomain {
        let axis = state.properties.get("axis").filter(|axis| *axis == "w");
        StateDomain::Checked(
            axis.map(|axis| OutOfDomain {
                key: "axis".to_owned(),
                value: axis.clone(),
                fault: DomainFault::ValueOutside {
                    values: ["x", "y", "z"].map(str::to_owned).to_vec(),
                },
                valid_at: Vec::new(),
            })
            .into_iter()
            .collect(),
        )
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
    BlockGrid::new(dims, palette, voxels).expect("the picture covers the grid")
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
        corner: (0, 0, 0),
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
        needed: Vec::new(),
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
fn a_state_the_target_needs_is_written_at_its_default() {
    let log = GridEntry {
        state: state("minecraft:oak_log[axis=y,lit=false]"),
        defaults: IndexMap::from([
            ("axis".to_owned(), "y".to_owned()),
            ("lit".to_owned(), "false".to_owned()),
        ]),
        unread: Vec::new(),
        needed: vec!["axis".to_owned()],
    };
    let out = written(&[unit(
        "log",
        grid(Dims { x: 1, y: 1, z: 1 }, "l", &[('l', log)]),
    )]);
    assert_eq!(
        body(&out),
        "struct log size=1x1\n  \
           raw_block mat=minecraft:oak_log[axis=y] at=0,0,0 origin=imported\n"
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
fn a_block_whose_id_or_state_no_mat_spells_is_refused() {
    let cases = [
        ("create:andesite-casing", UnspellablePart::Id),
        ("minecraft:true", UnspellablePart::Id),
        ("stone", UnspellablePart::Id),
        (
            "minecraft:stone[note-id=1]",
            UnspellablePart::Property("note-id".to_owned()),
        ),
        (
            "minecraft:stone[a=b,note=#1]",
            UnspellablePart::Value("note".to_owned()),
        ),
        (
            "minecraft:stone[note=1.5]",
            UnspellablePart::Value("note".to_owned()),
        ),
    ];
    for (spelled, part) in cases {
        let odd = ('o', GridEntry::exact(state(spelled)));
        let units = [unit(
            "odd",
            grid(Dims { x: 2, y: 1, z: 1 }, "so", &[stone(), odd]),
        )];
        assert_eq!(
            refused(&units),
            TransliterateError::Unspellable {
                unit: "odd".to_owned(),
                block: spelled.to_owned(),
                part,
            },
            "{spelled}"
        );
    }
    let odd = ('o', GridEntry::exact(state("minecraft:stone[note=#1]")));
    let units = [unit("odd", grid(Dims { x: 1, y: 1, z: 1 }, "o", &[odd]))];
    assert_eq!(
        refused(&units).to_string(),
        "`odd` holds `minecraft:stone[note=#1]`, whose value of `note` no `mat=` can spell: a \
         block is `namespace:path[property=value]`, each name ASCII letters, digits and `_` that \
         does not start with a digit, and a value such a name or digits alone. Fix: replace the \
         block in the structure with one the target declares, and save the file again"
    );
}

#[test]
fn a_value_of_digits_or_a_boolean_is_spelled_and_a_block_no_cell_holds_is_not_read() {
    let lit = (
        'l',
        GridEntry::exact(state("minecraft:stone[level=15,lit=true]")),
    );
    let unheld = ('u', GridEntry::exact(state("create:andesite-casing")));
    let out = written(&[unit(
        "lit",
        grid(Dims { x: 1, y: 1, z: 1 }, "l", &[lit, unheld]),
    )]);
    assert_eq!(
        body(&out),
        "struct lit size=1x1\n  raw_block mat=minecraft:stone[level=15,lit=true] at=0,0,0 \
         origin=imported\n"
    );
}

#[test]
fn a_grid_whose_voxels_do_not_fill_its_extents_or_index_its_palette_is_not_made() {
    let dims = Dims { x: 2, y: 1, z: 1 };
    let made = |voxels: &[u16]| {
        let palette = vec![GridEntry::exact(state("minecraft:stone"))];
        BlockGrid::new(
            dims,
            palette,
            voxels.iter().copied().map(PaletteIndex).collect(),
        )
    };
    let error = made(&[0, 0, 0]).expect_err("three voxels for two cells");
    assert_eq!(error, BlockGridError::VoxelCount { dims, voxels: 3 });
    assert_eq!(error.to_string(), "3 voxels are given for 2x1x1 cells");
    let error = made(&[0, 5]).expect_err("a voxel past the palette");
    assert_eq!(error, BlockGridError::PastPalette { index: 5, len: 1 });
    assert_eq!(error.to_string(), "a voxel names palette entry 5 of 1");
}

#[test]
fn a_grid_with_no_cell_is_refused_as_holding_nothing() {
    let palette = vec![GridEntry::exact(state("minecraft:stone"))];
    let grid = BlockGrid::new(Dims { x: 0, y: 1, z: 1 }, palette, Vec::new())
        .expect("no voxel fills no cell");
    assert_eq!(
        refused(&[unit("flat", grid)]),
        TransliterateError::Empty {
            unit: "flat".to_owned()
        }
    );
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
        "the written source does not compile at the stamped target, java 1.21.4:\n  \
         error[E_UNKNOWN_ID]: `minecraft:dirt` is not a block in `test 1.0` (at `raw_block \
         mat=minecraft:dirt at=0,0,0 origin=imported`)\nFix: open and save the file in java \
         1.21.4, which renames a block an older release spelled otherwise, or replace each block \
         no java 1.21.4 declares"
    );
}

#[test]
fn unit_names_are_identifiers_no_body_name_rule_refuses_and_no_two_alike() {
    let one = Dims { x: 1, y: 1, z: 1 };
    assert_eq!(
        unit_names(
            [
                "Main Region",
                "main-region",
                "CON",
                "3d",
                "",
                "struct",
                "--a--b--"
            ]
            .map(|name| (name, one))
        ),
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

#[test]
fn a_unit_name_never_takes_a_chunk_name_of_a_unit_past_the_footprint() {
    let one = Dims { x: 1, y: 1, z: 1 };
    let wide = Dims { x: 49, y: 2, z: 49 };
    let edge = Dims { x: 48, y: 1, z: 48 };
    assert_eq!(
        unit_names([("Big", wide), ("big_x0_z0", one)]),
        ["big", "big_x0_z0_2"]
    );
    assert_eq!(
        unit_names([("big_x1_z0", one), ("Big", wide)]),
        ["big_x1_z0", "big_2"]
    );
    assert_eq!(
        unit_names([("edge", edge), ("edge_x0_z0", one)]),
        ["edge", "edge_x0_z0"]
    );
}

#[test]
fn a_grid_as_wide_and_deep_as_the_chunk_footprint_is_written_as_one_struct() {
    let out = written(&[unit(
        "edge",
        grid(
            Dims { x: 48, y: 1, z: 48 },
            &"s".repeat(48 * 48),
            &[stone()],
        ),
    )]);
    assert_eq!(
        body(&out),
        "struct edge size=48x48\n  \
           raw_fill mat=minecraft:stone from=0,0,0 to=47,0,47 origin=imported\n"
    );
}

#[test]
fn a_cell_no_raw_member_writes_as_the_grid_holds_it_is_refused() {
    let bare = ('p', GridEntry::exact(state("minecraft:glass_pane")));
    let error = refused(&[unit(
        "p",
        grid(Dims { x: 2, y: 1, z: 1 }, "ps", &[bare, stone()]),
    )]);
    assert_eq!(
        error,
        TransliterateError::Unmatched {
            unit: "p".to_owned(),
            cell: (0, 0, 0),
            expected: "minecraft:glass_pane".to_owned(),
            built: "minecraft:glass_pane[east=true,north=false,south=false,west=false]".to_owned(),
        }
    );
    assert_eq!(
        error.to_string(),
        "`p` cell 0,0,0 holds `minecraft:glass_pane`, and the written raw members build \
         `minecraft:glass_pane[east=true,north=false,south=false,west=false]` there, so no raw \
         member writes it; the transliteration is refused rather than written wrong, and the \
         file is worth reporting"
    );
}

#[test]
fn a_refusal_gives_one_fix_for_each_kind_of_error_raised() {
    let error = refused(&[unit(
        "odd",
        grid(
            Dims { x: 3, y: 1, z: 1 },
            "dlt",
            &[
                ('d', GridEntry::exact(state("minecraft:dirt"))),
                ('l', GridEntry::exact(state("minecraft:oak_log[axis=w]"))),
                ('t', GridEntry::exact(state("minecraft:oak_log[axis=x]"))),
            ],
        ),
    )]);
    let TransliterateError::Refused { source, errors, .. } = &error else {
        panic!("expected the compile's refusal, got {error:?}");
    };
    let found: Vec<(&str, &str)> = errors
        .iter()
        .map(|e| (e.code.as_str(), &source[e.span.clone()]))
        .collect();
    assert_eq!(
        found,
        [
            ("E_UNKNOWN_ID", "minecraft:dirt"),
            ("E_STATE_DOMAIN", "minecraft:oak_log[axis=w]")
        ]
    );
    assert_eq!(
        error.to_string(),
        "the written source does not compile at the stamped target, java 1.21.4:\n  \
         error[E_UNKNOWN_ID]: `minecraft:dirt` is not a block in `test 1.0` (at `raw_block \
         mat=minecraft:dirt at=0,0,0 origin=imported`)\n  error[E_STATE_DOMAIN]: \
         `minecraft:oak_log` takes no `axis=w` at test 1.0; `axis` takes x, y, z (at `raw_block \
         mat=minecraft:oak_log[axis=w] at=1,0,0 origin=imported`)\nFix: open and save the file \
         in java 1.21.4, which renames a block an older release spelled otherwise, or replace \
         each block no java 1.21.4 declares\nFix: open and save the file in java 1.21.4, which \
         rewrites a state an older release stored otherwise, or replace each state java 1.21.4 \
         does not build"
    );
}

#[test]
fn a_refusal_no_block_of_the_file_causes_is_worth_reporting() {
    let cells = format!("{}{}", "s".repeat(49 * 49), ".".repeat(49 * 49));
    let error = refused(&[
        unit("big", grid(Dims { x: 49, y: 2, z: 49 }, &cells, &[stone()])),
        unit(
            "big_x0_z0",
            grid(Dims { x: 1, y: 1, z: 1 }, "s", &[stone()]),
        ),
    ]);
    let TransliterateError::Refused { source, errors, .. } = &error else {
        panic!("expected the compile's refusal, got {error:?}");
    };
    let found: Vec<(&str, &str)> = errors
        .iter()
        .map(|e| (e.code.as_str(), &source[e.span.clone()]))
        .collect();
    assert_eq!(
        found,
        [(
            "E_OUTPUT_NAME_COLLISION",
            "place id=big_x0_z0 use=big_x0_z0 theme=imported north_of=big_x0_z1 gap=0"
        )]
    );
    assert_eq!(
        error.to_string(),
        "the written source does not compile at the stamped target, java 1.21.4:\n  \
         error[E_OUTPUT_NAME_COLLISION]: `place id=big_x0_z0` in site `big` would be written to \
         the same file as `struct big_x0_z0` (at `place id=big_x0_z0 use=big_x0_z0 \
         theme=imported north_of=big_x0_z1 gap=0`)\nFix: none; a source the transliteration \
         writes compiles at any target that builds its blocks, so the file is worth reporting"
    );
}

fn log() -> (char, GridEntry) {
    ('l', GridEntry::exact(state("minecraft:oak_log")))
}

#[test]
fn a_box_folds_with_the_partner_that_makes_the_longest_run() {
    let out = written(&[unit(
        "runs",
        grid(Dims { x: 3, y: 1, z: 5 }, "s.s ... s.. ... s..", &[stone()]),
    )]);
    assert_eq!(
        body(&out),
        "struct runs size=3x5\n  \
           raw_repeat count=3 step=0,0,2\n    \
             raw_block mat=minecraft:stone at=0,0,0 origin=imported\n  \
           raw_block mat=minecraft:stone at=2,0,0 origin=imported\n"
    );
}

#[test]
fn boxes_repeated_the_same_number_of_times_at_one_step_share_a_raw_repeat() {
    let out = written(&[unit(
        "rows",
        grid(Dims { x: 3, y: 1, z: 3 }, "s.s ... l.l", &[stone(), log()]),
    )]);
    assert_eq!(
        body(&out),
        "struct rows size=3x3\n  \
           raw_repeat count=2 step=2,0,0\n    \
             raw_block mat=minecraft:stone at=0,0,0 origin=imported\n    \
             raw_block mat=minecraft:oak_log at=0,0,2 origin=imported\n"
    );
}

#[test]
fn a_box_tries_only_the_next_32_boxes_of_its_kind_as_partners() {
    let gap = ".".repeat(41);
    let cells = [
        format!("{}s", ".".repeat(40)),
        gap.clone(),
        format!("{}.", "s.".repeat(20)),
        gap.clone(),
        format!("{}{}", "s.".repeat(12), ".".repeat(17)),
        gap,
        format!("{}s", ".".repeat(40)),
    ]
    .join(" ");
    let out = written(&[unit(
        "window",
        grid(Dims { x: 41, y: 1, z: 7 }, &cells, &[stone()]),
    )]);
    assert_eq!(
        body(&out),
        "struct window size=41x7\n  \
           raw_block mat=minecraft:stone at=40,0,0 origin=imported\n  \
           raw_repeat count=20 step=2,0,0\n    \
             raw_block mat=minecraft:stone at=0,0,2 origin=imported\n  \
           raw_repeat count=12 step=2,0,0\n    \
             raw_block mat=minecraft:stone at=0,0,4 origin=imported\n  \
           raw_block mat=minecraft:stone at=40,0,6 origin=imported\n"
    );
}

#[test]
fn a_pane_across_a_chunk_edge_writes_the_side_the_next_chunk_holds() {
    let cells = format!("{}spps", ".".repeat(46));
    let out = written(&[unit(
        "edge",
        grid(
            Dims { x: 50, y: 1, z: 1 },
            &cells,
            &[
                stone(),
                pane('p', "east=true,north=false,south=false,west=true"),
            ],
        ),
    )]);
    assert_eq!(
        body(&out),
        "theme imported\n\
         \n\
         def edge_x0_z0 size=48x1\n  \
           raw_block mat=minecraft:stone at=46,0,0 origin=imported\n  \
           raw_block mat=minecraft:glass_pane[east=true,north=false,south=false,\
           waterlogged=false,west=true] at=47,0,0 origin=imported\n\
         \n\
         def edge_x1_z0 size=2x1\n  \
           raw_block mat=minecraft:glass_pane[east=true,north=false,south=false,\
           waterlogged=false,west=true] at=0,0,0 origin=imported\n  \
           raw_block mat=minecraft:stone at=1,0,0 origin=imported\n\
         \n\
         site edge\n  \
           place id=edge_x0_z0 use=edge_x0_z0 theme=imported at=origin\n  \
           place id=edge_x1_z0 use=edge_x1_z0 theme=imported east_of=edge_x0_z0 gap=0\n"
    );
}
