//! Parsing for `@requires [EDITION] version>=X` directives, and the label
//! comparison the `DataVersion` ordering falls back on.
//!
//! Spec context: `@requires` is a Minecraft-side capability floor (see
//! `spec/versioning-editions.md` §10.4). Floors compose by taking the
//! strictest across every `@requires` line, which is the
//! registry-compatible-range lower edge `cairn info` prints and the bound
//! `cairn compile --target` is held to.
//!
//! **The ordering key is `DataVersion`, and it does not live here.**
//! §10.1 makes `DataVersion` canonical precisely so ordering survives
//! Minecraft's move from semver-ish to date-based version labels, and
//! [`super::version_order::VersionOrder`] is what does the ordering — over
//! a table this crate cannot hold, since the table ships in the registry
//! pack and `core` does not depend on `cairn-lang-formats`. What is left
//! here is the two halves that are about the *text*: reading the directive,
//! and [`compare_versions`], which places a label against a table row when
//! the label names no row at all.
//!
//! Two things follow from the key being per-edition:
//!
//! - A floor may name the edition it is written in — `@requires java
//!   version>=1.21.4` — because Java ships `1.20.4 / 1.21 / 1.21.4` and
//!   Bedrock `1.21.0 / 1.21.40 / 1.21.60`, and `1.21.4` means different
//!   things against the two. An unscoped floor is a floor on whichever
//!   edition is being built, and is resolved in that edition's table.
//! - The label grammar accepts more than dotted decimals, because the
//!   table is what decides whether a label can be ordered. A pre-release
//!   (`1.21.4-rc1`) and a date-based or snapshot label (`24w14a`) are
//!   version labels this parser has no business refusing; whether either
//!   can be *placed* is a question for the edition's table, asked at the
//!   one command that pins an edition and a target.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use crate::edition::Edition;

/// A version floor an `@requires` line declares.
///
/// The edition is `Option` rather than defaulted to the build's, because
/// the two carry different obligations: a scoped floor constrains that
/// edition and is inert against the other, while an unscoped one
/// constrains whatever is being built. Collapsing them here would make the
/// second indistinguishable from a floor scoped to every edition, which is
/// not what it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Requirement<'a> {
    /// The edition the floor is written in, when the line names one.
    pub edition: Option<Edition>,
    /// The version label, verbatim. Known to be shaped like a version;
    /// not known to name one any edition ships.
    pub version: &'a str,
}

/// Why an `@requires` expression is not a version floor.
///
/// Carried out of [`parse_requirement`] rather than collapsed into `None`
/// so the diagnostic can say which part is wrong. "Not understood" is not
/// an actionable message for a directive whose whole job is to state one
/// constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RequirementError {
    /// The expression does not open with the `version` subject, with or
    /// without an edition before it.
    NotAVersionRequirement,
    /// A word before `version` that is not an edition Cairn knows. Holds
    /// it as written.
    ///
    /// Separate from [`Self::NotAVersionRequirement`]: `@requires jaba
    /// version>=1.21` is a floor with a typo in its scope, and telling its
    /// author the line is not a version requirement points at the half
    /// that is right.
    UnknownEditionScope(String),
    /// A comparison operator other than `>=`, which is the only one the
    /// spec defines. Holds the operator as written.
    UnsupportedOperator(String),
    /// `version>=` with nothing after it.
    EmptyVersion,
    /// A dot-separated component that is not shaped like one: empty, not
    /// starting with a digit, or holding something other than letters and
    /// digits. Holds the whole version as written and the offending
    /// component.
    ///
    /// "Starts with a digit" rather than "is a number", because a version
    /// label is not always a number: `24w14a` is a real Minecraft
    /// snapshot and `1.21.4-rc1` a real pre-release, and both are ordered
    /// by the edition's `DataVersion` table rather than by their text. What
    /// the rule still catches is the shape nothing could ever be: `1.a`
    /// and `1.2.beta` name no version, and a floor that names no version
    /// is a mistake in the file rather than a label the table has not
    /// heard of.
    Component {
        /// The version string as the author wrote it.
        version: String,
        /// The dot-separated component that could not be read.
        component: String,
    },
    /// A component of digits that does not fit in `u32`. Holds the whole
    /// version and the component.
    ///
    /// Not an ordering limit — [`compare_versions`] orders digit runs of
    /// any length, and the table orders by `DataVersion` — but a shape
    /// limit. `99.0` is a version Minecraft has not shipped and the answer
    /// to it is "no supported target satisfies this floor"; `4294967296`
    /// is not a version at all, and giving it that same answer would
    /// describe a floor nobody meant to write.
    ComponentTooLarge {
        /// The version string as the author wrote it.
        version: String,
        /// The dot-separated component that overflowed.
        component: String,
    },
    /// A `-` in the version with no readable pre-release tag after it.
    /// Holds the whole version and the tag as written.
    PreRelease {
        /// The version string as the author wrote it.
        version: String,
        /// Everything after the first `-`.
        tag: String,
    },
    /// Text after the version. Holds it verbatim.
    TrailingTokens(String),
}

impl RequirementError {
    /// Stable machine-readable name for this failure, for a consumer
    /// choosing a quick-fix without reading the prose.
    ///
    /// Kept beside the variants rather than derived from them, because
    /// [`crate::check::DiagnosticData`] carries these strings onto the wire
    /// and a rename there is a break for anything matching on them.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NotAVersionRequirement => "not_a_version_requirement",
            Self::UnknownEditionScope(_) => "unknown_edition_scope",
            Self::UnsupportedOperator(_) => "unsupported_operator",
            Self::EmptyVersion => "empty_version",
            Self::Component { .. } => "component_not_a_number",
            Self::ComponentTooLarge { .. } => "component_too_large",
            Self::PreRelease { .. } => "prerelease_not_a_tag",
            Self::TrailingTokens(_) => "trailing_tokens",
        }
    }
}

impl fmt::Display for RequirementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAVersionRequirement => {
                write!(f, "expected a version floor, as in `version>=1.21`")
            }
            Self::UnknownEditionScope(scope) => write!(
                f,
                "`{scope}` is not an edition; a floor may name `java` or `bedrock` before its \
                 subject, as in `java version>=1.21.4`",
            ),
            Self::UnsupportedOperator(operator) => write!(
                f,
                "`{operator}` is not a requirement operator; `>=` is the only one defined, \
                 as in `version>=1.21`",
            ),
            Self::EmptyVersion => write!(f, "`version>=` names no version, as in `version>=1.21`"),
            Self::Component { version, component } if component.is_empty() => {
                write!(
                    f,
                    "version `{version}` has an empty component; a version is dot-separated \
                     components that each begin with a digit, as in `1.21.4`",
                )
            }
            Self::Component { version, component } => write!(
                f,
                "version `{version}` has the component `{component}`, which does not begin with \
                 a digit. A version is dot-separated components of letters and digits, each \
                 beginning with one, as in `1.21.4`, `1.21.4-rc1`, or `24w14a`",
            ),
            Self::ComponentTooLarge { version, component } => write!(
                f,
                "version `{version}` has the component `{component}`, which is larger than {}",
                u32::MAX,
            ),
            Self::PreRelease { version, tag } if tag.is_empty() => write!(
                f,
                "version `{version}` ends in `-` with no pre-release tag after it, \
                 as in `1.21.4-rc1`",
            ),
            Self::PreRelease { version, tag } => write!(
                f,
                "version `{version}` has the pre-release tag `{tag}`, which is not \
                 dot-separated letters and digits, as in `1.21.4-rc1`",
            ),
            Self::TrailingTokens(rest) => {
                write!(
                    f,
                    "`{rest}` follows the version; a requirement is one floor"
                )
            }
        }
    }
}

/// Read an `@requires` expression as a version floor.
///
/// The accepted shape is an optional edition, the subject `version`, the
/// operator `>=`, and a version label, with optional whitespace between
/// them — `version>=1.21` and `version >= 1.21` are the same requirement.
/// Only `>=` is defined (spec syntax §5.3): a floor composes with other
/// floors by taking the strictest, which no other operator does.
///
/// The edition scope is what keeps two numbering schemes apart. Java ships
/// `1.20.4 / 1.21 / 1.21.4` and Bedrock `1.21.0 / 1.21.40 / 1.21.60`, so
/// `1.21.4` is Java's newest release and names no Bedrock release at all.
/// A floor that says which scheme it is written in can be held to that
/// edition and left out of the other's build.
///
/// # Errors
///
/// Returns the part of the expression that is not a floor, so the caller
/// can name it. Every shape that is not exactly the above is an error
/// rather than a silent `None`: the directive's only purpose is to state a
/// constraint, so one that states nothing is a defect in the file.
pub fn parse_requirement(raw: &str) -> Result<Requirement<'_>, RequirementError> {
    let (edition, rest) = strip_edition_scope(raw.trim_start())?;
    let rest = rest
        .strip_prefix("version")
        .ok_or(RequirementError::NotAVersionRequirement)?
        .trim_start();
    // Only the leading whitespace: what follows the version is trimmed by
    // the split below, so trimming it twice would be a second rule saying
    // the same thing.
    let rest = strip_operator(rest)?.trim_start();
    if rest.is_empty() {
        return Err(RequirementError::EmptyVersion);
    }
    // The version runs to the first whitespace; anything past it is a
    // second thing on a line that holds one requirement.
    let (version, trailing) = match rest.find(char::is_whitespace) {
        Some(at) => (&rest[..at], rest[at..].trim()),
        None => (rest, ""),
    };
    if !trailing.is_empty() {
        return Err(RequirementError::TrailingTokens(trailing.to_owned()));
    }
    validate_label(version)?;
    Ok(Requirement { edition, version })
}

/// Consume an edition before the subject, when the line names one.
///
/// A word is only read as a scope when `version` follows it. Without that
/// rule `@requires nonsense` would be reported as an unknown edition,
/// which describes a line the author did not write: they wrote something
/// that is not a requirement at all, and [`RequirementError::UnknownEditionScope`]
/// is for the line that is a requirement but for an edition Cairn has
/// never heard of.
fn strip_edition_scope(rest: &str) -> Result<(Option<Edition>, &str), RequirementError> {
    let end = rest
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    let (word, after) = rest.split_at(end);
    if word == "version" {
        return Ok((None, rest));
    }
    let tail = after.trim_start();
    // `javaversion>=1.21` splits as one word and no subject follows it, so
    // it falls through to the caller's "not a version requirement" rather
    // than being read as a scope that happens to abut its subject.
    if !tail.starts_with("version") {
        return Ok((None, rest));
    }
    match Edition::from_str(word) {
        Ok(edition) => Ok((Some(edition), tail)),
        Err(_) => Err(RequirementError::UnknownEditionScope(word.to_owned())),
    }
}

/// Consume the comparison operator, naming it when it is not `>=`.
///
/// The operator characters are recognised as a set rather than matched
/// against a list of spellings, so `=<` and `>>` are reported as the
/// operators they look like instead of falling through to "expected a
/// version floor", which would be true but unhelpful.
fn strip_operator(rest: &str) -> Result<&str, RequirementError> {
    let end = rest
        .find(|c: char| !matches!(c, '<' | '>' | '=' | '!' | '~' | '^'))
        .unwrap_or(rest.len());
    let (operator, tail) = rest.split_at(end);
    match operator {
        ">=" => Ok(tail),
        "" => Err(RequirementError::NotAVersionRequirement),
        other => Err(RequirementError::UnsupportedOperator(other.to_owned())),
    }
}

/// Check that a label is shaped like a Minecraft version.
///
/// The grammar is deliberately wider than dotted decimals and narrower
/// than "any text": every component begins with a digit and carries only
/// letters and digits, and an optional `-` introduces a pre-release tag of
/// the same. That admits every label shape §10.1 says may exist — the
/// semver-ish `1.21.4`, the pre-release `1.21.4-rc1`, a snapshot `24w14a`,
/// and whatever a date-based scheme spells — while still refusing `1.a`
/// and `x`, which name no version in any scheme.
fn validate_label(version: &str) -> Result<(), RequirementError> {
    let (core, tag) = split_prerelease(version);
    for component in core.split('.') {
        if !is_component(component) {
            return Err(RequirementError::Component {
                version: version.to_owned(),
                component: component.to_owned(),
            });
        }
        // `is_digits` before `parse`, because integer `FromStr` accepts a
        // leading `+`: without it `version>=1.+0` parses, and `+0` is then
        // not a digit run to the comparison, which stops treating it as the
        // zero it looks like. `is_component` already refuses the `+`; this
        // ordering keeps the overflow check on runs that are only digits.
        if is_digits(component) && component.parse::<u32>().is_err() {
            return Err(RequirementError::ComponentTooLarge {
                version: version.to_owned(),
                component: component.to_owned(),
            });
        }
    }
    if let Some(tag) = tag
        && !is_prerelease_tag(tag)
    {
        return Err(RequirementError::PreRelease {
            version: version.to_owned(),
            tag: tag.to_owned(),
        });
    }
    Ok(())
}

/// Split a label into its release core and its pre-release tag.
///
/// The first `-` is the boundary, so `1.21.4-pre-2` has the tag `pre-2`
/// rather than two boundaries to choose between.
fn split_prerelease(version: &str) -> (&str, Option<&str>) {
    match version.split_once('-') {
        Some((core, tag)) => (core, Some(tag)),
        None => (version, None),
    }
}

/// Whether a component is a run of letters and digits opening with a
/// digit.
fn is_component(s: &str) -> bool {
    s.bytes().next().is_some_and(|b| b.is_ascii_digit())
        && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Whether a pre-release tag is dot-separated letters and digits. Unlike a
/// release component it may open with a letter, since `rc1` is the usual
/// spelling and `1rc` is not.
fn is_prerelease_tag(s: &str) -> bool {
    !s.is_empty()
        && s.split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// Read an `@requires` expression as a version floor, or nothing.
///
/// The lenient face of [`parse_requirement`], for callers that only want
/// the floor and have no way to report why there is not one. A caller that
/// can report should use `parse_requirement`, or a malformed requirement
/// silently declares nothing — which is the defect this pair replaced.
#[must_use]
pub fn parse_min_version(raw: &str) -> Option<Requirement<'_>> {
    parse_requirement(raw).ok()
}

/// Compare two version labels by their text.
///
/// **This is not the ordering key.** `DataVersion` is
/// ([`super::version_order::VersionOrder`]), and this is what that falls
/// back on for the one question a table of known versions cannot answer
/// from its rows: where a label it has never seen sits relative to the
/// ones it has. `VersionOrder` only trusts the answer where it cannot be
/// wrong — a label below every row, or above every one — which is why this
/// staying a *total* order matters more than it being meaningful.
///
/// A missing trailing component is treated as `0`: `"1.20"` compares equal
/// to `"1.20.0"`, and so does `"1.020"` — leading zeros are padding, not
/// digits of the number. A pre-release sorts below the release it names,
/// so `"1.21.4-rc1" < "1.21.4"`, which is the one relation Minecraft's own
/// labels promise and the `DataVersion` table confirms wherever it carries
/// both.
///
/// This is `pub`, so it has to hold up on input [`parse_requirement`]
/// would have refused: a caller can reach it with a hand-edited string
/// that never passed through a directive, and `cairn info` describing such
/// a file is better than `cairn info` panicking on it. Being a total order
/// is the weaker promise than "meaningful", and the one `slice::sort_by`
/// requires on pain of a panic. Getting there needs components to be
/// classified before they are compared:
///
/// - A run of digits is a number, ordered by value however long the run
///   (digit count after leading zeros, then lexicographically — the
///   previous fallback compared long runs as plain strings and put
///   `4294967296` *below* `999`).
/// - Anything else is ordered lexicographically among its own kind, and
///   sorts above every number.
///
/// Mixing the two orders per pair, which is what this did before, is what
/// made it intransitive: `5abc < 999 < 4294967296 < 5abc`. Which side
/// non-numbers land on is arbitrary; that they all land on one side is not.
#[must_use]
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let (a_core, a_tag) = split_prerelease(a);
    let (b_core, b_tag) = split_prerelease(b);
    compare_cores(a_core, b_core).then_with(|| compare_tags(a_tag, b_tag))
}

/// Order two release cores component-wise.
fn compare_cores(a: &str, b: &str) -> Ordering {
    let mut left = a.split('.');
    let mut right = b.split('.');
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (Some(l), None) => {
                if is_zero(l) {
                    continue;
                }
                return Ordering::Greater;
            }
            (None, Some(r)) => {
                if is_zero(r) {
                    continue;
                }
                return Ordering::Less;
            }
            (Some(l), Some(r)) => match compare_segments(l, r) {
                Ordering::Equal => {}
                non_eq => return non_eq,
            },
        }
    }
}

/// Order two pre-release tags, absent being the release itself.
///
/// A release outranks every pre-release of it, which is the direction
/// Minecraft ships them in. Two tags are compared as text: `rc1` against
/// `rc2` reads correctly and `pre5` against `rc1` does not, and no
/// text rule would — that pair is what the `DataVersion` table is for.
fn compare_tags(a: Option<&str>, b: Option<&str>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (Some(l), Some(r)) => l.cmp(r),
    }
}

/// Order one pair of dot-separated components.
///
/// Classify first, compare second. Deciding per pair — numerically when
/// both happen to be digits, lexicographically otherwise — is what let
/// `5abc < 999` and `999 < 4294967296` coexist with `5abc > 4294967296`.
fn compare_segments(l: &str, r: &str) -> Ordering {
    match (numeric_key(l), numeric_key(r)) {
        // Both numbers. Once the leading zeros are gone the longer run is
        // the larger value, whatever `u32` can hold.
        (Some(l), Some(r)) => l.len().cmp(&r.len()).then_with(|| l.cmp(r)),
        // A number and something that is not one. Every number sorts below
        // every non-number so the two orders never interleave.
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => l.cmp(r),
    }
}

/// A component's value as a comparable digit run, or `None` when it is not
/// a number at all.
fn numeric_key(s: &str) -> Option<&str> {
    is_digits(s).then(|| trim_leading_zeros(s))
}

/// Whether a component is a run of ASCII digits, however long.
///
/// Empty is not a run: a version of `1.` has a component the grammar
/// refuses, and calling it zero here would make `1.` and `1.0` the same
/// version at the one layer that is supposed to keep them apart.
fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// A digit run without its leading zeros, so `007` and `7` are one number.
/// An all-zero run keeps one digit rather than becoming empty.
fn trim_leading_zeros(s: &str) -> &str {
    let trimmed = s.trim_start_matches('0');
    if trimmed.is_empty() { "0" } else { trimmed }
}

/// Whether a missing counterpart component may be treated as absent.
fn is_zero(s: &str) -> bool {
    is_digits(s) && trim_leading_zeros(s) == "0"
}

/// Whether a label is dot-separated decimals and nothing else.
///
/// [`super::version_order`] asks this before trusting [`compare_versions`]
/// to place a label outside a table's rows: two dotted decimals are one
/// numbering scheme and compare meaningfully, while a dotted decimal
/// against a date-based or snapshot label is exactly the comparison §10.1
/// makes `DataVersion` canonical to avoid.
#[must_use]
pub(super) fn is_dotted_decimal(label: &str) -> bool {
    split_prerelease(label).1.is_none() && label.split('.').all(is_digits)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The version a well-formed requirement declares, for the tests that
    /// are not about the edition scope.
    fn version_of(raw: &str) -> Option<&str> {
        parse_min_version(raw).map(|r| r.version)
    }

    #[test]
    fn parse_min_version_strips_prefix() {
        assert_eq!(version_of("version>=1.20"), Some("1.20"));
    }

    #[test]
    fn parse_min_version_tolerates_whitespace() {
        assert_eq!(version_of("  version>= 1.20.4 "), Some("1.20.4"));
    }

    #[test]
    fn parse_min_version_returns_none_for_other_shapes() {
        assert_eq!(version_of("version<=1.21"), None);
        assert_eq!(version_of("1.20"), None);
    }

    /// A space before the operator used to hide the whole floor. It is
    /// what a human writes, and `version>= 1.20` already worked, so the two
    /// spellings differed for no reason a reader could see.
    #[test]
    fn parse_min_version_accepts_whitespace_before_the_operator() {
        for raw in [
            "version >= 1.20.4",
            "version >=1.20.4",
            "version>= 1.20.4",
            "  version  >=  1.20.4  ",
        ] {
            assert_eq!(version_of(raw), Some("1.20.4"), "{raw:?}");
        }
    }

    /// The scope is what tells the two numbering schemes apart, so it has
    /// to come back out as the edition it names rather than as text.
    #[test]
    fn a_floor_may_name_the_edition_it_is_written_in() {
        for (raw, edition) in [
            ("java version>=1.21.4", Edition::Java),
            ("bedrock version>=1.21.40", Edition::Bedrock),
            ("  java   version >= 1.21.4  ", Edition::Java),
        ] {
            let parsed = parse_requirement(raw).unwrap_or_else(|e| panic!("{raw:?}: {e}"));
            assert_eq!(parsed.edition, Some(edition), "{raw:?}");
        }
        assert_eq!(
            parse_requirement("version>=1.21.4")
                .expect("unscoped")
                .edition,
            None,
        );
    }

    /// A typo in the scope is a floor with a typo, not a line that is not
    /// a floor. The two send an author to different halves of the line.
    #[test]
    fn a_scope_that_is_not_an_edition_is_named_as_a_scope() {
        assert_eq!(
            parse_requirement("jaba version>=1.21"),
            Err(RequirementError::UnknownEditionScope("jaba".to_owned())),
        );
        // Without a subject after it there is no floor to scope, so the
        // line is reported as the whole mistake it is.
        assert_eq!(
            parse_requirement("jaba"),
            Err(RequirementError::NotAVersionRequirement),
        );
        // And a word abutting the subject is one word, not a scope.
        assert_eq!(
            parse_requirement("javaversion>=1.21"),
            Err(RequirementError::NotAVersionRequirement),
        );
    }

    /// The error says which part is wrong. Collapsing every shape into
    /// `None` is what let the pass above report "not understood", and an
    /// unactionable diagnostic on a one-constraint directive is barely
    /// better than the silence it replaced.
    #[test]
    fn parse_requirement_names_the_part_that_is_wrong() {
        assert_eq!(
            parse_requirement("version<1.20"),
            Err(RequirementError::UnsupportedOperator("<".to_owned())),
        );
        assert_eq!(
            parse_requirement("version>="),
            Err(RequirementError::EmptyVersion),
        );
        assert_eq!(
            parse_requirement("nonsense"),
            Err(RequirementError::NotAVersionRequirement),
        );
        assert_eq!(
            parse_requirement("version>=1.21 extra"),
            Err(RequirementError::TrailingTokens("extra".to_owned())),
        );
        assert_eq!(
            parse_requirement("version>=1.a"),
            Err(RequirementError::Component {
                version: "1.a".to_owned(),
                component: "a".to_owned(),
            }),
        );
        assert_eq!(
            parse_requirement("version>=1..2"),
            Err(RequirementError::Component {
                version: "1..2".to_owned(),
                component: String::new(),
            }),
        );
    }

    /// The labels §10.1 says may exist are labels, not mistakes. Whether
    /// one can be *ordered* is the edition table's answer, and refusing
    /// them here pre-empts it with the wrong one.
    #[test]
    fn the_label_shapes_the_spec_names_are_accepted() {
        for raw in [
            "version>=1.21.4-rc1",
            "version>=1.21.4-pre.2",
            "version>=24w14a",
            "version>=2026.1",
            "version>=1.21.4",
        ] {
            assert!(parse_min_version(raw).is_some(), "{raw:?}");
        }
    }

    /// A component that opens with a letter names no version in any
    /// scheme, so the wider grammar still refuses it.
    #[test]
    fn a_component_that_does_not_open_with_a_digit_is_refused() {
        for raw in ["version>=1.a", "version>=x", "version>=1.2.beta"] {
            assert!(
                matches!(
                    parse_requirement(raw),
                    Err(RequirementError::Component { .. }),
                ),
                "{raw:?} -> {:?}",
                parse_requirement(raw),
            );
        }
    }

    /// A `-` with nothing readable after it is a truncated label rather
    /// than a pre-release, and says so in its own words.
    #[test]
    fn a_pre_release_tag_that_is_not_one_is_refused() {
        for raw in [
            "version>=1.21.4-",
            "version>=1.21.4-rc/1",
            "version>=1.21.4-.",
        ] {
            assert!(
                matches!(
                    parse_requirement(raw),
                    Err(RequirementError::PreRelease { .. }),
                ),
                "{raw:?} -> {:?}",
                parse_requirement(raw),
            );
        }
    }

    /// The two component failures say different things, because the repair
    /// differs. A label Cairn cannot read is not a number out of range.
    #[test]
    fn the_two_component_failures_read_differently() {
        let letters = parse_requirement("version>=1.beta").expect_err("not a component");
        let overflow = parse_requirement("version>=4294967296").expect_err("too large");
        assert_eq!(letters.kind(), "component_not_a_number");
        assert_eq!(overflow.kind(), "component_too_large");
        assert!(letters.to_string().contains("beta"), "{letters}");
        assert!(
            !letters.to_string().contains("4294967295"),
            "a component of letters is not a number out of range: {letters}",
        );
        assert!(overflow.to_string().contains("4294967295"), "{overflow}");
    }

    /// An empty component gets its own sentence — `1..2` is a typo, not a
    /// label Cairn cannot order, and the two repairs have nothing in
    /// common.
    #[test]
    fn an_empty_component_says_so() {
        let text = parse_requirement("version>=1..2")
            .expect_err("empty component")
            .to_string();
        assert!(
            text.contains("empty component"),
            "the message should name the shape of the mistake: {text}",
        );
    }

    /// An operator is read as the run of operator characters it is, so a
    /// transposed or doubled one is reported as an operator rather than as
    /// "expected a version floor" — true, but not what the author typed.
    #[test]
    fn parse_requirement_names_an_operator_it_does_not_know() {
        for operator in ["<", "<=", ">", "==", "=", "=>", "=<", "!=", "~>", "^"] {
            assert!(
                matches!(
                    parse_requirement(&format!("version{operator}1.20")),
                    Err(RequirementError::UnsupportedOperator(ref found)) if found == operator,
                ),
                "{operator}",
            );
        }
    }

    /// `4294967296` is all ASCII digits and no version, so the digit-only
    /// shape check has to be paired with a bound.
    #[test]
    fn parse_requirement_refuses_a_component_too_large_to_be_a_version() {
        assert_eq!(
            parse_requirement("version>=4294967296"),
            Err(RequirementError::ComponentTooLarge {
                version: "4294967296".to_owned(),
                component: "4294967296".to_owned(),
            }),
        );
        // One below the boundary is a number, however unlikely.
        assert_eq!(version_of("version>=4294967295"), Some("4294967295"));
    }

    /// Rust's integer `FromStr` accepts a leading `+`, so `parse::<u32>()`
    /// alone is looser than the label grammar this documents. The lexer
    /// refuses `+` in a `.crn` file, but both of these are `pub` and a
    /// caller can hand them any string.
    ///
    /// It is not only a tidiness point: `+0` is not a digit run to the
    /// comparison, so it stops being the zero it looks like and
    /// `1.20.+0 > 1.20`.
    #[test]
    fn parse_requirement_refuses_a_signed_component() {
        for raw in ["version>=+1", "version>=1.+0", "version>=+1.+20"] {
            assert!(
                matches!(
                    parse_requirement(raw),
                    Err(RequirementError::Component { .. }),
                ),
                "{raw:?} -> {:?}",
                parse_requirement(raw),
            );
        }
        // A leading `-` is read as an empty core with a pre-release tag,
        // which is the shape it is: `-1` names no release.
        assert!(
            matches!(
                parse_requirement("version>=-1"),
                Err(RequirementError::Component { .. }),
            ),
            "{:?}",
            parse_requirement("version>=-1"),
        );
    }

    #[test]
    fn parse_min_version_rejects_empty_payload() {
        // Regression: `version>=` with no body used to return `Some("")`,
        // which `derive_min_version` then propagated to `min` and produced
        // a malformed `{"min":""}` in `cairn info --format json`.
        assert_eq!(version_of("version>="), None);
        assert_eq!(version_of("version>=  "), None);
    }

    #[test]
    fn parse_min_version_rejects_non_numeric_segments() {
        // Regression: a non-numeric segment used to slip through and
        // `compare_versions` would fall back to ASCII compare, where
        // `'a' (0x61) > '2' (0x32)` made `1.a > 1.20`.
        assert_eq!(version_of("version>=1.a"), None);
        assert_eq!(version_of("version>=1..2"), None);
    }

    #[test]
    fn compare_versions_is_numeric_per_component() {
        assert_eq!(compare_versions("1.20", "1.20"), Ordering::Equal);
        assert_eq!(compare_versions("1.20", "1.20.4"), Ordering::Less);
        assert_eq!(compare_versions("1.20.4", "1.20"), Ordering::Greater);
        assert_eq!(compare_versions("1.21", "1.20.4"), Ordering::Greater);
        assert_eq!(compare_versions("1.2", "1.10"), Ordering::Less);
    }

    /// A pre-release sits below the release it names. It is the one
    /// relation a label's text does promise, and the table agrees wherever
    /// it carries both.
    #[test]
    fn a_pre_release_sorts_below_its_release() {
        assert_eq!(compare_versions("1.21.4-rc1", "1.21.4"), Ordering::Less);
        assert_eq!(compare_versions("1.21.4", "1.21.4-rc1"), Ordering::Greater);
        assert_eq!(compare_versions("1.21.4-rc1", "1.21.4-rc2"), Ordering::Less);
        assert_eq!(
            compare_versions("1.21.4-rc1", "1.21.4-rc1"),
            Ordering::Equal,
        );
        // The core still decides first: a pre-release of a later version
        // is later.
        assert_eq!(compare_versions("1.21.4-rc1", "1.21"), Ordering::Greater);
    }

    #[test]
    fn compare_versions_treats_trailing_zero_as_equal() {
        assert_eq!(compare_versions("1.20", "1.20.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.20.0.0", "1.20"), Ordering::Equal);
        // Leading zeros are not a different number either.
        assert_eq!(compare_versions("1.20", "1.020"), Ordering::Equal);
        assert_eq!(compare_versions("1.0007", "1.7"), Ordering::Equal);
    }

    /// This is `pub`, so it has to be right on input the directive would
    /// have refused: `4294967296` overflows `u32`, a string fallback
    /// compares it by first character, and `'4' < '9'` puts it below
    /// `999`.
    #[test]
    fn compare_versions_orders_digits_past_u32_by_value() {
        assert_eq!(compare_versions("4294967296", "999"), Ordering::Greater);
        assert_eq!(compare_versions("999", "4294967296"), Ordering::Less);
        // And among two that both overflow, by value rather than by the
        // first differing character.
        assert_eq!(
            compare_versions("99999999999", "100000000000"),
            Ordering::Less,
        );
        assert_eq!(compare_versions("4294967296", "4294967297"), Ordering::Less,);
        // Equal length, equal value.
        assert_eq!(
            compare_versions("4294967296", "4294967296"),
            Ordering::Equal,
        );
        // Leading zeros are padding, not digits of the number. Ordering by
        // length first is only numeric order once they are gone —
        // `04294967296` is eleven characters and the smaller value.
        assert_eq!(
            compare_versions("04294967296", "4294967296"),
            Ordering::Equal,
        );
        assert_eq!(
            compare_versions("04294967296", "4294967297"),
            Ordering::Less,
        );
        // And an all-zero run is zero rather than nothing, so a component
        // of `000` still stands in for an absent one.
        assert_eq!(compare_versions("1.20", "1.20.000"), Ordering::Equal);
        // The same, in the position a version would use it.
        assert_eq!(compare_versions("1.4294967296", "1.999"), Ordering::Greater);
    }

    /// A component that is not a number at all still orders, because this
    /// function is reachable from `cairn info` on a hand-edited file and a
    /// panic there would be worse than an arbitrary answer.
    ///
    /// Which way such a version sorts is not a promise. That it sorts *at
    /// all* is: `slice::sort_by` panics on a comparator that is not a total
    /// order, and nothing stops a caller passing this one.
    ///
    /// Antisymmetry alone does not get there — it survives cycles, and the
    /// version this replaced had them. Every triple over a fixed alphabet
    /// is the cheapest way to see the difference: 24 values is 13824
    /// comparisons, run in well under a millisecond.
    #[test]
    fn compare_versions_is_a_total_order_on_input_the_grammar_refuses() {
        // Chosen to mix the classes the comparison distinguishes: short and
        // long digit runs, runs past `u32`, leading zeros, non-digits,
        // empties, pre-release tags, and multi-component versions of each.
        const ALPHABET: &[&str] = &[
            "",
            "0",
            "00",
            "1",
            "1.",
            ".1",
            "1.0",
            "1.00",
            "1.20",
            "1.21",
            "1.21.0",
            "999",
            "5abc",
            "a",
            "x",
            "24w14a",
            "4294967296",
            "04294967296",
            "4294967297",
            "1.4294967296",
            "1.+0",
            "-",
            "1.21-",
            "1.21-rc1",
        ];

        for &a in ALPHABET {
            assert_eq!(compare_versions(a, a), Ordering::Equal, "{a:?}");
            for &b in ALPHABET {
                assert_eq!(
                    compare_versions(a, b).reverse(),
                    compare_versions(b, a),
                    "not antisymmetric: {a:?} vs {b:?}",
                );
                for &c in ALPHABET {
                    // Transitivity of `<=`, which for a total order also
                    // gives transitivity of `<` and of `==`.
                    if compare_versions(a, b).is_le() && compare_versions(b, c).is_le() {
                        assert!(
                            compare_versions(a, c).is_le(),
                            "not transitive: {a:?} <= {b:?} <= {c:?} but {a:?} > {c:?}",
                        );
                    }
                }
            }
        }
    }

    /// The classification the total order rests on: every version made of
    /// numbers sorts below every version that is not, whatever the
    /// characters would say. Mixing the two orders per pair is what put
    /// `5abc` below `999` and above `4294967296` at the same time.
    #[test]
    fn a_component_that_is_not_a_number_sorts_above_every_number() {
        assert_eq!(compare_versions("5abc", "999"), Ordering::Greater);
        assert_eq!(compare_versions("5abc", "4294967296"), Ordering::Greater);
        assert_eq!(compare_versions("a", "99999999999"), Ordering::Greater);
        // An empty component is not a number either, so `1.` is not `1.0`.
        // Arbitrary, but fixed: the alternative is a comparison that calls
        // a version the grammar refuses equal to one it accepts.
        assert_eq!(compare_versions("1.", "1.0"), Ordering::Greater);
        assert_eq!(compare_versions("1.0", "1."), Ordering::Less);
    }

    /// What `version_order` asks before trusting the text: one numbering
    /// scheme, not two.
    #[test]
    fn dotted_decimal_is_what_it_says() {
        for yes in ["1", "1.20", "1.21.4", "0.0", "4294967296"] {
            assert!(is_dotted_decimal(yes), "{yes:?}");
        }
        for no in ["1.21.4-rc1", "24w14a", "1.a", "1.", "", "1..2"] {
            assert!(!is_dotted_decimal(no), "{no:?}");
        }
    }
}
