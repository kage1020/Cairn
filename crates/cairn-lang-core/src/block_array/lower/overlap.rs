//! Placements of one site whose boxes share a cell, and walkway strips laid inside a placement they
//! do not join (`spec/entities` "Anchor conventions").

use std::fmt;

use indexmap::IndexMap;

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::resolve::ValidatedConnect;

use crate::block_array::Placement;

use super::anchor::PlaceAnchor;
use super::site_floor::PlacedBody;

/// A world cell, `(x, y, z)`.
type Cell = (i32, i32, i32);

/// The cells a placement's structure file writes, or the part two of them share.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorldBox {
    min: (i64, i64, i64),
    max: (i64, i64, i64),
}

impl WorldBox {
    /// Every cell `placement`'s array covers in the world.
    fn of(placement: &Placement) -> Self {
        let (x, y, z) = placement.origin;
        let far = |low: i32, extent: u32| i64::from(low) + i64::from(extent) - 1;
        Self {
            min: (i64::from(x), i64::from(y), i64::from(z)),
            max: (
                far(x, placement.dims.x),
                far(y, placement.dims.y),
                far(z, placement.dims.z),
            ),
        }
    }

    /// The box around every one of `cells`, or `None` when there are none.
    fn around(cells: impl IntoIterator<Item = Cell>) -> Option<Self> {
        cells.into_iter().fold(None, |held, (x, y, z)| {
            let cell = (i64::from(x), i64::from(y), i64::from(z));
            Some(held.map_or(Self::cell(cell), |held: Self| held.including(cell)))
        })
    }

    const fn cell(cell: (i64, i64, i64)) -> Self {
        Self {
            min: cell,
            max: cell,
        }
    }

    fn including(self, (x, y, z): (i64, i64, i64)) -> Self {
        Self {
            min: (self.min.0.min(x), self.min.1.min(y), self.min.2.min(z)),
            max: (self.max.0.max(x), self.max.1.max(y), self.max.2.max(z)),
        }
    }

    /// The cells both boxes hold, or `None` when they share none.
    fn meet(self, other: Self) -> Option<Self> {
        let min = (
            self.min.0.max(other.min.0),
            self.min.1.max(other.min.1),
            self.min.2.max(other.min.2),
        );
        let max = (
            self.max.0.min(other.max.0),
            self.max.1.min(other.max.1),
            self.max.2.min(other.max.2),
        );
        (min.0 <= max.0 && min.1 <= max.1 && min.2 <= max.2).then_some(Self { min, max })
    }

    fn contains(self, (x, y, z): Cell) -> bool {
        let (x, y, z) = (i64::from(x), i64::from(y), i64::from(z));
        (self.min.0..=self.max.0).contains(&x)
            && (self.min.1..=self.max.1).contains(&y)
            && (self.min.2..=self.max.2).contains(&z)
    }
}

impl fmt::Display for WorldBox {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            min: (x0, y0, z0),
            max: (x1, y1, z1),
        } = self;
        write!(f, "({x0}, {y0}, {z0}) to ({x1}, {y1}, {z1})")
    }
}

/// What every overlap finding says the two files do to each other.
const ERASES: &str = "each placement is written with air in every cell its body does not fill, so \
                      whichever of the two is placed second decides what those cells hold";

/// Report every `place` row whose box shares a cell with an earlier row's in the same site.
pub(super) fn report_placement_overlaps(
    placed: &IndexMap<String, PlacedBody>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let bodies: Vec<&PlacedBody> = placed.values().collect();
    for (later_at, later) in bodies.iter().enumerate() {
        let later_box = WorldBox::of(&later.placement);
        for earlier in &bodies[..later_at] {
            if earlier.placement.site != later.placement.site {
                continue;
            }
            let earlier_box = WorldBox::of(&earlier.placement);
            if let Some(shared) = later_box.meet(earlier_box) {
                diagnostics.push(diag_placement_overlap(
                    (later, later_box),
                    (earlier, earlier_box),
                    shared,
                ));
            }
        }
    }
}

fn diag_placement_overlap(
    (later, later_box): (&PlacedBody, WorldBox),
    (earlier, earlier_box): (&PlacedBody, WorldBox),
    shared: WorldBox,
) -> Diagnostic {
    let (later_id, earlier_id) = (&later.placement.place_id, &earlier.placement.place_id);
    let fix = match later.anchor {
        PlaceAnchor::EastOf { gap, .. } => format!(
            "write `gap={}` on this row to move it, and every row placed relative to it, east of \
             `place id={earlier_id}`",
            gap + (earlier_box.max.0 - later_box.min.0 + 1),
        ),
        PlaceAnchor::NorthOf { gap, .. } => format!(
            "write `gap={}` on this row to move it, and every row placed relative to it, north of \
             `place id={earlier_id}`",
            gap + (later_box.max.2 - earlier_box.min.2 + 1),
        ),
        PlaceAnchor::WorldOrigin => "this row's `at=origin` puts it on the world origin, which \
                                     no `gap=` moves; place it `east_of=` or `north_of=` the row \
                                     it should stand beside"
            .to_owned(),
    };
    Diagnostic {
        code: DiagnosticCode::PlacementOverlap,
        span: later.row.clone(),
        primary: format!(
            "`place id={later_id}` spans {later_box} and overlaps `place id={earlier_id}` from \
             {shared}",
        ),
        notes: vec![
            DiagnosticNote {
                span: Some(earlier.row.clone()),
                message: format!("`place id={earlier_id}` spans {earlier_box}"),
            },
            DiagnosticNote {
                span: None,
                message: format!("two placements of one site may share no cell: {ERASES}; {fix}"),
            },
        ],
        data: None,
    }
}

/// Report every placement of `connect`'s site, other than the two it joins, that `strip` lays a
/// cell inside.
pub(super) fn report_strip_overlaps(
    connect: &ValidatedConnect,
    strip: &[Cell],
    ends: (&str, &str),
    placed: &IndexMap<String, PlacedBody>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (key, body) in placed {
        if body.placement.site != connect.site || key == ends.0 || key == ends.1 {
            continue;
        }
        let body_box = WorldBox::of(&body.placement);
        let inside: Vec<Cell> = strip
            .iter()
            .copied()
            .filter(|&cell| body_box.contains(cell))
            .collect();
        let Some(shared) = WorldBox::around(inside.iter().copied()) else {
            continue;
        };
        let id = &body.placement.place_id;
        let cells = if inside.len() == 1 { "cell" } else { "cells" };
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::PlacementOverlap,
            span: connect.span.clone(),
            primary: format!(
                "walkway `{from} ↔ {to}` lays {count} {cells} inside `place id={id}`, from \
                 {shared}",
                from = connect.from,
                to = connect.to,
                count = inside.len(),
            ),
            notes: vec![
                DiagnosticNote {
                    span: Some(body.row.clone()),
                    message: format!("`place id={id}` spans {body_box}"),
                },
                DiagnosticNote {
                    span: None,
                    message: format!(
                        "a walkway may lay no cell inside a placement other than the two it \
                         joins: {ERASES}, its own block or the walkway's; move `place id={id}` \
                         clear of the strip"
                    ),
                },
            ],
            data: None,
        });
    }
}
