use super::WallColumn;

#[test]
fn a_single_walls_member_occupies_the_rows_above_the_floor_slab() {
    let column = WallColumn::from_walls([(0, 3)]);
    assert_eq!(column.to_string(), "y=1..=3");
    assert!(!column.contains_rows(0, 1), "the floor plane is not a wall");
    assert!(column.contains_rows(1, 3));
    assert!(column.contains_rows(3, 1));
    assert!(!column.contains_rows(3, 2), "one row past the top course");
    assert!(!column.contains_rows(4, 1));
}

#[test]
fn two_courses_that_touch_merge_into_one() {
    // `walls height=5` plus `level y=5 walls height=4`: rows 1..=5 and
    // 6..=9 abut.
    let column = WallColumn::from_walls([(0, 5), (5, 4)]);
    assert_eq!(column.to_string(), "y=1..=9");
    assert!(column.contains_rows(4, 3), "4..=6 crosses the seam");
    assert!(column.contains_rows(1, 9));
    assert!(!column.contains_rows(1, 10));
}

#[test]
fn two_courses_with_air_between_them_stay_apart() {
    // `walls height=2` plus `level y=6 walls height=2`: rows 1..=2 and
    // 7..=8, with 3..=6 open air.
    let column = WallColumn::from_walls([(0, 2), (6, 2)]);
    assert_eq!(column.to_string(), "y=1..=2, y=7..=8");
    assert!(column.contains_rows(1, 2));
    assert!(column.contains_rows(7, 2));
    assert!(!column.contains_rows(3, 3), "hangs in the gap");
    assert!(!column.contains_rows(2, 6), "spans the gap");
}

#[test]
fn overlapping_courses_collapse_to_their_union() {
    let column = WallColumn::from_walls([(0, 4), (0, 2)]);
    assert_eq!(column.to_string(), "y=1..=4");
    assert!(column.contains_rows(1, 4));
}

#[test]
fn the_spans_come_out_ascending_however_the_members_were_written() {
    // Source order is level order only by convention.
    let column = WallColumn::from_walls([(6, 2), (0, 2), (3, 1)]);
    assert_eq!(column.to_string(), "y=1..=2, y=4..=4, y=7..=8");
}

#[test]
fn a_walls_member_with_no_height_contributes_nothing() {
    let column = WallColumn::from_walls([(0, 0)]);
    assert!(column.is_empty());
    assert_eq!(column.to_string(), "none");
    assert!(!column.contains_rows(1, 1));
}

#[test]
fn a_struct_with_no_walls_has_an_empty_column() {
    let column = WallColumn::from_walls([]);
    assert!(column.is_empty());
    assert!(!column.contains_rows(0, 1));
    assert!(!column.contains_rows(1, 1));
}

#[test]
fn a_height_that_would_overflow_the_column_does_not_wrap() {
    // `walls height=4294967295` inside `level y=2`. A wrapped end would
    // fall below the start and hide the wall.
    let column = WallColumn::from_walls([(2, u32::MAX)]);
    assert_eq!(column.to_string(), format!("y=3..={}", u32::MAX));
    assert!(column.contains_rows(3, 1));
    assert!(column.contains_rows(u32::MAX, 1));
}

#[test]
fn the_course_a_row_sits_in_is_the_one_that_caps_what_opens_there() {
    // `walls height=2` plus `level y=6 walls height=2`, four rows of air
    // apart.
    let column = WallColumn::from_walls([(0, 2), (6, 2)]);
    assert_eq!(column.course_top_at(1), Some(2));
    assert_eq!(column.course_top_at(2), Some(2));
    assert_eq!(column.course_top_at(7), Some(8));
    assert_eq!(column.course_top_at(3), None, "the gap holds no course");
    assert_eq!(column.course_top_at(0), None, "the floor slab owns row 0");
    assert_eq!(column.course_top_at(9), None);
}

#[test]
fn a_row_in_merged_courses_is_capped_by_the_merge_and_not_by_its_member() {
    // `walls height=1` plus `level y=1 walls height=3`: rows 1..=1 and
    // 2..=4 abut.
    let column = WallColumn::from_walls([(0, 1), (1, 3)]);
    assert_eq!(column.to_string(), "y=1..=4");
    assert_eq!(column.course_top_at(1), Some(4));
}

#[test]
fn a_row_in_no_course_at_all_has_no_top() {
    let column = WallColumn::from_walls([]);
    assert_eq!(column.course_top_at(1), None);
}

#[test]
fn a_rectangle_whose_rows_overflow_is_not_inside_any_column() {
    // `y=4294967295 size=1x2` would end at `u32::MAX + 1`.
    let column = WallColumn::from_walls([(0, u32::MAX)]);
    assert!(!column.contains_rows(u32::MAX, 2));
}
