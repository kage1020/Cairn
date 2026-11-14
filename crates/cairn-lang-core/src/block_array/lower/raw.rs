//! Painting a `raw_block` or `raw_fill`: the block id its `mat=` names, written as given into the
//! cells its triples name (`spec/materials-themes` "Raw blocks").

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::args::diag_invalid_member;
use crate::block_array::{BlockEntityData, BlockState};
use crate::block_array::geometry::Declared;
use crate::block_array::material::{IdOrigin, block_id_state, validated_id};
use crate::resolve::{TokenKind, classify_token};

use super::canvas::{MemberCanvas, ScopePalette};
use super::cell_box::{Shape, written_cells};
use super::entity::{nbt_of, report_unread};
use super::members::StructCtx;
use super::token_diag::{IdSite, check_state_literal, diag_undeclared_id};

/// The note on a `mat=` that is missing or no block id, saying what one is.
const BLOCK_ID: &str = "write `mat=` as the block id itself, `namespace:path`, as in \
                        `mat=minecraft:stone`";

/// Paint a raw member's block into every cell its triples name, the one `shape` writes.
pub(super) fn fill_raw(
    member: &Member,
    shape: Shape,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // `mat=` is judged even where the cells are refused or deferred, as nothing else judges it.
    let cells =
        written_cells(member, shape, y_offset, ctx).map_err(|refusal| diagnostics.push(*refusal));
    let state = raw_state(member, ctx, diagnostics);
    let (Ok(cells), Some(state)) = (cells, state) else {
        return;
    };
    let idx = palette.intern(state);
    canvas.declare(Declared::boxed(cells.bbox(), None));
    for cell in cells.cells() {
        canvas.paint(cell, || idx);
    }
    // A `raw_fill` reads no `nbt=`, which `check` reports.
    if shape == Shape::Cell {
        let mut unread = Vec::new();
        if let Some(compound) = nbt_of(member, &mut unread) {
            for cell in cells.cells() {
                canvas.set_block_entity(
                    cell,
                    BlockEntityData::Nbt {
                        compound: compound.clone(),
                    },
                );
            }
        }
        report_unread(unread, diagnostics, |_| "the block is placed without it");
    }
}

/// The block a raw member's `mat=` names, checked against the target, or `None` after the finding
/// that refuses it.
fn raw_state(
    member: &Member,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<BlockState> {
    let keyword = member.role.keyword();
    let Some(written) = member.arguments.get("mat") else {
        diagnostics.push(diag_invalid_member(
            member,
            format!("{keyword} has no `mat=`, so nothing says which block it places"),
            vec![BLOCK_ID.to_owned()],
        ));
        return None;
    };
    let ValueKind::BlockId(id) = &written.value.kind else {
        let mut notes = vec![BLOCK_ID.to_owned()];
        if let ValueKind::Token(token) = &written.value.kind
            && classify_token(&written.value) == TokenKind::Canonical
        {
            notes.push(format!(
                "`@{token}` is a canonical token, which a theme binds to a `mat_slot=`; write \
                 `mat=minecraft:{token}` to place the block it names"
            ));
        }
        diagnostics.push(diag_invalid_member(
            member,
            format!(
                "{keyword} `mat=` must be a block id, not {}",
                written.value.describe()
            ),
            notes,
        ));
        return None;
    };
    match validated_id(block_id_state(id), ctx.registry, &IdOrigin::Authored) {
        Ok(state) => {
            check_state_literal(written, &state, ctx.registry, diagnostics).then_some(state)
        }
        Err(unknown) => {
            diagnostics.push(diag_undeclared_id(
                written.span.clone(),
                &unknown,
                IdSite::Raw,
            ));
            None
        }
    }
}
