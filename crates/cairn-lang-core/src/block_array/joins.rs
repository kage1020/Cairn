//! What a block is to the rules that join panes, fences, walls and chests to their neighbours
//! (`spec/blockstate` "Connections").

/// A face of a block, named for the direction it looks toward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Face {
    /// `-y`.
    Down,
    /// `+y`.
    Up,
    /// `-z`.
    North,
    /// `+z`.
    South,
    /// `-x`.
    West,
    /// `+x`.
    East,
}

impl Face {
    /// The four sideways faces, clockwise from north.
    pub const SIDES: [Self; 4] = [Self::North, Self::East, Self::South, Self::West];

    /// The face named `name`, as the game names a direction.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "down" => Self::Down,
            "up" => Self::Up,
            "north" => Self::North,
            "south" => Self::South,
            "west" => Self::West,
            "east" => Self::East,
            _ => return None,
        })
    }

    /// The direction's name, which is also the connection state a sideways face sets.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Down => "down",
            Self::Up => "up",
            Self::North => "north",
            Self::South => "south",
            Self::West => "west",
            Self::East => "east",
        }
    }

    /// The face looking the other way.
    #[must_use]
    pub const fn opposite(self) -> Self {
        match self {
            Self::Down => Self::Up,
            Self::Up => Self::Down,
            Self::North => Self::South,
            Self::South => Self::North,
            Self::West => Self::East,
            Self::East => Self::West,
        }
    }

    /// The sideways face a quarter turn clockwise from this one, seen from above; `None` for
    /// `Down` and `Up`.
    #[must_use]
    pub const fn clockwise(self) -> Option<Self> {
        Some(match self {
            Self::North => Self::East,
            Self::East => Self::South,
            Self::South => Self::West,
            Self::West => Self::North,
            Self::Down | Self::Up => return None,
        })
    }

    /// The sideways face a quarter turn counter-clockwise from this one, seen from above; `None`
    /// for `Down` and `Up`.
    #[must_use]
    pub const fn counter_clockwise(self) -> Option<Self> {
        match self.clockwise() {
            Some(face) => Some(face.opposite()),
            None => None,
        }
    }

    /// The step `(dx, dy, dz)` to the cell this face looks into.
    #[must_use]
    pub const fn step(self) -> (i32, i32, i32) {
        match self {
            Self::Down => (0, -1, 0),
            Self::Up => (0, 1, 0),
            Self::North => (0, 0, -1),
            Self::South => (0, 0, 1),
            Self::West => (-1, 0, 0),
            Self::East => (1, 0, 0),
        }
    }

    const fn bit(self) -> u8 {
        1 << self as u8
    }
}

/// A set of a block's faces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Faces(u8);

impl Faces {
    /// No face.
    pub const NONE: Self = Self(0);
    /// Every face.
    pub const ALL: Self = Self(0b11_1111);

    /// The set with `face` added.
    #[must_use]
    pub const fn with(self, face: Face) -> Self {
        Self(self.0 | face.bit())
    }

    /// Whether the set holds `face`.
    #[must_use]
    pub const fn contains(self, face: Face) -> bool {
        self.0 & face.bit() != 0
    }
}

impl FromIterator<Face> for Faces {
    fn from_iter<I: IntoIterator<Item = Face>>(faces: I) -> Self {
        faces.into_iter().fold(Self::NONE, Self::with)
    }
}

/// The kind of block whose connection states the rules derive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum JoinKind {
    /// A glass pane or a run of bars.
    Pane,
    /// A wall.
    Wall,
    /// A fence; fences join only fences of the same group.
    Fence {
        /// Whether it is a wooden fence rather than one of the others.
        wooden: bool,
    },
    /// A fence gate.
    FenceGate {
        /// The way the gate faces, a sideways face.
        facing: Face,
        /// Whether it stands open.
        open: bool,
    },
    /// A chest, which pairs with a chest of the same block and facing beside it.
    Chest {
        /// The way its front faces, a sideways face.
        facing: Face,
    },
}

/// The parts of a wall that the bottom of the block above it covers, which stand them tall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Covers {
    /// Whether it covers the wall's post.
    pub post: bool,
    /// The sides of the wall it covers, of [`Face::SIDES`].
    pub sides: Faces,
}

impl Covers {
    /// The part named `name`, `post` or a side's direction, added.
    #[must_use]
    pub fn with(self, name: &str) -> Option<Self> {
        if name == "post" {
            return Some(Self { post: true, ..self });
        }
        let side = Face::from_name(name).filter(|face| Face::SIDES.contains(face))?;
        Some(Self {
            sides: self.sides.with(side),
            ..self
        })
    }
}

/// What one block state is to the connection rules at the pinned target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Joins {
    /// The kind of connecting block it is, or `None` for one that only offers its faces.
    pub kind: Option<JoinKind>,
    /// The faces the state's model covers whole.
    pub sturdy: Faces,
    /// What its bottom covers of a wall below, read when it is no connecting block.
    pub covers: Covers,
    /// Whether a pane, fence or wall never joins it by a whole face.
    pub never_joined_by_face: bool,
    /// Whether the game's `wall_post_override` tag holds it.
    pub raises_post: bool,
}

#[cfg(test)]
mod tests {
    use super::Face;

    #[test]
    fn a_quarter_turn_steps_through_the_sideways_faces_and_leaves_down_and_up_unturned() {
        let turns = Face::SIDES.map(|face| (face.clockwise(), face.counter_clockwise()));
        assert_eq!(
            turns,
            [
                (Some(Face::East), Some(Face::West)),
                (Some(Face::South), Some(Face::North)),
                (Some(Face::West), Some(Face::East)),
                (Some(Face::North), Some(Face::South)),
            ],
        );
        for face in [Face::Down, Face::Up] {
            assert_eq!((face.clockwise(), face.counter_clockwise()), (None, None));
        }
    }
}
