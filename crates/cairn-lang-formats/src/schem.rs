//! The Sponge Schematic's `.schem`, written as version 3 and read as version 2 or 3: a
//! gzip-wrapped Java NBT `Schematic` compound (`spec/ecosystem-interop` "Sponge Schematic").
//! A palette maps each block state, spelled `id[name=value,...]`, to an index, and the block data
//! holds each voxel's index as a varint, in `(y, z, x)` order.

mod read;
mod write;

pub use read::{SchemReadError, SpongeSchematic, read_schem};
pub use write::{Schem, SchemError, SchemFile, prepare_schem, schem_files};

/// The characters a block state's spelling sets its id and properties apart with.
const STATE_SEPARATORS: [char; 4] = ['[', ']', ',', '='];
