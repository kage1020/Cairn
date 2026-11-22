use cairn_lang_core::block_array::{BlockState, Dims, PaletteIndex};
use cairn_lang_core::transliterate::{BlockGrid, GridEntry, GridUnit};
use cairn_lang_core::voxel_diff::{Cause, MemberRef, RegionKind};

use crate::common::read_example;
use crate::{THEME, hut, region_lines, report, units};

/// The cottage with its walls a row short and its windows a block toward the corners.
fn misbuilt_cottage() -> String {
    read_example("cottage.crn")
        .replace("height=4", "height=3")
        .replace("offset=2", "offset=1")
}

#[test]
fn the_cells_that_differ_cluster_into_regions_the_largest_first() {
    let structure = read_example("cottage.crn");
    let report = report(&units(&structure), &misbuilt_cottage());
    let summary: Vec<_> = report
        .regions
        .iter()
        .map(|r| {
            (
                r.kind,
                r.from,
                r.to,
                r.cells,
                r.mat.as_str(),
                r.built.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        summary,
        [
            (
                RegionKind::Missing,
                [0, 5, 0],
                [10, 9, 8],
                99,
                "minecraft:spruce_stairs",
                None
            ),
            (
                RegionKind::Extra,
                [0, 4, 0],
                [10, 8, 8],
                99,
                "minecraft:spruce_stairs",
                None
            ),
            (
                RegionKind::Missing,
                [1, 4, 1],
                [9, 4, 7],
                28,
                "minecraft:cobblestone",
                None
            ),
            (
                RegionKind::Replaced,
                [4, 2, 7],
                [6, 3, 7],
                4,
                "minecraft:glass_pane",
                Some("minecraft:cobblestone")
            ),
            (
                RegionKind::Replaced,
                [2, 2, 7],
                [2, 3, 7],
                2,
                "minecraft:cobblestone",
                Some("minecraft:glass_pane")
            ),
            (
                RegionKind::Replaced,
                [8, 2, 7],
                [8, 3, 7],
                2,
                "minecraft:cobblestone",
                Some("minecraft:glass_pane")
            ),
        ]
    );
}

#[test]
fn a_region_names_the_member_that_built_it_or_the_nearest_that_stops_short() {
    let structure = read_example("cottage.crn");
    let report = report(&units(&structure), &misbuilt_cottage());
    let member = |keyword: &str, address: &str, line: u32| MemberRef {
        keyword: keyword.into(),
        id: None,
        address: Some(address.into()),
        line,
    };
    let causes: Vec<_> = report.regions.iter().map(|r| r.cause.clone()).collect();
    assert_eq!(
        causes,
        [
            Cause::ShortOf {
                member: member("roof", "roof[0]", 18)
            },
            Cause::BuiltBy {
                member: member("roof", "roof[0]", 18)
            },
            Cause::ShortOf {
                member: member("walls", "walls[0]", 15)
            },
            Cause::BuiltBy {
                member: member("walls", "walls[0]", 15)
            },
            Cause::BuiltBy {
                member: member("window", "window@front[0]", 17)
            },
            Cause::BuiltBy {
                member: member("window", "window@front[0]", 17)
            },
        ]
    );
}

#[test]
fn cells_more_than_two_blocks_apart_are_separate_regions() {
    let lift = format!("{THEME}struct s size=9x1\n  floor mat_slot=floor\n");
    let structure = format!(
        "{lift}  raw_block mat=minecraft:stone at=0,0,0\n  raw_block mat=minecraft:stone at=2,0,0\n  \
         raw_block mat=minecraft:stone at=5,0,0\n"
    );
    let report = report(&units(&structure), &lift);
    assert_eq!(
        region_lines(&report),
        [
            "replaced s (0,0,0)..(2,0,0): 2 cells of minecraft:stone built as minecraft:oak_planks \
             → built by `floor[0]` (line 8)",
            "replaced s (5,0,0)..(5,0,0): 1 cell of minecraft:stone built as minecraft:oak_planks \
             → built by `floor[0]` (line 8)",
        ]
    );
}

#[test]
fn a_block_no_member_builds_near_names_no_member() {
    let lift = hut("");
    let structure = hut("  raw_block mat=minecraft:stone at=2,2,2\n");
    let report = report(&units(&structure), &lift);
    assert_eq!(
        region_lines(&report),
        ["missing s (2,2,2)..(2,2,2): 1 cell of minecraft:stone → no member builds it"]
    );
}

#[test]
fn a_cell_a_member_clears_names_that_member() {
    let lift = hut("  raw_block mat=minecraft:air at=2,0,2\n");
    let report = report(&units(&hut("")), &lift);
    assert_eq!(
        region_lines(&report),
        [
            "missing s (2,0,2)..(2,0,2): 1 cell of minecraft:oak_planks → cleared by \
             `raw_block[0]` (line 10)"
        ]
    );
}

#[test]
fn a_state_region_names_each_property_that_differs() {
    let lift = format!(
        "{THEME}struct s size=3x3\n  raw_block mat=minecraft:oak_stairs[facing=east] at=1,0,1\n"
    );
    let structure = lift.replace("facing=east", "facing=west");
    let report = report(&units(&structure), &lift);
    assert_eq!(
        region_lines(&report),
        [
            "state s (1,0,1)..(1,0,1): 1 cell of minecraft:oak_stairs (facing=west built as \
             east) → likely `raw_block[0]` `mat=minecraft:oak_stairs[facing=east]` where \
             `mat=minecraft:oak_stairs[facing=west]` builds it"
        ]
    );
}

#[test]
fn a_body_with_no_namesake_on_the_other_side_is_one_unpaired_region() {
    let structure = format!(
        "{THEME}struct a size=3x3\n  floor mat_slot=floor\n\n\
         struct b size=3x3\n  floor mat_slot=wall\n"
    );
    let lift = format!(
        "{THEME}struct a size=3x3\n  floor mat_slot=floor\n\n\
         struct c size=3x3\n  floor mat_slot=floor\n"
    );
    let report = report(&units(&structure), &lift);
    assert_eq!(
        region_lines(&report),
        [
            "missing b (0,0,0)..(2,0,2): 9 cells of minecraft:cobblestone → the source builds no \
             `b`",
            "extra c (0,0,0)..(2,0,2): 9 cells of minecraft:oak_planks → the structure holds no \
             `c`",
        ]
    );
}

#[test]
fn of_two_members_owning_as_many_cells_the_one_written_first_is_named() {
    let structure = format!(
        "{THEME}struct s size=3x3\n  raw_fill mat=minecraft:granite from=0,0,1 to=2,0,1\n  \
         raw_fill mat=minecraft:granite from=0,0,0 to=2,0,0\n"
    );
    let lift = structure.replace("granite", "diorite");
    let report = report(&units(&structure), &lift);
    let causes: Vec<_> = report.regions.iter().map(|r| r.cause.clone()).collect();
    // Addresses count by position, so the line written first, the farther row, is `raw_fill[1]`.
    assert_eq!(
        causes,
        [Cause::BuiltBy {
            member: MemberRef {
                keyword: "raw_fill".into(),
                id: None,
                address: Some("raw_fill[1]".into()),
                line: 8,
            }
        }]
    );
}

#[test]
fn a_site_is_laid_as_one_body_moved_to_its_lowest_corner() {
    let air = GridUnit {
        name: "nothing".into(),
        grid: BlockGrid::new(
            Dims { x: 1, y: 1, z: 1 },
            vec![GridEntry::exact(BlockState::air())],
            vec![PaletteIndex(0)],
        )
        .expect("one cell of air"),
        corner: (0, 0, 0),
        comment: Vec::new(),
    };
    let report = report(&[air], &read_example("l-walkway.crn"));
    // The walkway's box covers part of `home1`, laid before it, whose floor its air leaves whole.
    assert_eq!(
        region_lines(&report),
        [
            "extra duo (1,0,1)..(12,0,16): 27 cells of minecraft:gravel → laid by a walkway",
            "extra duo (13,1,0)..(15,3,2): 20 cells of minecraft:cobblestone → likely \
             `walls[id=walls]` `height=3` too large",
            "extra duo (0,1,13)..(2,3,15): 20 cells of minecraft:cobblestone → built by \
             `walls[id=walls]` (line 12)",
            "extra duo (13,1,13)..(15,3,15): 20 cells of minecraft:cobblestone → built by \
             `walls[id=walls]` (line 12)",
            "extra duo (13,0,0)..(15,0,2): 9 cells of minecraft:oak_planks → built by \
             `floor[id=floor]` (line 11)",
            "extra duo (0,0,13)..(2,0,15): 9 cells of minecraft:oak_planks → built by \
             `floor[id=floor]` (line 11)",
            "extra duo (13,0,13)..(15,0,15): 9 cells of minecraft:oak_planks → built by \
             `floor[id=floor]` (line 11)",
            "extra duo (13,1,1)..(14,2,2): 4 cells of minecraft:oak_door → built by \
             `door[id=entry]` (line 13)",
            "extra duo (0,1,14)..(1,2,15): 4 cells of minecraft:oak_door → built by \
             `door[id=entry]` (line 13)",
            "extra duo (13,1,14)..(14,2,15): 4 cells of minecraft:oak_door → built by \
             `door[id=entry]` (line 13)",
        ]
    );
}
