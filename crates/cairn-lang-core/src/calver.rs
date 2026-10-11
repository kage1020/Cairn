//! Reading the `@cairn` header's `CalVer` value.
//!
//! Cairn versions itself by date, `YYYY.M[.PATCH]`, the scheme [`crate::CAIRN_VERSION`] carries.
//! A `.crn` file may declare the language version it was written against (`spec/syntax`
//! "Headers"). The declaration is provenance and nothing branches on it, but it still has to be
//! readable: a later compiler can do nothing with `@cairn banana`.
//!
//! Stricter than [`crate::resolve::parse_requirement`] on purpose. A Minecraft label such as
//! `24w14a` or `1.21.4-rc1` only has to be ordered, but a `CalVer` is Cairn's own, so every
//! component is digits, the year has four of them and the month is a calendar month.
//!
//! The month is read as `2026.06` (`YYYY.0M`, what `spec/index` and `.crn` files write) and as
//! `2026.6` (`YYYY.M`, since Cargo's semver forbids a leading zero). The two compare equal.

use std::fmt;

use crate::CAIRN_VERSION;
use crate::diagnostic::DiagnosticNote;
use crate::error::Span;
use crate::lines::end_before;

/// Lowest month a `CalVer` names.
const FIRST_MONTH: u32 = 1;
/// Highest month a `CalVer` names.
const LAST_MONTH: u32 = 12;
/// Digits in the year component. Exact, so that a semver such as `1.2` is not read as the year 1;
/// the rule is about shape, not era, so `0001.1` passes.
const YEAR_DIGITS: usize = 4;

/// A Cairn language version, parsed from a `YYYY.M[.PATCH]` string.
///
/// Field order is the comparison order the derived [`Ord`] relies on.
///
/// The calendar ranges are [`parse_language_version`]'s guarantee, not this type's: the order is
/// total over any three numbers. `#[non_exhaustive]` makes [`LanguageVersion::new`] the only way
/// to build one outside the crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub struct LanguageVersion {
    /// Calendar year, four digits when parsed.
    pub year: u32,
    /// Calendar month, `1..=12` when parsed. Numeric, so `06` and `6` are one value.
    pub month: u32,
    /// Patch within the month; `0` when the source wrote none, so `2026.9` orders before
    /// `2026.9.2`.
    pub patch: u32,
}

impl LanguageVersion {
    /// Build a version from its three components, without validation.
    ///
    /// [`parse_language_version`] is the one that judges a string.
    #[must_use]
    pub const fn new(year: u32, month: u32, patch: u32) -> Self {
        Self { year, month, patch }
    }

    /// Whether this version is later than `other`.
    ///
    /// Named so the direction of the comparison reads at the call site, where a bare `>` would
    /// have to be read for argument order.
    #[must_use]
    pub fn is_newer_than(&self, other: &Self) -> bool {
        self > other
    }
}

impl fmt::Display for LanguageVersion {
    /// `2026.9` or `2026.9.2`, eliding a zero patch.
    ///
    /// Not a round trip: `2026.06` renders as `2026.6`, `2026.1.0` as `2026.1`, and `0001.1` as
    /// `1.1`, which does not parse. A message quotes the author's own string instead.
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
/// One variant per repair, which is why a failure is not collapsed into `None`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LanguageVersionError {
    /// Not two or three dot-separated components.
    ComponentCount {
        /// Number of dot-separated components found.
        found: usize,
    },
    /// A component that is empty or not all decimal digits.
    Component {
        /// The dot-separated component that could not be read.
        component: String,
    },
    /// A year that is not exactly four digits.
    Year {
        /// The year component as written.
        component: String,
    },
    /// A month outside `1..=12`.
    Month {
        /// The month component as written, so `013` reads back as typed.
        component: String,
    },
    /// A patch that does not fit in `u32`.
    PatchTooLarge {
        /// The patch component as written.
        component: String,
    },
    /// Text after the version, verbatim.
    ///
    /// Kept apart from [`Self::Component`] so `2026.06 draft` names `draft`, not a "component"
    /// `06 draft` that no quick-fix can replace.
    TrailingTokens(String),
}

impl LanguageVersionError {
    /// Stable machine-readable name for this failure.
    ///
    /// Spelled out rather than derived from the variant names, because
    /// [`crate::check::DiagnosticData`] puts these strings on the wire, where a rename breaks
    /// consumers.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ComponentCount { .. } => "component_count",
            Self::Component { .. } => "component_not_a_number",
            Self::Year { .. } => "year_not_four_digits",
            Self::Month { .. } => "month_out_of_range",
            Self::PatchTooLarge { .. } => "patch_too_large",
            Self::TrailingTokens(_) => "trailing_tokens",
        }
    }

    /// The fragment of the value the failure is about.
    ///
    /// Empty for [`Self::ComponentCount`], which is about the whole string, and for an empty
    /// [`Self::Component`] (`2026.`, `.6`), so a tool is never handed text the author did not
    /// write.
    #[must_use]
    pub fn offending_text(&self) -> &str {
        match self {
            Self::ComponentCount { .. } => "",
            Self::Component { component }
            | Self::Year { component }
            | Self::Month { component }
            | Self::PatchTooLarge { component }
            | Self::TrailingTokens(component) => component,
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
            Self::Component { component } if component.is_empty() => write!(
                f,
                "it has an empty component; a version is runs of decimal digits separated by single dots, as in `2026.6`",
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
            Self::TrailingTokens(rest) => write!(
                f,
                "`{rest}` follows the version; the directive names one version",
            ),
        }
    }
}

impl std::error::Error for LanguageVersionError {}

/// Read a `YYYY.M[.PATCH]` string as a [`LanguageVersion`].
///
/// The whole string has to be the version. Surrounding whitespace is not trimmed, because the
/// header value arrives trimmed; an interior space starts
/// [`LanguageVersionError::TrailingTokens`].
///
/// # Errors
///
/// [`LanguageVersionError`], naming the component that could not be read.
pub fn parse_language_version(text: &str) -> Result<LanguageVersion, LanguageVersionError> {
    // Before the split on dots, so a second word is reported whole.
    let (text, trailing) = match text.find(char::is_whitespace) {
        Some(at) => (&text[..at], text[at..].trim()),
        None => (text, ""),
    };
    if !trailing.is_empty() {
        return Err(LanguageVersionError::TrailingTokens(trailing.to_owned()));
    }
    let parts: Vec<&str> = text.split('.').collect();
    if !matches!(parts.len(), 2 | 3) {
        return Err(LanguageVersionError::ComponentCount { found: parts.len() });
    }
    // Digits first: `str::parse` accepts a leading `+`, which is no spelling of a date.
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
    // Four digits always fit; reading fallibly keeps the function free of panics.
    let year = year_text
        .parse::<u32>()
        .map_err(|_| LanguageVersionError::Year {
            component: year_text.to_owned(),
        })?;
    let month_text = parts[1];
    // A month that overflows `u32` is out of range like `13`: same repair, same variant.
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

/// The note a parse failure carries when the `@cairn` header has something to say about it.
///
/// No check pass runs on a file that does not parse, and a new syntactic form from a later
/// language is exactly what does not parse, so this note is how the author learns they need a
/// newer Cairn. It is a note rather than a second finding because `spec/lint` "Error vs warning"
/// makes a source that does not parse report `E_PARSE` alone.
///
/// A header naming a later language explains the error. A header naming no language version says
/// this build cannot tell whether the error is a version gap; the author hears nothing else,
/// because `W_INVALID_CAIRN_VERSION` comes from a check pass.
pub(crate) fn future_version_note(source: &str, line_starts: &[usize]) -> Option<DiagnosticNote> {
    let (declared, span) = declared_version(source, line_starts)?;
    let Ok(version) = parse_language_version(&declared) else {
        return Some(DiagnosticNote {
            span: Some(span),
            message: format!(
                "`@cairn {declared}` does not name a language version, so this build cannot judge whether the error below is a form a later Cairn adds",
            ),
        });
    };
    if !version.is_newer_than(&parse_language_version(CAIRN_VERSION).ok()?) {
        return None;
    }
    Some(DiagnosticNote {
        span: Some(span),
        message: format!(
            "this file declares Cairn `{declared}`, which is newer than this build (`{CAIRN_VERSION}`); the line this error names may be a form a later Cairn adds",
        ),
    })
}

/// The `@cairn` value a source declares and the span of its line, read from the text.
///
/// Read from text because a source that fails to lex has neither AST nor tokens (`floor a=%` is
/// the case this note most needs), and because `crate::parse` stops at an unknown directive above
/// the `@cairn` where this scan walks past it.
///
/// Only the header block is read: the lines before the first one that is not blank, a comment or
/// a top-level `@directive`, which is the only place [`crate::parse()`] takes a header from.
///
/// It must agree with the lexer exactly, because the note claims what the file declares. Lines
/// end where [`crate::lines`] says, so `\r\n` and a lone `\r` behave as everywhere else.
/// Horizontal space is `' '` alone, as in the lexer: a tab at indentation is
/// `LexError::TabIndent`, so `@cairn\t2099.1` declares nothing. A leading byte-order mark is
/// skipped, because the lexer skips it too.
///
/// The span matches [`Header::Cairn`](crate::ast::Header::Cairn)'s, from the `@` to the value's
/// last byte, so the note points where the check pass's finding would.
fn declared_version(source: &str, line_starts: &[usize]) -> Option<(String, Span)> {
    for (index, &start) in line_starts.iter().enumerate() {
        let end = match line_starts.get(index + 1) {
            Some(&next) => end_before(source, next),
            None => source.len(),
        };
        let mut text = &source[start..end];
        let mut base = start;
        if index == 0
            && let Some(behind) = text.strip_prefix('\u{feff}')
        {
            base += text.len() - behind.len();
            text = behind;
        }
        let probe = text.trim_matches(' ');
        if probe.is_empty() || probe.starts_with('#') {
            continue;
        }
        // An indented `@cairn` is a body row to the lexer, not a header.
        let rest = text.strip_prefix('@')?;
        let name_len = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
            .unwrap_or(rest.len());
        if &rest[..name_len] != "cairn" {
            continue;
        }
        // A `#` ends the value wherever it sits, as in the lexer.
        let tail = &text[1 + name_len..];
        let tail = tail.split_once('#').map_or(tail, |(before, _)| before);
        let declared = tail.trim_matches(' ');
        // Whitespace left inside is a tab the lexer rejects or a second token; either way
        // `parse` built no header, so the file declares nothing.
        if declared.is_empty() || declared.chars().any(char::is_whitespace) {
            return None;
        }
        let value_start = 1 + name_len + (tail.len() - tail.trim_start_matches(' ').len());
        return Some((
            declared.to_owned(),
            base..base + value_start + declared.len(),
        ));
    }
    None
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
        // `2026.06.1.2` has a valid year and month in front of the problem.
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
        // `kind` is on the wire, so it is pinned here rather than left to the one call site.
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
            ("2026.06 draft", "trailing_tokens", "draft"),
            // `2026.` has a component, the empty string, so empty does not mean the whole value.
            ("2026.", "component_not_a_number", ""),
        ] {
            let error = parse_language_version(text).expect_err("not a version");
            assert_eq!(error.kind(), kind, "{text:?}");
            assert_eq!(error.offending_text(), found, "{text:?}");
        }
    }

    #[test]
    fn a_version_renders_with_the_patch_only_when_it_has_one() {
        assert_eq!(LanguageVersion::new(2026, 9, 0).to_string(), "2026.9");
        assert_eq!(LanguageVersion::new(2026, 9, 2).to_string(), "2026.9.2");
    }

    #[test]
    fn rendering_is_not_a_round_trip_and_the_doc_says_so() {
        let padded = parse_language_version("2026.06").expect("a version");
        assert_eq!(padded.to_string(), "2026.6", "the zero does not survive");
        let explicit = parse_language_version("2026.1.0").expect("a version");
        assert_eq!(explicit.to_string(), "2026.1", "nor does an explicit `.0`");
        let year_one = parse_language_version("0001.1").expect("a version");
        assert_eq!(year_one.to_string(), "1.1");
        assert!(
            parse_language_version(&year_one.to_string()).is_err(),
            "and `1.1` does not read back as a version at all",
        );
    }

    #[test]
    fn a_second_word_after_the_version_is_named_as_one() {
        for (text, rest) in [
            ("2026.06 draft", "draft"),
            ("2026.6  and more", "and more"),
            ("banana split", "split"),
        ] {
            assert_eq!(
                parse_language_version(text),
                Err(LanguageVersionError::TrailingTokens(rest.to_owned())),
                "{text:?}",
            );
        }
    }

    #[test]
    fn an_empty_component_is_described_rather_than_quoted() {
        // A quoted empty component renders as ``, which tells the author nothing.
        for text in ["2026.", ".6", "2026..1"] {
            let error = parse_language_version(text).expect_err("not a version");
            let rendered = error.to_string();
            assert!(
                rendered.contains("empty component"),
                "{text:?} should name the empty component: {rendered}",
            );
            assert!(
                !rendered.contains("``"),
                "{text:?} should not quote nothing: {rendered}",
            );
        }
    }
}
