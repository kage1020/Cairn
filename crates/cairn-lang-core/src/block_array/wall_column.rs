//! The rows a struct's `walls` members hold: `walls height=H` under `level y=N` fills
//! `N + 1 ..= N + H`, and courses that touch merge into one.

use std::fmt;

/// The rows a struct's walls occupy, as sorted inclusive spans with no two touching.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct WallColumn {
    spans: Vec<(u32, u32)>,
}

impl WallColumn {
    /// Build the column from `(level y, height)` pairs, one per `walls` member.
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

    /// Whether the column holds no row.
    pub(super) fn is_empty(&self) -> bool {
        self.spans.is_empty()
    }

    /// Whether the `height` rows from `y_start` all lie inside one course, not merely the union.
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

    /// The last row of the course that holds `y`, or `None` when no course does.
    pub(super) fn course_top_at(&self, y: u32) -> Option<u32> {
        self.spans
            .iter()
            .find(|(start, end)| *start <= y && y <= *end)
            .map(|(_, end)| *end)
    }
}

impl fmt::Display for WallColumn {
    /// `y=1..=3`, or `y=1..=2, y=7..=8` across a gap, as findings quote it.
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
