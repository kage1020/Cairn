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
        "E_DUPLICATE_SIZE",
        "E_INVALID_PLACE_ID",
        "E_MISPLACED_MEMBER",
        "E_THEME_SELECTOR_UNMATCHED",
        "E_UNEXPECTED_POSITIONAL",
        "E_UNKNOWN_KEYWORD",
        "E_UNKNOWN_SLOT_TARGET",
        "E_UNRESOLVED_PLACE_REF",
        "E_UNRESOLVED_SLOT",
        "E_UNSUPPORTED_NESTING",
        "W_DEFERRED_MEMBER",
        "W_STRUCTURE_TOO_LARGE",
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
