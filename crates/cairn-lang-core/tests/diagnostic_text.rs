//! Diagnostic prose has to read as prose.
//!
//! Multi-line string literals in this crate are joined with a trailing `\`,
//! which swallows the newline and the next line's indentation. Drop the
//! backslash and the literal keeps both — the message still compiles, still
//! says the right thing, and renders with a twenty-space gap in the middle
//! of a sentence. `rustfmt` does not touch string contents and `clippy` has
//! no lint for it, so CI stayed green through three of these at once.
//!
//! Checking the rendered text is the only place the mistake is visible.

mod diagnostic_corpus;

use std::collections::BTreeSet;

use diagnostic_corpus::{diagnostics_for, noisy_sources};

/// Every string a diagnostic renders, tagged with where it came from.
fn rendered_strings() -> Vec<(String, String)> {
    let mut out = Vec::new();
    for source in noisy_sources() {
        for d in diagnostics_for(&source) {
            let code = d.code.as_str();
            out.push((format!("{code} primary"), d.primary.clone()));
            for (i, note) in d.notes.iter().enumerate() {
                out.push((format!("{code} note {i}"), note.message.clone()));
            }
        }
    }
    assert!(
        out.len() > 20,
        "the fixtures should light up a broad slice of the surface, got {} strings",
        out.len(),
    );
    out
}

#[test]
fn no_diagnostic_text_carries_a_run_of_spaces() {
    for (origin, text) in rendered_strings() {
        assert!(
            !text.contains("  "),
            "{origin} renders a run of spaces, which is a dropped `\\` line \
             continuation in the literal: {text:?}",
        );
    }
}

#[test]
fn no_diagnostic_text_carries_a_raw_newline_or_tab() {
    // The renderer puts each diagnostic on its own line and indents notes,
    // so a literal newline inside the text breaks that shape — and a tab
    // lands at a different column in every consumer.
    for (origin, text) in rendered_strings() {
        assert!(
            !text.contains('\n') && !text.contains('\t'),
            "{origin} embeds its own line break: {text:?}",
        );
    }
}

#[test]
fn no_diagnostic_text_is_empty_or_padded() {
    for (origin, text) in rendered_strings() {
        assert!(!text.trim().is_empty(), "{origin} renders nothing");
        assert_eq!(text.trim(), text, "{origin} has leading or trailing space");
    }
}

/// The three assertions above are only as good as the corpus: a code no
/// fixture reaches has its prose unchecked. The reach is asserted rather
/// than assumed, and named, so a fixture deleted for another reason shows
/// up as a shrunken list here instead of as silent coverage loss.
///
/// This is a floor, not the full code set — the block-array codes that
/// need a registry pack, and the resolver codes that need a multi-theme
/// file, are not all represented. Severity is deliberately not among the
/// properties guarded here: `Diagnostic::severity` reads the ledger, so
/// there is no per-diagnostic value left for a fixture to catch.
/// The codes are a floor and not the whole story: one code can render
/// several different sentences, and a fixture that stops reaching one of
/// them takes its prose out of the checks above without changing the code
/// set at all.
///
/// `W_IGNORED_ARGUMENT` is the code with the most: a primary per shape of
/// finding, and for an unreadable value a note per member and per outcome.
/// The two-repair-site branch is the one this pins: it is the only finding
/// in the set that names a second argument, so its sentence is the one a
/// subset check over codes cannot see go missing. The notes an unreadable
/// value can carry are pinned below.
#[test]
fn the_corpus_reaches_the_two_repair_site_branch_of_the_ignored_argument_prose() {
    let reached = rendered_strings().into_iter().any(|(origin, text)| {
        origin.starts_with("W_IGNORED_ARGUMENT")
            && text.contains("either argument may be the repair")
    });
    assert!(
        reached,
        "no fixture reaches the branch of `W_IGNORED_ARGUMENT` that names both repair sites",
    );
}

/// An unreadable value's note says what the default did, that the member
/// is not built either way, or that it was refused instead of given the
/// default, in words written per member. Each is its own literal, and so
/// its own chance at a dropped continuation; the two `i32` range refusals
/// beside them are assembled from a piece per end of the body, one of them
/// continued.
#[test]
fn the_corpus_reaches_every_note_an_unreadable_value_carries() {
    let rendered = rendered_strings();
    for (code, sentence) in [
        (
            "W_IGNORED_ARGUMENT",
            "the row is placed as `gap=0` places it",
        ),
        ("W_IGNORED_ARGUMENT", "this row is not placed either way"),
        (
            "W_IGNORED_ARGUMENT",
            "this row is not placed at `gap=0`, the value its origin was worked out with",
        ),
        ("W_IGNORED_ARGUMENT", "the stair is built with the default"),
        ("W_IGNORED_ARGUMENT", "this stair is not built either way"),
        (
            "W_IGNORED_ARGUMENT",
            "the window is drawn without its mirror",
        ),
        ("W_IGNORED_ARGUMENT", "this window is not cut either way"),
        (
            "W_IGNORED_ARGUMENT",
            "this window has `repeat=` greater than 1, which `sym=true` does not yet support, \
             so it is not cut on the `sym=false` default while `sym=` is unreadable",
        ),
        ("W_DEFERRED_MEMBER", "this placement's origin works out to"),
        ("W_DEFERRED_MEMBER", "this placement's body reaches"),
        (
            "W_DEFERRED_MEMBER",
            "; shrink the body with its `def`'s `size=` or a roof's `overhang=`, or shorten the \
             `gap=` on this row or on a row it is placed relative to",
        ),
    ] {
        assert!(
            rendered
                .iter()
                .any(|(origin, text)| origin.starts_with(code) && text.contains(sentence)),
            "no fixture reaches the `{code}` text `{sentence}`",
        );
    }
}

/// A `struct` / `def` header finding is worded apart from a member's:
/// `E_UNKNOWN_ARGUMENT` names the header rather than a keyword's arguments,
/// and the header's unreached `class=` has a primary and a note of its own.
/// Both codes are reached by member findings too, so the code set above
/// would stay whole if the header fixtures stopped reaching these.
#[test]
fn the_corpus_reaches_the_header_vocabulary_prose() {
    let rendered = rendered_strings();
    for (origin, sentence) in [
        (
            "E_UNKNOWN_ARGUMENT primary",
            "`siz=` is not an argument a `struct` header reads",
        ),
        (
            "W_IGNORED_ARGUMENT primary",
            "`class=` is an argument a `def` header takes and no pass reads yet",
        ),
        (
            "W_IGNORED_ARGUMENT note 0",
            "the `def` is built without it — remove the argument",
        ),
    ] {
        assert!(
            rendered
                .iter()
                .any(|(o, text)| o == origin && text.contains(sentence)),
            "no fixture reaches the {origin} `{sentence}`",
        );
    }
}

#[test]
fn the_corpus_reaches_the_codes_its_prose_assertions_are_written_for() {
    let mut seen: BTreeSet<&'static str> = BTreeSet::new();
    for source in noisy_sources() {
        for d in diagnostics_for(&source) {
            seen.insert(d.code.as_str());
        }
    }
    let expected: BTreeSet<&'static str> = [
        "E_DUPLICATE_HEADER",
        "E_DUPLICATE_ID",
        "E_DUPLICATE_ITEM",
        "E_DUPLICATE_SELECTOR",
        "E_DUPLICATE_SIZE",
        "E_INCOMPLETE_PLACE",
        "E_INVALID_PLACE_ID",
        "E_MISPLACED_MEMBER",
        "E_THEME_SELECTOR_UNMATCHED",
        "E_TRUTH_TABLE_CONFLICT",
        "E_TRUTH_TABLE_EMPTY",
        "E_UNEXPECTED_POSITIONAL",
        "E_UNKNOWN_ARGUMENT",
        "E_UNKNOWN_KEYWORD",
        "E_UNKNOWN_SLOT_TARGET",
        "E_UNRESOLVED_PLACE_REF",
        "E_UNRESOLVED_SLOT",
        "E_UNSUPPORTED_NESTING",
        "W_DEFERRED_MEMBER",
        "W_IGNORED_ARGUMENT",
        "W_STRUCTURE_TOO_LARGE",
        "W_STRUCT_NO_SIZE",
        "W_TRUTH_TABLE_DUPLICATE_ROW",
        "W_TRUTH_TABLE_PARTIAL",
        "W_UNUSED_DEF",
    ]
    .into_iter()
    .collect();
    let missing: Vec<&str> = expected.difference(&seen).copied().collect();
    assert!(
        missing.is_empty(),
        "the corpus stopped reaching {missing:?}; it reached {seen:?}",
    );
}
