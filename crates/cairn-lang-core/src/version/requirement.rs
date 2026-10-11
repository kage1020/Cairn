//! Parsing for `@requires [EDITION] version>=X` directives, and the label comparison the
//! `DataVersion` ordering falls back on.
//!
//! `@requires` is a Minecraft-side capability floor (`spec/versioning-editions` "Fail-loud and
//! minimum-version inference"). Floors compose by taking the strictest across every `@requires`
//! line, which is the lower edge of the range `cairn info` reports as declared
//! (`spec/versioning-editions` "The `registry compatibility` row") and the bound
//! `cairn compile --target` is held to.
//!
//! **The ordering key is `DataVersion`, and it does not live here.** The same chapter's "The
//! target is a compile-time parameter" makes `DataVersion` canonical, and
//! [`super::order::VersionOrder`] orders by it over a table the registry pack ships. What is left
//! here is about the *text*: reading the directive, and [`compare_versions`], which places a label
//! that names no row.
//!
//! Two things follow from the key being per-edition:
//!
//! - A floor may name its edition (`@requires java version>=1.21.4`), because `1.21.4` is a Java
//!   release and names no Bedrock one. An unscoped floor is on whichever edition is being built,
//!   and is resolved in that edition's table.
//! - The label grammar accepts more than dotted decimals, because the table decides whether a
//!   label can be ordered. A pre-release (`1.21.4-rc1`) and a snapshot or date-based label
//!   (`24w14a`) are labels this parser does not refuse; whether either can be *placed* is asked of
//!   the edition's table by the one command that pins an edition and a target.

use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

use crate::edition::Edition;

/// A version floor an `@requires` line declares.
///
/// The edition is `Option` rather than defaulted to the build's, because a scoped floor
/// constrains that edition and is inert against the other, while an unscoped one constrains
/// whatever is being built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Requirement<'a> {
    /// The edition the floor is written in, when the line names one.
    pub edition: Option<Edition>,
    /// The version label, verbatim. Shaped like a version; not known to name one any edition
    /// ships.
    pub version: &'a str,
}

/// Why an `@requires` expression is not a version floor.
///
/// Carried out of [`parse_requirement`] rather than collapsed into `None`, so the diagnostic can
/// say which part is wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum RequirementError {
    /// The expression does not open with the `version` subject, with or
    /// without an edition before it.
    NotAVersionRequirement,
    /// A word before `version` that is not an edition Cairn knows. Holds it as written.
    ///
    /// Separate from [`Self::NotAVersionRequirement`]: `@requires jaba version>=1.21` is a floor
    /// with a typo in its scope, not a line that is no version requirement.
    UnknownEditionScope(String),
    /// A comparison operator other than `>=`, the only one the spec defines. Holds it as written.
    UnsupportedOperator(String),
    /// `version>=` with nothing after it.
    EmptyVersion,
    /// A dot-separated component that is empty, does not start with a digit, or holds something
    /// other than letters and digits. Holds the whole version as written and the component.
    ///
    /// "Starts with a digit" rather than "is a number", because `24w14a` is a real snapshot and
    /// the `DataVersion` table orders it. `1.a` and `1.2.beta` name no version in any scheme, so
    /// they are a mistake in the file rather than a label the table has not heard of.
    Component {
        /// The version string as the author wrote it.
        version: String,
        /// The dot-separated component that could not be read.
        component: String,
    },
    /// A component of digits that does not fit in `u32`. Holds the whole version and the
    /// component.
    ///
    /// A shape limit, not an ordering one: [`compare_versions`] orders digit runs of any length.
    /// `99.0` is a version Minecraft has not shipped, answered "no supported target satisfies
    /// this floor"; `4294967296` is not a version at all.
    ComponentTooLarge {
        /// The version string as the author wrote it.
        version: String,
        /// The dot-separated component that overflowed.
        component: String,
    },
    /// A `-` with no readable pre-release tag after it. Holds the whole version and the tag.
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
    /// Stable machine-readable name for this failure, for a consumer choosing a quick-fix
    /// without reading the prose.
    ///
    /// Spelled out rather than derived from the variant names, because
    /// [`crate::check::DiagnosticData`] carries these strings onto the wire.
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
/// The accepted shape is an optional edition, the subject `version`, the operator `>=`, and a
/// version label, with optional whitespace between them: `version>=1.21` and `version >= 1.21` are
/// the same requirement. Only `>=` is defined (`spec/syntax` "Headers"), since a floor composes
/// with other floors by taking the strictest, which no other operator does.
///
/// # Errors
///
/// Returns the part of the expression that is not a floor, so the caller can name it. Every other
/// shape is an error rather than a silent `None`, since a directive that states nothing is a
/// defect in the file.
pub fn parse_requirement(raw: &str) -> Result<Requirement<'_>, RequirementError> {
    let (edition, rest) = strip_edition_scope(raw.trim_start())?;
    let rest = rest
        .strip_prefix("version")
        .ok_or(RequirementError::NotAVersionRequirement)?
        .trim_start();
    // Leading whitespace only; the split below trims what follows the version.
    let rest = strip_operator(rest)?.trim_start();
    if rest.is_empty() {
        return Err(RequirementError::EmptyVersion);
    }
    // The version runs to the first whitespace; anything past it is a second requirement.
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
/// A word is read as a scope only when `version` follows it, so `@requires nonsense` is not a
/// requirement at all rather than a [`RequirementError::UnknownEditionScope`].
fn strip_edition_scope(rest: &str) -> Result<(Option<Edition>, &str), RequirementError> {
    let end = rest
        .find(|c: char| !c.is_ascii_alphabetic())
        .unwrap_or(rest.len());
    let (word, after) = rest.split_at(end);
    if word == "version" {
        return Ok((None, rest));
    }
    let tail = after.trim_start();
    // `javaversion>=1.21` is one word with no subject after it: not a version requirement.
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
/// The operator characters are recognised as a set rather than a list of spellings, so `=<` and
/// `>>` are reported as the operators they look like rather than as "expected a version floor".
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
/// Every component begins with a digit and carries only letters and digits, and an optional `-`
/// introduces a pre-release tag. That admits every label shape `spec/versioning-editions` "The
/// target is a compile-time parameter" says may exist (`1.21.4`, `1.21.4-rc1`, `24w14a`, and
/// whatever a date-based scheme spells) while refusing `1.a` and `x`.
fn validate_label(version: &str) -> Result<(), RequirementError> {
    let (core, tag) = split_prerelease(version);
    for component in core.split('.') {
        if !is_component(component) {
            return Err(RequirementError::Component {
                version: version.to_owned(),
                component: component.to_owned(),
            });
        }
        // `is_digits` first, because integer `FromStr` accepts a leading `+`; the overflow check
        // is for runs of digits alone.
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
/// The first `-` is the boundary, so `1.21.4-pre-2` has the tag `pre-2`.
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

/// Whether a pre-release tag is dot-separated letters and digits. Unlike a release component it
/// may open with a letter, as `rc1` does.
fn is_prerelease_tag(s: &str) -> bool {
    !s.is_empty()
        && s.split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// Read an `@requires` expression as a version floor, or nothing.
///
/// The lenient face of [`parse_requirement`], for callers with no way to report why there is no
/// floor. A caller that can report should use `parse_requirement`, or a malformed requirement
/// silently declares nothing.
#[must_use]
pub fn parse_min_version(raw: &str) -> Option<Requirement<'_>> {
    parse_requirement(raw).ok()
}

/// Compare two version labels by their text.
///
/// **This is not the ordering key.** `DataVersion` is ([`super::order::VersionOrder`]); this is
/// what it falls back on to place a label it has never seen. `VersionOrder` trusts the answer
/// only below or above every row, so this being a *total* order matters more than its being
/// meaningful.
///
/// A missing trailing component is `0`: `"1.20"` equals `"1.20.0"`, and so does `"1.020"`, since
/// leading zeros are padding. A pre-release sorts below the release it names
/// (`"1.21.4-rc1" < "1.21.4"`), the one relation Minecraft's labels promise.
///
/// It must hold up on input [`parse_requirement`] would refuse, such as a hand-edited string
/// `cairn info` reads, and `slice::sort_by` panics on an order that is not total. So components
/// are classified before they are compared:
///
/// - A run of digits is a number, ordered by value however long the run (digit count after
///   leading zeros, then lexicographically).
/// - Anything else is ordered lexicographically among its own kind, and sorts above every number.
///
/// Mixing the two orders per pair would be intransitive: `5abc < 999 < 4294967296 < 5abc`. Which
/// side non-numbers land on is arbitrary; that they all land on one side is not.
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
/// A release outranks every pre-release of it. Two tags compare as text: `rc1` against `rc2`
/// reads correctly and `pre5` against `rc1` does not, and no text rule would; that pair is the
/// `DataVersion` table's.
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
/// Classify first, compare second: deciding per pair would let `5abc < 999` and
/// `999 < 4294967296` coexist with `5abc > 4294967296`.
fn compare_segments(l: &str, r: &str) -> Ordering {
    match (numeric_key(l), numeric_key(r)) {
        // Without leading zeros the longer run is the larger value, whatever `u32` can hold.
        (Some(l), Some(r)) => l.len().cmp(&r.len()).then_with(|| l.cmp(r)),
        // Every number sorts below every non-number, so the two orders never interleave.
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => l.cmp(r),
    }
}

/// A component's value as a comparable digit run, or `None` when it is not a number.
fn numeric_key(s: &str) -> Option<&str> {
    is_digits(s).then(|| trim_leading_zeros(s))
}

/// Whether a component is a run of ASCII digits, however long.
///
/// Empty is not a run: calling it zero would make `1.`, which the grammar refuses, the same
/// version as `1.0`.
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
/// [`super::order`] asks this before trusting [`compare_versions`] to place a label outside a
/// table's rows: two dotted decimals compare meaningfully, while a dotted decimal against a
/// snapshot label is the comparison `spec/versioning-editions` "The target is a compile-time
/// parameter" makes `DataVersion` canonical to avoid.
#[must_use]
pub(super) fn is_dotted_decimal(label: &str) -> bool {
    split_prerelease(label).1.is_none() && label.split('.').all(is_digits)
}

#[cfg(test)]
mod tests;
