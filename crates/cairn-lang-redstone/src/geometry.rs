//! Voxels and turns in a `circuit` reservation: `x` and `z` as the struct's array lays them out,
//! `y` counted up from the reservation's bottom layer (`spec/redstone` "Place-and-route").

use cairn_lang_core::block_array::Face;
use serde::{Serialize, Serializer};

/// One voxel of a reservation, `y = 0` its bottom layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct Voxel {
    /// Along the array's `x`.
    pub x: i32,
    /// Up from the reservation's bottom layer.
    pub y: i32,
    /// Along the array's `z`.
    pub z: i32,
}

impl Voxel {
    /// The voxel at `(x, y, z)`.
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// The voxel `face` looks into.
    #[must_use]
    pub const fn step(self, face: Face) -> Self {
        let (dx, dy, dz) = face.step();
        Self::new(self.x + dx, self.y + dy, self.z + dz)
    }

    /// The voxel above.
    #[must_use]
    pub const fn up(self) -> Self {
        Self::new(self.x, self.y + 1, self.z)
    }

    /// The voxel below.
    #[must_use]
    pub const fn down(self) -> Self {
        Self::new(self.x, self.y - 1, self.z)
    }

    /// `self` moved by `offset`.
    #[must_use]
    pub const fn offset(self, offset: Rel) -> Self {
        Self::new(self.x + offset.x, self.y + offset.y, self.z + offset.z)
    }

    /// The sideways face from `self` to the voxel beside it, at any height; `None` for any other.
    #[must_use]
    pub fn side_toward(self, other: Self) -> Option<Face> {
        match (other.x - self.x, other.z - self.z) {
            (1, 0) => Some(Face::East),
            (-1, 0) => Some(Face::West),
            (0, 1) => Some(Face::South),
            (0, -1) => Some(Face::North),
            _ => None,
        }
    }

    /// Steps along `x` and `z` plus steps along `y` between `self` and `other`.
    #[must_use]
    pub const fn distance(self, other: Self) -> u32 {
        self.x.abs_diff(other.x) + self.y.abs_diff(other.y) + self.z.abs_diff(other.z)
    }
}

/// An offset inside a tile, before the tile is turned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rel {
    /// Along the tile's own `x`, the way its signal runs.
    pub x: i32,
    /// Up.
    pub y: i32,
    /// Along the tile's own `z`.
    pub z: i32,
}

impl Rel {
    /// The offset `(x, y, z)`.
    #[must_use]
    pub const fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    /// The offset `(x, 0, z)`, in the tile's own layer.
    #[must_use]
    pub const fn flat(x: i32, z: i32) -> Self {
        Self::new(x, 0, z)
    }

    /// The offset `face` looks into from this one.
    #[must_use]
    pub const fn step(self, face: Face) -> Self {
        let (dx, dy, dz) = face.step();
        Self::new(self.x + dx, self.y + dy, self.z + dz)
    }
}

/// A quarter-turn count, clockwise seen from above, that turns a tile from facing east.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum Rotation {
    /// Its signal runs east, `+x`.
    #[default]
    East,
    /// South, `+z`.
    South,
    /// West, `-x`.
    West,
    /// North, `-z`.
    North,
}

impl Rotation {
    /// Every rotation, east first and then clockwise.
    pub const ALL: [Self; 4] = [Self::East, Self::South, Self::West, Self::North];

    /// The rotation that turns east to `face`; `None` for `Up` and `Down`.
    #[must_use]
    pub const fn toward(face: Face) -> Option<Self> {
        Some(match face {
            Face::East => Self::East,
            Face::South => Self::South,
            Face::West => Self::West,
            Face::North => Self::North,
            Face::Up | Face::Down => return None,
        })
    }

    /// How many clockwise quarter turns this is.
    const fn turns(self) -> u8 {
        match self {
            Self::East => 0,
            Self::South => 1,
            Self::West => 2,
            Self::North => 3,
        }
    }

    /// `rel` turned, then placed at `origin`.
    #[must_use]
    pub const fn place(self, origin: Voxel, rel: Rel) -> Voxel {
        let (x, z) = match self.turns() {
            0 => (rel.x, rel.z),
            1 => (-rel.z, rel.x),
            2 => (-rel.x, -rel.z),
            _ => (rel.z, -rel.x),
        };
        Voxel::new(origin.x + x, origin.y + rel.y, origin.z + z)
    }

    /// `face` turned; `Up` and `Down` stay.
    #[must_use]
    pub const fn face(self, face: Face) -> Face {
        let mut turned = face;
        let mut left = self.turns();
        while left > 0 {
            turned = match turned.clockwise() {
                Some(next) => next,
                None => return face,
            };
            left -= 1;
        }
        turned
    }

    /// The face the tile's signal runs toward.
    #[must_use]
    pub const fn forward(self) -> Face {
        self.face(Face::East)
    }
}

impl Serialize for Rotation {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.forward().as_str())
    }
}

/// `face` as the game names it, for a `#[serde(serialize_with)]`.
#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "`serialize_with` hands the field over by reference"
)]
pub(crate) fn serialize_face<S: Serializer>(face: &Face, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(face.as_str())
}

/// The four sideways faces, in the order every pass tries them.
pub(crate) const SIDES: [Face; 4] = [Face::East, Face::South, Face::West, Face::North];

#[cfg(test)]
mod tests;
