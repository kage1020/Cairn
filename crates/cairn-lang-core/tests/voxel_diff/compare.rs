use std::collections::HashSet;

use cairn_lang_core::block_array::{BlockState, Dims, PaletteIndex};
use cairn_lang_core::transliterate::{BlockGrid, GridEntry, GridUnit};
use cairn_lang_core::voxel_diff::{Counts, compare};

use crate::common::{only_structure, read_example, solid_cells};
use crate::{STEM, THEME, build, hut, in_file, region_lines, report, unit_edited, unit_of, units};

/// Counts of `blocks` cells that agree in every way, `residual` of them built by raw members.
fn agreeing(blocks: u64, residual: u64) -> Counts {
    Counts {
        structure_blocks: blocks,
        built_blocks: blocks,
        both_blocks: blocks,
        matched: blocks,
        states_matched: blocks,
        residual,
    }
}

#[test]
fn a_source_compared_with_its_own_build_agrees_at_every_cell() {
    let source = read_example("cottage.crn");
    let built = build(&source).expect("the cottage builds");
    let blocks = solid_cells(only_structure(&built)) as u64;
    let report = report(&units(&source), &source);
    assert_eq!(report.counts, agreeing(blocks, 0));
    assert!(report.converged);
    assert_eq!(report.code, None);
    assert_eq!(report.regions, []);
    assert_eq!(report.patch, None);
}

#[test]
fn every_cell_a_raw_member_builds_is_residual() {
    let structure = hut("");
    let lift = hut("  raw_fill mat=minecraft:oak_planks from=1,1,1 to=3,1,3\n");
    let units = {
        let built = build(&lift).expect("the lift builds");
        vec![unit_of("s", only_structure(&built))]
    };
    let blocks = solid_cells(only_structure(&build(&structure).expect("the hut builds"))) as u64;
    let report = report(&units, &lift);
    assert_eq!(report.counts, agreeing(blocks + 9, 9));
    assert!(!report.converged);
}

#[test]
fn a_state_the_build_leaves_out_counts_as_the_default_the_entry_gives_and_is_otherwise_unread() {
    let source = format!("{THEME}struct s size=3x3\n  raw_block mat=minecraft:stone at=1,0,1\n");
    let built = build(&source).expect("it builds");
    let mut stones = 0;
    let unit = unit_edited("s", only_structure(&built), |entry| {
        if entry.state.id == "minecraft:stone" {
            entry.state = BlockState {
                id: "minecraft:stone".into(),
                properties: [("powered", "false"), ("lit", "true")]
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .into_iter()
                    .collect(),
            };
            entry.defaults.insert("lit".into(), "false".into());
            stones += 1;
        }
    });
    assert_eq!(stones, 1);
    let units = [unit];
    let report = report(&units, &source);
    assert_eq!(
        (report.counts.matched, report.counts.states_matched),
        (1, 0)
    );
    let states: Vec<_> = report.regions[0]
        .states
        .iter()
        .map(|s| (s.key.as_str(), s.structure.as_deref(), s.built.as_deref()))
        .collect();
    assert_eq!(states, [("lit", Some("true"), Some("false"))]);
}

#[test]
fn a_needed_state_the_build_leaves_out_differs_though_the_entry_gives_its_default() {
    let source =
        format!("{THEME}struct s size=3x3\n  raw_block mat=minecraft:redstone_torch at=1,0,1\n");
    let built = build(&source).expect("it builds");
    let wall = unit_edited("s", only_structure(&built), |entry| {
        if entry.state.id == "minecraft:redstone_torch" {
            let pairs =
                [("facing", "north"), ("lit", "true")].map(|(k, v)| (k.to_owned(), v.to_owned()));
            entry.state.properties = pairs.clone().into_iter().collect();
            entry.defaults = pairs.into_iter().collect();
            entry.needed = vec!["facing".to_owned()];
        }
    });
    let units = [wall];
    let bare = report(&units, &source);
    assert_eq!((bare.counts.matched, bare.counts.states_matched), (1, 0));
    let states: Vec<_> = bare.regions[0]
        .states
        .iter()
        .map(|s| (s.key.as_str(), s.structure.as_deref(), s.built.as_deref()))
        .collect();
    assert_eq!(states, [("facing", Some("north"), None)]);
    let facing = source.replace("redstone_torch at", "redstone_torch[facing=north] at");
    assert_eq!(report(&units, &facing).counts, agreeing(1, 1));
}

#[test]
fn a_waterlogged_either_side_leaves_out_counts_as_dry() {
    let source = format!(
        "{THEME}struct s size=3x3\n  raw_block mat=minecraft:oak_stairs[facing=east] at=1,0,1\n"
    );
    let built = build(&source).expect("it builds");
    let with_water = |value: &str| {
        [unit_edited("s", only_structure(&built), |entry| {
            if entry.state.id == "minecraft:oak_stairs" {
                entry
                    .state
                    .properties
                    .insert("waterlogged".into(), value.into());
            }
        })]
    };
    let dry = report(&with_water("false"), &source);
    assert_eq!(dry.counts, agreeing(1, 1));
    let wet = report(&with_water("true"), &source);
    assert_eq!((wet.counts.matched, wet.counts.states_matched), (1, 0));
    let states: Vec<_> = wet.regions[0]
        .states
        .iter()
        .map(|s| (s.key.as_str(), s.structure.as_deref(), s.built.as_deref()))
        .collect();
    assert_eq!(states, [("waterlogged", Some("true"), Some("false"))]);

    let written_dry = source.replace("[facing=east]", "[facing=east,waterlogged=false]");
    assert_eq!(report(&units(&source), &written_dry).counts, agreeing(1, 1));
}

#[test]
fn a_state_the_build_writes_and_the_structure_does_not_differs() {
    let lift = format!(
        "{THEME}struct s size=3x3\n  raw_block mat=minecraft:oak_stairs[facing=east] at=1,0,1\n"
    );
    let structure =
        format!("{THEME}struct s size=3x3\n  raw_block mat=minecraft:oak_stairs at=1,0,1\n");
    let report = report(&units(&structure), &lift);
    let states: Vec<_> = report.regions[0]
        .states
        .iter()
        .map(|s| (s.key.as_str(), s.structure.as_deref(), s.built.as_deref()))
        .collect();
    assert_eq!(states, [("facing", None, Some("east"))]);
}

#[test]
fn air_of_any_spelling_holds_no_block() {
    let structure = format!(
        "{THEME}struct s size=3x3\n  raw_block mat=minecraft:stone at=1,0,1\n  \
         raw_block mat=minecraft:cave_air at=0,0,0\n  raw_block mat=minecraft:void_air at=2,0,2\n"
    );
    let lift = format!("{THEME}struct s size=3x3\n  raw_block mat=minecraft:stone at=1,0,1\n");
    assert_eq!(report(&units(&structure), &lift).counts, agreeing(1, 1));
}

#[test]
fn a_lone_body_on_each_side_is_compared_whatever_their_names() {
    let source = hut("");
    let mut units = units(&source);
    units[0].name = "imported".into();
    let report = report(&units, &source);
    assert!(report.converged);
    let pairs: Vec<_> = report
        .bodies
        .iter()
        .map(|b| (b.structure.as_deref(), b.source.as_deref()))
        .collect();
    assert_eq!(pairs, [(Some("imported"), Some("s"))]);
}

#[test]
fn bodies_pair_by_name_and_an_unpaired_body_counts_every_cell() {
    let two = format!(
        "{THEME}struct a size=3x3\n  floor mat_slot=floor\n\n\
         struct b size=3x3\n  floor mat_slot=wall\n"
    );
    let mut units = units(&two);
    units[1].name = "c".into();
    let report = report(&units, &two);
    let bodies: Vec<_> = report
        .bodies
        .iter()
        .map(|b| (b.structure.as_deref(), b.source.as_deref(), b.counts))
        .collect();
    let empty = Counts::default();
    assert_eq!(
        bodies,
        [
            (Some("a"), Some("a"), agreeing(9, 0)),
            (
                Some("c"),
                None,
                Counts {
                    structure_blocks: 9,
                    ..empty
                }
            ),
            (
                None,
                Some("b"),
                Counts {
                    built_blocks: 9,
                    ..empty
                }
            ),
        ]
    );
    assert_eq!(
        compare(in_file(&units), &build(&two).expect("it builds")).counts,
        Counts {
            structure_blocks: 18,
            built_blocks: 18,
            both_blocks: 9,
            matched: 9,
            states_matched: 9,
            residual: 0,
        }
    );
}

#[test]
fn a_site_spread_far_apart_is_compared_without_filling_the_space_between() {
    let lift = format!(
        "{THEME}def hut size=3x3:\n  raw_block mat=minecraft:stone at=1,0,1\n\n\
         site far:\n  place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=1000000\n  \
         place id=c use=hut theme=t north_of=a gap=1000000\n"
    );
    let structure = format!("{THEME}struct s size=3x3\n  raw_block mat=minecraft:stone at=1,0,1\n");
    let report = report(&units(&structure), &lift);
    assert_eq!(
        report.counts,
        Counts {
            structure_blocks: 1,
            built_blocks: 3,
            both_blocks: 1,
            matched: 1,
            states_matched: 1,
            residual: 3,
        }
    );
    assert_eq!(
        region_lines(&report),
        [
            "extra far (1,0,1000004)..(1,0,1000004): 1 cell of minecraft:stone → built by \
             `raw_block[0]` (line 8)",
            "extra far (1000004,0,1000004)..(1000004,0,1000004): 1 cell of minecraft:stone → \
             built by `raw_block[0]` (line 8)",
        ]
    );
}

/// A row of `blocks` along `x`, `None` for air, as a region whose minimum corner is `corner`.
fn row_at(name: &str, blocks: &[Option<&str>], corner: (i32, i32, i32)) -> GridUnit {
    let mut palette = vec![GridEntry::exact(BlockState::air())];
    let voxels = blocks
        .iter()
        .map(|block| {
            let Some(id) = block else {
                return PaletteIndex(0);
            };
            let slot = palette
                .iter()
                .position(|entry| entry.state.id == *id)
                .unwrap_or_else(|| {
                    palette.push(GridEntry::exact(BlockState::bare(*id)));
                    palette.len() - 1
                });
            PaletteIndex(u16::try_from(slot).expect("a short row"))
        })
        .collect();
    let x = u32::try_from(blocks.len()).expect("a short row");
    GridUnit {
        name: name.to_owned(),
        grid: BlockGrid::new(Dims { x, y: 1, z: 1 }, palette, voxels).expect("a row of cells"),
        corner,
        comment: Vec::new(),
    }
}

#[test]
fn regions_compared_with_one_body_are_laid_at_their_corners_a_later_block_over_an_earlier() {
    let stone = Some("minecraft:stone");
    let regions = [
        row_at("a", &[stone; 3], (-5, 3, 2)),
        row_at("b", &[Some("minecraft:granite"), None], (-4, 3, 2)),
    ];
    let lift = format!(
        "{THEME}struct s size=3x1\n  raw_block mat=minecraft:stone at=0,0,0\n  \
         raw_block mat=minecraft:granite at=1,0,0\n  raw_block mat=minecraft:stone at=2,0,0\n"
    );
    let report = report(&regions, &lift);
    // `b`'s granite lies over `a`'s stone, and its air leaves the stone beside it.
    assert_eq!(report.counts, agreeing(3, 3));
    let pairs: Vec<_> = report
        .bodies
        .iter()
        .map(|b| (b.structure.as_deref(), b.source.as_deref()))
        .collect();
    assert_eq!(pairs, [(Some(STEM), Some("s"))]);
}

#[test]
fn a_site_written_as_a_region_per_array_compares_with_the_site_cell_for_cell() {
    let source = read_example("l-walkway.crn");
    let built = build(&source).expect("the site builds");
    let mut cells = HashSet::new();
    let regions: Vec<GridUnit> = built
        .structures
        .iter()
        .map(|(key, array)| {
            let corner = built.placements.get(key).map_or_else(
                || built.walkways.get(key.as_str()).expect("a walkway").origin,
                |placement| placement.origin,
            );
            for (i, slot) in array.voxels.iter().enumerate() {
                let (x, y, z) = array.dims.position(i).expect("a cell of the array");
                if slot.0 != 0 {
                    cells.insert((
                        i64::from(corner.0) + i64::from(x),
                        i64::from(corner.1) + i64::from(y),
                        i64::from(corner.2) + i64::from(z),
                    ));
                }
            }
            GridUnit {
                corner,
                ..unit_of(key, array)
            }
        })
        .collect();
    assert_eq!(regions.len(), 4, "three placements and a walkway");
    let report = report(&regions, &source);
    assert_eq!(report.counts, agreeing(cells.len() as u64, 0));
    assert!(report.converged);
    let pairs: Vec<_> = report
        .bodies
        .iter()
        .map(|b| (b.structure.as_deref(), b.source.as_deref()))
        .collect();
    assert_eq!(pairs, [(Some(STEM), Some("duo"))]);
    assert_eq!(report.regions, []);
}
