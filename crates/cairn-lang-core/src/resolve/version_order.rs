//! Ordering Minecraft versions by `DataVersion`, per edition.
//!
//! `spec/versioning-editions.md` §10.1 makes `DataVersion` — the
//! monotonically increasing integer Mojang assigns — the canonical
//! ordering key, precisely so ordering survives the move from semver-ish
//! (`1.21.4`) to date-based labels. This module is that key, applied to
//! the one question `@requires` asks: does a target sit at or above the
//! floor a source declares?
//!
//! The table it orders against ships in the registry pack
//! (`registry-data/{java,bedrock}/data_versions.json`) and is handed in by
//! the caller, because `core` does not depend on `cairn-lang-formats` —
//! the same arrangement the per-edition portability counts use.
//!
//! # Why a floor needs more than a lookup
//!
//! The table names every *release* of its edition — which is not the same
//! set as the versions the pack can build for, and is the set that matters
//! here: it is what lets "inside the table's span, naming no row" mean
//! "not a release of this edition" rather than "not one of the three the
//! pack ships block data for". A floor may still name something no table
//! carries — a snapshot, a version newer than the pack, a label that is
//! not a version at all — so placing one is more than a lookup.
//!
//! What resolves that is a distinction between the questions a table
//! *cannot* be wrong about and the ones it can:
//!
//! - A label naming a row is that row's key. Exact.
//! - A label naming a *pre-release* of a row sits directly below it, and
//!   no release ships between the two, so against a table of releases it
//!   answers the same as the row it precedes.
//! - A label below every row is satisfied by every target, and one above
//!   every row by none. That answer is reached by comparing the floor's
//!   label against the first and last rows' *labels*, while "first" and
//!   "last" are decided by their *keys* — so it holds exactly when the
//!   table's labels sort the same way by text as by key. That is a real
//!   condition rather than a restatement of "one numbering scheme": two
//!   dotted decimals whose keys ran the other way would break it. The
//!   registry pack loader checks it at load time
//!   (`validate_version_order`), so every table that reaches this module
//!   satisfies it. The label itself still has to be a dotted decimal
//!   ([`is_dotted_decimal`]) to be compared at all.
//! - Anything else — a label inside the table's span that names no row —
//!   has no key and cannot be given one. It is [`FloorPlacement::Unplaceable`],
//!   and the caller refuses the build rather than guessing.
//!
//! That last case is not a rare corner. It is where the cross-edition
//! defect lives: `@requires version>=1.21.4` against Bedrock. Bedrock
//! numbers its patch releases in tens — `1.21.0`, `1.21.20`, `1.21.40` —
//! so Java's `1.21.4` names no Bedrock release, sits between two that
//! exist, and the dotted-decimal comparison this replaced read it as
//! satisfied by `1.21.40` on `40 > 4`, certifying a build against a
//! version below the floor. Here it is unplaceable, and the repair is the
//! edition scope the directive now accepts.
//!
//! The two editions' release-label sets are disjoint, which is what makes
//! that answer available: no label names a release of both, so a label
//! this edition's table cannot place and the other's can is a floor
//! written in the other's numbering, and the CLI says so.

use super::requires_parse::{compare_versions, is_dotted_decimal};

/// One edition's `(version label, DataVersion)` table, as an ordering.
///
/// Holds rows in ascending key order, which is the order §10.1 defines and
/// not necessarily the order the JSON file lists them in. Every question
/// below is answered from the keys; the labels are consulted only for the
/// exact lookup and for the two boundary comparisons the module doc names.
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
    /// The edition's version integer: Java's `DataVersion`, Bedrock's
    /// block-palette `version`. Only ever compared within one edition's
    /// table, which is what lets two different meanings share the column.
    key: i64,
}

/// Where a floor's label sits in a [`VersionOrder`].
///
/// Four answers rather than an `Option<i64>`, because "below every row"
/// and "above every row" are answers — every target satisfies the floor,
/// and none does — while "cannot be placed" is the absence of one. Folding
/// the three together would leave the caller unable to tell a floor every
/// target meets from a floor nothing can be said about.
/// Not `#[non_exhaustive]`: the four answers are a closed set — a label
/// is a row, below them all, above them all, or none of those — and the
/// callers compare the value with `==`. A fifth variant would compile
/// against those callers and answer wrongly, which is the opposite of
/// what the attribute is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloorPlacement {
    /// The label names a row, or the pre-release of one. Holds that row's
    /// key.
    At(i64),
    /// The label sits below every row the table carries.
    BelowEvery,
    /// The label sits above every row the table carries.
    AboveEvery,
    /// The table cannot place the label: it names no row, and no
    /// comparison this module trusts puts it outside the rows either.
    Unplaceable,
}

/// What one floor says about one target.
///
/// Closed for the same reason as [`FloorPlacement`], of which it is a
/// total function: the target is at or above the floor, below it, or the
/// floor cannot be placed at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloorVerdict {
    /// The target is at or above the floor.
    Satisfied,
    /// The target is below the floor.
    Below,
    /// The floor cannot be placed in this edition's table, so neither
    /// answer can be given. Distinct from [`Self::Below`]: a build refused
    /// for this reason is refused because the *floor* names no version
    /// here, and telling its author to raise `--target` would send them
    /// after a version that does not exist.
    Unplaceable,
}

impl VersionOrder {
    /// Build an ordering from a pack's `(label, key)` rows.
    ///
    /// Sorted here rather than trusted from the caller: the JSON file's
    /// row order is documented as informational, and every answer below
    /// reads the first and last rows as the table's bounds.
    ///
    /// Infallible, and the answers below are only meaningful on rows that
    /// are a version table: labels distinct under [`compare_versions`],
    /// keys distinct, and — for the boundary answers — labels sorting the
    /// same way by text as by key. Those are checked where a table is
    /// *read*, by the registry pack loader, rather than here, for the same
    /// reason `RegistryPack` cannot be built without going through
    /// validation: a caller assembling rows by hand (a test, a future
    /// `--registry-pack`) gets a total function and an arbitrary answer
    /// rather than a panic, and a pack that reached this constructor
    /// passed the checks already.
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
    /// The key rides along rather than being looked up again by label,
    /// because a caller weighing several floors at once — which is what a
    /// module declaring more than one `@requires` line needs — would
    /// otherwise re-find each row it is already holding, through a lookup
    /// that has to be told it cannot miss.
    pub fn rows(&self) -> impl Iterator<Item = (&str, i64)> {
        self.rows.iter().map(|row| (row.label.as_str(), row.key))
    }

    /// The key a label names, when it names a row.
    ///
    /// Matched with [`compare_versions`] rather than by string equality,
    /// so a row is found through the differences between two spellings of
    /// one version that carry no information: its trailing zeros, and the
    /// leading zeros of any component (`1.020` finds the `1.20` row).
    /// Bedrock's earliest supported release is `1.21.0` and a floor of
    /// `1.21` is that release, not a version between two others. Every
    /// other way two labels can differ leaves them unequal under that
    /// comparison, so this stays a lookup rather than becoming the
    /// text-ordering the module doc refuses.
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
            // An empty table places nothing. Reachable only through a pack
            // whose version component is empty, which is not a pack any
            // build can pin a target in either.
            return FloorPlacement::Unplaceable;
        };
        let (core, pre_release) = match label.split_once('-') {
            Some((core, _)) => (core, true),
            None => (label, false),
        };
        // A pre-release of a known release answers as that release does.
        // The two differ only for a target between them, and Minecraft
        // ships none: `1.21.4-rc1` is the candidate *for* `1.21.4`.
        if pre_release && let Some(key) = self.key_of(core) {
            return FloorPlacement::At(key);
        }
        if !is_dotted_decimal(core) {
            return FloorPlacement::Unplaceable;
        }
        // Below the lowest row: `core < lowest`, and a pre-release of
        // `core` is lower still. `core == lowest` cannot reach here, since
        // both lookups above would have caught it.
        if is_dotted_decimal(&lowest.label) && compare_versions(core, &lowest.label).is_lt() {
            return FloorPlacement::BelowEvery;
        }
        // Above the highest row: a pre-release of a `core` above every row
        // is above them too, since the release it precedes is the next one
        // to ship and the table's newest row is older than that.
        if is_dotted_decimal(&highest.label) && compare_versions(core, &highest.label).is_gt() {
            return FloorPlacement::AboveEvery;
        }
        FloorPlacement::Unplaceable
    }

    /// The rows a label falls between, when it falls between two.
    ///
    /// `(None, _)` when nothing the table carries is below it, `(_, None)`
    /// when nothing is above — so a caller can tell "between two releases"
    /// from "off one end", which are different things to say to an author.
    /// Labels this comparison cannot place against get `(None, None)`.
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
    /// The target is given as its key rather than as a label because the
    /// callers hold one already: one looked the target up in this table,
    /// the other took it from the resolved target the backend will stamp,
    /// which is the same integer for the same version. Taking a label back
    /// would mean looking up something already in hand, through a lookup
    /// that would then need an answer for a target that does not resolve —
    /// a state no caller has.
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

    /// The Java rows the built-in pack ships, so the unit tests here and
    /// the CLI's behaviour are talking about the same table.
    fn java() -> VersionOrder {
        VersionOrder::new([
            ("1.20.4".to_owned(), 3700),
            ("1.21".to_owned(), 3953),
            ("1.21.4".to_owned(), 4189),
        ])
    }

    /// The labels one floor admits, which is what a refusal offers as the
    /// closed set of targets that would work. The caller doing this for
    /// real folds several floors into one such list.
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
    /// A table whose label order disagrees with its key order is the only
    /// shape that can tell the two conventions apart: on every table Cairn
    /// ships the two agree, so a test built on those rows would pass under
    /// either. It is not a table the loader would accept — that is what
    /// `validate_version_order` refuses — which is exactly why the
    /// distinction has to be made here, where a `VersionOrder` can be
    /// built directly.
    ///
    /// Here `1.9` is the *newer* release and `1.21.4` the older one.
    /// Component-wise over dotted decimals, `1.21.4 > 1.9` and a target of
    /// `1.9` is below the floor. By `DataVersion` it is above it. Every
    /// assertion below goes through an exact row, so what it pins is the
    /// lookup and the comparison of keys — the boundary paths are the
    /// neighbouring tests.
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
        // And the older one is still below the floor, so the key is being
        // read rather than the comparison merely being skipped.
        assert_eq!(order.verdict("1.9", 4189), FloorVerdict::Below);
    }

    /// A floor below everything the table names is satisfied by
    /// everything. `version>=1.19` is an ordinary line and no table Cairn
    /// ships carries it.
    #[test]
    fn a_floor_below_every_row_is_met_by_every_target() {
        assert_eq!(java().place("1.19"), FloorPlacement::BelowEvery);
        assert_eq!(java().verdict("1.19", 3700), FloorVerdict::Satisfied);
        assert_eq!(
            satisfying(&java(), "1.19"),
            vec!["1.20.4", "1.21", "1.21.4"],
        );
    }

    /// And one above everything is met by nothing, which is a refusal that
    /// can name the whole supported set rather than a target to try.
    #[test]
    fn a_floor_above_every_row_is_met_by_no_target() {
        assert_eq!(java().place("99.0"), FloorPlacement::AboveEvery);
        assert_eq!(java().verdict("99.0", 4189), FloorVerdict::Below);
        assert!(satisfying(&java(), "99.0").is_empty());
    }

    /// The defect this module exists to remove. Java's newest release
    /// names no Bedrock version and sits between two that exist, so it has
    /// no key and cannot be given one — where the dotted-decimal
    /// comparison read `1.21.40` as satisfying it on `40 > 4`.
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

    /// Trailing zeros are padding, not a version between two others.
    /// Bedrock's earliest supported release is `1.21.0`, and
    /// `@requires version>=1.21` is that release rather than a label
    /// inside the table's span that names no row.
    #[test]
    fn a_row_is_found_through_its_trailing_zeros() {
        assert_eq!(bedrock().place("1.21"), FloorPlacement::At(18_153_472));
        assert_eq!(
            bedrock().verdict("1.21", 18_153_472),
            FloorVerdict::Satisfied,
        );
        assert_eq!(java().place("1.21.0"), FloorPlacement::At(3953));
        // And it is only zeros: `1.21.4` is still a different version.
        assert_eq!(bedrock().place("1.21.4"), FloorPlacement::Unplaceable);
    }

    /// A label in another scheme is not compared against one in this
    /// scheme, even to place it outside the rows. That comparison is the
    /// one §10.1 makes `DataVersion` canonical to avoid, and trusting it
    /// at the boundary would be trusting it everywhere the boundary moves.
    #[test]
    fn a_label_in_another_scheme_is_not_placed_by_its_text() {
        assert_eq!(java().place("24w14a"), FloorPlacement::Unplaceable);
        // Including when the rows are the ones in the other scheme.
        let dated = VersionOrder::new([("26w01a".to_owned(), 4500), ("26w02a".to_owned(), 4510)]);
        assert_eq!(dated.place("1.21.4"), FloorPlacement::Unplaceable);
        // A row it names exactly is still exact, whatever the scheme.
        assert_eq!(dated.place("26w02a"), FloorPlacement::At(4510));
    }

    /// The rows decide the bounds by key, not by the order the pack lists
    /// them in — which `data_versions.json` documents as informational.
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

    /// An empty table has no bounds to place anything against, and says so
    /// rather than reading `first()` and `last()` off nothing.
    #[test]
    fn an_empty_table_places_nothing() {
        let empty = VersionOrder::new([]);
        assert_eq!(empty.place("1.21"), FloorPlacement::Unplaceable);
        assert!(satisfying(&empty, "1.21").is_empty());
    }
}
