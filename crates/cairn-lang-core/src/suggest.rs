//! "Did you mean ...?" lookup over a closed vocabulary, by Damerau-Levenshtein distance under a
//! cap: one that grows with the input's length, or a single edit.

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
    distance_of: impl FnMut(&str, &str) -> usize,
) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    nearest_within(
        input,
        candidates,
        max_distance(input.chars().count()),
        distance_of,
    )
}

/// The candidate closest to `input` at most `max_edits` away, the first on a tie; `None` when
/// `input` is empty or is itself a candidate.
fn nearest_within<'a, I>(
    input: &str,
    candidates: I,
    max_edits: usize,
    mut distance_of: impl FnMut(&str, &str) -> usize,
) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    if input.is_empty() {
        return None;
    }
    let input_len = input.chars().count();
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

/// The first candidate exactly one edit from `input`; `None` when there is none or `input` is
/// itself a candidate.
#[must_use]
pub(crate) fn one_edit_match<'a, I>(input: &str, candidates: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    nearest_within(input, candidates, 1, damerau_levenshtein)
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
mod tests;
