//! The id a target spells a state with when that state makes it another block there, as Bedrock
//! spells an unlit torch, a powered repeater and a powered comparator each as a block of its own.

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockState, TargetRegistry};
use cairn_lang_formats::registry::builtin_pack;

use crate::common::supported_versions;

fn state(id: &str, properties: &[(&str, &str)]) -> BlockState {
    let mut state = BlockState::bare(id);
    for (key, value) in properties {
        state
            .properties
            .insert((*key).to_owned(), (*value).to_owned());
    }
    state.properties.sort_keys();
    state
}

/// Each state a Bedrock circuit rests in, with the id Bedrock spells it with when not its own.
fn circuit_states() -> [(BlockState, Option<&'static str>); 6] {
    let torch = |lit| {
        state(
            "minecraft:redstone_torch",
            &[("facing", "north"), ("lit", lit)],
        )
    };
    let repeater = |powered| {
        let properties = [
            ("delay", "1"),
            ("facing", "east"),
            ("locked", "false"),
            ("powered", powered),
        ];
        state("minecraft:unpowered_repeater", &properties)
    };
    let comparator = |powered| {
        let properties = [
            ("facing", "east"),
            ("mode", "compare"),
            ("powered", powered),
        ];
        state("minecraft:unpowered_comparator", &properties)
    };
    [
        (torch("false"), Some("minecraft:unlit_redstone_torch")),
        (torch("true"), None),
        (repeater("true"), Some("minecraft:powered_repeater")),
        (repeater("false"), None),
        (comparator("true"), Some("minecraft:powered_comparator")),
        (comparator("false"), None),
    ]
}

#[test]
fn bedrock_spells_a_lit_or_powered_state_with_the_block_that_holds_it() {
    let pack = builtin_pack(Edition::Bedrock);
    let versions = supported_versions(pack);
    for (state, spelled) in circuit_states() {
        for version in &versions {
            let pinned = pack.view(Some(version)).spelling(&state);
            assert_eq!(pinned.as_deref(), spelled, "{state:?} at {version}");
        }
        assert_eq!(
            pack.view(None).spelling(&state).as_deref(),
            spelled,
            "{state:?} unpinned"
        );
    }
}

#[test]
fn java_spells_every_state_with_its_own_id() {
    let pack = builtin_pack(Edition::Java);
    for lit in ["false", "true"] {
        let torch = state(
            "minecraft:redstone_wall_torch",
            &[("facing", "north"), ("lit", lit)],
        );
        for version in [None, Some("1.21.4")] {
            assert_eq!(
                pack.view(version).spelling(&torch),
                None,
                "{torch:?} at {version:?}"
            );
        }
    }
}
