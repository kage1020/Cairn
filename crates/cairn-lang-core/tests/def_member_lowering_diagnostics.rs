//! A `def` member's *lowering* diagnostics are reported once, however many
//! placements reach them.
//!
//! Companion to `def_member_diagnostics.rs`, which covers the resolver
//! stage. The shape is the same one stage down and the fix is not: a def
//! body is voxelised once per `place` that instantiates it — and never on
//! its own, because a def has no voxels until something places it — so a
//! finding about the def, or about the theme `slot` line the def reads,
//! comes back once per placement with nothing at all to tell the copies
//! apart. `E_INCOMPATIBLE_MATERIAL` is anchored on the theme's slot value,
//! so three placements produced three findings on one line, each ending in
//! a note saying every member reading that slot has it too.
//!
//! The rule lowering settles on is that **a lowering diagnostic's identity
//! is the diagnostic**: code, span, message, notes and data. It is stated
//! once, in `lower_to_block_array`, rather than per code — the resolver's
//! `(member, slot, theme)` triple is a claim about one finding and would
//! have to be re-derived for every code lowering learns to raise, and this
//! stage has two already.
//!
//! What that rule has to keep apart is the point of most of this file. A
//! finding that genuinely belongs to one placement carries the placement
//! in its span or its message, and these cases say so from both sides:
//!
//!  1. one placement — the baseline, so a later count means something;
//!  2. two placements under one theme — the count that used to scale;
//!  3. three of them — it does not scale any faster either;
//!  4. two placements under two themes — two findings, each naming the
//!     material its own theme bound, because two themes binding a
//!     non-stair are two lines to fix;
//!  5. two placements under two themes that bind the *same* material —
//!     still two, because the two slot lines are two edits;
//!  6. two defs sharing one bad theme slot — one finding, because the
//!     finding is anchored on the slot line both defs read and the note
//!     says as much;
//!  7. a per-placement finding — two `place` rows that each earn a
//!     `W_DEFERRED_MEMBER` with byte-identical text, kept apart by their
//!     spans. This is the case a dedup keyed on the message alone would
//!     collapse, taking a row the author has to fix with it;
//!  8. two geometry kinds reading one slot — two findings on one line,
//!     kept apart by their messages, and still two after two placements;
//!  9. a `struct` — the control. Nothing places a struct, so its count was
//!     never inflated and must not now deflate;
//! 10. two structs earning the same finding — two members, two findings,
//!     because a struct body is walked once each and neither is a repeat.

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::check::{Diagnostic, DiagnosticCode};
use cairn_lang_core::{lower, parse, resolve};

/// Every diagnostic `lower_to_block_array` reports for `src`, with no
/// registry — the route `cairn lower` takes.
fn lowered(src: &str) -> Vec<Diagnostic> {
    let module = parse(src).expect("parse");
    let intent = lower(&module);
    let resolution = resolve(&intent, None);
    lower_to_block_array(&intent, &resolution, None).diagnostics
}

/// Just the findings of one code.
fn of_code(src: &str, code: DiagnosticCode) -> Vec<Diagnostic> {
    lowered(src)
        .into_iter()
        .filter(|d| d.code == code)
        .collect()
}

/// `E_INCOMPATIBLE_MATERIAL` only — a `gable` roof bound to a material
/// that cannot carry `facing` / `half` / `shape`.
fn incompatible(src: &str) -> Vec<Diagnostic> {
    of_code(src, DiagnosticCode::IncompatibleMaterial)
}

/// The primaries of `found`, so a failing count shows what was reported
/// rather than only how much of it there was.
fn primaries(found: &[Diagnostic]) -> Vec<&str> {
    found.iter().map(|d| d.primary.as_str()).collect()
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
/// The opposite of `def_member_diagnostics.rs`'s
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
