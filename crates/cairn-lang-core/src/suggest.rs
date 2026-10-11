//! "Did you mean ...?" candidate lookup over a closed vocabulary.
//!
//! `spec/glossary` "Fail-loud" asks an error for the valid candidates and a suggested fix; this
//! module supplies the fix for a diagnostic that reports an unknown identifier against a known
//! set.
//!
//! The distance is Damerau-Levenshtein, which scores an adjacent swap (`walsl` for `walls`) as one
//! edit where Levenshtein charges two. The cap scales with input length, so a short identifier
//! cannot suggest a candidate the author could not plausibly have meant.

use strsim::damerau_levenshtein;

use crate::diagnostic::DiagnosticNote;

/// The sentence every suggestion is phrased as, so a diagnostic, a CLI
/// report and an error's `Display` word it alike. `suggested` is spelled
/// the way the source would write it (`@token`, `place.port`, `key=`).
#[must_use]
pub fn did_you_mean(suggested: &str) -> String {
    format!("did you mean `{suggested}`?")
}

/// [`did_you_mean`] as the note every unknown-identifier diagnostic
/// attaches.
#[must_use]
pub(crate) fn did_you_mean_note(suggested: &str) -> DiagnosticNote {
    DiagnosticNote {
        span: None,
        message: did_you_mean(suggested),
    }
}

/// Maximum edit distance for a suggestion, by input length in characters. Past three edits a
/// suggestion reads as a different word.
fn max_distance(input_len: usize) -> usize {
    // `nearest_match_by` returns early on an empty input, so 0 never arrives.
    match input_len {
        1..=3 => 1,
        4..=6 => 2,
        _ => 3,
    }
}

/// The candidate closest to `input` by Damerau-Levenshtein distance, within the length-scaled
/// cap.
///
/// `None` when `input` is empty, when no candidate is within the cap, or when `input` is itself a
/// candidate, because an exact match is not a typo. On a tie the first candidate wins, so the
/// caller's candidate order is the tie-break. A case difference costs one edit: identifiers are
/// case-sensitive, and a wrong case is a typo worth catching.
#[must_use]
pub fn nearest_match<'a, I>(input: &str, candidates: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    nearest_match_by(input, candidates, damerau_levenshtein)
}

/// [`nearest_match`] with the distance as an argument, so a test can count
/// which candidates the distance is computed for.
fn nearest_match_by<'a, I>(
    input: &str,
    candidates: I,
    mut distance_of: impl FnMut(&str, &str) -> usize,
) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    if input.is_empty() {
        return None;
    }
    let input_len = input.chars().count();
    let max_edits = max_distance(input_len);
    let mut best: Option<(usize, &'a str)> = None;
    for cand in candidates {
        if cand == input {
            return None;
        }
        // The distance is at least the length difference, so this skip changes no answer and
        // saves a quadratic distance computation.
        if cand.chars().count().abs_diff(input_len) > max_edits {
            continue;
        }
        let distance = distance_of(input, cand);
        if distance > max_edits {
            continue;
        }
        match best {
            // Only a strictly nearer candidate replaces the first one found.
            Some((best_distance, _)) if distance >= best_distance => {}
            _ => best = Some((distance, cand)),
        }
    }
    best.map(|(_, c)| c)
}

/// The candidate closest to `id`, compared on the path within `id`'s own
/// namespace.
///
/// `id` and every candidate are `namespace:path`.
///
/// - The path is scored, not the id: the cap scales with length, and the ten-character
///   `minecraft:` prefix would buy a third edit the path never earned (`dirt` for `light`).
/// - Only `id`'s own namespace is searched, because a `mod:` id is not repaired by a `minecraft:`
///   one.
///
/// `None` for an id with no `namespace:` prefix; otherwise [`nearest_match`]'s answer,
/// re-prefixed.
#[must_use]
pub fn nearest_namespaced_id<'a, I>(id: &str, candidates: I) -> Option<String>
where
    I: IntoIterator<Item = &'a str>,
{
    let (namespace, path) = id.split_once(':')?;
    let paths = candidates
        .into_iter()
        .filter_map(|known| known.split_once(':'))
        .filter(|(known_namespace, _)| *known_namespace == namespace)
        .map(|(_, known_path)| known_path);
    nearest_match(path, paths).map(|best| format!("{namespace}:{best}"))
}

/// How many candidates [`candidate_list`] prints before counting the
/// rest.
///
/// A registry-pack alias group can run to sixteen ids (`light_block_0` … `_15`), and printing
/// them all buries what went wrong.
pub const CANDIDATES_SHOWN: usize = 4;

/// Render a closed candidate set as the fragment a diagnostic note reads,
/// in the order given and truncated to [`CANDIDATES_SHOWN`].
///
/// The truncation is only about the sentence: the closed set an error must return
/// (`spec/versioning-editions` "Fail-loud and minimum-version inference") is in the diagnostic's
/// `data`.
#[must_use]
pub fn candidate_list(candidates: &[String]) -> String {
    let listed = candidates
        .iter()
        .take(CANDIDATES_SHOWN)
        .map(|c| format!("`{c}`"))
        .collect::<Vec<_>>()
        .join(", ");
    match candidates.len().saturating_sub(CANDIDATES_SHOWN) {
        0 => listed,
        rest => format!("{listed} and {rest} more"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYWORDS: &[&str] = &["floor", "walls", "door", "window", "roof", "stair", "level"];

    /// `block_ids` stands in for a pinned target's block table.
    #[test]
    fn a_candidate_too_far_away_in_length_is_never_scored() {
        fn answer_and_scored<'a>(input: &str, table: &'a [String]) -> (Option<&'a str>, usize) {
            let mut scored = 0_usize;
            let answer = nearest_match_by(input, table.iter().map(String::as_str), |a, b| {
                scored += 1;
                damerau_levenshtein(a, b)
            });
            (answer, scored)
        }
        let block_ids: Vec<String> = (0..2_000).map(|i| format!("block_number_{i}")).collect();
        let (answer, scored) = answer_and_scored(&"w".repeat(1_000), &block_ids);
        assert_eq!(answer, None);
        assert_eq!(
            scored, 0,
            "a candidate far shorter than the input was scored"
        );
        let long_ids: Vec<String> = block_ids
            .iter()
            .map(|id| format!("{id}{}", "w".repeat(1_000)))
            .collect();
        let (answer, scored) = answer_and_scored("wall", &long_ids);
        assert_eq!(answer, None);
        assert_eq!(
            scored, 0,
            "a candidate far longer than the input was scored"
        );
    }

    /// Inputs a few characters longer or shorter than a candidate sit on the prune's edge, where
    /// an off-by-one would drop a match.
    #[test]
    fn the_length_prune_agrees_with_scoring_every_candidate() {
        /// Scores every candidate; the first of equally near ones wins.
        fn unpruned<'a>(input: &str, candidates: &[&'a str]) -> Option<&'a str> {
            if input.is_empty() {
                return None;
            }
            let max_edits = max_distance(input.chars().count());
            let mut best: Option<(usize, &'a str)> = None;
            for &cand in candidates {
                if cand == input {
                    return None;
                }
                let distance = damerau_levenshtein(input, cand);
                if distance <= max_edits && best.is_none_or(|(b, _)| distance < b) {
                    best = Some((distance, cand));
                }
            }
            best.map(|(_, c)| c)
        }
        let candidates = [
            "a",
            "ab",
            "abc",
            "abcd",
            "abcde",
            "abcdef",
            "abcdefg",
            "abcdefgh",
            "abcdefghi",
            "abcdefghij",
            "floor",
            "walls",
            "window",
            "oak_planks",
            "stone_bricks",
            // Three chars in nine bytes: a prune measuring bytes would drop it for `日本語z`.
            "日本語",
        ];
        let mut inputs: Vec<String> = Vec::new();
        for cand in candidates {
            for extra in 0..=4 {
                inputs.push(format!("{cand}{}", "z".repeat(extra)));
                inputs.push(cand.chars().skip(extra).collect());
            }
        }
        for input in &inputs {
            assert_eq!(
                nearest_match(input, candidates.iter().copied()),
                unpruned(input, &candidates),
                "the prune changed the answer for `{input}`",
            );
        }
    }

    #[test]
    fn one_letter_deletion_suggests_match() {
        assert_eq!(
            nearest_match("wals", KEYWORDS.iter().copied()),
            Some("walls")
        );
    }

    #[test]
    fn adjacent_swap_counts_as_one_edit() {
        // A three-character input allows one edit, and Levenshtein charges a swap two.
        assert_eq!(nearest_match("dfe", ["def"]), Some("def"));
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
        // Far beyond the cap of 2 for a five-character input.
        assert!(nearest_match("xyzzy", KEYWORDS.iter().copied()).is_none());
    }

    #[test]
    fn exact_match_returns_none() {
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
        assert_eq!(nearest_match("Walls", ["walls"].into_iter()), Some("walls"),);
    }

    #[test]
    fn tie_break_prefers_first_in_iteration_order() {
        // Both candidates are one edit from `aa`.
        let cands: &[&str] = &["ab", "ba"];
        assert_eq!(nearest_match("aa", cands.iter().copied()), Some("ab"));
    }

    #[test]
    fn a_three_character_input_allows_one_edit() {
        assert_eq!(nearest_match("abc", ["abd"]), Some("abd"));
        assert_eq!(nearest_match("abc", ["axy"]), None);
    }

    fn owned(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| (*id).to_owned()).collect()
    }

    #[test]
    fn a_short_candidate_list_is_printed_whole() {
        assert_eq!(
            candidate_list(&owned(&["minecraft:oak_sign", "minecraft:spruce_sign"])),
            "`minecraft:oak_sign`, `minecraft:spruce_sign`",
        );
    }

    #[test]
    fn a_long_candidate_list_counts_the_rest() {
        let ids: Vec<String> = (0..16)
            .map(|i| format!("minecraft:light_block_{i}"))
            .collect();
        assert_eq!(
            candidate_list(&ids),
            "`minecraft:light_block_0`, `minecraft:light_block_1`, \
             `minecraft:light_block_2`, `minecraft:light_block_3` and 12 more",
        );
    }

    #[test]
    fn an_empty_candidate_list_renders_empty() {
        // Callers check for an empty set first, but a bug must still not render "and 0 more".
        assert_eq!(candidate_list(&[]), "");
    }

    #[test]
    fn long_input_admits_three_edits() {
        // Three substitutions, within the cap of 3 for seven characters or more.
        let cands: &[&str] = &["minecraft"];
        assert_eq!(
            nearest_match("minexyzft", cands.iter().copied()),
            Some("minecraft"),
        );
    }
}
