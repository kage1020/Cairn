//! Ordering Minecraft versions by `DataVersion`, per edition, to answer whether a target meets an
//! `@requires` floor (`spec/versioning-editions` "The target is a compile-time parameter").
//!
//! A label inside a table's span that names no row, such as Java's `1.21.4` against Bedrock's
//! table, is [`FloorPlacement::Unplaceable`] rather than ordered by its text.

use super::requirement::{compare_versions, is_dotted_decimal};

/// One edition's `(version label, DataVersion)` table, ascending by key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionOrder {
    rows: Vec<VersionRow>,
}

/// One row of a [`VersionOrder`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct VersionRow {
    /// Human-facing label, e.g. `"1.21.4"`.
    label: String,
    /// Java's `DataVersion` or Bedrock's block-palette `version`; compared only within one table.
    key: i64,
}

/// Where a floor's label sits in a [`VersionOrder`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloorPlacement {
    /// The label names a row, or the pre-release of one. Holds that row's key.
    At(i64),
    /// The label sits below every row the table carries.
    BelowEvery,
    /// The label sits above every row the table carries.
    AboveEvery,
    /// The label names no row, and no comparison this module trusts puts it outside the rows.
    Unplaceable,
}

impl FloorPlacement {
    /// Whether the table names the label: a row, or the pre-release of one.
    #[must_use]
    pub const fn names_release(self) -> bool {
        match self {
            Self::At(_) => true,
            Self::BelowEvery | Self::AboveEvery | Self::Unplaceable => false,
        }
    }
}

/// What one floor says about one target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloorVerdict {
    /// The target is at or above the floor.
    Satisfied,
    /// The target is below the floor.
    Below,
    /// The floor cannot be placed in this edition's table, so no higher `--target` meets it.
    Unplaceable,
}

impl VersionOrder {
    /// Build an ordering from a pack's `(label, key)` rows, in any order. The answers hold only for
    /// distinct labels and keys, with the labels' text order matching the key order.
    #[must_use]
    pub fn new(rows: impl IntoIterator<Item = (String, i64)>) -> Self {
        let mut rows: Vec<VersionRow> = rows
            .into_iter()
            .map(|(label, key)| VersionRow { label, key })
            .collect();
        rows.sort_by_key(|row| row.key);
        Self { rows }
    }

    /// The table's `(label, key)` rows, ascending by key.
    pub fn rows(&self) -> impl Iterator<Item = (&str, i64)> {
        self.rows.iter().map(|row| (row.label.as_str(), row.key))
    }

    /// The key a label names, matched by [`compare_versions`] so `1.020` finds `1.20`.
    #[must_use]
    pub fn key_of(&self, label: &str) -> Option<i64> {
        self.rows
            .iter()
            .find(|row| compare_versions(&row.label, label).is_eq())
            .map(|row| row.key)
    }

    /// Place a floor's label against the table.
    #[must_use]
    pub fn place(&self, label: &str) -> FloorPlacement {
        if let Some(key) = self.key_of(label) {
            return FloorPlacement::At(key);
        }
        let (Some(lowest), Some(highest)) = (self.rows.first(), self.rows.last()) else {
            return FloorPlacement::Unplaceable;
        };
        let (core, pre_release) = match label.split_once('-') {
            Some((core, _)) => (core, true),
            None => (label, false),
        };
        // A pre-release answers as its release: Minecraft ships nothing between them.
        if pre_release && let Some(key) = self.key_of(core) {
            return FloorPlacement::At(key);
        }
        if !is_dotted_decimal(core) {
            return FloorPlacement::Unplaceable;
        }
        if is_dotted_decimal(&lowest.label) && compare_versions(core, &lowest.label).is_lt() {
            return FloorPlacement::BelowEvery;
        }
        if is_dotted_decimal(&highest.label) && compare_versions(core, &highest.label).is_gt() {
            return FloorPlacement::AboveEvery;
        }
        FloorPlacement::Unplaceable
    }

    /// The rows just below and above a label, `None` past an end; `(None, None)` for a label that
    /// is not a dotted decimal or a pre-release of one.
    #[must_use]
    pub fn neighbours(&self, label: &str) -> (Option<&str>, Option<&str>) {
        if !is_dotted_decimal(label.split_once('-').map_or(label, |(core, _)| core)) {
            return (None, None);
        }
        let below = self
            .rows
            .iter()
            .rev()
            .find(|row| compare_versions(&row.label, label).is_lt())
            .map(|row| row.label.as_str());
        let above = self
            .rows
            .iter()
            .find(|row| compare_versions(&row.label, label).is_gt())
            .map(|row| row.label.as_str());
        (below, above)
    }

    /// Weigh a floor against a target's key.
    #[must_use]
    pub fn verdict(&self, floor: &str, target_key: i64) -> FloorVerdict {
        match self.place(floor) {
            FloorPlacement::At(key) => {
                if target_key >= key {
                    FloorVerdict::Satisfied
                } else {
                    FloorVerdict::Below
                }
            }
            FloorPlacement::BelowEvery => FloorVerdict::Satisfied,
            FloorPlacement::AboveEvery => FloorVerdict::Below,
            FloorPlacement::Unplaceable => FloorVerdict::Unplaceable,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn java() -> VersionOrder {
        VersionOrder::new([
            ("1.20.4".to_owned(), 3700),
            ("1.21".to_owned(), 3953),
            ("1.21.4".to_owned(), 4189),
        ])
    }

    fn satisfying<'a>(order: &'a VersionOrder, floor: &str) -> Vec<&'a str> {
        order
            .rows()
            .filter(|&(_, key)| order.verdict(floor, key) == FloorVerdict::Satisfied)
            .map(|(label, _)| label)
            .collect()
    }

    fn bedrock() -> VersionOrder {
        VersionOrder::new([
            ("1.21.0".to_owned(), 18_153_472),
            ("1.21.40".to_owned(), 18_163_712),
            ("1.21.60".to_owned(), 18_168_865),
        ])
    }

    #[test]
    fn a_label_naming_a_row_is_that_row() {
        assert_eq!(java().place("1.21"), FloorPlacement::At(3953));
        assert_eq!(java().verdict("1.21", 3953), FloorVerdict::Satisfied);
        assert_eq!(java().verdict("1.21", 4189), FloorVerdict::Satisfied);
        assert_eq!(java().verdict("1.21", 3700), FloorVerdict::Below);
    }

    #[test]
    fn the_key_decides_when_the_labels_disagree_with_it() {
        let order = VersionOrder::new([("1.21.4".to_owned(), 4189), ("1.9".to_owned(), 5000)]);
        assert!(
            compare_versions("1.9", "1.21.4").is_lt(),
            "the premise: the labels say the newer release is the lower one",
        );
        assert_eq!(order.verdict("1.21.4", 5000), FloorVerdict::Satisfied);
        assert_eq!(
            satisfying(&order, "1.21.4"),
            vec!["1.21.4", "1.9"],
            "and the rows come back in key order, not label order",
        );
        assert_eq!(order.verdict("1.9", 4189), FloorVerdict::Below);
    }

    #[test]
    fn a_floor_below_every_row_is_met_by_every_target() {
        assert_eq!(java().place("1.19"), FloorPlacement::BelowEvery);
        assert_eq!(java().verdict("1.19", 3700), FloorVerdict::Satisfied);
        assert_eq!(
            satisfying(&java(), "1.19"),
            vec!["1.20.4", "1.21", "1.21.4"],
        );
    }

    #[test]
    fn a_floor_above_every_row_is_met_by_no_target() {
        assert_eq!(java().place("99.0"), FloorPlacement::AboveEvery);
        assert_eq!(java().verdict("99.0", 4189), FloorVerdict::Below);
        assert!(satisfying(&java(), "99.0").is_empty());
    }

    #[test]
    fn a_java_shaped_floor_cannot_be_placed_in_bedrock_numbering() {
        assert_eq!(bedrock().place("1.21.4"), FloorPlacement::Unplaceable);
        assert_eq!(
            bedrock().verdict("1.21.4", 18_163_712),
            FloorVerdict::Unplaceable,
        );
        assert!(
            compare_versions("1.21.40", "1.21.4").is_gt(),
            "the premise: the labels alone say the Bedrock release is above the floor",
        );
    }

    #[test]
    fn a_pre_release_of_a_known_release_places_at_it() {
        assert_eq!(java().place("1.21.4-rc1"), FloorPlacement::At(4189));
        assert_eq!(java().verdict("1.21.4-rc1", 4189), FloorVerdict::Satisfied);
        assert_eq!(java().verdict("1.21.4-rc1", 3953), FloorVerdict::Below);
        assert_eq!(java().place("1.19-rc1"), FloorPlacement::BelowEvery);
        assert_eq!(java().place("99.0-rc1"), FloorPlacement::AboveEvery);
        assert_eq!(bedrock().place("1.21.4-rc1"), FloorPlacement::Unplaceable);
    }

    #[test]
    fn only_a_row_or_its_pre_release_names_a_release() {
        assert!(java().place("1.21").names_release(), "a row");
        assert!(
            java().place("1.21.4-rc1").names_release(),
            "a row's pre-release"
        );
        assert!(bedrock().place("1.21.40").names_release(), "a row");
        assert_eq!(java().place("1.19"), FloorPlacement::BelowEvery);
        assert!(!java().place("1.19").names_release(), "below every row");
        assert_eq!(java().place("99.0"), FloorPlacement::AboveEvery);
        assert!(!java().place("99.0").names_release(), "above every row");
        assert_eq!(bedrock().place("1.21.4"), FloorPlacement::Unplaceable);
        assert!(
            !bedrock().place("1.21.4").names_release(),
            "inside, naming no row"
        );
    }

    #[test]
    fn a_row_is_found_through_its_trailing_zeros() {
        assert_eq!(bedrock().place("1.21"), FloorPlacement::At(18_153_472));
        assert_eq!(
            bedrock().verdict("1.21", 18_153_472),
            FloorVerdict::Satisfied,
        );
        assert_eq!(java().place("1.21.0"), FloorPlacement::At(3953));
        assert_eq!(bedrock().place("1.21.4"), FloorPlacement::Unplaceable);
    }

    #[test]
    fn a_label_in_another_scheme_is_not_placed_by_its_text() {
        assert_eq!(java().place("24w14a"), FloorPlacement::Unplaceable);
        let dated = VersionOrder::new([("26w01a".to_owned(), 4500), ("26w02a".to_owned(), 4510)]);
        assert_eq!(dated.place("1.21.4"), FloorPlacement::Unplaceable);
        assert_eq!(dated.place("26w02a"), FloorPlacement::At(4510));
    }

    #[test]
    fn rows_are_ordered_by_key_whatever_order_they_arrive_in() {
        let shuffled =
            VersionOrder::new([("1.21.4".to_owned(), 4189), ("1.20.4".to_owned(), 3700)]);
        assert_eq!(
            shuffled.rows().map(|(label, _)| label).collect::<Vec<_>>(),
            ["1.20.4", "1.21.4"],
        );
        assert_eq!(shuffled.place("1.19"), FloorPlacement::BelowEvery);
    }

    #[test]
    fn an_empty_table_places_nothing() {
        let empty = VersionOrder::new([]);
        assert_eq!(empty.place("1.21"), FloorPlacement::Unplaceable);
        assert!(satisfying(&empty, "1.21").is_empty());
    }
}
