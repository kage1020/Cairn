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
//! wall reached. A number cannot answer the question an opening has to
//! ask — a `window`'s "is every row I cut into a wall?", a `door`'s "is
//! the row I open at one?" — in either direction: it says nothing about
//! the rows below the first course (so a `window y=0` carved a hole
//! through the floor slab) and nothing about the gap between two courses
//! (so a `window` between them hung glass in open air, and a `door` under
//! walls that start a storey up carved air over air). All of them cut
//! silently, because a check that cannot see the fault cannot report it.
//!
//! [`WallColumn`] is that set, kept as sorted inclusive spans with
//! overlapping *and adjacent* runs merged. Merging adjacency is what makes
//! a level-on-level tower read as one wall: `walls height=5` plus
//! `level y=5 walls height=4` is `1..=9`, a window spanning the seam is
//! cut into masonry the whole way up, and a doorway at the foot of it
//! opens its two rows across the two members that paint them.

use std::fmt;

/// The rows a struct's walls occupy, as sorted inclusive `[start, end]`
/// spans with no two of them touching.
///
/// Built once per struct and consulted by every member that has to sit
/// inside a wall rather than merely below the roof.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct WallColumn {
    /// Disjoint, non-adjacent, ascending. Empty when no `walls` member
    /// will paint a row — no positive `height=`, or a `mat_slot=` that
    /// does not resolve.
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
        self.course_top_at(y_start)
            .is_some_and(|course_top| y_end <= course_top)
    }

    /// The last row of the course that holds `y`, or `None` when no
    /// course does.
    ///
    /// The question a `door` asks: it opens at one row and takes what
    /// the course above that row will give it, so the answer has to be
    /// the top of *that* course rather than the top of the tallest one.
    /// A `window` asks the same column [`Self::contains_rows`], which is
    /// this answer plus "and the rectangle ends at or below it" — one
    /// implementation, so the two members cannot come to disagree about
    /// where the masonry is.
    pub(super) fn course_top_at(&self, y: u32) -> Option<u32> {
        self.spans
            .iter()
            .find(|(start, end)| *start <= y && y <= *end)
            .map(|(_, end)| *end)
    }
}

impl fmt::Display for WallColumn {
    /// `y=1..=3`, or `y=1..=2, y=7..=8` when the column has a gap — the
    /// form the deferral message quotes, so an author reading it can see
    /// which rows were available without counting `height=` by hand.
    ///
    /// An empty column renders as `none`, which no caller prints: every
    /// site that formats a column asks `is_empty()` first — or holds one a
    /// non-empty check produced — and says "no walls" in its own words, so
    /// only this file's tests reach the fallback.
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
mod tests;
