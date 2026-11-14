//! The canonical state a Bedrock block reads as, which an import writes: the state the map writes
//! as the block, spelled with an id the target declares.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockState, TargetRegistry};
use cairn_lang_formats::registry::{CanonicalSpelling, StateValue, builtin_pack};

use crate::common::props;

/// The canonical spelling of Bedrock `id` with `states` at 1.21.60, `water` standing with it.
fn spelled(id: &str, states: &[(&str, StateValue)], water: bool) -> Option<CanonicalSpelling> {
    let pack = builtin_pack(Edition::Bedrock);
    let map = pack.states_at("1.21.60").expect("a shipped map");
    let view = pack.view(Some("1.21.60"));
    let ids = view.block_ids().expect("a pinned id table");
    let states: Vec<(String, StateValue)> = states
        .iter()
        .map(|(key, value)| ((*key).to_owned(), value.clone()))
        .collect();
    map.canonical_spelling(id, &states, water, |id| ids.contains(id))
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
            waterlogged: true,
        })
    );
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
