//! The world rows a struct's `walls` members actually paint.
//!
//! A `walls height=H` member under `level y=N` fills the rows
//! `N + 1 ..= N + H` — one voxel above the level's base plane, because the
//! floor slab owns the base plane itself. A struct with several `walls`
//! members therefore occupies a *set* of rows, and the set can have gaps:
//! `walls height=2` plus a `level y=6 walls height=2` leaves `y = 3..=6`
//! as open air between the two courses.
//!
//! The lowering used to carry that set as one number, the highest row any
//! wall reached. A number cannot answer the question a `window` has to ask
//! — "is every row I cut into a wall?" — in either direction: it says
//! nothing about the rows below the first course (so a `window y=0` carved
//! a hole through the floor slab) and nothing about the gap between two
//! courses (so a `window` between them hung glass in open air). Both cut
//! silently, because a check that cannot see the fault cannot report it.
//!
//! [`WallColumn`] is that set, kept as sorted inclusive spans with
//! overlapping *and adjacent* runs merged. Merging adjacency is what makes
//! a level-on-level tower read as one wall: `walls height=5` plus
//! `level y=5 walls height=4` is `1..=9`, and a window spanning the seam
//! is cut into masonry the whole way up.

use std::fmt;

/// The rows a struct's walls occupy, as sorted inclusive `[start, end]`
/// spans with no two of them touching.
///
/// Built once per struct and consulted by every member that has to sit
/// inside a wall rather than merely below the roof.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct WallColumn {
    /// Disjoint, non-adjacent, ascending. Empty when the struct declares
    /// no `walls` with a positive `height=`.
    spans: Vec<(u32, u32)>,
}

impl WallColumn {
    /// Build the column from `(y_offset, height)` pairs — one per `walls`
    /// member the pass will paint, `y_offset` being the enclosing
    /// `level y=N` (`0` in the struct body).
    ///
    /// A member with `height = 0` contributes nothing: it paints no row,
    /// and its own `W_DEFERRED_MEMBER` fires in the massing phase.
    pub(super) fn from_walls(walls: impl IntoIterator<Item = (u32, u32)>) -> Self {
        let mut spans: Vec<(u32, u32)> = walls
            .into_iter()
            .filter(|(_, height)| *height > 0)
            .map(|(y_offset, height)| {
                let start = y_offset.saturating_add(1);
                (start, y_offset.saturating_add(height))
            })
            .collect();
        spans.sort_unstable();
        let mut merged: Vec<(u32, u32)> = Vec::with_capacity(spans.len());
        for (start, end) in spans {
            match merged.last_mut() {
                // `start <= last.1 + 1` merges touching spans as well as
                // overlapping ones, so two courses stacked without a gap
                // read as the single wall they build.
                Some(last) if start <= last.1.saturating_add(1) => last.1 = last.1.max(end),
                _ => merged.push((start, end)),
            }
        }
        Self { spans: merged }
    }

    /// Whether the struct paints any wall row at all.
    pub(super) fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Whether the `height` rows starting at `y_start` all lie inside one
    /// course.
    ///
    /// One course rather than the union: a rectangle that leaves masonry
    /// and re-enters it above the gap is not cut into a wall, it is two
    /// cuts with a hole between them. The spans are already merged, so
    /// "one course" and "the union, with no gap crossed" are the same
    /// test.
    ///
    /// `height = 0` cannot occur from source (`size=WxH` parses as
    /// `NonZeroU32`) and an empty rectangle vacuously fits, so it answers
    /// `true`. A rectangle whose last row does not fit in a `u32` answers
    /// `false`: no column can hold it, and a wrapped sum would land the
    /// question back at row 0 and answer `true` for a window nowhere near
    /// a wall.
    pub(super) fn contains_rows(&self, y_start: u32, height: u32) -> bool {
        let Some(last_offset) = height.checked_sub(1) else {
            return true;
        };
        let Some(y_end) = y_start.checked_add(last_offset) else {
            return false;
        };
        self.spans
            .iter()
            .any(|(start, end)| *start <= y_start && y_end <= *end)
    }
}

impl fmt::Display for WallColumn {
    /// `y=1..=3`, or `y=1..=2, y=7..=8` when the column has a gap — the
    /// form the deferral message quotes, so an author reading it can see
    /// which rows were available without counting `height=` by hand.
    ///
    /// An empty column renders as `none`; the one caller that can reach
    /// that case says so in its own words instead.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.spans.is_empty() {
            return f.write_str("none");
        }
        for (i, (start, end)) in self.spans.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            write!(f, "y={start}..={end}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
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
        // 6..=9 abut, so a window crossing the seam is cut into masonry
        // the whole way.
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
        // Two walls members on the same level, one taller: the shorter is
        // wholly inside the taller and contributes no second span.
        let column = WallColumn::from_walls([(0, 4), (0, 2)]);
        assert_eq!(column.to_string(), "y=1..=4");
        assert!(column.contains_rows(1, 4));
    }

    #[test]
    fn the_spans_come_out_ascending_however_the_members_were_written() {
        // Source order is the level order only by convention; the merge
        // depends on the spans being sorted, so it sorts them.
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
        // `walls height=4294967295` inside `level y=2`. The saturating add
        // keeps the span at the top of the range rather than wrapping to a
        // span that starts above where it ends — which would answer
        // `false` for every row and hide the wall entirely.
        let column = WallColumn::from_walls([(2, u32::MAX)]);
        assert_eq!(column.to_string(), format!("y=3..={}", u32::MAX));
        assert!(column.contains_rows(3, 1));
        assert!(column.contains_rows(u32::MAX, 1));
    }

    #[test]
    fn a_rectangle_whose_rows_overflow_is_not_inside_any_column() {
        // `y=4294967295 size=1x2` would end at `u32::MAX + 1`. Nothing can
        // contain it, and the answer has to come from the check rather
        // than from a wrapped sum that lands back at row 0.
        let column = WallColumn::from_walls([(0, u32::MAX)]);
        assert!(!column.contains_rows(u32::MAX, 2));
    }
}
