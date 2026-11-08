//! Which blocks the built-in packs say hold water, and what a water `fill` built against them
//! writes (`spec/blockstate` "`waterlogged`").

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{BlockArray, BlockState, TargetRegistry};
use cairn_lang_formats::bedrock_structure::build_mcstructure_tag;
use cairn_lang_formats::data_version::resolve_bedrock_target;
use cairn_lang_formats::registry::builtin_pack;
use cairn_lang_nbt::tag::{Compound, Tag};

use crate::common::{block_palette, lower_source, states_for, structure_compound};

/// A 5x4 tank with 2-row cobblestone walls, panes across the front, two chests side by side
/// against the back with the second kept dry, and water poured over the bottom row.
const TANK: &str = "theme t:\n  slot wall -> @cobblestone\n  slot pane -> @glass_pane\n\n\
                    struct s size=5x4\n  walls mat_slot=wall height=2\n  \
                    window side=front offset=1 y=1 size=3x1 mat_slot=pane\n  \
                    chest at=inside.back offset=1 y=1\n  \
                    chest at=inside.back offset=2 y=1 waterlogged=false\n  \
                    fill fluid=water from=0,1,0 to=4,1,3\n";

fn holds_water(edition: Edition, version: Option<&str>, id: &str) -> Option<bool> {
    let state = BlockState::bare(format!("minecraft:{id}"));
    builtin_pack(edition).view(version).holds_water(&state)
}

fn slab(slab_type: &str) -> BlockState {
    let mut state = BlockState::bare("minecraft:oak_slab");
    state
        .properties
        .insert("type".to_owned(), slab_type.to_owned());
    state
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

#[test]
fn a_double_slab_fills_its_cell_and_holds_no_water() {
    for edition in [Edition::Java, Edition::Bedrock] {
        let view = builtin_pack(edition).view(None);
        let held: Vec<Option<bool>> = ["bottom", "top", "double"]
            .into_iter()
            .map(|slab_type| view.holds_water(&slab(slab_type)))
            .collect();
        assert_eq!(held, [Some(true), Some(true), Some(false)], "{edition:?}");
        assert_eq!(
            view.holds_water(&BlockState::bare("minecraft:oak_slab")),
            Some(true),
            "{edition:?}: a slab whose state writes no `type` is the bottom half"
        );
    }
}

#[test]
fn a_bedrock_block_with_no_java_counterpart_gets_no_answer() {
    for version in [Some("1.21.0"), Some("1.26.60"), None] {
        assert_eq!(
            holds_water(Edition::Bedrock, version, "hard_glass_pane"),
            None,
            "{version:?}"
        );
    }
}

/// The state at `(x, y, z)` of `array`, as `id[key=value,...]`.
fn written_at(array: &BlockArray, x: u32, y: u32, z: u32) -> String {
    let i = array.dims.index(x, y, z).expect("inside the array");
    let state = &array.palette.entries[usize::from(array.voxels[i].0)];
    format!("{}[{}]", state.id, state.properties_text())
}

#[test]
fn a_java_tank_waterlogs_its_panes_as_they_join_and_its_chests_pair_keeping_their_own_water() {
    let out = lower_source(TANK, Edition::Java, Some("26.3"));
    assert_eq!(out.diagnostics, []);
    let array = &out.structures["struct::s"];
    let pane = "minecraft:glass_pane[east=true,north=false,south=false,waterlogged=true,west=true]";
    let row: Vec<String> = (0..5).map(|x| written_at(array, x, 1, 3)).collect();
    assert_eq!(
        row,
        [
            "minecraft:cobblestone[]",
            pane,
            pane,
            pane,
            "minecraft:cobblestone[]",
        ],
    );
    let back: Vec<String> = (1..4).map(|x| written_at(array, x, 1, 1)).collect();
    assert_eq!(
        back,
        [
            "minecraft:water[level=0]",
            "minecraft:chest[facing=south,type=right,waterlogged=false]",
            "minecraft:chest[facing=south,type=left,waterlogged=true]",
        ],
    );
}

/// Each cell the second layer of `root`'s `block_indices` holds a block in, with its slot, for an
/// array `dy` tall and `dz` deep read z fastest.
fn second_layer(root: &Compound, dy: u32, dz: u32) -> Vec<((u32, u32, u32), i32)> {
    let Some(Tag::List(layers)) = structure_compound(root).entries.get("block_indices") else {
        panic!("no block_indices");
    };
    let Some(Tag::List(water)) = layers.items.get(1) else {
        panic!("no second layer");
    };
    let mut cells = Vec::new();
    for (i, slot) in water.items.iter().enumerate() {
        let Tag::Int(slot) = slot else {
            panic!("block index {i}: {slot:?}");
        };
        if *slot == -1 {
            continue;
        }
        let i = u32::try_from(i).expect("small array");
        cells.push(((i / (dy * dz), i / dz % dy, i % dz), *slot));
    }
    cells
}

#[test]
fn a_bedrock_tank_holds_the_water_of_its_waterlogged_blocks_in_the_second_layer() {
    let version = "1.21.0";
    let out = lower_source(TANK, Edition::Bedrock, Some(version));
    assert_eq!(out.diagnostics, []);
    let array = &out.structures["struct::s"];
    let target = resolve_bedrock_target(version).expect("known target");
    let (root, _notes) =
        build_mcstructure_tag(array, &target, &states_for(&target)).expect("build");
    let held = second_layer(&root, array.dims.y, array.dims.z);
    let cells: Vec<(u32, u32, u32)> = held.iter().map(|(cell, _)| *cell).collect();
    assert_eq!(
        cells,
        [(1, 1, 3), (2, 1, 3), (3, 1, 1), (3, 1, 3)],
        "the wet chest and the three panes; the dry chest and the poured water hold none",
    );
    let palette = block_palette(&root);
    for (cell, slot) in held {
        let Some(Tag::Compound(entry)) = usize::try_from(slot)
            .ok()
            .and_then(|slot| palette.get(slot))
        else {
            panic!("{cell:?}: slot {slot} is past the palette");
        };
        assert_eq!(
            entry.entries.get("name"),
            Some(&Tag::String("minecraft:water".to_owned())),
            "{cell:?}",
        );
    }
}
