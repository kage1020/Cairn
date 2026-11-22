use crate::intent::MemberRole;
use crate::voxel_diff::bodies::Owned;
use crate::voxel_diff::regions::Region;
use crate::voxel_diff::{Cause, RegionKind};
use crate::{lower, parse};

use super::{candidates, cut_back};

#[test]
fn a_window_is_tried_with_another_slot_each_number_within_range_and_a_repeat() {
    let source = "theme t:\n  slot wall -> @cobblestone\n  slot glass -> @glass_pane\n\n\
                  struct s size=5x5\n  walls mat_slot=wall height=3\n  \
                  window side=front y=0 offset=9223372036854775807 mat_slot=glass\n";
    let module = lower(&parse(source).expect("the source parses"));
    let found = module.addressed_members();
    let window = found
        .iter()
        .map(|found| found.member)
        .find(|member| member.role == MemberRole::Window)
        .expect("the window");
    let owned = Owned {
        role: "window".to_owned(),
        id: None,
        address: window.address.clone(),
        span: window.span.clone(),
        raw: false,
        declared_low: (0, 0, 0),
        blocks: Vec::new(),
    };
    let region = Region {
        kind: RegionKind::Replaced,
        body: "s".to_owned(),
        from: [1, 1, 4],
        to: [1, 1, 4],
        cells: 1,
        mat: "minecraft:cobblestone".to_owned(),
        built: Some("minecraft:glass_pane".to_owned()),
        states: Vec::new(),
        cause: Cause::NoMember,
        patch: None,
        fix: None,
        culprit: None,
        sample: None,
    };
    let slots = ["wall".to_owned(), "glass".to_owned()];
    let tried: Vec<(String, String)> = candidates(window, &owned, &region, source, &slots)
        .into_iter()
        .map(|candidate| (candidate.line, candidate.fix.to_string()))
        .collect();
    let pairs: Vec<(&str, &str)> = tried
        .iter()
        .map(|(line, fix)| (line.as_str(), fix.as_str()))
        .collect();
    assert_eq!(
        pairs,
        [
            (
                "edit window@front[0] set mat_slot=wall",
                "`window@front[0]` `mat_slot=glass` where `mat_slot=wall` builds it"
            ),
            (
                "edit window@front[0] set y=1",
                "`window@front[0]` `y=0` too small"
            ),
            (
                "edit window@front[0] set y=2",
                "`window@front[0]` `y=0` too small"
            ),
            (
                "edit window@front[0] set offset=9223372036854775806",
                "`window@front[0]` `offset=9223372036854775807` too large"
            ),
            (
                "edit window@front[0] set offset=9223372036854775805",
                "`window@front[0]` `offset=9223372036854775807` too large"
            ),
            (
                "edit window@front[0] set repeat=2",
                "`window@front[0]` without `repeat=2`"
            ),
            (
                "edit window@front[0] set repeat=3",
                "`window@front[0]` without `repeat=3`"
            ),
        ]
    );
}

#[test]
fn a_box_is_cut_back_from_a_slab_across_it_at_either_end_only() {
    let (low, high) = ([0, 0, 0], [4, 0, 4]);
    assert_eq!(
        cut_back(low, high, [0, 0, 0], [4, 0, 1]),
        Some(([0, 0, 2], [4, 0, 4]))
    );
    assert_eq!(
        cut_back(low, high, [0, 0, 3], [4, 0, 4]),
        Some(([0, 0, 0], [4, 0, 2]))
    );
    assert_eq!(cut_back(low, high, [0, 0, 2], [4, 0, 2]), None);
    assert_eq!(cut_back(low, high, [1, 0, 3], [4, 0, 4]), None);
}
