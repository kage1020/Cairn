//! Voxel dimensions as the `i32` both NBT dialects put on the wire.

use cairn_lang_core::block_array::{BlockArray, Dims};

/// A dimension too large for an `i32` wire field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DimensionOverflow {
    /// The axis, `"x"`, `"y"` or `"z"`.
    pub(crate) axis: &'static str,
    /// The dimension.
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

/// Why a grid's voxel count cannot be the declared length of its per-voxel lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VolumeError {
    /// `x * y * z` overflows `usize`.
    Overflow { x: u32, y: u32, z: u32 },
    /// The voxel count is past the `i32` length prefix of an NBT list.
    PastListLimit { volume: usize },
    /// The grid holds a different number of voxels than its dims.
    Mismatch { volume: usize, voxels: usize },
}

/// The voxel count of `block_array`, checked to fit a list length and to equal `voxels.len()`.
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

/// Whether `len` fits an NBT list's `i32` length prefix.
pub(crate) fn fits_list_limit(len: usize) -> bool {
    i32::try_from(len).is_ok()
}

/// The extents a stored `size` of three ints names, or `None` for any other shape or a negative.
pub(crate) fn read_extents(size: Option<&[i32]>) -> Option<Dims> {
    let [x, y, z] = size? else {
        return None;
    };
    Some(Dims {
        x: u32::try_from(*x).ok()?,
        y: u32::try_from(*y).ok()?,
        z: u32::try_from(*z).ok()?,
    })
}

/// The voxel count of `dims`, saturated at `u64::MAX`.
pub(crate) fn saturating_volume(dims: Dims) -> u64 {
    u64::from(dims.x)
        .saturating_mul(u64::from(dims.y))
        .saturating_mul(u64::from(dims.z))
}
