//! What a constraint catalog requires of the cells around a block for the game to keep it where a
//! build puts it (`spec/lint` "Constraint checks").

use serde::Deserialize;

use super::BlockState;
use super::joins::Face;

/// What one block state needs of the cells beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Constraint {
    /// It hangs on the face of the cell `host` leads to that looks back at it, which must be
    /// whole.
    Hangs {
        /// The way from the block to the cell it hangs on.
        host: Face,
    },
    /// It stands on the cell below it or hangs from the cell above it, whose face toward it must
    /// give `needs`.
    Rests {
        /// The cell that holds it.
        host: Vertical,
        /// What that cell's face must give.
        needs: Support,
    },
    /// It falls when the cell below it holds air or a fluid.
    Falls,
}

/// The space an entity takes in the world, in blocks (`spec/lint` "Entity AABB").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntityBox {
    /// Across the entity, or along the face it hangs on; per block of a painting's variant.
    pub width: f64,
    /// Up the entity, or up the face it hangs on; per block of a painting's variant.
    pub height: f64,
    /// Out of the face, for an entity that hangs on one; `None` for one that stands.
    pub depth: Option<f64>,
}

/// The cell below a block or the cell above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Vertical {
    /// The cell below, which the block stands on.
    Below,
    /// The cell above, which the block hangs from.
    Above,
}

impl Vertical {
    /// The way from the block to the cell.
    #[must_use]
    pub fn face(self) -> Face {
        match self {
            Self::Below => Face::Down,
            Self::Above => Face::Up,
        }
    }
}

/// What a face gives a block set against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Support {
    /// The whole face, as a full block's side gives and a glass pane's does not.
    Sturdy,
    /// The centre of the face, which a post gives as well as a whole face does.
    Center,
}

/// What a cell holds, as the constraint checks read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Contents {
    /// Air: `air`, `cave_air` or `void_air`.
    Air,
    /// Water.
    Water,
    /// Lava.
    Lava,
    /// Any other block.
    Block,
}

impl Contents {
    /// What a cell holding the canonical block `id` holds.
    #[must_use]
    pub fn of_id(id: &str) -> Self {
        match id {
            BlockState::AIR_ID | "minecraft:cave_air" | "minecraft:void_air" => Self::Air,
            BlockState::WATER_ID => Self::Water,
            "minecraft:lava" => Self::Lava,
            _ => Self::Block,
        }
    }
}
