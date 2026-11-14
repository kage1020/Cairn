//! Litematica's `.litematic`: a gzip-wrapped Java NBT root of `MinecraftDataVersion`, `Version`,
//! `SubVersion`, `Metadata` and `Regions`, one region per block array the file holds
//! (`spec/ecosystem-interop` "Litematica"). A region's `BlockStates` packs each voxel's palette
//! index, in `(y, z, x)` order, into the fewest bits its palette needs, letting an index run on
//! from one long into the next.

mod read;
mod write;

pub use read::{
    LitematicMetadata, LitematicReadError, LitematicSchematic, ReadRegion, read_litematic,
};
pub use write::{
    Litematic, LitematicError, LitematicFile, LitematicRegion, litematic_files, prepare_litematic,
};

/// The first `MinecraftDataVersion`, 1.20.5's, that Litematica saves as `Version` 7; a Litematica
/// for 1.20.4 reads only up to 6.
const VERSION_7_FROM: i32 = 3837;

/// Litematica's `SubVersion`, which every `Version` it writes carries.
const SUB_VERSION: i32 = 1;

/// The `Version` a Litematica for `data_version` writes and reads.
fn schematic_version(data_version: i32) -> i32 {
    if data_version >= VERSION_7_FROM { 7 } else { 6 }
}

/// The bits a palette index takes in `BlockStates` for a palette of `len` entries.
fn bits_per_entry(len: usize) -> u32 {
    let largest = len.saturating_sub(1);
    (usize::BITS - largest.leading_zeros()).max(2)
}

/// How many longs `BlockStates` holds for `volume` indices of `bits` bits each.
fn long_count(volume: usize, bits: u32) -> usize {
    volume
        .saturating_mul(usize::try_from(bits).expect("at most 32 bits"))
        .div_ceil(64)
}

/// The block ids Litematica's `TotalBlocks` does not count.
const AIR_IDS: [&str; 3] = ["minecraft:air", "minecraft:cave_air", "minecraft:void_air"];
