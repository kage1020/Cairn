//! Lowering with no target (`spec/lint` "Error vs warning"): the findings every target raises,
//! worded as a pinned lowering words them, and none whose verdict is a target's pack's to give.

use cairn_lang_core::block_array::{findings_without_target, lower_to_block_array};
use cairn_lang_core::{Diagnostic, Edition, lower, parse, resolve};

use crate::common::{lowered, slice};

/// `source`'s findings lowered with no target, for `edition` or for both.
fn without_target(source: &str, edition: Option<Edition>) -> Vec<Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    findings_without_target(&lower(&module), edition)
}

/// `source`'s findings lowered for `edition` with no registry pack.
fn lowered_for(source: &str, edition: Edition) -> Vec<Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    lower_to_block_array(&ir, &resolve(&ir, Some(edition)), None).diagnostics
}

/// Each finding as its code and the text it covers.
fn located<'a>(source: &'a str, found: &[Diagnostic]) -> Vec<(&'static str, &'a str)> {
    found
        .iter()
        .map(|d| (d.code.as_str(), slice(source, d)))
        .collect()
}

/// A shed roof outside the stair family, and a door whose side is a number.
const SHED_ON_COBBLESTONE: &str = concat!(
    "theme t:\n",
    "  slot wall -> @cobblestone\n",
    "  slot roof -> @cobblestone\n\n",
    "struct hut size=5x5\n",
    "  walls mat_slot=wall height=3\n",
    "  roof kind=shed slope_to=front mat_slot=roof\n",
    "  door side=3\n",
);

#[test]
fn a_finding_no_target_changes_is_the_one_a_pinned_lowering_raises() {
    let found = without_target(SHED_ON_COBBLESTONE, None);
    assert_eq!(
        located(SHED_ON_COBBLESTONE, &found),
        [
            ("E_INCOMPATIBLE_MATERIAL", "@cobblestone"),
            ("W_DEFERRED_MEMBER", "door side=3"),
        ],
    );
    for edition in [Edition::Java, Edition::Bedrock] {
        assert_eq!(
            found,
            lowered_for(SHED_ON_COBBLESTONE, edition),
            "{edition:?}"
        );
        assert_eq!(
            without_target(SHED_ON_COBBLESTONE, Some(edition)),
            found,
            "{edition:?}"
        );
    }
}

#[test]
fn an_abstract_token_is_not_deferred_and_defers_nothing_built_on_it() {
    let source = concat!(
        "theme t:\n",
        "  slot wall -> @wall.stone.cobble\n\n",
        "struct s size=5x5\n",
        "  walls mat_slot=wall height=3\n",
        "  door side=front at=center\n",
    );
    assert_eq!(
        located(source, &lowered(source).diagnostics),
        [
            ("W_ABSTRACT_TOKEN_DEFERRED", "@wall.stone.cobble"),
            ("W_DEFERRED_MEMBER", "door side=front at=center"),
        ],
        "premise: with no pack at all the token cannot paint, and the door has no wall",
    );
    assert_eq!(without_target(source, None), []);
}

#[test]
fn whether_an_abstract_token_is_a_stair_is_the_targets_to_say() {
    let source = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "  slot roof -> @wall.stone.cobble\n\n",
        "struct s size=5x5\n",
        "  walls mat_slot=wall height=3\n",
        "  roof kind=gable mat_slot=roof\n",
    );
    assert_eq!(without_target(source, None), []);
}

#[test]
fn an_id_or_token_only_a_pack_can_judge_is_not_reported() {
    let source = concat!(
        "theme t:\n",
        "  slot floor -> @totally_not_a_block\n",
        "  slot wall -> @wall.stone.cobbel\n\n",
        "struct s size=3x3\n",
        "  floor mat_slot=floor\n",
        "  walls mat_slot=wall height=2\n",
    );
    assert_eq!(without_target(source, None), []);
}

#[test]
fn a_phase_conflict_is_reported_only_between_blocks_no_target_changes() {
    let walls = |second: &str| {
        format!(
            "theme t:\n  slot a -> @cobblestone\n  slot b -> {second}\n\n\
             struct s size=3x3\n  walls mat_slot=a height=2\n  walls mat_slot=b height=2\n"
        )
    };
    let canonical = walls("@stone");
    assert_eq!(
        located(&canonical, &without_target(&canonical, None)),
        [("W_PHASE_CONFLICT", "walls mat_slot=b height=2")],
    );
    let abstract_token = walls("@wall.stone.cobble");
    assert_eq!(without_target(&abstract_token, None), []);
}

#[test]
fn a_finding_one_edition_variant_raises_needs_that_edition() {
    let source = concat!(
        "theme t_java:\n",
        "  slot wall -> @stone\n",
        "  slot roof -> @cobblestone\n\n",
        "theme t_bedrock:\n",
        "  slot wall -> @stone\n",
        "  slot roof -> @oak_stairs\n\n",
        "struct s size=5x5\n",
        "  walls mat_slot=wall height=3\n",
        "  roof kind=gable mat_slot=roof\n",
    );
    assert_eq!(
        located(source, &without_target(source, Some(Edition::Java))),
        [("E_INCOMPATIBLE_MATERIAL", "@cobblestone")],
    );
    assert_eq!(without_target(source, Some(Edition::Bedrock)), []);
    assert_eq!(without_target(source, None), []);
}

/// Two huts whose doors are walled in by a pressure plate on each port cell, `plate` naming its
/// `mat_slot=` row argument.
fn plated_huts(plate: &str) -> String {
    format!(
        "def hut size=5x5:\n  floor id=f mat_slot=wall\n  walls id=w mat_slot=wall height=3\n  \
         roof kind=flat mat_slot=wall overhang=1\n  door id=e side=front at=center\n  \
         pressure_plate id=pp at=front.outside offset=2 y=0{plate}\n\n\
         theme t:\n  slot wall -> @cobblestone\n  slot plate -> @stone_pressure_plate\n\n\
         site s:\n  place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=4\n  connect a.e to b.e path=@gravel\n"
    )
}

#[test]
fn a_blocked_walkway_is_reported_when_its_note_names_no_packs_block() {
    let authored = plated_huts(" mat_slot=plate");
    let found = without_target(&authored, None);
    assert_eq!(
        located(&authored, &found),
        [("W_WALKWAY_BLOCKED", "connect a.e to b.e path=@gravel")],
    );
    assert_eq!(found, lowered_for(&authored, Edition::Java));

    // The plate a row leaves unbound is the pack's, which only a target names.
    assert_eq!(without_target(&plated_huts(""), None), []);
}
