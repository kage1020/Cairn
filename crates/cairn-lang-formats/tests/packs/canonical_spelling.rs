//! The canonical state a Bedrock block reads as, which an import writes: the state the map writes
//! as the block, spelled with an id the target declares.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockState, TargetRegistry};
use cairn_lang_formats::registry::{CanonicalSpelling, StateValue, builtin_pack};

use crate::common::props;

/// The canonical spelling of Bedrock `id` with `states` at 1.21.60, `water` standing with it.
fn spelled(id: &str, states: &[(&str, StateValue)], water: bool) -> Option<CanonicalSpelling> {
    spelled_at("1.21.60", id, states, water)
}

/// The canonical spelling of Bedrock `id` with `states` at `version`, `water` standing with it.
fn spelled_at(
    version: &str,
    id: &str,
    states: &[(&str, StateValue)],
    water: bool,
) -> Option<CanonicalSpelling> {
    let pack = builtin_pack(Edition::Bedrock);
    let map = pack.states_at(version).expect("a shipped map");
    let view = pack.view(Some(version));
    let ids = view.block_ids().expect("a pinned id table");
    let states: Vec<(String, StateValue)> = states
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect();
    map.canonical_spelling(id, &states, water, |id| ids.contains(id))
}

/// The spelling of a block with no state unread or needed, out of water.
fn plain(id: &str, pairs: &[(&str, &str)], defaults: &[(&str, &str)]) -> CanonicalSpelling {
    CanonicalSpelling {
        state: state(id, pairs),
        defaults: defaults
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
        unread: Vec::new(),
        needed: Vec::new(),
        waterlogged: false,
    }
}

fn state(id: &str, pairs: &[(&str, &str)]) -> BlockState {
    BlockState {
        id: id.to_owned(),
        properties: pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

#[test]
fn a_stair_reads_as_its_java_state_with_the_states_bedrock_drops_unread() {
    let stairs = spelled(
        "minecraft:oak_stairs",
        &[
            ("upside_down_bit", StateValue::Bool(true)),
            ("weirdo_direction", StateValue::Int(0)),
        ],
        true,
    );
    assert_eq!(
        stairs,
        Some(CanonicalSpelling {
            state: state(
                "minecraft:oak_stairs",
                &[
                    ("facing", "east"),
                    ("half", "top"),
                    ("shape", "straight"),
                    ("waterlogged", "true"),
                ]
            ),
            defaults: props([
                ("facing", "north"),
                ("half", "bottom"),
                ("shape", "straight"),
                ("waterlogged", "false"),
            ]),
            unread: vec!["shape".to_owned()],
            needed: Vec::new(),
            waterlogged: true,
        })
    );
}

#[test]
fn a_state_at_its_default_that_picks_the_java_block_is_needed() {
    let torch = spelled(
        "minecraft:redstone_torch",
        &[(
            "torch_facing_direction",
            StateValue::Str("south".to_owned()),
        )],
        false,
    );
    assert_eq!(
        torch,
        Some(CanonicalSpelling {
            state: state(
                "minecraft:redstone_torch",
                &[("facing", "north"), ("lit", "true")]
            ),
            defaults: props([("facing", "north"), ("lit", "true")]),
            unread: vec!["lit".to_owned()],
            needed: vec!["facing".to_owned()],
            waterlogged: false,
        })
    );
}

#[test]
fn a_needed_state_bedrock_keeps_in_the_block_entity_stays_unread() {
    let skull = spelled_at(
        "1.21.0",
        "minecraft:skull",
        &[("facing_direction", StateValue::Int(1))],
        false,
    );
    assert_eq!(
        skull,
        Some(CanonicalSpelling {
            state: state(
                "minecraft:skull",
                &[("powered", "false"), ("rotation", "0")]
            ),
            defaults: props([("powered", "false"), ("rotation", "0")]),
            unread: vec!["powered".to_owned(), "rotation".to_owned()],
            needed: vec!["rotation".to_owned()],
            waterlogged: false,
        })
    );
}

#[test]
fn a_tie_keeps_the_first_state_in_a_count_whose_first_read_property_moves_fastest() {
    let floor_lever = [
        (
            "lever_direction",
            StateValue::Str("up_north_south".to_owned()),
        ),
        ("open_bit", StateValue::Bool(true)),
    ];
    let lever_defaults = [("face", "wall"), ("facing", "north"), ("powered", "false")];
    for version in builtin_pack(Edition::Bedrock).targetable_versions() {
        assert_eq!(
            spelled_at(&version, "minecraft:lever", &floor_lever, false),
            Some(plain(
                "minecraft:lever",
                &[("face", "floor"), ("facing", "north"), ("powered", "true")],
                &lever_defaults
            )),
            "at {version}"
        );
    }
    let sides = ["down", "east", "north", "south", "up", "west"];
    let mushroom_defaults = sides.map(|side| (side, "true"));
    let east_closed = sides.map(|side| (side, if side == "east" { "false" } else { "true" }));
    for id in [
        "minecraft:brown_mushroom_block",
        "minecraft:red_mushroom_block",
    ] {
        assert_eq!(
            spelled_at(
                "1.26.60",
                id,
                &[("huge_mushroom_bits", StateValue::Int(1))],
                false
            ),
            Some(plain(id, &east_closed, &mushroom_defaults)),
            "{id}"
        );
    }
}

#[test]
fn a_block_bedrock_names_apart_reads_as_the_java_block_whose_state_it_is() {
    let torch = spelled(
        "minecraft:unlit_redstone_torch",
        &[("torch_facing_direction", StateValue::Str("top".to_owned()))],
        false,
    )
    .expect("an unlit torch has a canonical state");
    assert_eq!(
        torch.state,
        state("minecraft:redstone_torch", &[("lit", "false")])
    );
    let slab = spelled(
        "minecraft:oak_double_slab",
        &[(
            "minecraft:vertical_half",
            StateValue::Str("bottom".to_owned()),
        )],
        false,
    )
    .expect("a double slab has a canonical state");
    assert_eq!(
        slab.state,
        state(
            "minecraft:oak_slab",
            &[("type", "double"), ("waterlogged", "false")]
        )
    );
}

#[test]
fn states_no_canonical_state_writes_have_no_spelling() {
    assert_eq!(
        spelled(
            "minecraft:oak_stairs",
            &[("upside_down_bit", StateValue::Bool(true))],
            false
        ),
        None
    );
    assert_eq!(spelled("minecraft:not_a_block", &[], false), None);
}
