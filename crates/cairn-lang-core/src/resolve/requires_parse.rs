//! Parsing for `@requires version>=X` directives and tuple-wise version
//! comparison.
//!
//! Spec context: `@requires` is a Minecraft-side capability floor (see
//! `spec/versioning-editions.md` §10.5). The compiler intersects every
//! `@requires` line with the registry-derived domain; for `cairn info` we
//! only need the **maximum** lower bound across all `@requires` lines, which
//! is the registry-compatible-range lower edge.
//!
//! Version strings are compared component-wise as a tuple of `u32` (dot
//! separated). This matches the spec note that version strings should be
//! treated as opaque labels backed by `DataVersion`-as-ordering-key, but
//! degrades gracefully to dotted-decimal ordering when `DataVersion` is
//! not available (it is not wired through yet — the table arrives with
//! the registry pack's data-version catalog).

use std::cmp::Ordering;

/// Strip the `version>=` prefix from a raw `@requires` expression and return
/// the remaining version string, trimmed of whitespace.
///
/// Returns `None` when the requirement:
///
/// - does not begin with `version>=` (future requirement shapes like
///   `version<=` or range syntax will need their own parser, so the
///   function is intentionally narrow rather than best-effort),
/// - is empty after stripping the prefix (`@requires version>=` with no
///   payload), or
/// - is not a dotted-decimal sequence of unsigned integers
///   (`version>=1.a` is not a version Cairn knows how to compare against,
///   and feeding malformed strings to [`compare_versions`] would let
///   lexicographic fallbacks produce nonsense orderings such as
///   `1.a > 1.20`).
#[must_use]
pub fn parse_min_version(raw: &str) -> Option<&str> {
    let stripped = raw.trim().strip_prefix("version>=")?.trim();
    if stripped.is_empty() {
        return None;
    }
    if !stripped
        .split('.')
        .all(|segment| !segment.is_empty() && segment.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    Some(stripped)
}

/// Compare two dotted-decimal version strings component-wise.
///
/// A missing trailing component is treated as `0`: `"1.20"` compares equal
/// to `"1.20.0"`. Non-numeric segments compare lexicographically as a
/// fallback so a malformed-but-present version never panics — at worst it
/// sorts oddly, which is preferable to crashing `cairn info` on a
/// hand-edited source file.
#[must_use]
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let mut left = a.split('.');
    let mut right = b.split('.');
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (Some(l), None) => {
                if parse_segment(l) == 0 {
                    continue;
                }
                return Ordering::Greater;
            }
            (None, Some(r)) => {
                if parse_segment(r) == 0 {
                    continue;
                }
                return Ordering::Less;
            }
            (Some(l), Some(r)) => match (l.parse::<u32>(), r.parse::<u32>()) {
                (Ok(li), Ok(ri)) => match li.cmp(&ri) {
                    Ordering::Equal => {}
                    non_eq => return non_eq,
                },
                _ => match l.cmp(r) {
                    Ordering::Equal => {}
                    non_eq => return non_eq,
                },
            },
        }
    }
}

fn parse_segment(s: &str) -> u32 {
    s.parse::<u32>().unwrap_or(0)
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
    }
}
