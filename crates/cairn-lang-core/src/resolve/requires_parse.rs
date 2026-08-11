//! Parsing for `@requires version>=X` directives and tuple-wise version
//! comparison.
//!
//! Spec context: `@requires` is a Minecraft-side capability floor (see
//! `spec/versioning-editions.md` §10.4). Floors compose by taking the
//! strictest across every `@requires` line, which is the
//! registry-compatible-range lower edge `cairn info` prints and the bound
//! `cairn compile --target` is held to.
//!
//! **Ordering is dotted-decimal, and the spec says it should not be.**
//! §10.1 makes `DataVersion` the canonical ordering key precisely so that
//! ordering survives Minecraft's move from semver-ish to date-based version
//! labels. The registry pack does ship that table now
//! (`crates/cairn-lang-formats/registry-data/*/data_versions.json`), so the
//! obstacle is no longer its absence — it is coverage: the table names the
//! handful of versions the pack was built for, and a floor like
//! `version>=1.19` names a version that is not one of them and still has to
//! be ordered. Until a lookup can answer for an arbitrary label, comparison
//! stays component-wise over dotted decimals.
//!
//! Two things that convention gets wrong, both recorded in the spec's
//! "Ordering, and where it stops" and neither fixable here:
//!
//! - A date-based label ordered against a semver one.
//! - **Two editions' numbering compared as if they were one.** Java ships
//!   `1.20.4 / 1.21 / 1.21.4`, Bedrock `1.21.0 / 1.21.40 / 1.21.60`; a floor
//!   carries no edition, so `version>=1.21.4` reads as satisfied by Bedrock
//!   `1.21.40` on `40 > 4`. Whether `@requires` is edition-neutral at all is
//!   an open language question, not an oversight here.
//!
//! That is also why [`RequirementError::Component`] exists rather than a
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
#[non_exhaustive]
pub enum RequirementError {
    /// The expression does not open with the `version` subject.
    NotAVersionRequirement,
    /// A comparison operator other than `>=`, which is the only one the
    /// spec defines. Holds the operator as written.
    UnsupportedOperator(String),
    /// `version>=` with nothing after it.
    EmptyVersion,
    /// A component that is not a decimal number: empty, or holding
    /// something other than digits. Holds the whole version as written and
    /// the offending component.
    ///
    /// Separate from [`Self::ComponentTooLarge`] because the repair
    /// differs. `24w14a` is a real Minecraft snapshot label, and telling
    /// its author it "is not a number between 0 and 4294967295" describes
    /// the check rather than the problem: the version is fine, Cairn cannot
    /// order it yet.
    Component {
        /// The version string as the author wrote it.
        version: String,
        /// The dot-separated component that could not be read.
        component: String,
    },
    /// A component of digits that does not fit in `u32`, which is what the
    /// comparison orders by. Holds the whole version and the component.
    ComponentTooLarge {
        /// The version string as the author wrote it.
        version: String,
        /// The dot-separated component that overflowed.
        component: String,
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
            Self::UnsupportedOperator(_) => "unsupported_operator",
            Self::EmptyVersion => "empty_version",
            Self::Component { .. } => "component_not_a_number",
            Self::ComponentTooLarge { .. } => "component_too_large",
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
            Self::UnsupportedOperator(operator) => write!(
                f,
                "`{operator}` is not a requirement operator; `>=` is the only one defined, \
                 as in `version>=1.21`",
            ),
            Self::EmptyVersion => write!(f, "`version>=` names no version, as in `version>=1.21`"),
            Self::Component { version, component } if component.is_empty() => {
                write!(
                    f,
                    "version `{version}` has an empty component; a version is digits \
                     separated by single dots, as in `1.21.4`",
                )
            }
            Self::Component { version, component } => write!(
                f,
                "version `{version}` has the component `{component}`, which is not a decimal \
                 number. Cairn orders versions by their components, so a label it cannot read \
                 that way — a snapshot such as `24w14a`, or a pre-release suffix — cannot be \
                 used as a floor yet",
            ),
            Self::ComponentTooLarge { version, component } => write!(
                f,
                "version `{version}` has the component `{component}`, which is larger than {}",
                u32::MAX,
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
    for component in version.split('.') {
        // `is_digits` before `parse`, because integer `FromStr` accepts a
        // leading `+`: without it `version>=1.+0` parses, and `+0` is then
        // not a digit run to the comparison, which stops treating it as the
        // zero it looks like.
        if !is_digits(component) {
            return Err(RequirementError::Component {
                version: version.to_owned(),
                component: component.to_owned(),
            });
        }
        if component.parse::<u32>().is_err() {
            return Err(RequirementError::ComponentTooLarge {
                version: version.to_owned(),
                component: component.to_owned(),
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

/// Read an `@requires` expression as a version floor, or nothing.
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
/// to `"1.20.0"`, and so does `"1.020"` — leading zeros are padding, not
/// digits of the number.
///
/// This is `pub`, so it has to hold up on input [`parse_requirement`] would
/// have refused: a caller can reach it with a hand-edited string that never
/// passed through a directive, and `cairn info` describing such a file is
/// better than `cairn info` panicking on it. It is a **total order** — a
/// weaker promise than "meaningful", and the one `slice::sort_by` requires
/// on pain of a panic. Getting there needs components to be classified
/// before they are compared:
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

    /// The two component failures say different things, because the repair
    /// differs. A snapshot label is a real version Cairn cannot order; a
    /// component past `u32` is a number too big to be one.
    #[test]
    fn the_two_component_failures_read_differently() {
        let snapshot = parse_requirement("version>=24w14a").expect_err("not a number");
        let overflow = parse_requirement("version>=4294967296").expect_err("too large");
        assert_eq!(snapshot.kind(), "component_not_a_number");
        assert_eq!(overflow.kind(), "component_too_large");
        assert!(snapshot.to_string().contains("24w14a"), "{snapshot}");
        assert!(
            !snapshot.to_string().contains("4294967295"),
            "a snapshot label is not a number out of range: {snapshot}",
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
            assert_eq!(
                parse_requirement(&format!("version{operator}1.20")),
                Err(RequirementError::UnsupportedOperator(operator.to_owned())),
                "{operator}",
            );
        }
    }

    /// `4294967296` is all ASCII digits, so a digit-only check passes it
    /// through to a comparison that cannot order it.
    #[test]
    fn parse_requirement_refuses_a_component_too_large_to_order() {
        assert_eq!(
            parse_requirement("version>=4294967296"),
            Err(RequirementError::ComponentTooLarge {
                version: "4294967296".to_owned(),
                component: "4294967296".to_owned(),
            }),
        );
        // One below the boundary is a number, however unlikely.
        assert_eq!(parse_min_version("version>=4294967295"), Some("4294967295"));
    }

    /// Rust's integer `FromStr` accepts a leading `+`, so `parse::<u32>()`
    /// alone is looser than the dotted-decimal grammar this documents. The
    /// lexer refuses `+` in a `.crn` file, but both of these are `pub` and
    /// a caller can hand them any string.
    ///
    /// It is not only a tidiness point: `+0` is not a digit run to the
    /// comparison, so it stops being the zero it looks like and
    /// `1.20.+0 > 1.20`.
    #[test]
    fn parse_requirement_refuses_a_signed_component() {
        for raw in [
            "version>=+1",
            "version>=1.+0",
            "version>=+1.+20",
            "version>=-1",
        ] {
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
    /// is the cheapest way to see the difference: 21 values is 9261
    /// comparisons, run in well under a millisecond.
    #[test]
    fn compare_versions_is_a_total_order_on_input_the_grammar_refuses() {
        // Chosen to mix the classes the comparison distinguishes: short and
        // long digit runs, runs past `u32`, leading zeros, non-digits,
        // empties, and multi-component versions of each.
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
}
