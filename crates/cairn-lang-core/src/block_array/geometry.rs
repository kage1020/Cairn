//! Where a member sits in its body's block array and what it occupies there: its anchor, its
//! declared and actual bboxes and its host face (`spec/entities` "Anchor conventions").

use serde::Serialize;

use super::joins::Face;
use super::openings::WallSide;

/// A cell of a block array, `(x, y, z)`.
type Cell = (u32, u32, u32);

/// A box of cells, both corners included.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct Bbox {
    /// The corner with the lowest `x`, `y` and `z`.
    pub min: (u32, u32, u32),
    /// The corner with the highest `x`, `y` and `z`.
    pub max: (u32, u32, u32),
}

impl Bbox {
    /// The box holding `cell` alone.
    #[must_use]
    pub const fn cell(cell: (u32, u32, u32)) -> Self {
        Self {
            min: cell,
            max: cell,
        }
    }

    /// The box with `a` and `b` as two of its opposite corners, in either order.
    #[must_use]
    pub fn spanning(a: (u32, u32, u32), b: (u32, u32, u32)) -> Self {
        Self::cell(a).including(b)
    }

    /// The smallest box holding this one and `cell`.
    #[must_use]
    pub fn including(self, cell: (u32, u32, u32)) -> Self {
        Self {
            min: (
                self.min.0.min(cell.0),
                self.min.1.min(cell.1),
                self.min.2.min(cell.2),
            ),
            max: (
                self.max.0.max(cell.0),
                self.max.1.max(cell.1),
                self.max.2.max(cell.2),
            ),
        }
    }

    /// The smallest box holding every one of `cells`, or `None` when there are none.
    pub(crate) fn around(cells: impl IntoIterator<Item = Cell>) -> Option<Self> {
        cells.into_iter().fold(None, |held, cell| {
            Some(held.map_or(Self::cell(cell), |held: Self| held.including(cell)))
        })
    }

    /// The box `rows` higher.
    #[must_use]
    pub(crate) fn raised(self, rows: u32) -> Self {
        Self {
            min: raised(self.min, rows),
            max: raised(self.max, rows),
        }
    }
}

/// `cell`, `rows` higher.
pub(crate) fn raised((x, y, z): Cell, rows: u32) -> Cell {
    (x, y.saturating_add(rows), z)
}

/// A member's anchor, declared bbox, actual bbox and host face, in its body's block array.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MemberGeometry {
    /// The cell the member's line places it by.
    pub anchor: (u32, u32, u32),
    /// The box the member's line declares.
    pub declared_bbox: Bbox,
    /// The box around every cell the member holds once its body is painted.
    pub actual_bbox: Bbox,
    /// The face of the wall or wall top the member is set against, named for the way it looks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_face: Option<Face>,
}

impl MemberGeometry {
    /// This geometry `rows` higher, as a circuit laid under the body raises it.
    #[must_use]
    pub(crate) fn raised(self, rows: u32) -> Self {
        Self {
            anchor: raised(self.anchor, rows),
            declared_bbox: self.declared_bbox.raised(rows),
            actual_bbox: self.actual_bbox.raised(rows),
            ..self
        }
    }
}

/// What a member's line declares, before lowering knows which cells it holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Declared {
    pub(crate) anchor: Cell,
    pub(crate) bbox: Bbox,
    pub(crate) host_face: Option<Face>,
}

impl Declared {
    /// A member that declares the one `cell` it is anchored at.
    pub(crate) const fn cell(cell: Cell, host_face: Option<Face>) -> Self {
        Self {
            anchor: cell,
            bbox: Bbox::cell(cell),
            host_face,
        }
    }

    /// A member that declares `bbox`, anchored at its low corner.
    pub(crate) const fn boxed(bbox: Bbox, host_face: Option<Face>) -> Self {
        Self {
            anchor: bbox.min,
            bbox,
            host_face,
        }
    }

    /// The geometry of a member that declared this and holds the cells `actual` bounds.
    pub(crate) const fn holding(self, actual: Bbox) -> MemberGeometry {
        MemberGeometry {
            anchor: self.anchor,
            declared_bbox: self.bbox,
            actual_bbox: actual,
            host_face: self.host_face,
        }
    }
}

/// The face of the `side` wall that looks out of the struct.
pub(crate) const fn outer_face(side: WallSide) -> Face {
    match side {
        WallSide::Front => Face::South,
        WallSide::Back => Face::North,
        WallSide::Left => Face::West,
        WallSide::Right => Face::East,
    }
}
