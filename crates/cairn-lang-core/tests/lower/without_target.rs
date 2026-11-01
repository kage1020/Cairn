//! Lowering with no target (`spec/lint` "Error vs warning"): the findings every target that reads
//! their line raises, weighed as if every id some version declares were declared, and none whose
//! verdict is a target's pack's to give.

use cairn_lang_core::block_array::{findings_without_target, lower_to_block_array};
use cairn_lang_core::{Diagnostic, Edition, lower, parse, resolve};

use crate::common::{lowered, notes, slice};

/// `source`'s findings lowered with no target, for `edition` or for both, `declared` saying
/// whether some version of an edition declares an id.
fn without_target_declaring(
    source: &str,
    edition: Option<Edition>,
    declared: &dyn Fn(Edition, &str) -> bool,
) -> Vec<Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    findings_without_target(&lower(&module), edition, declared)
}

/// [`without_target_declaring`] with every id declared.
fn without_target(source: &str, edition: Option<Edition>) -> Vec<Diagnostic> {
    without_target_declaring(source, edition, &|_, _| true)
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

/// Each finding as its code and the 1-based `line:column` it starts at.
fn positioned(source: &str, found: &[Diagnostic]) -> Vec<(&'static str, String)> {
    found
        .iter()
        .map(|d| {
            let before = &source[..d.span.start];
            let line = before.matches('\n').count() + 1;
            let column = before.len() - before.rfind('\n').map_or(0, |i| i + 1) + 1;
            (d.code.as_str(), format!("{line}:{column}"))
        })
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
fn a_finding_every_target_raises_is_worded_as_a_lowering_with_no_pack_words_it() {
    let found = without_target(SHED_ON_COBBLESTONE, None);
    assert_eq!(
        located(SHED_ON_COBBLESTONE, &found),
        [
            ("E_INCOMPATIBLE_MATERIAL", "@cobblestone"),
            ("E_INVALID_MEMBER", "door side=3"),
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
fn whether_an_eave_stair_is_bound_to_a_stair_is_the_targets_to_say_only_for_a_token() {
    let eave = |block: &str| {
        format!(
            "theme t:\n  slot wall -> @cobblestone\n  slot roof -> @oak_stairs\n  \
             slot eave -> {block}\n\nstruct s size=5x5\n  walls mat_slot=wall height=3\n  \
             roof kind=gable mat_slot=roof overhang=1\n  \
             stair kind=stairs mat_slot=eave side=front half=top facing=out\n"
        )
    };
    assert_eq!(without_target(&eave("@wall.stone.cobble"), None), []);
    let canonical = eave("@cobblestone");
    assert_eq!(
        located(&canonical, &without_target(&canonical, None)),
        [("E_INCOMPATIBLE_MATERIAL", "@cobblestone")],
    );
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
fn an_id_no_edition_sharing_the_lowering_declares_is_left_to_a_pinned_check() {
    let source = concat!(
        "theme t:\n",
        "  slot wall -> @cobblestone\n",
        "  slot roof -> @oak_stair\n\n",
        "struct hut size=5x5\n",
        "  walls mat_slot=wall height=3\n",
        "  roof kind=gable mat_slot=roof\n",
    );
    let not_a_stair = [("E_INCOMPATIBLE_MATERIAL", "@oak_stair")];
    let java_only = |edition: Edition, _: &str| edition == Edition::Java;
    assert_eq!(
        located(source, &without_target(source, None)),
        not_a_stair,
        "premise: an id some version declares is weighed as that block",
    );
    assert_eq!(without_target_declaring(source, None, &|_, _| false), []);
    assert_eq!(without_target_declaring(source, None, &java_only), []);
    assert_eq!(
        located(
            source,
            &without_target_declaring(source, Some(Edition::Java), &java_only),
        ),
        not_a_stair,
    );
    assert_eq!(
        without_target_declaring(source, Some(Edition::Bedrock), &java_only),
        [],
    );
}

/// Two `walls` rows painting the same ring, the second over the first.
fn overwritten_walls(first: &str, second: &str) -> String {
    format!(
        "theme t:\n  slot a -> {first}\n  slot b -> {second}\n\n\
         struct s size=3x3\n  walls mat_slot=a height=2\n  walls mat_slot=b height=2\n"
    )
}

#[test]
fn a_phase_conflict_is_reported_only_between_blocks_no_target_changes() {
    let canonical = overwritten_walls("@cobblestone", "@stone");
    assert_eq!(
        located(&canonical, &without_target(&canonical, None)),
        [("W_PHASE_CONFLICT", "walls mat_slot=b height=2")],
    );
    for (first, second) in [
        ("@cobblestone", "@wall.stone.cobble"),
        ("@wall.stone.cobble", "@cobblestone"),
    ] {
        let source = overwritten_walls(first, second);
        assert_eq!(without_target(&source, None), [], "{first} then {second}");
    }
}

#[test]
fn a_stand_in_under_one_member_leaves_the_conflict_between_two_others() {
    let source = concat!(
        "theme t:\n",
        "  slot ground -> @wall.stone.cobble\n",
        "  slot a -> @cobblestone\n",
        "  slot b -> @stone\n\n",
        "struct s size=3x3\n",
        "  floor mat_slot=ground\n",
        "  walls mat_slot=a height=2\n",
        "  walls mat_slot=b height=2\n",
    );
    assert_eq!(
        located(source, &without_target(source, None)),
        [("W_PHASE_CONFLICT", "walls mat_slot=b height=2")],
    );
}

#[test]
fn a_stand_in_overwritten_by_air_is_a_conflict_at_every_target() {
    let source = concat!(
        "theme t:\n",
        "  slot wall -> @stone\n",
        "  slot glass -> @wood.light\n\n",
        "struct s size=7x5\n",
        "  walls mat_slot=wall height=4\n",
        "  window side=front mat_slot=glass offset=2 y=1 size=2x2\n",
        "  window side=front offset=2 y=1 size=2x2\n",
    );
    assert_eq!(
        located(source, &without_target(source, None)),
        [(
            "W_PHASE_CONFLICT",
            "window side=front offset=2 y=1 size=2x2"
        )],
    );
}

/// `t_java` and `t_bedrock` binding the roof slot to `java` and `bedrock`, under a gable roof and
/// a door whose side is a number.
fn variant_roofs(java: &str, bedrock: &str) -> String {
    format!(
        "@cairn 2026.06\n\ntheme t_java:\n  slot wall -> @stone\n  slot roof -> {java}\n\n\
         theme t_bedrock:\n  slot wall -> @stone\n  slot roof -> {bedrock}\n\n\
         struct s size=5x5\n  walls mat_slot=wall height=3\n  roof kind=gable mat_slot=roof\n  \
         door side=3\n"
    )
}

/// Each edition's own findings on `source`, and what a check with no edition reports.
fn by_edition(source: &str) -> [Vec<(&'static str, String)>; 3] {
    [Some(Edition::Java), Some(Edition::Bedrock), None]
        .map(|edition| positioned(source, &without_target(source, edition)))
}

#[test]
fn a_finding_each_edition_raises_on_its_own_variant_is_reported_for_each() {
    let source = variant_roofs("@cobblestone", "@cobblestone");
    let java = [
        ("E_INCOMPATIBLE_MATERIAL", "5:16".to_owned()),
        ("E_INVALID_MEMBER", "14:3".to_owned()),
    ];
    let bedrock = [
        ("E_INCOMPATIBLE_MATERIAL", "9:16".to_owned()),
        ("E_INVALID_MEMBER", "14:3".to_owned()),
    ];
    assert_eq!(
        by_edition(&source),
        [
            java.to_vec(),
            bedrock.to_vec(),
            vec![java[0].clone(), bedrock[0].clone(), java[1].clone()],
        ],
    );
    for edition in [Edition::Java, Edition::Bedrock] {
        assert_eq!(
            without_target(&source, Some(edition)),
            lowered_for(&source, edition),
            "{edition:?}",
        );
    }
}

#[test]
fn a_finding_one_edition_variant_raises_is_that_editions_verdict() {
    let door = ("E_INVALID_MEMBER", "14:3".to_owned());
    let java_roof = ("E_INCOMPATIBLE_MATERIAL", "5:16".to_owned());
    assert_eq!(
        by_edition(&variant_roofs("@cobblestone", "@oak_stairs")),
        [
            vec![java_roof.clone(), door.clone()],
            vec![door.clone()],
            vec![java_roof, door.clone()],
        ],
    );
    let bedrock_roof = ("E_INCOMPATIBLE_MATERIAL", "9:16".to_owned());
    assert_eq!(
        by_edition(&variant_roofs("@oak_stairs", "@cobblestone")),
        [
            vec![door.clone()],
            vec![bedrock_roof.clone(), door.clone()],
            vec![bedrock_roof, door],
        ],
    );
}

#[test]
fn a_finding_outside_the_variants_is_reported_once_as_each_edition_words_it() {
    let source = variant_roofs("@oak_stairs", "@spruce_stairs");
    let found = without_target(&source, None);
    assert_eq!(
        located(&source, &found),
        [("E_INVALID_MEMBER", "door side=3")]
    );
    for edition in [Edition::Java, Edition::Bedrock] {
        assert_eq!(found, lowered_for(&source, edition), "{edition:?}");
    }
}

/// Two huts whose doors are walled in by a pressure plate on each port cell, `plate_a` and
/// `plate_b` naming each plate's `mat_slot=` row argument.
fn plated_huts(plate_a: &str, plate_b: &str) -> String {
    let hut = |name: &str, plate: &str| {
        format!(
            "def {name} size=5x5:\n  floor id=f mat_slot=wall\n  \
             walls id=w mat_slot=wall height=3\n  roof kind=flat mat_slot=wall overhang=1\n  \
             door id=e side=front at=center\n  \
             pressure_plate id=pp at=front.outside offset=2 y=0{plate}\n\n"
        )
    };
    format!(
        "{}{}theme t:\n  slot wall -> @cobblestone\n  slot plate -> @stone_pressure_plate\n\n\
         site s:\n  place id=a use=hut_a theme=t at=origin\n  \
         place id=b use=hut_b theme=t east_of=a gap=4\n  connect a.e to b.e path=@gravel\n",
        hut("hut_a", plate_a),
        hut("hut_b", plate_b),
    )
}

#[test]
fn a_blocked_walkway_is_reported_whichever_plate_buries_its_port() {
    let authored = " mat_slot=plate";
    let controlled = plated_huts(authored, authored);
    assert_eq!(
        without_target(&controlled, None),
        lowered_for(&controlled, Edition::Java),
        "premise: with every plate named, the finding is the one a lowering with no pack raises",
    );
    let buried = |port: &str, block: &str| {
        format!(
            "port `{port}.e` sits on {block} its own placement `{port}` lays on that cell; \
             move that block or the door/window"
        )
    };
    let stone = "the `minecraft:stone_pressure_plate`";
    for (plate_a, plate_b, block_a, block_b) in [
        (authored, authored, stone, stone),
        ("", authored, "a block", stone),
        (authored, "", stone, "a block"),
        (" mat_slot=nosuch", authored, "a block", stone),
    ] {
        let source = plated_huts(plate_a, plate_b);
        let found = without_target(&source, None);
        assert_eq!(
            located(&source, &found),
            [("W_WALKWAY_BLOCKED", "connect a.e to b.e path=@gravel")],
            "{plate_a:?} / {plate_b:?}",
        );
        assert_eq!(
            notes(&found[0]),
            [format!(
                "{}; {}",
                buried("a", block_a),
                buried("b", block_b)
            )],
            "{plate_a:?} / {plate_b:?}",
        );
    }
}

#[test]
fn a_finding_on_a_line_both_editions_read_needs_each_to_raise_it_there() {
    // Java's roof alone drops its state literal's properties, on the shared `roof` line.
    let source = variant_roofs("@oak_stairs[half=top]", "@spruce_stairs");
    let literal = ("W_STATE_LITERAL_UNCHECKED", "5:16".to_owned());
    let roof = ("W_DEFERRED_MEMBER", "13:3".to_owned());
    let door = ("E_INVALID_MEMBER", "14:3".to_owned());
    assert_eq!(
        by_edition(&source),
        [
            vec![literal.clone(), roof, door.clone()],
            vec![door.clone()],
            vec![literal, door],
        ],
    );
}

#[test]
fn a_theme_both_editions_bind_is_no_ones_own() {
    let source = concat!(
        "@cairn 2026.06\n\n",
        "theme t_java:\n  slot wall -> @stone\n\n",
        "theme t_bedrock:\n  slot wall -> @cobblestone\n\n",
        "theme roofs:\n  slot wall -> @stone\n  slot roof -> @oak_stair\n\n",
        "def house size=5x5:\n  walls mat_slot=wall height=3\n\n",
        "def hut size=5x5:\n  walls mat_slot=wall height=3\n  roof kind=gable mat_slot=roof\n\n",
        "site s:\n  place id=a use=house theme=t at=origin\n  \
         place id=b use=hut theme=roofs east_of=a gap=2\n",
    );
    let java_only = |edition: Edition, _: &str| edition == Edition::Java;
    assert_eq!(
        positioned(
            source,
            &without_target_declaring(source, Some(Edition::Java), &java_only),
        ),
        [("E_INCOMPATIBLE_MATERIAL", "11:16".to_owned())],
        "premise: Java declares the id, so its lowering weighs the roof",
    );
    assert_eq!(without_target_declaring(source, None, &java_only), []);
}

#[test]
fn a_plate_its_own_line_puts_nowhere_is_refused_whichever_edition_paints_its_walls() {
    let plate = "pressure_plate at=inside.front offset=0 y=2 -> sig.a";
    let source = format!(
        "theme t_java:\n  slot wall -> @stone\n\nstruct s size=5x5\n  \
         walls mat_slot=wall height=3\n  {plate}\n"
    );
    let refused = ("E_INVALID_MEMBER", "6:3".to_owned());
    assert_eq!(
        by_edition(&source),
        [
            vec![refused.clone()],
            vec![("W_NO_THEME_BOUND", "4:1".to_owned()), refused.clone()],
            vec![refused],
        ],
        "premise: only Java binds a theme, so only Java paints the walls",
    );
    let found = without_target(&source, None);
    assert_eq!(found, lowered_for(&source, Edition::Java));
    assert_eq!(
        (
            located(&source, &found),
            found[0].primary.as_str(),
            notes(&found[0])
        ),
        (
            vec![("E_INVALID_MEMBER", plate)],
            "pressure_plate `at=inside.front offset=0` is at a corner of the front wall, so the \
             voxel inside it belongs to the neighbouring wall",
            vec![
                "use an `offset=` from 1 to 3 to reach an interior voxel",
                "its signal binding still reaches the netlist, with no plate placed to drive it",
            ],
        ),
    );
}
