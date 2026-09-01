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
//! What the copies are *not* is redundant. Each resolution binds a
//! different theme when the placements name different ones, and two themes
//! missing the same slot are two findings an author has to fix separately.
//! So the rule is *at most once per member per bound theme* — which of the
//! resolutions produced the surviving copy is not promised, because two of
//! them can bind one theme and still judge the slot differently.
//!
//!  1. one placement — the shape the report was filed on;
//!  2. two placements under one theme — the count that used to scale;
//!  3. two placements under two themes — both findings survive, each
//!     naming its own theme;
//!  4. a def nothing places — still reported, against the theme the module
//!     auto-picks, because a file of defs and no site is a file worth
//!     checking;
//!  5. two defs sharing the bad slot name — two findings, because the rule
//!     keys on the member and not on the name;
//!  6. a placement the resolver abandons before the body — the def's own
//!     finding survives, because nothing else is left to report it;
//!  7. a nested member — the same rule inside a `level` block;
//!  8. a `struct` — the control. Nothing places a struct, so its count was
//!     never inflated and must not now deflate;
//!  9. a resolution that stayed silent followed by a stricter one — the
//!     finding the stricter one owes is still reported. This is the case
//!     that says "already reported" has to mean reported, not walked;
//! 10. the same disagreement between two *placements* rather than between
//!     the module and a placement — the shape the spec paragraph uses;
//! 11. an `--edition` pin — the path every build command actually runs,
//!     and the one where sibling softening does not apply at all;
//! 12. two `site` blocks placing one def — the ledger spans the whole
//!     resolution, not one site;
//! 13. a module with two logical themes, and one with none — the two ends
//!     the spec paragraph promises silence at;
//! 14. a theme applied only through a `place` — nothing else in the
//!     repository covers that route, and it is where a walk-memoising
//!     rewrite of this rule would drop a finding first.

use cairn_lang_core::check::{Diagnostic, DiagnosticCode};
use cairn_lang_core::{Edition, Resolution, lower, parse, resolve};

/// Resolve `src` under `edition`.
fn resolution(src: &str, edition: Option<Edition>) -> Resolution {
    let module = parse(src).expect("parse");
    resolve(&lower(&module), edition)
}

/// Every `E_UNRESOLVED_SLOT` the resolver reports for `src` under
/// `edition`, in the order it reported them.
fn unresolved_under(src: &str, edition: Option<Edition>) -> Vec<Diagnostic> {
    resolution(src, edition)
        .diagnostics
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::UnresolvedSlot)
        .collect()
}

/// [`unresolved_under`] with no edition pinned, which is what `cairn check`
/// runs by default.
fn unresolved(src: &str) -> Vec<Diagnostic> {
    unresolved_under(src, None)
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
    let spans: std::collections::HashSet<_> = found.iter().map(|d| d.span.clone()).collect();
    assert_eq!(
        spans.len(),
        2,
        "the two findings must sit on the two members that earned them",
    );
}

/// A placement with no `theme=` is dropped before the def body is
/// resolved at all, so the def's own resolution is the only one that can
/// report what is inside it.
///
/// This case says nothing about *when* the ledger is written — there is no
/// second resolution to be spoken for. What it guards is the decision to
/// keep resolving a def as its own scope: drop that and this file stops
/// being checked the moment a `place` mentions it, however broken the
/// placement is.
#[test]
fn a_placement_the_resolver_abandons_does_not_silence_the_def() {
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
        "the placement never reaches the body, so the def's own resolution \
         must still report it; got {:?}",
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

/// The two resolutions of one def body do not always judge a slot the
/// same way, which is why "already reported" is recorded where a
/// diagnostic is pushed rather than where a body is walked.
///
/// With no `--edition` pin the module-level pick binds `shop_java` and
/// unions in the slots its sibling variant declares, so `bedrock_only`
/// passes. The placement writes the variant name, which asks about that
/// variant's slots alone — no sibling softens it, and the slot is missing.
/// A ledger keyed on the walk would let the silent resolution speak for the
/// strict one and the file would build with an undeclared slot.
#[test]
fn a_stricter_later_resolution_still_reports_what_the_softer_one_passed() {
    const VARIANTS: &str = "theme shop_java:\n  slot floor -> @oak_planks\n\n\
theme shop_bedrock:\n  \
slot floor -> @oak_planks\n  \
slot bedrock_only -> @dark_oak_planks\n\n\
def hut size=4x4:\n  \
floor mat_slot=bedrock_only\n";

    // Premise: the module-level resolution alone passes the slot, so the
    // finding below can only come from the placement.
    assert!(
        unresolved(VARIANTS).is_empty(),
        "premise: an unpinned module unions its sibling variant's slots",
    );

    let found = unresolved(&format!(
        "{VARIANTS}\n\
site s:\n  \
place id=home use=hut theme=shop_java at=origin\n"
    ));
    assert_eq!(
        found.len(),
        1,
        "naming the variant asks about that variant's slots alone; got {:?}",
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

/// The two resolutions that disagree can both be placements.
///
/// `theme=shop_java` names a variant, so no sibling softens it and the
/// slot is missing; `theme=shop` names the logical theme, so the sibling's
/// slots are unioned in and the same member passes. Both bind `shop_java`.
/// The rule is therefore *at most* once per member per bound theme: which
/// of the two produced the surviving copy is not something the resolver
/// decides, and reversing the placements must not change the count.
#[test]
fn two_placements_can_bind_one_theme_and_still_disagree() {
    const VARIANTS: &str = "theme shop_java:\n  slot floor -> @oak_planks\n\n\
theme shop_bedrock:\n  \
slot floor -> @oak_planks\n  \
slot bedrock_only -> @dark_oak_planks\n\n\
def hut size=4x4:\n  \
floor mat_slot=bedrock_only\n";

    let strict_first = unresolved(&format!(
        "{VARIANTS}\n\
site s:\n  \
place id=a use=hut theme=shop_java at=origin\n  \
place id=b use=hut theme=shop east_of=a gap=5\n"
    ));
    let softened_first = unresolved(&format!(
        "{VARIANTS}\n\
site s:\n  \
place id=b use=hut theme=shop at=origin\n  \
place id=a use=hut theme=shop_java east_of=b gap=5\n"
    ));

    for (order, found) in [
        ("strict first", &strict_first),
        ("softened first", &softened_first),
    ] {
        assert_eq!(
            found.len(),
            1,
            "{order}: one member, one bound theme, one finding; got {:?}",
            primaries(found),
        );
        assert!(
            found[0].primary.contains("theme `shop_java`"),
            "{order}: the finding names the theme both placements bound; got {:?}",
            primaries(found),
        );
    }
}

/// The `--edition` path, which is what every build command runs and what
/// the report was filed against.
///
/// A pin removes sibling softening on both routes (`resolver.rs`'s
/// module-level pick and `bind_place_theme` agree on that), so the module
/// and the placement both have grounds to report — which is exactly the
/// "once per placement plus once more" shape.
#[test]
fn the_count_holds_under_an_edition_pin() {
    const VARIANTS: &str = "theme shop_java:\n  slot floor -> @oak_planks\n\n\
theme shop_bedrock:\n  \
slot floor -> @oak_planks\n  \
slot bedrock_only -> @dark_oak_planks\n\n\
def hut size=4x4:\n  \
floor mat_slot=bedrock_only\n";

    let found = unresolved_under(
        &format!(
            "{VARIANTS}\n\
site s:\n  \
place id=home use=hut theme=shop at=origin\n"
        ),
        Some(Edition::Java),
    );
    assert_eq!(
        found.len(),
        1,
        "the pinned path is deduplicated like the unpinned one; got {:?}",
        primaries(&found),
    );
    assert!(
        found[0].primary.contains("theme `shop_java`"),
        "the pin decides which variant the finding is about; got {:?}",
        primaries(&found),
    );
}

/// The ledger spans the resolution, not one `site` body.
///
/// Two sites placing one def is the arrangement that would still pass
/// every case above if the ledger were a local of the placement loop.
#[test]
fn two_sites_placing_one_def_report_it_once() {
    let found = unresolved(&format!(
        "{ONE_THEME}\
def hut size=4x4:\n  \
floor mat_slot=missing\n\n\
site north:\n  \
place id=home use=hut theme=plain at=origin\n\n\
site south:\n  \
place id=home use=hut theme=plain at=origin\n"
    ));
    assert_eq!(
        found.len(),
        1,
        "one member and one theme, however many sites reach it; got {:?}",
        primaries(&found),
    );
}

/// Both ends of the case the spec paragraph promises silence at.
///
/// A def's own scope is resolved against the theme the module picks, and
/// the module picks one only when there is exactly one logical theme to
/// pick. With two, or with none, nothing binds and the slot names are not
/// judged until a `place` chooses a theme.
#[test]
fn an_unplaced_def_is_silent_when_the_module_cannot_pick_a_theme() {
    let two = unresolved(
        "theme alpha:\n  slot floor -> @oak_planks\n\n\
theme beta:\n  slot other -> @stone\n\n\
def hut size=4x4:\n  \
floor mat_slot=missing\n",
    );
    assert!(
        two.is_empty(),
        "two logical themes: no theme binds to the def's own scope; got {:?}",
        primaries(&two),
    );

    let none = unresolved("def hut size=4x4:\n  floor mat_slot=missing\n");
    assert!(
        none.is_empty(),
        "no themes at all: there is nothing to judge the slot against; got {:?}",
        primaries(&none),
    );
}

/// A theme applied only through a `place`, carrying one selector that
/// matches and one that does not.
///
/// Nothing else in the repository takes this route — every other selector
/// test uses the `struct` shape, which the module-level pick binds. Both
/// halves matter to the rule here, and the second more than the first:
/// `check_unmatched_selectors` skips a theme no scope applied, so a
/// rewrite that resolved each `(def, theme)` pair once and reused the
/// result would have to keep marking the theme applied on the reused path
/// or `E_THEME_SELECTOR_UNMATCHED` would stop firing for it — a finding
/// disappearing with nothing else in the suite to notice.
#[test]
fn a_theme_applied_only_through_a_placement_still_judges_its_selectors() {
    // Two logical themes, so the module-level pick binds nothing and the
    // placement is the only route into the def body.
    let resolved = resolution(
        "theme alpha:\n  \
slot wall -> @cobblestone\n  \
window[class=small] -> frame=@spruce_wood\n  \
door[class=grand] -> frame=@dark_oak_wood\n\n\
theme beta:\n  slot other -> @stone\n\n\
def hut size=6x6:\n  \
walls mat_slot=wall height=3\n  \
window class=small side=front\n\n\
site s:\n  \
place id=a use=hut theme=alpha at=origin\n",
        None,
    );

    let scope = resolved
        .scopes
        .get("site::s::a")
        .expect("the placement builds a scope");
    assert!(
        scope
            .members
            .values()
            .any(|m| m.selector_extras.contains_key("frame")),
        "the matching selector's bindings reach the member it matched",
    );

    let unmatched: Vec<&str> = resolved
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::ThemeSelectorUnmatched)
        .map(|d| d.primary.as_str())
        .collect();
    assert_eq!(
        unmatched.len(),
        1,
        "the theme was applied, so its selectors are judged; got {unmatched:?}",
    );
    assert!(
        unmatched[0].contains("door[class=grand]"),
        "and the one reported is the one that matched nothing; got {unmatched:?}",
    );
}
