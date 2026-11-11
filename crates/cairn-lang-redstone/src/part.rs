//! The blocks a circuit is built from, as the simulator, the router and the writer read them.

use cairn_lang_core::block_array::Face;
use serde::Serialize;

use crate::geometry::{Rotation, Voxel, serialize_face};

/// One block of a `circuit` reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "part", rename_all = "snake_case")]
pub enum Part {
    /// A block that passes power on: the reservation's bottom layer, a torch's block, the block
    /// under dust.
    Block,
    /// Redstone dust.
    Dust,
    /// A redstone torch on the side of the block behind it, pointing toward `facing`.
    Torch {
        /// The way it points, away from its block.
        #[serde(serialize_with = "serialize_face")]
        facing: Face,
    },
    /// A repeater reading the cell behind it and driving the cell toward `facing`.
    Repeater {
        /// The way it drives.
        #[serde(serialize_with = "serialize_face")]
        facing: Face,
        /// Its delay.
        delay: RepeaterDelay,
    },
    /// A comparator reading the cell behind it and its sides, and driving the cell toward `facing`.
    Comparator {
        /// The way it drives.
        #[serde(serialize_with = "serialize_face")]
        facing: Face,
        /// Whether it subtracts its sides from its rear.
        subtract: bool,
    },
}

/// A repeater's delay, 1 to 4 ticks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct RepeaterDelay(u8);

impl RepeaterDelay {
    /// One tick, the shortest.
    pub const ONE: Self = Self::new(1);
    /// Two ticks.
    pub const TWO: Self = Self::new(2);

    /// The delay of `ticks`.
    ///
    /// # Panics
    ///
    /// On `ticks` outside 1 to 4, which no repeater can be set to.
    #[must_use]
    pub const fn new(ticks: u8) -> Self {
        assert!(matches!(ticks, 1..=4), "a repeater delays 1 to 4 ticks");
        Self(ticks)
    }

    /// Its ticks.
    #[must_use]
    pub const fn ticks(self) -> u8 {
        self.0
    }
}

impl Part {
    /// Whether the part is a block that passes power on.
    #[must_use]
    pub const fn conducts(self) -> bool {
        matches!(self, Self::Block)
    }

    /// The way a repeater or comparator drives; `None` for any other part.
    #[must_use]
    pub const fn drives(self) -> Option<Face> {
        match self {
            Self::Repeater { facing, .. } | Self::Comparator { facing, .. } => Some(facing),
            Self::Block | Self::Dust | Self::Torch { .. } => None,
        }
    }

    /// Whether the part at `at` drives `into` as a repeater or comparator does.
    #[must_use]
    pub fn drives_into(self, at: Voxel, into: Voxel) -> bool {
        self.drives().is_some_and(|facing| at.step(facing) == into)
    }

    /// The part turned by `rotation`.
    #[must_use]
    pub const fn turned(self, rotation: Rotation) -> Self {
        match self {
            Self::Block => Self::Block,
            Self::Dust => Self::Dust,
            Self::Torch { facing } => Self::Torch {
                facing: rotation.face(facing),
            },
            Self::Repeater { facing, delay } => Self::Repeater {
                facing: rotation.face(facing),
                delay,
            },
            Self::Comparator { facing, subtract } => Self::Comparator {
                facing: rotation.face(facing),
                subtract,
            },
        }
    }
}
