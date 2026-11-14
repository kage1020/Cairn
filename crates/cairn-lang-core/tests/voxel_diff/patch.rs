use cairn_lang_core::voxel_diff::{MAX_EDITS, Metrics};

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
    assert_eq!(report.patch.expect("a patch").edits.len(), MAX_EDITS);
}
