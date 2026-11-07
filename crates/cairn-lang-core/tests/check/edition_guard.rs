//! `E_SEMANTIC_IN_EDITION_GUARD`: an `@edition` guard holds only raw members and Tier 0 parts,
//! since the semantic layer is edition-neutral
//! (`spec/versioning-editions` "An `@edition` guard holds the escape hatch").

use cairn_lang_core::DiagnosticCode;

use crate::common::{diagnose, notes, slice};

const THEME: &str = "theme t\n  slot f -> @stone\n\n";

/// [`THEME`] and a struct whose body is a floor and then `lines`.
fn struct_with(lines: &str) -> String {
    format!("{THEME}struct s size=5x5\n  floor mat_slot=f\n{lines}")
}

/// Each finding of `source` as its code and the text it points at.
fn located(source: &str) -> Vec<(&'static str, &str)> {
    diagnose(source)
        .iter()
        .map(|d| (d.code.as_str(), slice(source, d)))
        .collect()
}

#[test]
fn a_guard_of_raw_members_is_clean() {
    let source = struct_with(
        "  @edition java\n    raw_block mat=minecraft:light[level=15] at=2,1,2\n  \
         @edition bedrock\n    raw_fill mat=minecraft:light_block_15 from=0,1,0 to=1,1,1\n",
    );
    assert_eq!(located(&source), []);
}

#[test]
fn a_guard_of_tier0_parts_is_clean() {
    let source = struct_with(
        "  @edition java\n    repeater at=1,1,1 facing=east delay=2\n    \
         comparator at=2,1,1 facing=east\n  @edition bedrock\n    redstone_torch at=3,1,1\n    \
         redstone_dust at=1,1,2\n",
    );
    assert_eq!(located(&source), []);
}

#[test]
fn a_semantic_member_under_a_guard_is_refused_at_its_line() {
    let source = struct_with(
        "  @edition java\n    walls mat_slot=f height=2\n    \
         raw_block mat=minecraft:stone at=1,0,1\n",
    );
    let found = diagnose(&source);
    let [d] = found.as_slice() else {
        panic!("expected one finding, got {found:#?}");
    };
    assert_eq!(d.code, DiagnosticCode::SemanticInEditionGuard);
    assert_eq!(slice(&source, d), "walls mat_slot=f height=2");
    assert_eq!(
        d.primary,
        "a `walls` stands under an `@edition java` guard, which holds only raw members and Tier 0 \
         parts: the semantic layer is edition-neutral",
    );
    assert_eq!(
        notes(d),
        [
            "the guard opens here",
            "dedent the line out of the guard so that every edition builds it, and bind what \
             differs per edition from a `theme NAME_java` and a `theme NAME_bedrock`; the guard is \
             for `raw_block`, `raw_fill` and the Tier 0 parts `repeater`, `comparator`, \
             `redstone_torch` and `redstone_dust`",
        ],
    );
    assert_eq!(
        d.notes[0].span.clone().map(|span| &source[span]),
        Some("@edition java"),
    );
}

#[test]
fn a_logic_or_assert_line_under_a_guard_is_refused() {
    let source = struct_with(
        "  @edition bedrock\n    logic sig.a = sig.b\n    \
         assert always(sig.a -> eventually sig.b within 4)\n    \
         assert latency(sig.b -> sig.a) <= 2\n",
    );
    let found = diagnose(&source);
    let refused: Vec<(&str, &str)> = found
        .iter()
        .map(|d| (slice(&source, d), d.primary.as_str()))
        .collect();
    assert_eq!(
        refused,
        [
            (
                "logic sig.a = sig.b",
                "a `logic` line stands under an `@edition bedrock` guard, which holds only raw \
                 members and Tier 0 parts: the semantic layer is edition-neutral",
            ),
            (
                "assert always(sig.a -> eventually sig.b within 4)",
                "an `assert` line stands under an `@edition bedrock` guard, which holds only raw \
                 members and Tier 0 parts: the semantic layer is edition-neutral",
            ),
            (
                "assert latency(sig.b -> sig.a) <= 2",
                "an `assert` line stands under an `@edition bedrock` guard, which holds only raw \
                 members and Tier 0 parts: the semantic layer is edition-neutral",
            ),
        ],
    );
}

#[test]
fn a_refused_line_takes_what_is_indented_under_it_along() {
    let source = struct_with(
        "  @edition java\n    level y=1\n      walls mat_slot=f height=2\n      \
         @edition bedrock\n        raw_block mat=minecraft:stone at=1,0,1\n",
    );
    assert_eq!(
        located(&source),
        [("E_SEMANTIC_IN_EDITION_GUARD", "level y=1")],
    );
}

#[test]
fn a_guard_under_a_level_holds_raw_members_as_one_in_the_body_does() {
    let source = struct_with(
        "  walls mat_slot=f height=3\n  level y=1\n    @edition java\n      \
         raw_block mat=minecraft:stone at=1,0,1\n",
    );
    assert_eq!(located(&source), []);
}

#[test]
fn in_a_site_body_a_place_is_refused_for_its_guard_and_a_raw_member_for_its_body() {
    let source = format!(
        "{THEME}def d size=3x3\n  floor mat_slot=f\n\nsite v\n  \
         place id=p use=d theme=t at=origin\n  @edition java\n    \
         place id=q use=d theme=t east_of=p gap=2\n    raw_block mat=minecraft:stone at=1,0,1\n"
    );
    assert_eq!(
        located(&source),
        [
            (
                "E_SEMANTIC_IN_EDITION_GUARD",
                "place id=q use=d theme=t east_of=p gap=2"
            ),
            (
                "E_MISPLACED_MEMBER",
                "raw_block mat=minecraft:stone at=1,0,1"
            ),
        ],
    );
}

#[test]
fn an_unknown_keyword_under_a_guard_is_offered_what_a_guard_body_holds() {
    let source = struct_with("  @edition java\n    raw_blok mat=minecraft:stone at=1,0,1\n");
    let found = diagnose(&source);
    let [d] = found.as_slice() else {
        panic!("expected one finding, got {found:#?}");
    };
    assert_eq!(
        (d.code, slice(&source, d), d.primary.as_str()),
        (
            DiagnosticCode::UnknownKeyword,
            "raw_blok mat=minecraft:stone at=1,0,1",
            "unknown keyword `raw_blok`"
        ),
    );
    assert_eq!(
        notes(d),
        [
            "did you mean `raw_block`?",
            "expected one of: repeater, comparator, redstone_torch, redstone_dust, raw_block, \
             raw_fill"
        ],
    );
}

#[test]
fn guards_for_different_editions_may_reuse_an_id_and_nothing_else_may() {
    let branches = struct_with(
        "  @edition java\n    raw_block id=lamp mat=minecraft:light at=1,0,1\n  \
         @edition bedrock\n    raw_block id=lamp mat=minecraft:light_block_15 at=1,0,1\n",
    );
    assert_eq!(located(&branches), []);
    let shared = struct_with(
        "  @edition java\n    raw_block id=lamp mat=minecraft:light at=1,0,1\n    \
         raw_block id=lamp mat=minecraft:glowstone at=2,0,1\n  @edition bedrock\n    \
         raw_block id=lamp mat=minecraft:light_block_15 at=1,0,1\n  \
         raw_block id=lamp mat=minecraft:glowstone at=3,0,1\n",
    );
    let at = |n: usize| shared.match_indices("id=lamp").nth(n).expect("an id").0;
    let repeats: Vec<(&str, usize, Option<usize>)> = diagnose(&shared)
        .iter()
        .map(|d| {
            (
                d.code.as_str(),
                d.span.start,
                d.notes[0].span.as_ref().map(|span| span.start),
            )
        })
        .collect();
    assert_eq!(
        repeats,
        [
            ("E_DUPLICATE_ID", at(1), Some(at(0))),
            ("E_DUPLICATE_ID", at(3), Some(at(0))),
        ],
    );
}
