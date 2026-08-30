//! A `def` member's resolver diagnostics are reported once per member per
//! theme.
//!
//! A def body is walked once as its own scope and once more per `place`
//! that instantiates it, and every walk resolves the same `mat_slot=`
//! against a theme. Left alone that makes the count of a bad slot "one
//! plus the number of placements", which is not a fact about the source —
//! the author sees the same line, the same code and the same note with
//! nothing to tell the copies apart.
//!
//! What the copies are *not* is redundant. Each walk binds a different
//! theme when the placements name different ones, and two themes missing
//! the same slot are two findings an author has to fix separately. So the
//! rule is per member **and** per theme, and this file pins both halves:
//!
//! 1. one placement — the shape the report was filed on;
//! 2. two placements under one theme — the count that used to scale;
//! 3. two placements under two themes — both findings survive, each
//!    naming its own theme;
//! 4. a def nothing places — still reported, against the theme the module
//!    auto-picks, because a file of defs and no site is a file worth
//!    checking;
//! 5. two defs sharing the bad slot name — two findings, because the rule
//!    keys on the member and not on the name;
//! 6. a placement the resolver abandons — the def's own finding is still
//!    reported, because "already said" has to mean said, not walked;
//! 7. a nested member — the same rule inside a `level` block;
//! 8. a `struct` — the control. Nothing places a struct, so its count was
//!    never inflated and must not now deflate.

use cairn_lang_core::check::{Diagnostic, DiagnosticCode};
use cairn_lang_core::{lower, parse, resolve};

/// Every `E_UNRESOLVED_SLOT` the resolver reports for `src`, in the order
/// it reported them.
fn unresolved(src: &str) -> Vec<Diagnostic> {
    let module = parse(src).expect("parse");
    let ir = lower(&module);
    resolve(&ir, None)
        .diagnostics
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::UnresolvedSlot)
        .collect()
}

/// The primaries of `found`, so a failing count shows what was reported
/// rather than only how much of it there was.
fn primaries(found: &[Diagnostic]) -> Vec<&str> {
    found.iter().map(|d| d.primary.as_str()).collect()
}

const ONE_THEME: &str = "theme plain:\n  slot floor -> @oak_planks\n\n";

#[test]
fn a_placed_def_reports_a_bad_slot_once() {
    let found = unresolved(&format!(
        "{ONE_THEME}\
def hut size=4x4:\n  \
floor mat_slot=missing\n\n\
site s:\n  \
place id=home use=hut theme=plain at=origin\n"
    ));
    assert_eq!(
        found.len(),
        1,
        "the def's own walk and the placement's walk bind the same theme, \
         so they are one finding; got {:?}",
        primaries(&found),
    );
}

#[test]
fn two_placements_under_one_theme_do_not_multiply_the_finding() {
    let found = unresolved(&format!(
        "{ONE_THEME}\
def hut size=4x4:\n  \
floor mat_slot=missing\n\n\
site s:\n  \
place id=home use=hut theme=plain at=origin\n  \
place id=away use=hut theme=plain east_of=home gap=5\n"
    ));
    assert_eq!(
        found.len(),
        1,
        "the count must not scale with the number of placements; got {:?}",
        primaries(&found),
    );
}

#[test]
fn two_placements_under_two_themes_report_both_themes() {
    let found = unresolved(
        "theme alpha:\n  slot floor -> @oak_planks\n\n\
theme beta:\n  slot other -> @stone\n\n\
def hut size=4x4:\n  \
floor mat_slot=missing\n\n\
site s:\n  \
place id=a use=hut theme=alpha at=origin\n  \
place id=b use=hut theme=beta east_of=a gap=5\n",
    );
    assert_eq!(
        found.len(),
        2,
        "two themes missing the same slot are two things to fix; got {:?}",
        primaries(&found),
    );
    // Named rather than counted: a rule that deduped on the member alone
    // would keep exactly one of these, and the count on its own cannot
    // tell that apart from keeping two copies of one theme.
    assert!(
        found.iter().any(|d| d.primary.contains("theme `alpha`")),
        "the finding against `alpha` must survive; got {:?}",
        primaries(&found),
    );
    assert!(
        found.iter().any(|d| d.primary.contains("theme `beta`")),
        "the finding against `beta` must survive; got {:?}",
        primaries(&found),
    );
}

#[test]
fn a_def_nothing_places_still_reports_its_bad_slot() {
    let found = unresolved(&format!(
        "{ONE_THEME}\
def hut size=4x4:\n  \
floor mat_slot=missing\n"
    ));
    assert_eq!(
        found.len(),
        1,
        "a def is a template, but a template with a bad slot is still \
         wrong and there is no placement to say so; got {:?}",
        primaries(&found),
    );
}

#[test]
fn two_defs_reading_the_same_missing_slot_report_twice() {
    let found = unresolved(&format!(
        "{ONE_THEME}\
def hut size=4x4:\n  \
floor mat_slot=missing\n\n\
def shed size=4x4:\n  \
floor mat_slot=missing\n\n\
site s:\n  \
place id=a use=hut  theme=plain at=origin\n  \
place id=b use=shed theme=plain east_of=a gap=5\n"
    ));
    assert_eq!(
        found.len(),
        2,
        "the rule keys on the member, not on the slot name — two members \
         are two findings; got {:?}",
        primaries(&found),
    );
    assert_ne!(
        found[0].span, found[1].span,
        "the two findings must sit on the two members that earned them",
    );
}

#[test]
fn a_placement_the_resolver_abandons_does_not_silence_the_def() {
    // No `theme=`: the placement is reported as incomplete and never
    // reaches the def body, so the def's own walk is the only one left
    // that can report the slot.
    let found = unresolved(&format!(
        "{ONE_THEME}\
def hut size=4x4:\n  \
floor mat_slot=missing\n\n\
site s:\n  \
place id=home use=hut at=origin\n"
    ));
    assert_eq!(
        found.len(),
        1,
        "a walk that emitted nothing must not count as having said it; \
         got {:?}",
        primaries(&found),
    );
}

#[test]
fn a_nested_member_reports_once_as_well() {
    let found = unresolved(&format!(
        "{ONE_THEME}\
def hut size=4x4:\n  \
level y=0\n    \
floor mat_slot=missing\n\n\
site s:\n  \
place id=home use=hut theme=plain at=origin\n"
    ));
    assert_eq!(
        found.len(),
        1,
        "the rule has to reach the members a `level` block holds; got {:?}",
        primaries(&found),
    );
}

#[test]
fn a_struct_still_reports_its_bad_slot_once() {
    let found = unresolved(&format!(
        "{ONE_THEME}\
struct s size=4x4:\n  \
floor mat_slot=missing\n"
    ));
    assert_eq!(
        found.len(),
        1,
        "nothing places a struct, so its count was never inflated; \
         got {:?}",
        primaries(&found),
    );
}
