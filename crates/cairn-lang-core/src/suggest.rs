//! "Did you mean ...?" candidate lookup over a closed vocabulary.
//!
//! [`nearest_match`] is the single entry point. Diagnostics that report an
//! unknown identifier against a known set (`E_UNKNOWN_KEYWORD`,
//! `E_UNRESOLVED_SLOT`, the `--target` resolver in `cairn-lang-formats`) call
//! it to attach a `did you mean X?` note. The function is fail-loud's
//! second half — `spec/glossary.md` "Fail-loud" requires errors return the
//! closed set of valid candidates *and* a suggested DSL fix; the existing
//! `expected one of: ...` notes cover the former and this module covers the
//! latter.
//!
//! Distance metric is Damerau-Levenshtein (`strsim::damerau_levenshtein`),
//! which scores a single adjacent-character swap as 1. Pure Levenshtein
//! would charge 2 for the same swap and miss the common `walsl` ↔ `walls`
//! typo. The threshold scales with input length so a 2-char identifier
//! cannot suggest a 4-char candidate the user could not plausibly have
//! meant.

use strsim::damerau_levenshtein;

/// Maximum Damerau-Levenshtein distance allowed for a suggestion, scaled by
/// the user's input length. The cut-offs (3 / 6 chars) are picked so a
/// 2-3 char identifier admits at most one edit, mid-length identifiers two,
/// and longer ones three — past which the suggestion reads as a different
/// word entirely.
fn max_distance(input_len: usize) -> usize {
    // [`nearest_match`] returns early on an empty input, so 0 never reaches
    // here; the lower bound is 1.
    match input_len {
        1..=3 => 1,
        4..=6 => 2,
        _ => 3,
    }
}

/// Return the candidate closest to `input` under Damerau-Levenshtein
/// distance, subject to the length-scaled threshold from [`max_distance`].
///
/// Returns `None` when `input` is empty, no candidate sits within the
/// threshold, or `input` exactly matches some candidate (an exact match is
/// not a typo and the caller is already in a "this should not be reported
/// at all" branch). On a distance tie the first candidate in iteration
/// order wins, so call sites controlling the candidate order
/// (`known_keywords()`, `theme.slots.keys()`) determine the tie-break.
///
/// Comparison goes through Damerau-Levenshtein character-by-character, so
/// case differences cost one edit each. DSL identifiers are case-sensitive
/// (spec `syntax.md`) — `Walls` is a different keyword from `walls` — but
/// a wrong case is exactly the typo this function is meant to catch, and
/// the distance threshold prevents the suggestion from drifting beyond
/// "one or two edits away."
#[must_use]
pub fn nearest_match<'a, I>(input: &str, candidates: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    if input.is_empty() {
        return None;
    }
    let cap = max_distance(input.chars().count());
    let mut best: Option<(usize, &'a str)> = None;
    for cand in candidates {
        if cand == input {
            // Exact match — the caller is on an error path because *something*
            // about the input was wrong (wrong scope, etc.), not because of a
            // typo. Returning the input unchanged would render as a useless
            // `did you mean \`walls\`?` next to the user's literal `walls`.
            return None;
        }
        let d = damerau_levenshtein(input, cand);
        if d > cap {
            continue;
        }
        match best {
            // Strict `<` keeps the tie-break "first wins": the candidate
            // iterator's order is the contract surface for ambiguous cases.
            Some((bd, _)) if d >= bd => {}
            _ => best = Some((d, cand)),
        }
    }
    best.map(|(_, c)| c)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYWORDS: &[&str] = &["floor", "walls", "door", "window", "roof", "stair", "level"];

    #[test]
    fn one_letter_deletion_suggests_match() {
        assert_eq!(
            nearest_match("wals", KEYWORDS.iter().copied()),
            Some("walls")
        );
    }

    #[test]
    fn adjacent_swap_suggests_match() {
        // Damerau-Levenshtein scores `wlals` ↔ `walls` as 1 (one swap of
        // positions 0-1). Pure Levenshtein would score it 2 and miss this.
        assert_eq!(
            nearest_match("wlals", KEYWORDS.iter().copied()),
            Some("walls"),
        );
    }

    #[test]
    fn one_letter_substitution_suggests_match() {
        assert_eq!(
            nearest_match("walks", KEYWORDS.iter().copied()),
            Some("walls"),
        );
    }

    #[test]
    fn distant_input_returns_none() {
        // `xyzzy` is 5 chars from every keyword in `KEYWORDS` — well above
        // the length-scaled cap of 2 for a 5-char input.
        assert!(nearest_match("xyzzy", KEYWORDS.iter().copied()).is_none());
    }

    #[test]
    fn exact_match_returns_none() {
        // The caller is already on an error path; echoing the input back as
        // a "did you mean" suggestion would mask the real cause.
        assert!(nearest_match("walls", KEYWORDS.iter().copied()).is_none());
    }

    #[test]
    fn empty_input_returns_none() {
        assert!(nearest_match("", KEYWORDS.iter().copied()).is_none());
    }

    #[test]
    fn empty_candidates_returns_none() {
        assert!(nearest_match("walls", std::iter::empty::<&str>()).is_none());
    }

    #[test]
    fn case_difference_is_a_one_edit_suggestion() {
        // DSL identifiers are case-sensitive (spec/syntax.md) — `Walls` is
        // a different keyword from `walls` — but wrong case is exactly the
        // typo we want to catch, so the suggestion fires (distance 1, well
        // inside the cap of 2 for a 5-char input).
        assert_eq!(nearest_match("Walls", ["walls"].into_iter()), Some("walls"),);
    }

    #[test]
    fn tie_break_prefers_first_in_iteration_order() {
        // Both candidates sit at distance 1 from `aa`. The contract is
        // "first candidate wins"; the call site controls suggestion order by
        // choosing the candidate iterator's order.
        let cands: &[&str] = &["ab", "ba"];
        assert_eq!(nearest_match("aa", cands.iter().copied()), Some("ab"));
    }

    #[test]
    fn short_input_has_tighter_cap() {
        // A 3-char input gets cap 1; a 2-edit candidate must not be picked.
        let cands: &[&str] = &["abcdef"];
        assert!(nearest_match("abc", cands.iter().copied()).is_none());
    }

    #[test]
    fn long_input_admits_three_edits() {
        // A 7-char input gets cap 3; three single-char substitutions match.
        let cands: &[&str] = &["minecraft"];
        // Replace three characters out of nine — distance 3, within cap.
        assert_eq!(
            nearest_match("minexyzft", cands.iter().copied()),
            Some("minecraft"),
        );
    }
}
