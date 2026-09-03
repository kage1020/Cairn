//! Reading the `@cairn` header's `CalVer` value.
//!
//! Cairn versions itself by date: `YYYY.M[.PATCH]`, the scheme
//! `CHANGELOG.md` names and [`crate::CAIRN_VERSION`] carries. A `.crn` file
//! MAY declare the language version it was written against
//! (`spec/syntax.md` §5.3), and that declaration is provenance — nothing
//! branches on it, and the artifact is the same whatever it says.
//!
//! Provenance still has to be readable. `spec/index.md` gives the header
//! its job — "so a future compiler can parse and warn correctly" — and a
//! compiler handed `@cairn banana` has nothing to branch on when that
//! future arrives. This module is where the value stops being a string.
//!
//! **Stricter than [`crate::resolve::parse_requirement`], deliberately.**
//! That one reads a Minecraft version label, where the label space is
//! Mojang's and Cairn only has to order it; a component is any decimal
//! number. Here the space is Cairn's own, so `2026.13` is a month that
//! does not exist and `1.2` is not a year — refusing both is what makes
//! this a `CalVer` rather than any dotted number, and each refusal names
//! one component with an obvious repair.
//!
//! **Two spellings of the month, both accepted.** `spec/index.md` and
//! every shipped `.crn` write `2026.06`, which is calver.org's `YYYY.0M`;
//! [`crate::CAIRN_VERSION`] is `YYYY.M`, because Cargo's `version` field is
//! semver and semver forbids a leading zero. Both are in this repository
//! today, so both are read, and the comparison is numeric — `2026.06` and
//! `2026.6` are one version, not two.

use std::fmt;

/// Lowest month a `CalVer` names.
const FIRST_MONTH: u32 = 1;
/// Highest month a `CalVer` names.
const LAST_MONTH: u32 = 12;
/// Digits in the year component. Fixed rather than a range because the
/// point of the check is to separate a year from any other number: `1.2`
/// is a semver, and reading it as the year 1 would let a file declare a
/// language that predates the calendar Cairn versions by.
const YEAR_DIGITS: usize = 4;

/// A Cairn language version, parsed from a `YYYY.M[.PATCH]` string.
///
/// Field order is the comparison order, which is what the derived [`Ord`]
/// rests on: a later year wins whatever the month says, and a later month
/// wins whatever the patch says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LanguageVersion {
    /// Four-digit calendar year.
    pub year: u32,
    /// Calendar month, `1..=12`. Numeric, so `06` and `6` are one value.
    pub month: u32,
    /// Patch within the month. `0` when the source wrote none, which is
    /// what makes `2026.9` the earliest release of that month rather than
    /// a version that cannot be ordered against `2026.9.2`.
    pub patch: u32,
}

impl LanguageVersion {
    /// Build a version from its three components.
    ///
    /// No validation: this is the constructor a caller uses when the
    /// numbers came from somewhere other than a `@cairn` line — a test
    /// pinning a comparison, or a future consumer holding a version it
    /// derived. [`parse_language_version`] is the one that judges a string.
    #[must_use]
    pub const fn new(year: u32, month: u32, patch: u32) -> Self {
        Self { year, month, patch }
    }

    /// Whether this version is later than `other`.
    ///
    /// Named rather than left to `>` at the call site because the question
    /// the pass asks is directional — "is the file's language newer than
    /// this compiler's" — and reading that off an operator means reading
    /// the argument order right every time.
    #[must_use]
    pub fn is_newer_than(&self, other: &Self) -> bool {
        self > other
    }
}

impl fmt::Display for LanguageVersion {
    /// `2026.9` or `2026.9.2` — the patch is elided when it is zero, so a
    /// version round-trips to the spelling that produced it.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self { year, month, patch } = self;
        if *patch == 0 {
            write!(f, "{year}.{month}")
        } else {
            write!(f, "{year}.{month}.{patch}")
        }
    }
}

/// Why a string is not a Cairn language version.
///
/// Carried out of [`parse_language_version`] rather than collapsed into
/// `None` for the reason [`crate::resolve::RequirementError`] is: the
/// repairs differ. A month of `13` is one keystroke; a four-component
/// string is a line to shorten; a year of `20261` is a typo in a place a
/// reader's eye slides over.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LanguageVersionError {
    /// The string is not two or three dot-separated components. Holds how
    /// many there were.
    ComponentCount {
        /// Number of dot-separated components found.
        found: usize,
    },
    /// A component that is not a run of decimal digits: empty, or holding
    /// something else. Holds the component as written.
    Component {
        /// The dot-separated component that could not be read.
        component: String,
    },
    /// A year that is not exactly [`YEAR_DIGITS`] digits. Holds it as
    /// written.
    Year {
        /// The year component as written.
        component: String,
    },
    /// A month outside `1..=12`. Holds it as written, so `013` reads back
    /// the way the author typed it.
    Month {
        /// The month component as written.
        component: String,
    },
    /// A patch of digits that does not fit in `u32`, which is what the
    /// comparison orders by. Holds it as written.
    PatchTooLarge {
        /// The patch component as written.
        component: String,
    },
}

impl LanguageVersionError {
    /// Stable machine-readable name for this failure, for a consumer
    /// choosing a quick-fix without reading the prose.
    ///
    /// Kept beside the variants rather than derived from them, because
    /// [`crate::check::DiagnosticData`] carries these strings onto the wire
    /// and a rename there is a break for anything matching on them.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ComponentCount { .. } => "component_count",
            Self::Component { .. } => "component_not_a_number",
            Self::Year { .. } => "year_not_four_digits",
            Self::Month { .. } => "month_out_of_range",
            Self::PatchTooLarge { .. } => "patch_too_large",
        }
    }

    /// The fragment of the value the failure is about, for a consumer
    /// building a quick-fix over it.
    ///
    /// Empty for [`Self::ComponentCount`], which is about the whole string
    /// rather than about a part of it — inventing a substring there would
    /// give a tool something to replace that the author never wrote.
    #[must_use]
    pub fn offending_text(&self) -> &str {
        match self {
            Self::ComponentCount { .. } => "",
            Self::Component { component }
            | Self::Year { component }
            | Self::Month { component }
            | Self::PatchTooLarge { component } => component,
        }
    }
}

impl fmt::Display for LanguageVersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ComponentCount { found } => write!(
                f,
                "expected two or three dot-separated components (`YYYY.M` or `YYYY.M.PATCH`), found {found}",
            ),
            Self::Component { component } => {
                write!(f, "`{component}` is not a run of decimal digits")
            }
            Self::Year { component } => write!(f, "the year `{component}` is not four digits"),
            Self::Month { component } => write!(
                f,
                "the month `{component}` is not between {FIRST_MONTH} and {LAST_MONTH}",
            ),
            Self::PatchTooLarge { component } => write!(
                f,
                "the patch `{component}` is too large to order (the ceiling is {})",
                u32::MAX,
            ),
        }
    }
}

impl std::error::Error for LanguageVersionError {}

/// Read a `YYYY.M[.PATCH]` string as a [`LanguageVersion`].
///
/// The whole string has to be the version: no surrounding whitespace is
/// trimmed here, because the one caller reads a header value the parser
/// has already trimmed, and trimming twice would accept `@cairn " 2026.6"`
/// written as a quoted string somewhere a later reader does not expect it.
///
/// # Errors
///
/// [`LanguageVersionError`], naming the component that could not be read.
pub fn parse_language_version(text: &str) -> Result<LanguageVersion, LanguageVersionError> {
    let parts: Vec<&str> = text.split('.').collect();
    if !matches!(parts.len(), 2 | 3) {
        return Err(LanguageVersionError::ComponentCount { found: parts.len() });
    }
    // Digits first, for every component, so the year and month rules below
    // are about numbers rather than about text. `str::parse` would accept
    // a leading `+` and a `-` sign, which are not spellings of a date.
    for part in &parts {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(LanguageVersionError::Component {
                component: (*part).to_owned(),
            });
        }
    }
    let year_text = parts[0];
    if year_text.len() != YEAR_DIGITS {
        return Err(LanguageVersionError::Year {
            component: year_text.to_owned(),
        });
    }
    // Four digits always fit in `u32`. Read fallibly anyway, so the
    // function has no panic for a reader to rule out and the ceiling is a
    // named failure rather than an `expect` message.
    let year = year_text
        .parse::<u32>()
        .map_err(|_| LanguageVersionError::Year {
            component: year_text.to_owned(),
        })?;
    let month_text = parts[1];
    // `013` and any longer run of leading zeros parse to the same month,
    // and a month that overflows `u32` is out of range for the same reason
    // `13` is — one arm, because the repair is the same.
    let month = month_text
        .parse::<u32>()
        .ok()
        .filter(|m| (FIRST_MONTH..=LAST_MONTH).contains(m))
        .ok_or_else(|| LanguageVersionError::Month {
            component: month_text.to_owned(),
        })?;
    let patch = match parts.get(2) {
        None => 0,
        Some(text) => text
            .parse::<u32>()
            .map_err(|_| LanguageVersionError::PatchTooLarge {
                component: (*text).to_owned(),
            })?,
    };
    Ok(LanguageVersion { year, month, patch })
}

#[cfg(test)]
mod tests {
    use super::{LanguageVersion, LanguageVersionError, parse_language_version};

    #[test]
    fn the_two_month_spellings_are_one_version() {
        assert_eq!(
            parse_language_version("2026.06"),
            Ok(LanguageVersion::new(2026, 6, 0)),
        );
        assert_eq!(
            parse_language_version("2026.6"),
            Ok(LanguageVersion::new(2026, 6, 0)),
        );
    }

    #[test]
    fn a_patch_is_optional_and_absent_means_zero() {
        assert_eq!(
            parse_language_version("2026.9.2"),
            Ok(LanguageVersion::new(2026, 9, 2)),
        );
        assert_eq!(
            parse_language_version("2026.9"),
            Ok(LanguageVersion::new(2026, 9, 0)),
        );
        assert!(LanguageVersion::new(2026, 9, 2).is_newer_than(&LanguageVersion::new(2026, 9, 0)),);
    }

    #[test]
    fn ordering_runs_year_then_month_then_patch() {
        let earlier = LanguageVersion::new(2025, 12, 9);
        let later = LanguageVersion::new(2026, 1, 0);
        assert!(later.is_newer_than(&earlier), "a year outranks a month");
        assert!(!earlier.is_newer_than(&later));
        assert!(
            !LanguageVersion::new(2026, 6, 0).is_newer_than(&LanguageVersion::new(2026, 6, 0)),
            "a version is not newer than itself",
        );
    }

    #[test]
    fn the_count_is_judged_before_the_components() {
        // A one-component string names no month to be out of range, and a
        // four-component one has a valid year and month in front of the
        // problem — so the count has to be the first thing read.
        assert_eq!(
            parse_language_version("banana"),
            Err(LanguageVersionError::ComponentCount { found: 1 }),
        );
        assert_eq!(
            parse_language_version("2026.06.1.2"),
            Err(LanguageVersionError::ComponentCount { found: 4 }),
        );
    }

    #[test]
    fn a_component_that_is_not_digits_is_named() {
        for (text, component) in [
            ("2026.", ""),
            (".6", ""),
            ("2026.6.x", "x"),
            ("2026.-6", "-6"),
            ("2026.+6", "+6"),
            ("2026.6.1_0", "1_0"),
        ] {
            assert_eq!(
                parse_language_version(text),
                Err(LanguageVersionError::Component {
                    component: component.to_owned(),
                }),
                "{text:?}",
            );
        }
    }

    #[test]
    fn a_year_is_exactly_four_digits() {
        for text in ["1.2", "202.6", "20261.6", "0.1"] {
            assert!(
                matches!(
                    parse_language_version(text),
                    Err(LanguageVersionError::Year { .. }),
                ),
                "{text:?} should not name a year: {:?}",
                parse_language_version(text),
            );
        }
        assert!(
            parse_language_version("0001.1").is_ok(),
            "four digits is four digits"
        );
    }

    #[test]
    fn a_month_outside_the_calendar_is_named() {
        for text in ["2026.0", "2026.13", "2026.99", "2026.99999999999999999999"] {
            assert!(
                matches!(
                    parse_language_version(text),
                    Err(LanguageVersionError::Month { .. }),
                ),
                "{text:?} should not name a month: {:?}",
                parse_language_version(text),
            );
        }
        assert!(
            parse_language_version("2026.012").is_ok(),
            "`012` is December"
        );
    }

    #[test]
    fn a_patch_past_the_ordering_ceiling_is_named() {
        assert_eq!(
            parse_language_version("2026.6.99999999999999999999"),
            Err(LanguageVersionError::PatchTooLarge {
                component: "99999999999999999999".to_owned(),
            }),
        );
        assert!(parse_language_version(&format!("2026.6.{}", u32::MAX)).is_ok());
    }

    #[test]
    fn every_failure_names_itself_and_the_text_it_is_about() {
        // The payload a consumer reads. `kind` is on the wire, so it is
        // pinned here rather than left to the one call site.
        for (text, kind, found) in [
            ("banana", "component_count", ""),
            ("2026.6.x", "component_not_a_number", "x"),
            ("20261.6", "year_not_four_digits", "20261"),
            ("2026.13", "month_out_of_range", "13"),
            (
                "2026.6.99999999999999999999",
                "patch_too_large",
                "99999999999999999999",
            ),
        ] {
            let error = parse_language_version(text).expect_err("not a version");
            assert_eq!(error.kind(), kind, "{text:?}");
            assert_eq!(error.offending_text(), found, "{text:?}");
        }
    }

    #[test]
    fn a_version_renders_as_the_spelling_that_names_it() {
        assert_eq!(LanguageVersion::new(2026, 9, 0).to_string(), "2026.9");
        assert_eq!(LanguageVersion::new(2026, 9, 2).to_string(), "2026.9.2");
    }
}
