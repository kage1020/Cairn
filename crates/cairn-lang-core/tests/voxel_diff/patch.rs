use cairn_lang_core::voxel_diff::Metrics;

use crate::common::read_example;
use crate::{THEME, hut, region_lines, report, units};

#[test]
fn the_patch_sets_back_the_numbers_a_lift_got_wrong() {
    let structure = read_example("cottage.crn");
    let lift = structure
        .replace("height=4", "height=3")
        .replace("offset=2", "offset=1");
    let report = report(&units(&structure), &lift);
    let patch = report.patch.expect("a patch");
    assert_eq!(
        patch.edits,
        [
            "edit walls[0] set height=4",
            "edit window@front[0] set offset=2"
        ]
    );
    assert_eq!(
        patch.metrics,
        Metrics {
            block_iou: 1.0,
            state_accuracy: 1.0,
            residual_ratio: 0.0,
        }
    );
    let placed: Vec<_> = report.regions.iter().map(|r| r.patch.as_deref()).collect();
    assert_eq!(
        placed,
        [
            None,
            None,
            Some("edit walls[0] set height=4"),
            None,
            Some("edit window@front[0] set offset=2"),
            None,
        ]
    );
}

#[test]
fn a_member_with_an_id_is_patched_through_its_selector() {
    let structure = hut("");
    let lift = structure.replace(
        "walls mat_slot=wall height=3",
        "walls id=w mat_slot=wall height=2",
    );
    let report = report(&units(&structure), &lift);
    assert_eq!(
        report.patch.as_ref().expect("a patch").edits,
        ["edit walls[id=w] set height=3"]
    );
    assert_eq!(
        region_lines(&report),
        [
            "missing s (0,3,0)..(4,3,4): 16 cells of minecraft:cobblestone → likely `walls[id=w]` \
          `height=2` too small"
        ]
    );
}

#[test]
fn a_raw_member_of_the_wrong_block_is_patched_to_the_structures() {
    let structure =
        format!("{THEME}struct s size=3x3\n  raw_fill mat=minecraft:stone from=0,0,0 to=2,0,2\n");
    let lift = structure.replace("minecraft:stone", "minecraft:granite");
    let report = report(&units(&structure), &lift);
    assert_eq!(
        report.patch.expect("a patch").edits,
        ["edit raw_fill[0] set mat=minecraft:stone"]
    );
}

#[test]
fn a_raw_box_is_grown_over_the_cells_it_stops_short_of() {
    let structure =
        format!("{THEME}struct s size=5x5\n  raw_fill mat=minecraft:stone from=0,0,0 to=4,0,4\n");
    let lift = structure.replace("to=4,0,4", "to=4,0,2");
    let report = report(&units(&structure), &lift);
    assert_eq!(
        report.patch.expect("a patch").edits,
        ["edit raw_fill[0] set from=0,0,0 to=4,0,4"]
    );
}

#[test]
fn no_patch_is_offered_when_no_candidate_lowers_the_cells_that_differ() {
    let structure = hut("");
    let lift = structure.replace("slot wall  -> @cobblestone", "slot wall  -> @stone");
    let report = report(&units(&structure), &lift);
    assert_eq!(report.patch, None);
    assert!(!report.converged);
}

#[test]
fn a_patch_holds_at_most_three_lines() {
    let structure = format!(
        "{THEME}struct s size=9x9\n  floor mat_slot=floor\n  walls mat_slot=wall height=3\n  \
         raw_fill mat=minecraft:stone from=1,1,1 to=1,2,1\n  \
         raw_fill mat=minecraft:stone from=7,1,1 to=7,2,1\n  \
         raw_fill mat=minecraft:stone from=1,1,7 to=1,2,7\n  \
         raw_fill mat=minecraft:stone from=7,1,7 to=7,2,7\n"
    );
    let lift = structure.replace(",2,", ",1,");
    let report = report(&units(&structure), &lift);
    assert_eq!(
        report.patch.expect("a patch").edits,
        [
            "edit raw_fill[0] set from=1,1,1 to=1,2,1",
            "edit raw_fill[1] set from=7,1,1 to=7,2,1",
            "edit raw_fill[2] set from=1,1,7 to=1,2,7",
        ]
    );
}

#[test]
fn a_box_under_a_level_is_grown_as_its_line_measures_it() {
    let structure = format!(
        "{THEME}struct s size=5x5\n  walls mat_slot=wall height=7\n  level y=1\n    \
         raw_fill mat=minecraft:stone from=1,0,1 to=3,5,3\n"
    );
    let lift = structure.replace("to=3,5,3", "to=3,5,2");
    let report = report(&units(&structure), &lift);
    assert_eq!(
        region_lines(&report),
        [
            "missing s (1,1,3)..(3,6,3): 18 cells of minecraft:stone → likely \
             `level[0].raw_fill[0]` `from=1,0,1 to=3,5,2` where `from=1,0,1 to=3,5,3` builds it"
        ]
    );
    assert_eq!(report.patch.expect("a patch").counts.differing(), 0);
}

#[test]
fn a_box_of_a_def_placed_away_from_the_sites_corner_is_grown_in_the_def() {
    let lift = format!(
        "{THEME}def shed size=3x3:\n  raw_block mat=minecraft:oak_planks at=1,0,1\n\n\
         def hut size=5x5:\n  raw_fill id=slab mat=minecraft:stone from=0,0,0 to=4,0,2\n\n\
         site v:\n  place id=a use=shed theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=4\n"
    );
    let structure = format!(
        "{THEME}struct v size=12x5\n  raw_block mat=minecraft:oak_planks at=1,0,1\n  \
         raw_fill mat=minecraft:stone from=7,0,0 to=11,0,4\n"
    );
    let report = report(&units(&structure), &lift);
    assert_eq!(
        region_lines(&report),
        [
            "missing v (7,0,3)..(11,0,4): 10 cells of minecraft:stone → likely `raw_fill[id=slab]` \
             `from=0,0,0 to=4,0,2` where `from=0,0,0 to=4,0,4` builds it"
        ]
    );
    assert_eq!(report.patch.expect("a patch").counts.differing(), 0);
}

#[test]
fn a_raw_box_is_cut_back_from_the_cells_it_overruns() {
    let structure =
        format!("{THEME}struct s size=5x5\n  raw_fill mat=minecraft:stone from=0,0,0 to=4,0,2\n");
    let lift = structure.replace("to=4,0,2", "to=4,0,4");
    let report = report(&units(&structure), &lift);
    assert_eq!(
        region_lines(&report),
        [
            "extra s (0,0,3)..(4,0,4): 10 cells of minecraft:stone → likely `raw_fill[0]` \
             `from=0,0,0 to=4,0,4` where `from=0,0,0 to=4,0,2` builds it"
        ]
    );
    assert_eq!(
        report.patch.expect("a patch").edits,
        ["edit raw_fill[0] set from=0,0,0 to=4,0,2"]
    );
}

#[test]
fn a_number_too_large_is_set_back_down() {
    let structure = hut("");
    let lift = structure.replace("height=3", "height=4");
    let report = report(&units(&structure), &lift);
    assert_eq!(
        region_lines(&report),
        [
            "extra s (0,4,0)..(4,4,4): 16 cells of minecraft:cobblestone → likely `walls[0]` \
             `height=4` too large"
        ]
    );
    assert_eq!(
        report.patch.expect("a patch").edits,
        ["edit walls[0] set height=3"]
    );
}

#[test]
fn a_member_of_the_wrong_slot_is_moved_to_the_slot_that_builds_the_structures_block() {
    let lift = hut("  window id=w side=front y=1 offset=1 size=1x1 mat_slot=glass\n");
    let report = report(&units(&hut("")), &lift);
    assert_eq!(
        region_lines(&report),
        [
            "replaced s (1,1,4)..(1,1,4): 1 cell of minecraft:cobblestone built as \
             minecraft:glass_pane → likely `window[id=w]` `mat_slot=glass` where `mat_slot=wall` \
             builds it"
        ]
    );
    assert_eq!(
        report.patch.expect("a patch").edits,
        ["edit window[id=w] set mat_slot=wall"]
    );
}

#[test]
fn a_state_argument_is_set_to_the_structures_value() {
    let structure = hut("  door side=front at=center hinge=left\n");
    let lift = structure.replace("hinge=left", "hinge=right");
    let report = report(&units(&structure), &lift);
    assert_eq!(
        region_lines(&report),
        [
            "state s (2,1,4)..(2,2,4): 2 cells of minecraft:oak_door (hinge=left built as right) \
             → likely `door@front[0]` `hinge=right` where `hinge=left` builds it"
        ]
    );
    assert_eq!(
        report.patch.expect("a patch").edits,
        ["edit door@front[0] set hinge=left"]
    );
}

#[test]
fn a_number_at_the_top_of_its_range_is_tried_without_overflowing() {
    let lift = hut("  roof kind=flat mat_slot=roof overhang=9223372036854775807\n");
    let report = report(&units(&hut("")), &lift);
    assert_eq!(
        region_lines(&report),
        [
            "extra s (0,4,0)..(4,4,4): 25 cells of minecraft:spruce_stairs → built by `roof[0]` \
             (line 10)"
        ]
    );
    assert_eq!(report.patch, None);
}
