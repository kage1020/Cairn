//! Standing a `column`, a run of its material upright from a cell, and each copy its `repeat=` and
//! `step=` ask for (`spec/blockstate` "Derive by default, promote on override").

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::BlockState;
use crate::block_array::args::diag_invalid_member;
use crate::block_array::geometry::Declared;

use super::canvas::{MemberCanvas, ScopePalette};
use super::cell_box::{Shape, repeated_cells};
use super::fixture::Refusal;
use super::geometry_material::{GeometryMemberDescription, geometry_material_id};
use super::members::{MemberState, StructCtx, default_block_id, resolve_member_state};
use super::repeat::read_series;

/// Catalog token a `column`'s default block comes from; the pack spells it per edition.
const COLUMN_TOKEN: &str = "column.default";

/// Column block id used when the pack cannot supply one, in the base edition's spelling.
const COLUMN_BASE_ID: &str = "minecraft:stone_bricks";

/// The note on a `height=` that is missing or no positive whole number, saying what one is.
const HEIGHT_FORM: &str = "write `height=N`, where `N` is a positive whole number: the column \
                           rises `N` rows from the cell its `at=` names, that cell included";

/// Stand a `column` and each copy of its series.
pub(super) fn fill_column(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Err(refusal) = stand_column(member, y_offset, ctx, palette, canvas, diagnostics) {
        diagnostics.push(*refusal);
    }
}

/// [`fill_column`], or the finding that refuses it; a state literal the target refuses is reported
/// where it is written and stands nothing.
fn stand_column(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), Refusal> {
    let height = column_height(member)?;
    let series = read_series(member)?;
    let cells = repeated_cells(member, Shape::Column { height }, y_offset, series, ctx)?;
    let Some(state) = column_block(member, ctx, diagnostics)? else {
        return Ok(());
    };
    let index = palette.intern(state);
    canvas.declare(Declared::boxed(cells.repeated(series).bbox(), None));
    for shift in series.shifts() {
        for cell in cells.shifted(shift).cells() {
            canvas.paint(cell, || index);
        }
    }
    Ok(())
}

/// The rows a column's `height=` raises it, or the refusal of one that is missing or no positive
/// whole number.
fn column_height(member: &Member) -> Result<u32, Refusal> {
    let refuse = |primary: String| {
        Box::new(diag_invalid_member(
            member,
            primary,
            vec![HEIGHT_FORM.into()],
        ))
    };
    let Some(raw) = member.arguments.get("height") else {
        return Err(refuse(
            "column has no `height=`, so nothing says how many rows it rises".to_owned(),
        ));
    };
    match raw.value.kind {
        ValueKind::Int(n) if n >= 1 => u32::try_from(n).ok(),
        _ => None,
    }
    .ok_or_else(|| {
        refuse(format!(
            "column `height=` must be a positive integer that fits in u32, not {}",
            raw.value.describe(),
        ))
    })
}

/// The block a column stands: its `mat_slot=` binding as written, or the pack's default column;
/// `None` for a binding whose state literal the target refused.
fn column_block(
    member: &Member,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<BlockState>, Refusal> {
    match resolve_member_state(
        member,
        ctx.scope,
        ctx.registry,
        diagnostics,
        ctx.theme_missing,
    ) {
        MemberState::Bound(state) => Ok(Some(state)),
        MemberState::Refused => Ok(None),
        MemberState::Unbound => geometry_material_id(
            member,
            ctx.scope,
            ctx.registry,
            None,
            || default_block_id(member, ctx.registry, COLUMN_TOKEN, COLUMN_BASE_ID),
            &GeometryMemberDescription {
                subject: "column".to_owned(),
                states_from: "its material",
                requires: None,
            },
            diagnostics,
        )
        .map(|id| Some(BlockState::bare(id))),
    }
}
