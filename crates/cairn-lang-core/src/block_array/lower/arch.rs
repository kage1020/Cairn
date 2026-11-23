//! Cutting an `arch`, an opening through the box between its corners sprung from an upside-down
//! stair at each end of its top row, and each copy its `repeat=` and `step=` ask for
//! (`spec/blockstate` "Derive by default, promote on override").

use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::args::{diag_invalid_member, ident_or_ignore};
use crate::block_array::geometry::Declared;
use crate::block_array::roof::{StairShape, stair_state};
use crate::block_array::{Compass, PaletteIndex};

use super::canvas::{MemberCanvas, ScopePalette};
use super::cell_box::{Cell, CellBox, Shape, repeated_cells};
use super::fixture::Refusal;
use super::geometry_material::{Family, GeometryMemberDescription, geometry_material_id};
use super::members::{MemberState, StructCtx, default_block_id, resolve_member_state};
use super::repeat::{Repeat, read_series};
use super::stand::word;

/// Catalog token an `arch`'s default stair comes from; the pack spells it per edition.
const ARCH_TOKEN: &str = "arch.default";

/// Arch stair id used when the pack cannot supply one, in the base edition's spelling.
const ARCH_BASE_ID: &str = "minecraft:stone_brick_stairs";

/// The axes an arch spans.
const AXES: [&str; 2] = ["x", "z"];

/// The horizontal axis an arch spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Span {
    X,
    Z,
}

impl Span {
    /// Where `cell` lies along the span.
    fn along(self, cell: Cell) -> u32 {
        match self {
            Self::X => cell.0,
            Self::Z => cell.2,
        }
    }

    /// The way the stair at the span's low end faces, out of the opening, and the high end's.
    fn facings(self) -> (Compass, Compass) {
        match self {
            Self::X => (Compass::West, Compass::East),
            Self::Z => (Compass::North, Compass::South),
        }
    }
}

/// Cut an `arch` and each copy of its series.
pub(super) fn fill_arch(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let (axis, unread) = match ident_or_ignore(member, "axis", "`x` or `z`") {
        Ok(axis) => (axis, None),
        Err(argument) => (None, Some(argument)),
    };
    let not_cut = match cut_arch(member, axis, y_offset, ctx, palette, canvas, diagnostics) {
        Ok(true) => None,
        Ok(false) => Some(
            "this arch is not cut either way — see the finding on the material its `mat_slot=` \
             names",
        ),
        Err(refusal) => {
            diagnostics.push(*refusal);
            Some("this arch is not cut either way — see the finding on the same line")
        }
    };
    diagnostics.extend(unread.map(|argument| {
        argument.report(not_cut.unwrap_or(
            "the arch spans the longer of its box's `x` and `z` sides, as with no `axis=`",
        ))
    }));
}

/// [`fill_arch`], answering whether the arch was cut, or the finding that refuses it; a state
/// literal the target refuses is reported where it is written and cuts nothing.
fn cut_arch(
    member: &Member,
    axis: Option<&str>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<bool, Refusal> {
    let axis = word(member, "axis", axis, &AXES)?;
    let series = read_series(member)?;
    let cells = repeated_cells(member, Shape::Arch, y_offset, series, ctx)?;
    let span = span_of(member, axis, cells)?;
    if let Some(refusal) = overlap_refusal(member, cells, series) {
        return Err(Box::new(refusal));
    }
    let Some(id) = arch_block_id(member, ctx, diagnostics)? else {
        return Ok(false);
    };
    let (low_facing, high_facing) = span.facings();
    let half = "top";
    let low_stair = palette.intern(stair_state(&id, low_facing, half, StairShape::Straight));
    let high_stair = palette.intern(stair_state(&id, high_facing, half, StairShape::Straight));
    canvas.declare(Declared::boxed(cells.repeated(series).bbox(), None));
    for shift in series.shifts() {
        let copy = cells.shifted(shift);
        let (low, high) = copy.corners();
        for cell in copy.cells() {
            let slot = if cell.1 != high.1 {
                PaletteIndex::AIR
            } else if span.along(cell) == span.along(low) {
                low_stair
            } else if span.along(cell) == span.along(high) {
                high_stair
            } else {
                PaletteIndex::AIR
            };
            canvas.paint(cell, || slot);
        }
    }
    Ok(true)
}

/// The axis an arch spans, or the refusal of a box whose span cannot be told or is shorter than 2.
fn span_of(member: &Member, axis: Option<&str>, cells: CellBox) -> Result<Span, Refusal> {
    let (low, high) = cells.corners();
    let width = high.0.saturating_sub(low.0).saturating_add(1);
    let depth = high.2.saturating_sub(low.2).saturating_add(1);
    let too_short = |along: &str| {
        Box::new(diag_invalid_member(
            member,
            format!("arch spans 1 cell along {along}, leaving no room for its two stairs"),
            vec![
                "an arch spans at least 2 cells: widen its box along the axis it spans".to_owned(),
            ],
        ))
    };
    if axis.is_none() && width < 2 && depth < 2 {
        return Err(too_short("`x` and along `z`"));
    }
    let span = match axis {
        Some("x") => Span::X,
        Some(_) => Span::Z,
        None if width > depth => Span::X,
        None if depth > width => Span::Z,
        None => {
            return Err(Box::new(diag_invalid_member(
                member,
                format!(
                    "arch box is {width} long on `x` and on `z`, so nothing says which way it \
                     spans"
                ),
                vec!["write `axis=x` or `axis=z` for the way the arch spans".to_owned()],
            )));
        }
    };
    let (name, length) = match span {
        Span::X => ("x", width),
        Span::Z => ("z", depth),
    };
    if length < 2 {
        return Err(too_short(&format!("`{name}`")));
    }
    Ok(span)
}

/// The refusal of a series whose copies share cells, where one copy's air would cut another's
/// stair, or `None`.
fn overlap_refusal(member: &Member, cells: CellBox, series: Repeat) -> Option<Diagnostic> {
    let (low, high) = cells.corners();
    let extent = |l: u32, h: u32| h.saturating_sub(l).saturating_add(1);
    let (w, h, d) = (
        extent(low.0, high.0),
        extent(low.1, high.1),
        extent(low.2, high.2),
    );
    let (dx, dy, dz) = series.step();
    if !series.repeats() || dx >= w || dy >= h || dz >= d {
        return None;
    }
    Some(diag_invalid_member(
        member,
        format!(
            "arch `step={dx},{dy},{dz}` is shorter than its box on every axis, so each copy \
             shares cells with the one before"
        ),
        vec![format!(
            "the box is {w} long on `x`, {h} on `y` and {d} on `z`: give `step=` at least that on \
             one axis, so no two copies share a cell"
        )],
    ))
}

/// The stair an arch springs from: its `mat_slot=`, or the pack's default arch; `None` for a
/// binding whose state literal the target refused, which cuts no arch.
fn arch_block_id(
    member: &Member,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<String>, Refusal> {
    let resolved = match resolve_member_state(
        member,
        ctx.scope,
        ctx.registry,
        diagnostics,
        ctx.theme_missing,
    ) {
        MemberState::Bound(state) => Some(state),
        MemberState::Unbound => None,
        MemberState::Refused => return Ok(None),
    };
    geometry_material_id(
        member,
        ctx.scope,
        ctx.registry,
        resolved.as_ref(),
        || default_block_id(member, ctx.registry, ARCH_TOKEN, ARCH_BASE_ID),
        &GeometryMemberDescription {
            subject: "arch".to_owned(),
            states_from: "the end of its top row each stair lies at",
            requires: Some(Family::Stair),
        },
        diagnostics,
    )
    .map(Some)
}
