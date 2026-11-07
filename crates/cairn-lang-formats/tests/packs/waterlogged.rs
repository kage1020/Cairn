//! Which blocks the built-in packs say hold water, the ones a water `fill` waterlogs
//! (`spec/blockstate` "`waterlogged`").

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockState, TargetRegistry};
use cairn_lang_formats::registry::builtin_pack;

fn holds_water(edition: Edition, version: Option<&str>, id: &str) -> Option<bool> {
    let state = BlockState::bare(format!("minecraft:{id}"));
    builtin_pack(edition).view(version).holds_water(&state)
}

#[test]
fn a_java_block_holds_water_where_its_version_gives_it_a_waterlogged_state() {
    for version in ["1.20.4", "26.3"] {
        let at = |id| holds_water(Edition::Java, Some(version), id);
        for id in ["oak_stairs", "chest", "glass_pane", "oak_fence"] {
            assert_eq!(at(id), Some(true), "{id} at {version}");
        }
        for id in ["cobblestone", "air", "oak_door", "stone_pressure_plate"] {
            assert_eq!(at(id), Some(false), "{id} at {version}");
        }
        assert_eq!(at("no_such_block"), None, "{version}");
    }
    let copper_bars = |version| holds_water(Edition::Java, Some(version), "copper_bars");
    assert_eq!(copper_bars("26.3"), Some(true));
    assert_eq!(
        copper_bars("1.21"),
        None,
        "a version that does not declare the block"
    );
}

#[test]
fn an_unpinned_java_lowering_reads_what_every_version_declaring_the_block_agrees_on() {
    let unpinned = |id| holds_water(Edition::Java, None, id);
    assert_eq!(unpinned("oak_stairs"), Some(true));
    assert_eq!(unpinned("copper_bars"), Some(true));
    assert_eq!(unpinned("cobblestone"), Some(false));
    assert_eq!(unpinned("no_such_block"), None);
}

#[test]
fn a_bedrock_block_holds_water_as_the_canonical_block_its_state_map_names() {
    for version in [Some("1.21.0"), Some("1.26.60"), None] {
        let at = |id| holds_water(Edition::Bedrock, version, id);
        for id in ["oak_stairs", "chest", "glass_pane"] {
            assert_eq!(at(id), Some(true), "{id} at {version:?}");
        }
        assert_eq!(at("cobblestone"), Some(false), "{version:?}");
        assert_eq!(at("no_such_block"), None, "{version:?}");
    }
}
