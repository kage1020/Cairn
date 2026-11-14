//! What a constraint catalog requires of the cells around a block for the game to keep it where a
//! build puts it (`spec/lint` "Constraint checks").

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
        /// [`Face::Down`] or [`Face::Up`]: the way from the block to the cell that holds it.
        host: Face,
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

/// What a face gives a block set against it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Support {
    /// The whole face, as a full block's side gives and a glass pane's does not.
    Sturdy,
    /// The centre of the face, which a post gives as well as a whole face does.
    Center,
}
