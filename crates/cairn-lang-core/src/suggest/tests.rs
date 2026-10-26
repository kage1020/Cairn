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

#[test]
fn one_edit_match_returns_none_once_the_input_is_itself_a_candidate() {
    assert_eq!(one_edit_match("height", ["heigh", "height"]), None);
}

#[test]
fn one_edit_match_takes_the_first_of_two_one_edit_candidates() {
    assert_eq!(one_edit_match("rof", ["roof", "row"]), Some("roof"));
}

#[test]
fn one_edit_match_refuses_two_edits_on_a_long_key() {
    // `nearest_match` would take three edits on a key this long.
    assert_eq!(nearest_match("ovrhng", ["overhang"]), Some("overhang"));
    assert_eq!(one_edit_match("ovrhng", ["overhang"]), None);
    assert_eq!(one_edit_match("overhng", ["overhang"]), Some("overhang"));
}
