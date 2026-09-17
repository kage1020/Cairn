//! Voxel dimensions as the `i32` both NBT dialects put on the wire.

use cairn_lang_core::block_array::Dims;

/// A dimension too large for an `i32` wire field. Each backend maps it
/// into its own error so the message can name the dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DimensionOverflow {
    /// Which axis overflowed (`"x"` / `"y"` / `"z"`).
    pub(crate) axis: &'static str,
    /// Offending dimension value.
    pub(crate) value: u32,
}

pub(crate) fn dim_to_i32(value: u32, axis: &'static str) -> Result<i32, DimensionOverflow> {
    i32::try_from(value).map_err(|_| DimensionOverflow { axis, value })
}

/// `[x, y, z]` of `dims`, refusing at the first axis that does not fit.
pub(crate) fn dims_to_i32(dims: &Dims) -> Result<[i32; 3], DimensionOverflow> {
    Ok([
        dim_to_i32(dims.x, "x")?,
        dim_to_i32(dims.y, "y")?,
        dim_to_i32(dims.z, "z")?,
    ])
}
