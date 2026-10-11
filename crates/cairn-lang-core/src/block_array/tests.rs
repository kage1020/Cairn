use super::*;

/// A palette with `len` entries: air and `len - 1` distinct states.
fn filled(len: usize) -> Palette {
    let mut palette = Palette::new_with_air();
    palette
        .entries
        .extend((1..len).map(|i| BlockState::bare(format!("test:b{i}"))));
    palette
}

#[test]
fn try_intern_refuses_only_a_new_state_once_every_index_is_taken() {
    let mut palette = filled(PALETTE_CAPACITY - 1);
    assert_eq!(
        palette.try_intern(BlockState::bare("test:last")),
        Ok(PaletteIndex(u16::MAX)),
    );
    assert_eq!(palette.entries.len(), PALETTE_CAPACITY);
    assert_eq!(
        palette.try_intern(BlockState::bare("test:one_too_many")),
        Err(PaletteFull {
            capacity: PALETTE_CAPACITY
        }),
    );
    assert_eq!(
        palette.entries.len(),
        PALETTE_CAPACITY,
        "a refusal appends nothing"
    );
    assert_eq!(
        palette.try_intern(BlockState::bare("test:b1")),
        Ok(PaletteIndex(1)),
    );
    assert_eq!(palette.try_intern(BlockState::air()), Ok(PaletteIndex::AIR));
}

/// Without the clamp this panics, so it is caught to fail as the assertion
/// instead.
#[test]
fn try_intern_within_clamps_a_capacity_past_what_an_index_can_name() {
    let mut palette = filled(PALETTE_CAPACITY);
    let answered = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        palette.try_intern_within(BlockState::bare("test:one_too_many"), PALETTE_CAPACITY + 1)
    }));
    assert_eq!(
        answered.ok(),
        Some(Err(PaletteFull {
            capacity: PALETTE_CAPACITY
        })),
        "a capacity of {} refuses at {PALETTE_CAPACITY}",
        PALETTE_CAPACITY + 1,
    );
    assert_eq!(palette.entries.len(), PALETTE_CAPACITY);
}

#[test]
fn dims_index_round_trips() {
    let d = Dims { x: 3, y: 2, z: 4 };
    // (1, 1, 2) → ((1 * 4) + 2) * 3 + 1 = 19
    assert_eq!(d.index(1, 1, 2), Some(19));
}

#[test]
fn dims_index_rejects_out_of_bounds() {
    let d = Dims { x: 3, y: 2, z: 4 };
    assert!(d.index(3, 0, 0).is_none());
    assert!(d.index(0, 2, 0).is_none());
    assert!(d.index(0, 0, 4).is_none());
}

#[test]
fn palette_seeds_air_at_index_zero() {
    let p = Palette::new_with_air();
    assert_eq!(p.entries.len(), 1);
    assert_eq!(p.entries[0].id, BlockState::AIR_ID);
}

#[test]
fn palette_intern_dedupes() {
    let mut p = Palette::new_with_air();
    let a = p.intern(BlockState::bare("minecraft:cobblestone"));
    let b = p.intern(BlockState::bare("minecraft:cobblestone"));
    assert_eq!(a, b);
    assert_eq!(p.entries.len(), 2, "palette grew despite duplicate insert");
}

#[test]
fn palette_intern_appends_distinct_states() {
    let mut p = Palette::new_with_air();
    let cobble = p.intern(BlockState::bare("minecraft:cobblestone"));
    let planks = p.intern(BlockState::bare("minecraft:oak_planks"));
    assert_ne!(cobble, planks);
    assert_eq!(p.entries.len(), 3);
}

fn state(id: &str, properties: &[(&str, &str)]) -> BlockState {
    BlockState {
        id: id.to_owned(),
        properties: properties
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

fn ids(p: &Palette) -> Vec<&str> {
    p.entries.iter().map(|s| s.id.as_str()).collect()
}

#[test]
fn canonicalize_sorts_by_id_and_pins_air_at_slot_zero() {
    // `minecraft:acacia_log` sorts ahead of `minecraft:air`, so the
    // pin is doing work here rather than agreeing with the sort.
    let mut p = Palette::new_with_air();
    p.intern(BlockState::bare("minecraft:oak_planks"));
    p.intern(BlockState::bare("minecraft:acacia_log"));
    p.intern(BlockState::bare("minecraft:cobblestone"));
    let remap = p.canonicalize();
    assert_eq!(
        ids(&p),
        [
            "minecraft:air",
            "minecraft:acacia_log",
            "minecraft:cobblestone",
            "minecraft:oak_planks",
        ],
    );
    assert_eq!(
        remap,
        vec![
            PaletteIndex(0),
            PaletteIndex(3),
            PaletteIndex(1),
            PaletteIndex(2),
        ],
    );
}

#[test]
fn canonicalize_breaks_an_id_tie_on_the_properties() {
    let mut p = Palette::new_with_air();
    p.intern(state("minecraft:oak_stairs", &[("facing", "south")]));
    p.intern(state("minecraft:oak_stairs", &[("facing", "north")]));
    let _ = p.canonicalize();
    let facings: Vec<&str> = p.entries[1..]
        .iter()
        .map(|s| s.properties["facing"].as_str())
        .collect();
    assert_eq!(facings, ["north", "south"]);
}

#[test]
fn canonicalize_reads_properties_in_name_order_not_insertion_order() {
    // Same id on both, and `half` first in one bag but second in the
    // other, so a key that read the pairs in place would compare
    // `half=top` against `facing=south`. A source state literal arrives
    // sorted, but a registry's `lookup` or a hand-built state need not.
    let mut p = Palette::new_with_air();
    p.intern(state(
        "minecraft:oak_stairs",
        &[("half", "top"), ("facing", "north")],
    ));
    p.intern(state(
        "minecraft:oak_stairs",
        &[("facing", "south"), ("half", "bottom")],
    ));
    let _ = p.canonicalize();
    let facings: Vec<&str> = p.entries[1..]
        .iter()
        .map(|s| s.properties["facing"].as_str())
        .collect();
    assert_eq!(facings, ["north", "south"]);
}

#[test]
fn canonicalizing_a_block_array_renumbers_the_grid_onto_the_new_slots() {
    let mut palette = Palette::new_with_air();
    let planks = palette.intern(BlockState::bare("minecraft:oak_planks"));
    let cobble = palette.intern(BlockState::bare("minecraft:cobblestone"));
    let mut array = BlockArray {
        dims: Dims { x: 2, y: 1, z: 1 },
        palette,
        voxels: vec![planks, cobble],
        block_entities: Vec::new(),
        entities: Vec::new(),
        source_scope: "struct::t".to_owned(),
    };
    array.canonicalize_palette();
    let read: Vec<&str> = array
        .voxels
        .iter()
        .map(|i| array.palette.entries[usize::from(i.0)].id.as_str())
        .collect();
    assert_eq!(read, ["minecraft:oak_planks", "minecraft:cobblestone"]);
    assert_eq!(
        ids(&array.palette),
        [
            "minecraft:air",
            "minecraft:cobblestone",
            "minecraft:oak_planks",
        ],
    );
}

#[test]
fn canonicalizing_leaves_an_index_the_palette_does_not_have_alone() {
    let mut array = BlockArray {
        dims: Dims { x: 1, y: 1, z: 1 },
        palette: Palette::new_with_air(),
        voxels: vec![PaletteIndex(7)],
        block_entities: Vec::new(),
        entities: Vec::new(),
        source_scope: "struct::t".to_owned(),
    };
    array.canonicalize_palette();
    assert_eq!(array.first_index_outside_palette(), Some((7, 1)));
}

#[test]
fn properties_text_joins_the_pairs_in_insertion_order() {
    let mut state = BlockState::bare("minecraft:oak_stairs");
    assert_eq!(state.properties_text(), "");
    state.properties.insert("half".to_owned(), "top".to_owned());
    state
        .properties
        .insert("facing".to_owned(), "east".to_owned());
    assert_eq!(state.properties_text(), "half=top,facing=east");
}
