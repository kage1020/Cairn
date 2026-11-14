//! Painting a `raw_block` or `raw_fill`: the block id its `mat=` names, written as given into the
//! cells its triples name and those of each copy its `raw_repeat` asks for (`spec/materials-themes`
//! "Raw blocks").

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::BlockState;
use crate::block_array::args::diag_invalid_member;
use crate::block_array::material::{IdOrigin, block_id_state, validated_id};
use crate::resolve::{TokenKind, classify_token};

use super::canvas::{MemberCanvas, ScopePalette};
use super::cell_box::{Shape, repeated_cells};
use super::members::StructCtx;
use super::repeat::origin_refusal;
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
    let repeat = ctx.repeat_of(member);
    // `mat=` is judged even where the cells are refused or deferred, as nothing else judges it.
    let cells = repeated_cells(member, shape, y_offset, repeat, ctx)
        .map_err(|refusal| diagnostics.push(*refusal));
    let origin = origin_refusal(member).map(|refusal| diagnostics.push(refusal));
    let state = raw_state(member, ctx, diagnostics);
    let (Ok(cells), None, Some(state)) = (cells, origin, state) else {
        return;
    };
    let idx = palette.intern(state);
    for shift in repeat.shifts() {
        for cell in cells.shifted(shift).cells() {
            canvas.paint(cell, || idx);
        }
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
