//! A `def` member's resolver findings are reported at most once per member per
//! bound theme. A def body is resolved once as its own scope and once per
//! `place`, so without the rule a bad slot counts one plus the placements; two
//! themes missing one slot are still two findings, and which resolution's copy
//! survives is not promised.

use cairn_lang_core::check::{Diagnostic, DiagnosticCode};
use cairn_lang_core::{Edition, Resolution, lower, parse, resolve};

use crate::common::primaries;

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
/// A pin removes sibling softening on both routes (the module-level pick
/// and the `place` row's theme agree on that), so the module
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
/// unmatched selectors are judged only for a theme some scope applied, so a
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

mod lowering {
    //! A `def` member's lowering findings are reported once however many
    //! placements reach them. A finding's identity is the whole diagnostic
    //! (code, span, message, notes and data), so a finding that belongs to one
    //! placement survives by carrying it in its span or its message.

    use cairn_lang_core::check::{Diagnostic, DiagnosticCode};

    use crate::common::{lowered, primaries};

    /// Just the findings of one code.
    fn of_code(src: &str, code: DiagnosticCode) -> Vec<Diagnostic> {
        lowered(src)
            .diagnostics
            .into_iter()
            .filter(|d| d.code == code)
            .collect()
    }

    /// `E_INCOMPATIBLE_MATERIAL` only — a `gable` roof bound to a material
    /// that cannot carry `facing` / `half` / `shape`.
    fn incompatible(src: &str) -> Vec<Diagnostic> {
        of_code(src, DiagnosticCode::IncompatibleMaterial)
    }

    /// One theme whose roof slot is bound to planks, which is not a stair.
    const PLAIN: &str = "theme plain:\n  \
slot wall -> @cobblestone\n  \
slot roof -> @spruce_planks\n\n";

    /// A def whose `roof` reads that slot.
    const HUT: &str = "def hut size=6x6:\n  \
walls mat_slot=wall height=3\n  \
roof kind=gable mat_slot=roof overhang=1\n\n";

    #[test]
    fn one_placement_reports_the_bad_roof_slot_once() {
        let found = incompatible(&format!(
            "{PLAIN}{HUT}\
site s:\n  \
place id=a use=hut theme=plain at=origin\n"
        ));
        assert_eq!(
            found.len(),
            1,
            "baseline: one placement, one finding; got {:?}",
            primaries(&found),
        );
    }

    #[test]
    fn two_placements_under_one_theme_do_not_multiply_the_finding() {
        let found = incompatible(&format!(
            "{PLAIN}{HUT}\
site s:\n  \
place id=a use=hut theme=plain at=origin\n  \
place id=b use=hut theme=plain east_of=a gap=8\n"
        ));
        assert_eq!(
            found.len(),
            1,
            "both placements read one slot line and there is one edit to make; \
         got {:?}",
            primaries(&found),
        );
    }

    /// Two is the count the report was filed on; three is what says the rule
    /// is a rule and not an off-by-one.
    #[test]
    fn the_count_does_not_scale_with_the_placement_list() {
        let found = incompatible(&format!(
            "{PLAIN}{HUT}\
site s:\n  \
place id=a use=hut theme=plain at=origin\n  \
place id=b use=hut theme=plain east_of=a gap=8\n  \
place id=c use=hut theme=plain east_of=b gap=8\n"
        ));
        assert_eq!(
            found.len(),
            1,
            "three placements are still one slot line; got {:?}",
            primaries(&found),
        );
    }

    /// Two themes binding the roof slot to two non-stairs are two findings on
    /// two lines, and each has to name the material its own theme bound.
    ///
    /// Named rather than counted: a rule that kept the first finding per
    /// *member* would keep exactly one of these, and a count of two cannot
    /// tell "both themes survived" apart from "one theme survived twice".
    #[test]
    fn two_themes_binding_two_materials_report_both() {
        let found = incompatible(
            "theme plain:\n  \
slot wall -> @cobblestone\n  \
slot roof -> @spruce_planks\n\n\
theme other:\n  \
slot wall -> @cobblestone\n  \
slot roof -> @oak_planks\n\n\
def hut size=6x6:\n  \
walls mat_slot=wall height=3\n  \
roof kind=gable mat_slot=roof overhang=1\n\n\
site s:\n  \
place id=a use=hut theme=plain at=origin\n  \
place id=b use=hut theme=other east_of=a gap=8\n  \
place id=c use=hut theme=plain east_of=b gap=8\n",
        );
        assert_eq!(
            found.len(),
            2,
            "two themes binding a non-stair are two lines to fix, and the third \
         placement repeats the first; got {:?}",
            primaries(&found),
        );
        assert!(
            found
                .iter()
                .any(|d| d.primary.contains("minecraft:spruce_planks")),
            "the finding against `plain` must survive; got {:?}",
            primaries(&found),
        );
        assert!(
            found
                .iter()
                .any(|d| d.primary.contains("minecraft:oak_planks")),
            "the finding against `other` must survive; got {:?}",
            primaries(&found),
        );
    }

    /// Two themes that bind the roof slot to the *same* material still earn
    /// two findings, because the fix is two edits on two lines.
    ///
    /// This is the case the message alone cannot separate: both primaries name
    /// `minecraft:spruce_planks` and both carry the same two notes. Only the
    /// span differs, which is why the span is part of the identity.
    #[test]
    fn two_themes_binding_one_material_are_still_two_lines_to_fix() {
        let found = incompatible(
            "theme plain:\n  \
slot wall -> @cobblestone\n  \
slot roof -> @spruce_planks\n\n\
theme other:\n  \
slot wall -> @cobblestone\n  \
slot roof -> @spruce_planks\n\n\
def hut size=6x6:\n  \
walls mat_slot=wall height=3\n  \
roof kind=gable mat_slot=roof overhang=1\n\n\
site s:\n  \
place id=a use=hut theme=plain at=origin\n  \
place id=b use=hut theme=other east_of=a gap=8\n",
        );
        assert_eq!(
            found.len(),
            2,
            "one material, two slot lines, two edits; got {:?}",
            primaries(&found),
        );
        let spans: std::collections::HashSet<_> = found.iter().map(|d| d.span.clone()).collect();
        assert_eq!(
            spans.len(),
            2,
            "and the two findings sit on the two slot lines that earned them",
        );
    }

    /// Two defs reading one bad slot are one finding, not two.
    ///
    /// The opposite of the outer module's
    /// `two_defs_reading_the_same_missing_slot_report_twice`, and deliberately
    /// so: `E_UNRESOLVED_SLOT` is anchored on the member, because a member
    /// naming a slot the theme does not declare is that member's mistake.
    /// `E_INCOMPATIBLE_MATERIAL` is anchored on the theme's slot value,
    /// because the material is wrong for every member that reads it — which is
    /// what its second note tells the author. Two codes, two anchors, two
    /// counts.
    #[test]
    fn two_defs_reading_one_bad_slot_report_it_once() {
        let found = incompatible(&format!(
            "{PLAIN}\
def hut size=6x6:\n  \
walls mat_slot=wall height=3\n  \
roof kind=gable mat_slot=roof overhang=1\n\n\
def shed size=6x6:\n  \
walls mat_slot=wall height=3\n  \
roof kind=gable mat_slot=roof overhang=1\n\n\
site s:\n  \
place id=a use=hut  theme=plain at=origin\n  \
place id=b use=shed theme=plain east_of=a gap=8\n"
        ));
        assert_eq!(
            found.len(),
            1,
            "the finding names the slot line both defs read, and its note says \
         every member reading that slot has it too; got {:?}",
            primaries(&found),
        );
    }

    /// A finding that belongs to one `place` row survives beside an identical
    /// one on another row.
    ///
    /// `place id=a` uses a sizeless def and does not lower; `b` takes its
    /// origin from `a` and `c` from `b`, so both earn the same deferral with
    /// byte-identical primary and note. They are two rows the author has to
    /// fix and the spans are the only thing that says so — a dedup keyed on
    /// the message, or on the code, would report one and silently drop the
    /// other.
    #[test]
    fn two_place_rows_earning_one_message_both_survive() {
        let found = of_code(
            &format!(
                "{PLAIN}\
def nosize:\n  \
walls mat_slot=wall height=3\n\n\
def hut size=4x4:\n  \
walls mat_slot=wall height=3\n\n\
site s:\n  \
place id=a use=nosize theme=plain at=origin\n  \
place id=b use=hut theme=plain east_of=a gap=5\n  \
place id=c use=hut theme=plain east_of=b gap=5\n"
            ),
            DiagnosticCode::DeferredMember,
        );
        let cascaded: Vec<&Diagnostic> = found
            .iter()
            .filter(|d| d.primary.contains("cannot be resolved"))
            .collect();
        assert_eq!(
            cascaded.len(),
            2,
            "two `place` rows lost their origin and both have to be fixed; \
         got {:?}",
            primaries(&found),
        );
        // Premise for the case: without this the test would pass on a dedup
        // that happened to keep both for some other reason.
        assert_eq!(
            cascaded[0].primary, cascaded[1].primary,
            "the two messages are identical, so only the span separates them",
        );
        assert_ne!(
            cascaded[0].span, cascaded[1].span,
            "and the spans are what keeps them apart",
        );
    }

    /// Two members of different geometry kinds reading one theme slot earn two
    /// findings on that one line, and two placements do not make four.
    ///
    /// The span here is the slot value both members reached through, so the
    /// message is the only thing that separates them: a `gable` roof takes its
    /// states "from the geometry" and an eave `stair` takes them "from its own
    /// arguments". An identity of `(code, span)` would report one of these and
    /// drop the other — the author would fix the roof, rebuild, and meet the
    /// eave.
    #[test]
    fn two_geometry_kinds_reading_one_slot_report_once_each() {
        let src = "theme plain:\n  \
slot wall -> @cobblestone\n  \
slot trim -> @spruce_planks\n\n\
def hut size=6x6:\n  \
walls mat_slot=wall height=3\n  \
roof kind=gable mat_slot=trim overhang=1\n  \
stair kind=stairs mat_slot=trim side=front\n\n\
site s:\n  \
place id=a use=hut theme=plain at=origin\n  \
place id=b use=hut theme=plain east_of=a gap=8\n";
        let found = incompatible(src);
        assert_eq!(
            found.len(),
            2,
            "one slot line, two geometry kinds, two placements: two findings; \
         got {:?}",
            primaries(&found),
        );
        assert!(
            found.iter().any(|d| d.primary.starts_with("`gable` roof")),
            "the roof's finding must survive; got {:?}",
            primaries(&found),
        );
        assert!(
            found.iter().any(|d| d.primary.starts_with("eave `stair`")),
            "and so must the eave's; got {:?}",
            primaries(&found),
        );
        // The premise the case rests on: both sit on the same slot value, so
        // nothing but the message is available to keep them apart.
        assert_eq!(
            found[0].span, found[1].span,
            "both findings are anchored on the one slot line that binds the \
         material",
        );
        assert_eq!(
            found[0].notes, found[1].notes,
            "and their notes agree too, both naming `mat_slot=trim`",
        );
    }

    #[test]
    fn a_struct_still_reports_its_bad_roof_slot_once() {
        let found = incompatible(&format!(
            "{PLAIN}\
struct s size=6x6:\n  \
walls mat_slot=wall height=3\n  \
roof kind=gable mat_slot=roof overhang=1\n"
        ));
        assert_eq!(
            found.len(),
            1,
            "nothing places a struct, so its count was never inflated; got {:?}",
            primaries(&found),
        );
    }

    /// Two structs reading one bad slot are one finding for the same reason
    /// two defs are: the anchor is the slot line, not the member.
    ///
    /// The control against the opposite failure. A struct body is walked once
    /// each, so neither finding is a repeat of the other in the way a second
    /// placement's is — and the rule still collapses them, because they are
    /// byte-identical and there is one line to edit. If this ever needs to be
    /// two, the anchor is what has to change, not the rule.
    #[test]
    fn two_structs_reading_one_bad_slot_report_it_once() {
        let found = incompatible(&format!(
            "{PLAIN}\
struct a size=6x6:\n  \
walls mat_slot=wall height=3\n  \
roof kind=gable mat_slot=roof overhang=1\n\n\
struct b size=6x6:\n  \
walls mat_slot=wall height=3\n  \
roof kind=gable mat_slot=roof overhang=1\n"
        ));
        assert_eq!(
            found.len(),
            1,
            "one slot line, one edit, whatever reads it; got {:?}",
            primaries(&found),
        );
    }

    /// Two sibling-variant themes deferring one slot report once each, naming
    /// the theme that deferred.
    ///
    /// With no `--edition` pin, a slot only the `_bedrock` sibling declares
    /// is softened at the resolver, so lowering is the only reporter, and it
    /// anchors on the `def` body's member line, which every placement shares.
    /// The theme in the message is what keeps the two findings apart:
    /// without it this rule would keep one, and `cairn lower` would say the
    /// same thing whether both themes were broken or only one.
    #[test]
    fn two_sibling_variant_themes_deferring_one_slot_report_both() {
        const VARIANTS: &str = "theme alpha_java:\n  \
slot wall -> @cobblestone\n\n\
theme alpha_bedrock:\n  \
slot wall -> @cobblestone\n  \
slot rooftrim -> @spruce_stairs\n\n\
theme beta_java:\n  \
slot wall -> @cobblestone\n\n\
theme beta_bedrock:\n  \
slot wall -> @cobblestone\n  \
slot rooftrim -> @spruce_stairs\n\n\
def hut size=9x7:\n  \
walls mat_slot=wall height=3\n  \
roof kind=gable mat_slot=rooftrim overhang=1\n\n\
site s:\n  \
place id=a use=hut theme=alpha at=origin\n  \
place id=b use=hut theme=beta east_of=a gap=4\n";

        let deferred: Vec<Diagnostic> = of_code(VARIANTS, DiagnosticCode::DeferredMember)
            .into_iter()
            .filter(|d| d.primary.contains("did not resolve to a block id"))
            .collect();
        assert_eq!(
            deferred.len(),
            2,
            "two themes deferred the slot and both have to be fixed; got {:?}",
            primaries(&deferred),
        );
        assert!(
            deferred.iter().any(|d| d.primary.contains("`alpha_java`")),
            "the finding against `alpha_java` must survive; got {:?}",
            primaries(&deferred),
        );
        assert!(
            deferred.iter().any(|d| d.primary.contains("`beta_java`")),
            "the finding against `beta_java` must survive; got {:?}",
            primaries(&deferred),
        );
        // The premise: both sit on the one `roof` line in the `def` body, so the
        // theme in the message is the only thing separating them.
        assert_eq!(
            deferred[0].span, deferred[1].span,
            "both are anchored on the shared member line",
        );

        // And the count moves when one of the two is repaired, which is what the
        // author needs it to do.
        let alpha_fixed = VARIANTS.replace(
            "theme alpha_java:\n  slot wall -> @cobblestone",
            "theme alpha_java:\n  slot wall -> @cobblestone\n  slot rooftrim -> @spruce_stairs",
        );
        let after: Vec<Diagnostic> = of_code(&alpha_fixed, DiagnosticCode::DeferredMember)
            .into_iter()
            .filter(|d| d.primary.contains("did not resolve to a block id"))
            .collect();
        assert_eq!(
            after.len(),
            1,
            "repairing one theme leaves the other; got {:?}",
            primaries(&after),
        );
        assert!(
            after[0].primary.contains("`beta_java`"),
            "and the one left is the one still broken; got {:?}",
            primaries(&after),
        );
    }

    /// Two findings that agree on code, span, primary and data, and differ only
    /// inside a note, are two findings.
    ///
    /// `W_PHASE_CONFLICT` fans in on the member that overrides: one `walls` row
    /// overwriting two earlier ones earns two warnings on its own line, with
    /// the same voxel count in both primaries and `data: None` on both. The
    /// only thing that separates them is `notes[0].span`, which points at the
    /// member each one is about — two different lines to move.
    ///
    /// So the notes are load-bearing, and so is the code beside them. No
    /// `place` is involved: a struct body is walked once, and the pass emits
    /// both of these inside that one walk.
    #[test]
    fn two_phase_conflicts_differing_only_in_a_note_both_survive() {
        let found = of_code(
            "theme t:\n  \
slot a -> @cobblestone\n  \
slot b -> @oak_planks\n  \
slot c -> @stone\n\n\
struct tower size=6x6:\n  \
walls mat_slot=a height=2\n  \
level y=2\n    \
walls mat_slot=b height=2\n  \
walls mat_slot=c height=4\n",
            DiagnosticCode::PhaseConflict,
        );
        assert_eq!(
            found.len(),
            2,
            "the overriding member conflicts with two earlier ones, and each is \
         a line to move; got {:?}",
            primaries(&found),
        );
        // The premises, asserted rather than assumed: everything outside the
        // notes agrees, so a rule that dropped the notes from the identity —
        // or the code — would report one of these and lose the other.
        assert_eq!(found[0].span, found[1].span, "same overriding member line");
        assert_eq!(found[0].primary, found[1].primary, "same primary");
        assert_eq!(found[0].data, found[1].data, "same payload");
        assert_ne!(
            found[0].notes, found[1].notes,
            "and the notes are the whole difference: each points at the member \
         it overwrote",
        );
    }

    /// A sizeless `def` placed twice is one finding, on the `def` line.
    ///
    /// The collapse case with no theme in it: the finding is anchored on the
    /// def header, so every placement that fails to derive a footprint
    /// produces a copy. One header is one thing to fix. The `place` rows that
    /// lost their origin are a separate finding each, which
    /// `two_place_rows_earning_one_message_both_survive` covers.
    #[test]
    fn a_sizeless_def_placed_twice_reports_its_header_once() {
        let found = of_code(
            &format!(
                "{PLAIN}\
def nosize:\n  \
walls mat_slot=wall height=3\n\n\
site s:\n  \
place id=a use=nosize theme=plain at=origin\n  \
place id=b use=nosize theme=plain at=origin\n"
            ),
            DiagnosticCode::DefNoSize,
        );
        assert_eq!(
            found.len(),
            1,
            "one `def` header is one `size=WxH` to add; got {:?}",
            primaries(&found),
        );
    }
}
