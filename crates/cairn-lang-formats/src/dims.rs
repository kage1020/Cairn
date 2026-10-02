//! Voxel dimensions as the `i32` both NBT dialects put on the wire.

use cairn_lang_core::block_array::{BlockArray, Dims};

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

/// Why a grid cannot have its voxel count written as the declared length
/// of its per-voxel lists. Each backend maps it into its own error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VolumeError {
    /// `x * y * z` overflows `usize`.
    Overflow { x: u32, y: u32, z: u32 },
    /// The voxel count is past the `i32` length prefix of an NBT list.
    PastListLimit { volume: usize },
    /// The grid holds a different number of voxels than its dims.
    Mismatch { volume: usize, voxels: usize },
}

/// The voxel count of `block_array`, checked to fit an NBT list's length
/// prefix and to be the length of its `voxels`, so a writer can declare it
/// as a list length and index `voxels` at any coordinate inside the dims.
pub(crate) fn list_volume(block_array: &BlockArray) -> Result<usize, VolumeError> {
    let Dims { x, y, z } = block_array.dims;
    let volume = block_array
        .dims
        .checked_volume()
        .ok_or(VolumeError::Overflow { x, y, z })?;
    if !fits_list_limit(volume) {
        return Err(VolumeError::PastListLimit { volume });
    }
    let voxels = block_array.voxels.len();
    if voxels != volume {
        return Err(VolumeError::Mismatch { volume, voxels });
    }
    Ok(volume)
}

/// Whether a list of `len` entries fits the `i32` length prefix of an NBT
/// list.
pub(crate) fn fits_list_limit(len: usize) -> bool {
    i32::try_from(len).is_ok()
}
