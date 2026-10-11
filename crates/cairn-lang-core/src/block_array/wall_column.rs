//! The world rows a struct's `walls` members paint.
//!
//! A `walls height=H` member under `level y=N` fills rows `N + 1 ..= N + H`,
//! one above the level's base plane, which the floor slab owns. Several
//! `walls` members occupy a *set* of rows, which can have gaps:
//! `walls height=2` plus `level y=6 walls height=2` leaves `y = 3..=6` open
//! air between the two courses.
//!
//! An opening asks about that set, not about the highest row: a `window`
//! whether every row it cuts is wall, a `door` whether the row it opens at
//! is. A single height cannot see the rows below the first course or the
//! gap between two, so a `window y=0` would cut the floor slab, a window
//! between courses would hang glass in air, and a door under walls that
//! start a storey up would carve air over air, all silently.
//!
//! [`WallColumn`] keeps the set as sorted inclusive spans with overlapping
//! *and adjacent* runs merged, so a level-on-level tower reads as one wall:
//! `walls height=5` plus `level y=5 walls height=4` is `1..=9`, and a window
//! or doorway across the seam cuts masonry the whole way.

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
    /// course, not merely inside the union: a rectangle crossing a gap is
    /// two cuts with a hole between them.
    ///
    /// `height = 0`, which source cannot write (`size=` is `NonZeroU32`),
    /// vacuously fits. Rows past `u32::MAX` never fit, rather than
    /// wrapping back to row 0.
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
    /// A door opening at `y` is capped by this, not by the tallest course.
    /// [`Self::contains_rows`] is built on it, so doors and windows agree
    /// on where the masonry is.
    pub(super) fn course_top_at(&self, y: u32) -> Option<u32> {
        self.spans
            .iter()
            .find(|(start, end)| *start <= y && y <= *end)
            .map(|(_, end)| *end)
    }
}

impl fmt::Display for WallColumn {
    /// `y=1..=3`, or `y=1..=2, y=7..=8` across a gap, as deferral messages
    /// quote it. An empty column renders as `none`, which no caller prints:
    /// each checks `is_empty()` and says "no walls" itself.
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
