//! `DiagnosticCode::severity` is the ledger, and every emission has to
//! agree with it.
//!
//! Each pass builds its findings as struct literals, so the `severity`
//! field was a value each site wrote for itself — twenty-eight of them
//! across `resolve` and `block_array`. Nothing compared them to the
//! ledger, so reclassifying a code by editing `severity()` alone changed
//! the documentation and left every actual diagnostic at its old level:
//! `cairn check` would keep exiting 0 on the source that motivated the
//! change.
//!
//! The sites now read `DiagnosticCode::X.severity()`. This test is what
//! keeps the next one honest — a literal written back in is invisible in
//! review and invisible at compile time, and shows up only here.

mod diagnostic_corpus;

use std::collections::BTreeSet;

use diagnostic_corpus::{diagnostics_for, noisy_sources};

#[test]
fn every_emitted_diagnostic_matches_the_severity_ledger() {
    for source in noisy_sources() {
        for d in diagnostics_for(&source) {
            assert_eq!(
                d.severity,
                d.code.severity(),
                "{} was emitted at {:?} but the ledger says {:?}\nsource:\n{source}",
                d.code.as_str(),
                d.severity,
                d.code.severity(),
            );
        }
    }
}

/// The test above is only as strong as the corpus. A source set that
/// reached three codes would pass it while leaving the rest unchecked,
/// so the reach is asserted rather than assumed — and named, so a
/// fixture deleted for another reason shows up as a shrunken list here
/// instead of as silent coverage loss.
#[test]
fn the_corpus_reaches_a_broad_slice_of_the_code_surface() {
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
