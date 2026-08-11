//! Parsing for `@requires version>=X` directives and tuple-wise version
//! comparison.
//!
//! Spec context: `@requires` is a Minecraft-side capability floor (see
//! `spec/versioning-editions.md` §10.5). Floors compose by taking the
//! strictest across every `@requires` line, which is the
//! registry-compatible-range lower edge `cairn info` prints and the bound
//! `cairn compile --target` is held to.
//!
//! **Ordering is dotted-decimal, and the spec says it should not be.**
//! §10.1 makes `DataVersion` the canonical ordering key precisely so that
//! ordering survives Minecraft's move from semver-ish to date-based version
//! labels. The registry pack does ship that table now
//! (`registry-data/*/data_versions.json`), so the obstacle is no longer its
//! absence — it is coverage: the table names the handful of versions the
//! pack was built for, and a floor like `version>=1.19` names a version
//! that is not one of them and still has to be ordered. Until a lookup can
//! answer for an arbitrary label, comparison stays component-wise over
//! dotted decimals, which orders every label Cairn currently accepts and
//! will mis-order a date-based one against a semver one.
//!
//! That is also why [`RequirementError::Segment`] exists rather than a
//! lenient fallback: a label this comparison cannot order is refused at the
//! directive instead of sorting oddly three layers later.

use std::cmp::Ordering;
use std::fmt;

/// Why an `@requires` expression is not a version floor.
///
/// Carried out of [`parse_requirement`] rather than collapsed into `None`
/// so the diagnostic can say which part is wrong. "Not understood" is not
/// an actionable message for a directive whose whole job is to state one
/// constraint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequirementError {
    /// The expression does not open with the `version` subject.
    NotAVersionRequirement,
    /// A comparison operator other than `>=`, which is the only one the
    /// spec defines. Holds the operator as written.
    UnsupportedOperator(String),
    /// `version>=` with nothing after it.
    EmptyVersion,
    /// A segment that is not a number this comparison can order: empty,
    /// non-digit, or too large for `u32`. Holds the whole version as
    /// written and the offending segment.
    Segment {
        /// The version string as the author wrote it.
        version: String,
        /// The dot-separated segment that could not be read.
        segment: String,
    },
    /// Text after the version. Holds it verbatim.
    TrailingTokens(String),
}

impl fmt::Display for RequirementError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAVersionRequirement => {
                write!(f, "expected a version floor, as in `version>=1.21`")
            }
            Self::UnsupportedOperator(operator) => write!(
                f,
                "`{operator}` is not a requirement operator; `>=` is the only one defined, \
                 as in `version>=1.21`",
            ),
            Self::EmptyVersion => write!(f, "`version>=` names no version, as in `version>=1.21`"),
            Self::Segment { version, segment } => {
                if segment.is_empty() {
                    write!(f, "version `{version}` has an empty component")
                } else {
                    write!(
                        f,
                        "version `{version}` has the component `{segment}`, which is not a \
                         number between 0 and {}",
                        u32::MAX,
                    )
                }
            }
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
/// The accepted shape is the subject `version`, the operator `>=`, and a
/// dotted-decimal version, with optional whitespace between the three —
/// `version>=1.21` and `version >= 1.21` are the same requirement. Only
/// `>=` is defined (spec syntax §5.3): a floor composes with other floors
/// by taking the strictest, which no other operator does.
///
/// # Errors
///
/// Returns the part of the expression that is not a floor, so the caller
/// can name it. Every shape that is not exactly the above is an error
/// rather than a silent `None`: the directive's only purpose is to state a
/// constraint, so one that states nothing is a defect in the file.
pub fn parse_requirement(raw: &str) -> Result<&str, RequirementError> {
    let rest = raw
        .trim_start()
        .strip_prefix("version")
        .ok_or(RequirementError::NotAVersionRequirement)?
        .trim_start();
    let rest = strip_operator(rest)?.trim();
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
    for segment in version.split('.') {
        if segment.parse::<u32>().is_err() {
            return Err(RequirementError::Segment {
                version: version.to_owned(),
                segment: segment.to_owned(),
            });
        }
    }
    Ok(version)
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

/// Strip the `version>=` prefix from a raw `@requires` expression and return
/// the remaining version string.
///
/// The lenient face of [`parse_requirement`], for callers that only want
/// the floor and have no way to report why there is not one. A caller that
/// can report should use `parse_requirement`, or a malformed requirement
/// silently declares nothing — which is the defect this pair replaced.
#[must_use]
pub fn parse_min_version(raw: &str) -> Option<&str> {
    parse_requirement(raw).ok()
}

/// Compare two dotted-decimal version strings component-wise.
///
/// A missing trailing component is treated as `0`: `"1.20"` compares equal
/// to `"1.20.0"`.
///
/// This is `pub`, so it has to hold up on input [`parse_requirement`] would
/// have refused — a caller can reach it with a hand-edited string that
/// never passed through a directive. Segments too large for `u32` are
/// therefore ordered by digit count and then lexicographically, which is
/// numeric order for any run of digits: the previous fallback compared them
/// as plain strings and put `4294967296` *below* `999`. Segments that are
/// not digits at all keep the lexicographic answer, which is arbitrary but
/// total; refusing them is [`parse_requirement`]'s job, and panicking here
/// would take out `cairn info` on a file it could otherwise describe.
#[must_use]
pub fn compare_versions(a: &str, b: &str) -> Ordering {
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

/// Order one pair of dot-separated components.
fn compare_segments(l: &str, r: &str) -> Ordering {
    match (l.parse::<u32>(), r.parse::<u32>()) {
        (Ok(li), Ok(ri)) => li.cmp(&ri),
        // At least one overflowed `u32`. Both being all digits is the
        // reachable case, and there the longer run is the larger number
        // once leading zeros are out of the way.
        _ if is_digits(l) && is_digits(r) => {
            let (l, r) = (trim_leading_zeros(l), trim_leading_zeros(r));
            l.len().cmp(&r.len()).then_with(|| l.cmp(r))
        }
        _ => l.cmp(r),
    }
}

/// Whether a component is a run of ASCII digits, however long.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_min_version_strips_prefix() {
        assert_eq!(parse_min_version("version>=1.20"), Some("1.20"));
    }

    #[test]
    fn parse_min_version_tolerates_whitespace() {
        assert_eq!(parse_min_version("  version>= 1.20.4 "), Some("1.20.4"));
    }

    #[test]
    fn parse_min_version_returns_none_for_other_shapes() {
        assert_eq!(parse_min_version("version<=1.21"), None);
        assert_eq!(parse_min_version("1.20"), None);
    }

    /// The reported case: a space before the operator hid the whole floor.
    /// It is what a human writes, and `version>= 1.20` already worked, so
    /// the two spellings differed for no reason a reader could see.
    #[test]
    fn parse_min_version_accepts_whitespace_before_the_operator() {
        for raw in [
            "version >= 1.20.4",
            "version >=1.20.4",
            "version>= 1.20.4",
            "  version  >=  1.20.4  ",
        ] {
            assert_eq!(parse_min_version(raw), Some("1.20.4"), "{raw:?}");
        }
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
            Err(RequirementError::Segment {
                version: "1.a".to_owned(),
                segment: "a".to_owned(),
            }),
        );
    }

    /// An operator is read as the run of operator characters it is, so a
    /// transposed or doubled one is reported as an operator rather than as
    /// "expected a version floor" — true, but not what the author typed.
    #[test]
    fn parse_requirement_names_an_operator_it_does_not_know() {
        for operator in ["<", "<=", ">", "==", "=", "=>", "=<", "!=", "~>", "^"] {
            assert_eq!(
                parse_requirement(&format!("version{operator}1.20")),
                Err(RequirementError::UnsupportedOperator(operator.to_owned())),
                "{operator}",
            );
        }
    }

    /// The overflow the audit found. `4294967296` is all ASCII digits, so
    /// the old digit check passed it through to a comparison that could not
    /// order it.
    #[test]
    fn parse_requirement_refuses_a_segment_too_large_to_order() {
        assert_eq!(
            parse_requirement("version>=4294967296"),
            Err(RequirementError::Segment {
                version: "4294967296".to_owned(),
                segment: "4294967296".to_owned(),
            }),
        );
        // One below the boundary is a number, however unlikely.
        assert_eq!(parse_min_version("version>=4294967295"), Some("4294967295"));
    }

    #[test]
    fn parse_min_version_rejects_empty_payload() {
        // Regression: `version>=` with no body used to return `Some("")`,
        // which `derive_min_version` then propagated to `min` and produced
        // a malformed `{"min":""}` in `cairn info --format json`.
        assert_eq!(parse_min_version("version>="), None);
        assert_eq!(parse_min_version("version>=  "), None);
    }

    #[test]
    fn parse_min_version_rejects_non_numeric_segments() {
        // Regression: a non-numeric segment used to slip through and
        // `compare_versions` would fall back to ASCII compare, where
        // `'a' (0x61) > '2' (0x32)` made `1.a > 1.20`.
        assert_eq!(parse_min_version("version>=1.a"), None);
        assert_eq!(parse_min_version("version>=1.20.4-rc1"), None);
        assert_eq!(parse_min_version("version>=1..2"), None);
    }

    #[test]
    fn compare_versions_is_numeric_per_component() {
        assert_eq!(compare_versions("1.20", "1.20"), Ordering::Equal);
        assert_eq!(compare_versions("1.20", "1.20.4"), Ordering::Less);
        assert_eq!(compare_versions("1.20.4", "1.20"), Ordering::Greater);
        assert_eq!(compare_versions("1.21", "1.20.4"), Ordering::Greater);
        assert_eq!(compare_versions("1.2", "1.10"), Ordering::Less);
    }

    #[test]
    fn compare_versions_treats_trailing_zero_as_equal() {
        assert_eq!(compare_versions("1.20", "1.20.0"), Ordering::Equal);
        assert_eq!(compare_versions("1.20.0.0", "1.20"), Ordering::Equal);
        // Leading zeros are not a different number either.
        assert_eq!(compare_versions("1.20", "1.020"), Ordering::Equal);
        assert_eq!(compare_versions("1.0007", "1.7"), Ordering::Equal);
    }

    /// The reported inversion. This is `pub`, so it has to be right on
    /// input the directive would have refused: `4294967296` overflows
    /// `u32`, the old fallback compared it as a string, and `'4' < '9'`
    /// put it below `999`.
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
        // The same, in the position a version would use it.
        assert_eq!(compare_versions("1.4294967296", "1.999"), Ordering::Greater);
    }

    /// A component that is not a number at all still orders, because this
    /// function is reachable from `cairn info` on a hand-edited file and a
    /// panic there would be worse than an arbitrary answer. Which way it
    /// sorts is not a promise; that it is total, and antisymmetric, is.
    #[test]
    fn compare_versions_is_total_on_input_the_grammar_refuses() {
        for (a, b) in [("1.a", "1.20"), ("x", "1"), ("1.", "1.0"), ("", "0")] {
            assert_eq!(
                compare_versions(a, b).reverse(),
                compare_versions(b, a),
                "{a:?} vs {b:?}",
            );
        }
    }
}
