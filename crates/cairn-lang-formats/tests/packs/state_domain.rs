//! What a pinned view of a built-in pack says about a block state's properties: Java's from the
//! block tables, Bedrock's from the canonical vocabulary its state map reads
//! (`spec/versioning-editions` "A state is checked against its block's states at the target").

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{
    BlockState, DomainFault, OutOfDomain, StateDomain, TargetRegistry,
};
use cairn_lang_formats::registry::builtin_pack;

fn state(id: &str, pairs: &[(&str, &str)]) -> BlockState {
    BlockState {
        id: format!("minecraft:{id}"),
        properties: pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

fn domain(edition: Edition, version: &str, id: &str, pairs: &[(&str, &str)]) -> StateDomain {
    builtin_pack(edition)
        .view(Some(version))
        .state_domain(&state(id, pairs))
}

fn owned(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn outside(key: &str, value: &str, fault: DomainFault, valid_at: &[&str]) -> StateDomain {
    StateDomain::Checked(vec![OutOfDomain {
        key: key.to_owned(),
        value: value.to_owned(),
        fault,
        valid_at: owned(valid_at),
    }])
}

#[test]
fn a_java_state_is_checked_against_the_pinned_versions_block_table() {
    assert_eq!(
        domain(Edition::Java, "1.21.4", "oak_log", &[("axis", "q")]),
        outside(
            "axis",
            "q",
            DomainFault::ValueOutside {
                values: owned(&["x", "y", "z"]),
            },
            &[],
        ),
    );
    assert_eq!(
        domain(Edition::Java, "1.21.4", "oak_planks", &[("axis", "x")]),
        outside(
            "axis",
            "x",
            DomainFault::UnknownProperty {
                properties: Vec::new(),
            },
            &[],
        ),
    );
    let stair = [
        ("facing", "east"),
        ("half", "top"),
        ("shape", "outer_left"),
        ("waterlogged", "true"),
    ];
    assert_eq!(
        domain(Edition::Java, "1.21.4", "oak_stairs", &stair),
        StateDomain::Checked(Vec::new()),
    );
}

#[test]
fn a_java_pair_names_the_other_versions_whose_tables_take_it() {
    assert_eq!(
        domain(
            Edition::Java,
            "26.3",
            "creaking_heart",
            &[("active", "true")]
        ),
        outside(
            "active",
            "true",
            DomainFault::UnknownProperty {
                properties: owned(&["axis", "creaking_heart_state", "natural"]),
            },
            &["1.21.4"],
        ),
    );
    assert_eq!(
        domain(
            Edition::Java,
            "1.20.4",
            "trial_spawner",
            &[("ominous", "true")]
        ),
        outside(
            "ominous",
            "true",
            DomainFault::UnknownProperty {
                properties: owned(&["trial_spawner_state"]),
            },
            &["1.21", "1.21.4", "26.3"],
        ),
    );
}

#[test]
fn an_id_the_target_does_not_declare_is_left_to_the_id_check() {
    assert_eq!(
        domain(Edition::Java, "1.21", "pale_oak_log", &[("axis", "x")]),
        StateDomain::AnsweredElsewhere,
    );
}

#[test]
fn a_bedrock_state_is_checked_against_the_canonical_states_its_map_reads() {
    assert_eq!(
        domain(
            Edition::Bedrock,
            "1.21.60",
            "oak_stairs",
            &[("facing", "up")]
        ),
        outside(
            "facing",
            "up",
            DomainFault::ValueOutside {
                values: owned(&["north", "south", "west", "east"]),
            },
            &[],
        ),
    );
    // A double slab is another Bedrock block, but still a state of the Java slab.
    assert_eq!(
        domain(
            Edition::Bedrock,
            "1.21.60",
            "oak_slab",
            &[("type", "double")]
        ),
        StateDomain::Checked(Vec::new()),
    );
    // `wooden_door` is Bedrock's spelling, read as the Java door mapped onto it.
    assert_eq!(
        domain(
            Edition::Bedrock,
            "1.21.60",
            "wooden_door",
            &[("hinge", "sideways")]
        ),
        outside(
            "hinge",
            "sideways",
            DomainFault::ValueOutside {
                values: owned(&["left", "right"]),
            },
            &[],
        ),
    );
}

#[test]
fn a_bedrock_id_that_is_its_own_java_block_takes_the_states_of_every_block_it_reads_as() {
    assert_eq!(
        domain(
            Edition::Bedrock,
            "1.21.60",
            "redstone_torch",
            &[("facing", "north"), ("lit", "true")],
        ),
        StateDomain::Checked(Vec::new()),
    );
    assert_eq!(
        domain(
            Edition::Bedrock,
            "1.21.60",
            "redstone_torch",
            &[("facing", "up")]
        ),
        outside(
            "facing",
            "up",
            DomainFault::ValueOutside {
                values: owned(&["north", "south", "west", "east"]),
            },
            &[],
        ),
    );
}

#[test]
fn a_bedrock_block_its_map_names_no_states_for_is_left_to_the_state_map() {
    let refused = state("cherry_wood", &[("axis", "x")]);
    let pinned = builtin_pack(Edition::Bedrock).view(Some("1.21.40"));
    assert_eq!(
        pinned.state_domain(&refused),
        StateDomain::AnsweredElsewhere
    );
    assert!(pinned.unmapped_state(&refused).is_some());
    // No Java block maps onto `frame`, so the map names none of its states.
    let frame = state("frame", &[("facing", "north")]);
    let at_1_21_60 = builtin_pack(Edition::Bedrock).view(Some("1.21.60"));
    assert_eq!(
        at_1_21_60.state_domain(&frame),
        StateDomain::AnsweredElsewhere
    );
    assert!(at_1_21_60.unmapped_state(&frame).is_some());
    assert_eq!(
        domain(Edition::Bedrock, "1.21.60", "cherry_wood", &[("axs", "x")]),
        outside(
            "axs",
            "x",
            DomainFault::UnknownProperty {
                properties: owned(&["axis"]),
            },
            &[],
        ),
    );
}

#[test]
fn an_unpinned_view_leaves_every_state_to_a_pinned_one() {
    for edition in [Edition::Java, Edition::Bedrock] {
        assert_eq!(
            builtin_pack(edition)
                .view(None)
                .state_domain(&state("oak_log", &[("axis", "q")])),
            StateDomain::Unpinned,
            "{edition:?}",
        );
    }
}
