//! Placing a `pressure_plate` member.

use crate::diagnostic::{Diagnostic, DiagnosticNote};
use crate::intent::Member;

use crate::block_array::BlockState;
use crate::block_array::args::diag_deferred_member_reason;

use super::canvas::{MemberCanvas, ScopePalette};
use super::fixture::{Fixture, Refusal, fixture_cell, outside_the_array};
use super::members::{StructCtx, default_block_id, resolve_member_state};

/// Catalog token a `pressure_plate`'s default material comes from; the pack spells it per edition.
pub(super) const PRESSURE_PLATE_TOKEN: &str = "pressure_plate.default";

/// Pressure plate id used when the pack cannot supply one: the Java spelling, Java being the base
/// edition (`spec/versioning-editions` "Backend = data tables").
pub(super) const PRESSURE_PLATE_BASE_ID: &str = "minecraft:oak_pressure_plate";

/// How a plate's findings name it.
const PLATE: Fixture = Fixture {
    keyword: "pressure_plate",
    noun: "plate",
};

/// The note on every plate left unplaced that carries a `->` binding.
const BINDING_STILL_READ: &str =
    "its signal binding still reaches the netlist, with no plate placed to drive it";

/// `refusal`, with [`BINDING_STILL_READ`] after its notes when the plate has a `->` binding.
fn with_binding_note(member: &Member, refusal: Refusal) -> Diagnostic {
    let mut refusal = *refusal;
    // Synthesis reads the binding off the member line, never the block array.
    if member.binding.is_some() {
        refusal.notes.push(DiagnosticNote {
            span: None,
            message: BINDING_STILL_READ.to_owned(),
        });
    }
    refusal
}

/// Paint a `pressure_plate` fixture as a single plate voxel.
pub(super) fn fill_pressure_plate(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Err(refusal) = place_plate(member, y_offset, ctx, palette, canvas, diagnostics) {
        diagnostics.push(with_binding_note(member, refusal));
    }
}

/// [`fill_pressure_plate`], answering the finding that refuses the plate.
fn place_plate(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), Refusal> {
    let (_, cell) = fixture_cell(PLATE, member, y_offset, ctx)?;
    let base_id = plate_id_for_member(member, ctx, diagnostics)?;
    // Interned only once the cell exists, so a refusal leaves no palette entry.
    if canvas.try_paint(cell, || palette.intern(BlockState::bare(base_id))) {
        Ok(())
    } else {
        Err(Box::new(outside_the_array(PLATE, member)))
    }
}

/// The block id a `pressure_plate` paints: its `mat_slot=`, or the pack's default.
///
/// A binding with a state literal defers and paints nothing. An unresolved binding was already
/// reported, so falling back adds no finding about the binding.
fn plate_id_for_member(
    member: &Member,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<String, Refusal> {
    let resolved = resolve_member_state(
        member,
        ctx.scope,
        ctx.registry,
        diagnostics,
        ctx.theme_missing,
    );
    if let Some(state) = &resolved
        && !state.properties.is_empty()
    {
        return Err(Box::new(diag_deferred_member_reason(
            member,
            &format!(
                "pressure_plate does not honour bracketed state literals; the `mat_slot=` binding to `{}[...]` was not applied",
                state.id,
            ),
        )));
    }
    match resolved {
        Some(state) => Ok(state.id),
        None => default_block_id(
            member,
            ctx.registry,
            PRESSURE_PLATE_TOKEN,
            PRESSURE_PLATE_BASE_ID,
        ),
    }
}
