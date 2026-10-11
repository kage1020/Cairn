//! Parsing for `@requires [EDITION] version>=X` floors, and the text comparison the `DataVersion`
//! order falls back on for a label no table row names.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use crate::edition::Edition;

/// A version floor an `@requires` line declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Requirement<'a> {
    /// The edition the floor is written in; unscoped, it constrains whatever is being built.
    pub edition: Option<Edition>,
    /// The version label, verbatim.
    pub version: &'a str,
}

/// Why an `@requires` expression is not a version floor.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RequirementError {
    /// The expression does not open with the `version` subject, with or without an edition.
    NotAVersionRequirement,
    /// A word before `version` that is not an edition Cairn knows, as written.
    UnknownEditionScope(String),
    /// A comparison operator other than `>=`, as written.
    UnsupportedOperator(String),
    /// `version>=` with nothing after it.
    EmptyVersion,
    /// A component that is empty, opens with something other than a digit, or is not alphanumeric.
    Component {
        /// The version string as the author wrote it.
        version: String,
        /// The dot-separated component that could not be read.
        component: String,
    },
    /// A component of digits that does not fit in `u32`.
    ComponentTooLarge {
        /// The version string as the author wrote it.
        version: String,
        /// The dot-separated component that overflowed.
        component: String,
    },
    /// A `-` with no readable pre-release tag after it.
    PreRelease {
        /// The version string as the author wrote it.
        version: String,
        /// Everything after the first `-`.
        tag: String,
    },
    /// Text after the version, verbatim.
    TrailingTokens(String),
}

impl RequirementError {
    /// Stable machine-readable name for this failure, which [`crate::check::DiagnosticData`]
    /// carries onto the wire.
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

/// Read an `@requires` expression as a version floor: an optional edition, `version`, `>=` and a
/// label, with optional whitespace between them.
///
/// # Errors
///
/// [`RequirementError`] naming the part of the expression that is not a floor.
pub fn parse_requirement(raw: &str) -> Result<Requirement<'_>, RequirementError> {
    let (edition, rest) = strip_edition_scope(raw.trim_start())?;
    let rest = rest
        .strip_prefix("version")
        .ok_or(RequirementError::NotAVersionRequirement)?
        .trim_start();
    let rest = strip_operator(rest)?.trim_start();
    if rest.is_empty() {
        return Err(RequirementError::EmptyVersion);
    }
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

/// Consume an edition before the subject; a word is a scope only when `version` follows it.
fn strip_edition_scope(rest: &str) -> Result<(Option<Edition>, &str), RequirementError> {
    let end = rest
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    let (word, after) = rest.split_at(end);
    if word == "version" {
        return Ok((None, rest));
    }
    let tail = after.trim_start();
    if !tail.starts_with("version") {
        return Ok((None, rest));
    }
    match Edition::from_str(word) {
        Ok(edition) => Ok((Some(edition), tail)),
        Err(_) => Err(RequirementError::UnknownEditionScope(word.to_owned())),
    }
}

/// Consume the comparison operator, read as a run of operator characters so `=<` is named.
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

/// Check that a label is shaped like a Minecraft version: `1.21.4`, `1.21.4-rc1`, `24w14a`.
fn validate_label(version: &str) -> Result<(), RequirementError> {
    let (core, tag) = split_prerelease(version);
    for component in core.split('.') {
        if !is_component(component) {
            return Err(RequirementError::Component {
                version: version.to_owned(),
                component: component.to_owned(),
            });
        }
        // `is_digits` first: integer `FromStr` accepts a leading `+`.
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

/// Split a label at its first `-` into its release core and pre-release tag.
fn split_prerelease(version: &str) -> (&str, Option<&str>) {
    match version.split_once('-') {
        Some((core, tag)) => (core, Some(tag)),
        None => (version, None),
    }
}

/// Whether a component is a run of letters and digits opening with a digit.
fn is_component(s: &str) -> bool {
    s.bytes().next().is_some_and(|b| b.is_ascii_digit())
        && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Whether a pre-release tag is dot-separated letters and digits.
fn is_prerelease_tag(s: &str) -> bool {
    !s.is_empty()
        && s.split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// [`parse_requirement`] for a caller with no way to report why there is no floor.
#[must_use]
pub fn parse_min_version(raw: &str) -> Option<Requirement<'_>> {
    parse_requirement(raw).ok()
}

/// Compare two version labels by their text; a missing trailing component is `0` and a
/// pre-release sorts below its release.
///
/// Total even on labels [`parse_requirement`] refuses, since `slice::sort_by` panics otherwise.
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
fn compare_tags(a: Option<&str>, b: Option<&str>) -> Ordering {
    match (a, b) {
        (None, None) => Ordering::Equal,
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (Some(l), Some(r)) => l.cmp(r),
    }
}

/// Order one pair of components, every number below every non-number so the order stays
/// transitive.
fn compare_segments(l: &str, r: &str) -> Ordering {
    match (numeric_key(l), numeric_key(r)) {
        // Without leading zeros, the longer run is the larger value.
        (Some(l), Some(r)) => l.len().cmp(&r.len()).then_with(|| l.cmp(r)),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => l.cmp(r),
    }
}

/// A component's value as a comparable digit run, or `None` when it is not a number.
fn numeric_key(s: &str) -> Option<&str> {
    is_digits(s).then(|| trim_leading_zeros(s))
}

/// Whether a component is a non-empty run of ASCII digits; `1.` is not `1.0`.
fn is_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// A digit run without its leading zeros, keeping one digit of an all-zero run.
fn trim_leading_zeros(s: &str) -> &str {
    let trimmed = s.trim_start_matches('0');
    if trimmed.is_empty() { "0" } else { trimmed }
}

fn is_zero(s: &str) -> bool {
    is_digits(s) && trim_leading_zeros(s) == "0"
}

/// Whether a label is dot-separated decimals and nothing else, which [`compare_versions`] orders
/// meaningfully.
#[must_use]
pub(super) fn is_dotted_decimal(label: &str) -> bool {
    split_prerelease(label).1.is_none() && label.split('.').all(is_digits)
}

#[cfg(test)]
mod tests;
