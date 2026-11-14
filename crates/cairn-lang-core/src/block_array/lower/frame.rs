//! Laying the `frame=` a theme binds on a `door` or `window` round the openings it cut
//! (`spec/materials-themes` "Selector bindings").

use crate::block_array::BlockState;
use crate::block_array::openings::{WallSide, wall_length, wall_local_to_grid};

use super::canvas::{Canvas, LazySlot, ScopePalette};
use super::members::StructCtx;

/// One rectangle an opening cut, in the wall-local columns and rows of its side.
#[derive(Debug, Clone, Copy)]
pub(super) struct Cut {
    pub(super) side: WallSide,
    pub(super) offset: u32,
    pub(super) y_start: u32,
    pub(super) width: u32,
    pub(super) height: u32,
}

impl Cut {
    /// Whether wall-local `(u, v)` lies inside the rectangle.
    fn holds(self, u: u32, v: u32) -> bool {
        (self.offset..self.offset.saturating_add(self.width)).contains(&u)
            && (self.y_start..self.y_start.saturating_add(self.height)).contains(&v)
    }

    /// The cells one step round the rectangle, in the wall course that holds it and off the
    /// wall's two end columns, which it shares with the walls round each corner.
    fn ring(self, ctx: &StructCtx<'_>) -> Vec<(u32, u32, u32)> {
        let Some((bottom, top)) = ctx.wall_column.course_at(self.y_start) else {
            return Vec::new();
        };
        let len = wall_length(self.side, ctx.interior_w, ctx.interior_h);
        let last_row = self.y_start.saturating_add(self.height.saturating_sub(1));
        let rows = self.y_start.saturating_sub(1).max(bottom)..=last_row.saturating_add(1).min(top);
        let columns = self.offset.saturating_sub(1)..=self.offset.saturating_add(self.width);
        rows.flat_map(|v| columns.clone().map(move |u| (u, v)))
            .filter(|&(u, v)| 0 < u && u.saturating_add(1) < len && !self.holds(u, v))
            .filter_map(|(u, v)| {
                wall_local_to_grid(
                    self.side,
                    u,
                    v,
                    ctx.overhang,
                    ctx.interior_w,
                    ctx.interior_h,
                    ctx.dims,
                )
            })
            .collect()
    }
}

/// The frame a `door` or `window` binds, with the openings it cut.
pub(super) struct Frame {
    /// The member's index in the flattened paint list.
    member: u32,
    slot: LazySlot,
    cuts: Vec<Cut>,
}

impl Frame {
    pub(super) fn new(member: u32, state: BlockState, cuts: Vec<Cut>) -> Self {
        Self {
            member,
            slot: LazySlot::new(state),
            cuts,
        }
    }
}

/// Lay each of `frames`, in paint-list order, on the cells of the ring round its openings that a
/// member of `walls` held once every opening was cut.
pub(super) fn lay_frames(
    frames: Vec<Frame>,
    walls: &[u32],
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut Canvas,
) {
    let wall_cells = canvas.cells_owned_by(walls);
    for Frame {
        member,
        mut slot,
        cuts,
    } in frames
    {
        let mut member_canvas = canvas.for_member(member);
        for cell in cuts.into_iter().flat_map(|cut| cut.ring(ctx)) {
            if wall_cells.contains(&cell) {
                member_canvas.paint(cell, || slot.intern(palette));
            }
        }
    }
}
