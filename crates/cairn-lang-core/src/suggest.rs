//! "Did you mean ...?" lookup over a closed vocabulary, by Damerau-Levenshtein distance under a
//! cap that grows with the input's length.

use strsim::damerau_levenshtein;

use crate::diagnostic::DiagnosticNote;

/// The sentence every suggestion is phrased as; `suggested` is spelled as the source writes it.
#[must_use]
pub fn did_you_mean(suggested: &str) -> String {
    format!("did you mean `{suggested}`?")
}

/// [`did_you_mean`] as a diagnostic note.
#[must_use]
pub(crate) fn did_you_mean_note(suggested: &str) -> DiagnosticNote {
    DiagnosticNote {
        span: None,
        message: did_you_mean(suggested),
    }
}

/// Maximum edit distance for a suggestion, by input length in characters.
fn max_distance(input_len: usize) -> usize {
    match input_len {
        1..=3 => 1,
        4..=6 => 2,
        _ => 3,
    }
}

/// The candidate closest to `input` within the cap, the first on a tie; `None` when `input` is
/// empty or is itself a candidate.
#[must_use]
pub fn nearest_match<'a, I>(input: &str, candidates: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    nearest_match_by(input, candidates, damerau_levenshtein)
}

/// [`nearest_match`] with the distance as an argument, so a test can count what is scored.
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
        // The distance is at least the length difference, so this skip changes no answer.
        if cand.chars().count().abs_diff(input_len) > max_edits {
            continue;
        }
        let distance = distance_of(input, cand);
        if distance > max_edits {
            continue;
        }
        match best {
            Some((best_distance, _)) if distance >= best_distance => {}
            _ => best = Some((distance, cand)),
        }
    }
    best.map(|(_, c)| c)
}

/// The candidate in `id`'s namespace closest to it, scored on the path alone so the prefix buys no
/// extra edits; `None` when `id` has no `namespace:` prefix.
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

/// How many candidates [`candidate_list`] prints before counting the rest.
pub const CANDIDATES_SHOWN: usize = 4;

/// Render candidates as a note fragment, truncated to [`CANDIDATES_SHOWN`]; the full set belongs in
/// the diagnostic's `data`.
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

    #[test]
    fn the_length_prune_agrees_with_scoring_every_candidate() {
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
            // Three chars in nine bytes.
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
        assert_eq!(candidate_list(&[]), "");
    }

    #[test]
    fn long_input_admits_three_edits() {
        let cands: &[&str] = &["minecraft"];
        assert_eq!(
            nearest_match("minexyzft", cands.iter().copied()),
            Some("minecraft"),
        );
    }
}
