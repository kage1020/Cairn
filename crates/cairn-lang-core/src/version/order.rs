//! Ordering Minecraft versions by `DataVersion`, per edition.
//!
//! `spec/versioning-editions` "The target is a compile-time parameter" makes `DataVersion`, the
//! monotonically increasing integer Mojang assigns, the canonical ordering key, so ordering
//! survives the move from `1.21.4`-style to date-based labels. This module applies that key to the
//! question `@requires` asks: does a target sit at or above the floor a source declares?
//!
//! The table ships in the registry pack (`registry-data/{java,bedrock}/data_versions.json`) and is
//! handed in by the caller, because `core` does not depend on `cairn-lang-formats`.
//!
//! # Why a floor needs more than a lookup
//!
//! The table names every *release* of its edition, not only the versions the pack builds for, so
//! "inside the table's span, naming no row" means "not a release of this edition". A floor may
//! still name something no table carries (a snapshot, a version newer than the pack, a label that
//! is not a version), so the answers split by what a table cannot be wrong about:
//!
//! - A label naming a row is that row's key. Exact.
//! - A label naming a *pre-release* of a row sits directly below it, and no release ships between
//!   the two, so it answers as that row does.
//! - A label below every row is met by every target, and one above every row by none. That
//!   compares the floor's label with the first and last rows' *labels*, while first and last are
//!   decided by *keys*, so it holds only when the table's labels sort the same way by text as by
//!   key, which two dotted decimals need not. The registry pack loader checks it
//!   (`validate_version_order`). The label itself must be a dotted decimal
//!   ([`is_dotted_decimal`]) to be compared at all.
//! - Any other label inside the span names no row and has no key. It is
//!   [`FloorPlacement::Unplaceable`], and the caller refuses the build rather than guessing.
//!
//! That last case is the cross-edition one: `@requires version>=1.21.4` against Bedrock. Bedrock
//! numbers patch releases in tens (`1.21.0`, `1.21.20`, `1.21.40`), so Java's `1.21.4` names no
//! Bedrock release. A dotted-decimal comparison would call it met by `1.21.40` on `40 > 4`,
//! certifying a build below its floor; here it is unplaceable, and the repair is the directive's
//! edition scope.
//!
//! The two editions' release labels are disjoint, so a label this edition's table cannot place and
//! the other's names (a row, or a row's pre-release: [`FloorPlacement::names_release`]) is a floor
//! in the other's numbering, and the CLI says so. A label the other table only places below or
//! above every row is not a release there.

use super::requirement::{compare_versions, is_dotted_decimal};

/// One edition's `(version label, DataVersion)` table, as an ordering.
///
/// Rows are held in ascending key order, which `spec/versioning-editions` "The target is a
/// compile-time parameter" defines, not necessarily the JSON file's order. Labels are consulted
/// only for the exact lookup and the two boundary comparisons.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionOrder {
    /// Rows, ascending by [`VersionRow::key`].
    rows: Vec<VersionRow>,
}

/// One row of a [`VersionOrder`].
#[derive(Debug, Clone, PartialEq, Eq)]
struct VersionRow {
    /// Human-facing label, e.g. `"1.21.4"`.
    label: String,
    /// The edition's version integer: Java's `DataVersion`, Bedrock's block-palette `version`.
    /// Compared only within one edition's table, so the two meanings can share the column.
    key: i64,
}

/// Where a floor's label sits in a [`VersionOrder`].
///
/// Four answers rather than an `Option<i64>`, because "below every row" (every target meets the
/// floor) and "above every row" (none does) are answers, while "cannot be placed" is the absence
/// of one.
///
/// Not `#[non_exhaustive]`: the four answers are a closed set and callers compare with `==`, so a
/// fifth variant would compile against them and answer wrongly.
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
    ///
    /// [`Self::BelowEvery`] and [`Self::AboveEvery`] order the label without naming it, so here
    /// they answer as [`Self::Unplaceable`] does. A `match` rather than `matches!`, so a new
    /// variant has to pick a side.
    #[must_use]
    pub const fn names_release(self) -> bool {
        match self {
            Self::At(_) => true,
            Self::BelowEvery | Self::AboveEvery | Self::Unplaceable => false,
        }
    }
}

/// What one floor says about one target.
///
/// Closed for the same reason as [`FloorPlacement`], of which it is a total function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloorVerdict {
    /// The target is at or above the floor.
    Satisfied,
    /// The target is below the floor.
    Below,
    /// The floor cannot be placed in this edition's table. Distinct from [`Self::Below`], because
    /// telling the author to raise `--target` would send them after a version that does not exist.
    Unplaceable,
}

impl VersionOrder {
    /// Build an ordering from a pack's `(label, key)` rows.
    ///
    /// Sorted here, because the JSON file's row order is informational and every answer reads the
    /// first and last rows as the table's bounds.
    ///
    /// The answers are meaningful only on a version table: labels distinct under
    /// [`compare_versions`], keys distinct, and, for the boundary answers, labels sorting the same
    /// way by text as by key. The registry pack loader checks those where a table is read; rows
    /// assembled by hand get an arbitrary answer rather than a panic.
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
    ///
    /// The key rides along so a caller weighing several floors need not look each row up again.
    pub fn rows(&self) -> impl Iterator<Item = (&str, i64)> {
        self.rows.iter().map(|row| (row.label.as_str(), row.key))
    }

    /// The key a label names, when it names a row.
    ///
    /// Matched with [`compare_versions`] rather than string equality, so trailing zeros and a
    /// component's leading zeros carry no information: `1.020` finds `1.20`, and a floor of `1.21`
    /// is Bedrock's `1.21.0`. Any other difference leaves two labels unequal, so this stays a
    /// lookup rather than a text ordering.
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
            // Reachable only through a pack with no versions, which no build can target either.
            return FloorPlacement::Unplaceable;
        };
        let (core, pre_release) = match label.split_once('-') {
            Some((core, _)) => (core, true),
            None => (label, false),
        };
        // A pre-release answers as its release: Minecraft ships nothing between them
        // (`1.21.4-rc1` is the candidate *for* `1.21.4`).
        if pre_release && let Some(key) = self.key_of(core) {
            return FloorPlacement::At(key);
        }
        if !is_dotted_decimal(core) {
            return FloorPlacement::Unplaceable;
        }
        // A pre-release of `core` is lower still. The lookups above caught `core == lowest`.
        if is_dotted_decimal(&lowest.label) && compare_versions(core, &lowest.label).is_lt() {
            return FloorPlacement::BelowEvery;
        }
        // A pre-release of a `core` above every row is above them too: the table's newest row is
        // older than the release it precedes.
        if is_dotted_decimal(&highest.label) && compare_versions(core, &highest.label).is_gt() {
            return FloorPlacement::AboveEvery;
        }
        FloorPlacement::Unplaceable
    }

    /// The rows a label falls between, when it falls between two.
    ///
    /// `(None, _)` when nothing the table carries is below it, `(_, None)` when nothing is above,
    /// so a caller can tell "between two releases" from "off one end". A label that is not a
    /// dotted decimal, or a pre-release of one, gets `(None, None)`.
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
    ///
    /// Takes the key rather than a label because every caller already holds it, and a lookup
    /// would need an answer for a target that does not resolve.
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

    /// Three rows of the built-in pack's Java table, with their real keys.
    fn java() -> VersionOrder {
        VersionOrder::new([
            ("1.20.4".to_owned(), 3700),
            ("1.21".to_owned(), 3953),
            ("1.21.4".to_owned(), 4189),
        ])
    }

    /// The labels one floor admits, which a refusal offers as the targets that would work.
    fn satisfying<'a>(order: &'a VersionOrder, floor: &str) -> Vec<&'a str> {
        order
            .rows()
            .filter(|&(_, key)| order.verdict(floor, key) == FloorVerdict::Satisfied)
            .map(|(label, _)| label)
            .collect()
    }

    /// And Bedrock's, whose numbering is the reason the edition scope
    /// exists.
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

    /// **The ordering key is the `DataVersion`, not the label.**
    ///
    /// Only a table whose label order disagrees with its key order tells the two apart, and the
    /// loader refuses one (`validate_version_order`), so it is built directly here. `1.9` is the
    /// newer release: component-wise `1.21.4 > 1.9`, by `DataVersion` the reverse. Every assertion
    /// goes through an exact row; the boundary paths are the neighbouring tests.
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
        // The older one is still below, so the keys are compared rather than skipped.
        assert_eq!(order.verdict("1.9", 4189), FloorVerdict::Below);
    }

    /// A floor below everything the table names is met by every target.
    #[test]
    fn a_floor_below_every_row_is_met_by_every_target() {
        assert_eq!(java().place("1.19"), FloorPlacement::BelowEvery);
        assert_eq!(java().verdict("1.19", 3700), FloorVerdict::Satisfied);
        assert_eq!(
            satisfying(&java(), "1.19"),
            vec!["1.20.4", "1.21", "1.21.4"],
        );
    }

    /// A floor above everything is met by no target.
    #[test]
    fn a_floor_above_every_row_is_met_by_no_target() {
        assert_eq!(java().place("99.0"), FloorPlacement::AboveEvery);
        assert_eq!(java().verdict("99.0", 4189), FloorVerdict::Below);
        assert!(satisfying(&java(), "99.0").is_empty());
    }

    /// Java's `1.21.4` names no Bedrock release and sits between two that do, so it has no key,
    /// though by labels alone `1.21.40` would meet it on `40 > 4`.
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

    /// A pre-release is the candidate for the release it names, and
    /// nothing ships between them, so it answers as that release does.
    #[test]
    fn a_pre_release_of_a_known_release_places_at_it() {
        assert_eq!(java().place("1.21.4-rc1"), FloorPlacement::At(4189));
        assert_eq!(java().verdict("1.21.4-rc1", 4189), FloorVerdict::Satisfied);
        assert_eq!(java().verdict("1.21.4-rc1", 3953), FloorVerdict::Below);
        // A pre-release of an unknown release is placed by its release.
        assert_eq!(java().place("1.19-rc1"), FloorPlacement::BelowEvery);
        assert_eq!(java().place("99.0-rc1"), FloorPlacement::AboveEvery);
        assert_eq!(bedrock().place("1.21.4-rc1"), FloorPlacement::Unplaceable);
    }

    /// Naming a release is `At` and nothing else: a label ordered below or above every row is a
    /// comparison, not a release. Built on hand-made tables so a pack refresh cannot move it.
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

    /// Trailing zeros are padding: on Bedrock, `@requires version>=1.21` is the `1.21.0` row.
    #[test]
    fn a_row_is_found_through_its_trailing_zeros() {
        assert_eq!(bedrock().place("1.21"), FloorPlacement::At(18_153_472));
        assert_eq!(
            bedrock().verdict("1.21", 18_153_472),
            FloorVerdict::Satisfied,
        );
        assert_eq!(java().place("1.21.0"), FloorPlacement::At(3953));
        // Only zeros: `1.21.4` is still a different version.
        assert_eq!(bedrock().place("1.21.4"), FloorPlacement::Unplaceable);
    }

    /// A label in another scheme is not compared with this one's, even to place it outside the
    /// rows: `spec/versioning-editions` "The target is a compile-time parameter" makes
    /// `DataVersion` canonical to avoid that comparison.
    #[test]
    fn a_label_in_another_scheme_is_not_placed_by_its_text() {
        assert_eq!(java().place("24w14a"), FloorPlacement::Unplaceable);
        // Nor when the rows are the ones in the other scheme.
        let dated = VersionOrder::new([("26w01a".to_owned(), 4500), ("26w02a".to_owned(), 4510)]);
        assert_eq!(dated.place("1.21.4"), FloorPlacement::Unplaceable);
        // A row it names exactly is still exact, whatever the scheme.
        assert_eq!(dated.place("26w02a"), FloorPlacement::At(4510));
    }

    /// The bounds are decided by key, not by the pack's informational row order.
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

    /// An empty table has no bounds, so it places nothing.
    #[test]
    fn an_empty_table_places_nothing() {
        let empty = VersionOrder::new([]);
        assert_eq!(empty.place("1.21"), FloorPlacement::Unplaceable);
        assert!(satisfying(&empty, "1.21").is_empty());
    }
}
