use super::*;

/// A palette with `len` entries: air and `len - 1` distinct states.
fn filled(len: usize) -> Palette {
    let mut palette = Palette::new_with_air();
    palette
        .entries
        .extend((1..len).map(|i| BlockState::bare(format!("test:b{i}"))));
    palette
}

/// The last index a palette can name is still given out, the one past
/// it is refused rather than panicking, and a state already present is
/// still found once every index is taken.
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

/// A capacity past [`PALETTE_CAPACITY`] is clamped to it: a full
/// palette refuses a new state rather than reaching for an index a
/// [`PaletteIndex`] cannot name, and the refusal carries the capacity
/// that was in force, not the one asked for. Caught rather than left to
/// fail the test, so the clamp's absence is reported as this assertion
/// and not as the panic it would cause.
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
    // The map has to name where each old slot went, or a grid painted
    // against the old numbering cannot follow.
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
    // Same id on both, so the property bag is the whole decision.
    // `half` is written first in one bag and second in the other, so a
    // key that read the pairs in place would compare `half=top`
    // against `facing=south` and order the two by which property the
    // author happened to write first.
    //
    // Reachable from source, not only by hand: `parse_state_literal`
    // fills the `IndexMap` in the order the literal spells its
    // properties, so `@oak_stairs[half=top,facing=north]` beside
    // `@oak_stairs[facing=south,half=bottom]` is a pair of theme slots
    // someone can write.
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
    // Same two blocks in the same two cells, read back through the
    // palette the reordering left behind.
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
    // A hand-assembled disagreement stays visible to
    // `first_index_outside_palette`, which is what both backends ask
    // before they write anything.
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
