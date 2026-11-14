//! Placing a `note_block` in the cell its coordinate triple names, with the `note` and
//! `instrument` its line writes (`spec/blockstate` "Derive by default, promote on override").

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::BlockState;
use crate::block_array::args::{
    UnreadArgument, diag_invalid_member, ident_or_ignore, read_or_ignore,
};

use super::canvas::{MemberCanvas, ScopePalette};
use super::cell_box::{Shape, written_cells};
use super::fixture::{Fixture, Refusal, outside_the_array};
use super::members::{StructCtx, default_block_id};
use super::token_diag::check_written_state;

/// Catalog token a `note_block`'s block comes from; the pack spells it per edition.
const NOTE_BLOCK_TOKEN: &str = "note_block.default";

/// Note block id used when the pack cannot supply one, in the base edition's spelling.
const NOTE_BLOCK_BASE_ID: &str = "minecraft:note_block";

/// How a note block's findings name it.
const NOTE_BLOCK: Fixture = Fixture {
    keyword: "note_block",
    noun: "note block",
};

/// The highest `note` a note block plays, two octaves above its lowest.
const HIGHEST_NOTE: i64 = 24;

/// The `note=` and `instrument=` a note block's line writes, read before anything can refuse it.
#[derive(Debug, Clone, Copy)]
struct WrittenStates<'m> {
    note: Option<i64>,
    instrument: Option<&'m str>,
}

/// Place a `note_block` at its triple, playing the note its line writes.
pub(super) fn fill_note_block(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut unread: Vec<(UnreadArgument, &str)> = Vec::new();
    let note = read_or_ignore(
        member,
        "note",
        |kind| match kind {
            ValueKind::Int(note) => Some(*note),
            _ => None,
        },
        "a whole number from 0 to 24",
    )
    .unwrap_or_else(|argument| {
        unread.push((argument, "the note block is built with `note=0`"));
        None
    });
    let instrument = ident_or_ignore(
        member,
        "instrument",
        "the name of an instrument, such as `harp`, `bass` or `bit`",
    )
    .unwrap_or_else(|argument| {
        unread.push((
            argument,
            "the note block is built with no `instrument`, which the game sets from the block under it",
        ));
        None
    });
    let written = WrittenStates { note, instrument };
    let built = match place_note_block(member, written, y_offset, ctx, palette, canvas, diagnostics)
    {
        Ok(built) => built,
        Err(refusal) => {
            diagnostics.push(*refusal);
            false
        }
    };
    for (argument, consequence) in unread {
        diagnostics.push(argument.report(if built {
            consequence
        } else {
            "this note block is not built either way — see the finding on the same line"
        }));
    }
}

/// [`fill_note_block`], answering whether the note block was built, or the finding that refuses
/// it; an `instrument=` the target refuses is reported here and builds nothing.
fn place_note_block(
    member: &Member,
    written: WrittenStates<'_>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<bool, Refusal> {
    let WrittenStates { note, instrument } = written;
    if let Some(note) = note.filter(|note| !(0..=HIGHEST_NOTE).contains(note)) {
        return Err(Box::new(diag_invalid_member(
            member,
            format!("note_block `note={note}` is no note a note block plays"),
            vec![
                "`note=` takes a whole number from 0 to 24, the semitones above the note block's \
                 lowest note"
                    .to_owned(),
            ],
        )));
    }
    let cell = written_cells(member, Shape::Cell, y_offset, ctx)?
        .cells()
        .next()
        .expect("a cell box holds at least one cell");
    let id = default_block_id(member, ctx.registry, NOTE_BLOCK_TOKEN, NOTE_BLOCK_BASE_ID)?;
    let mut properties = IndexMap::new();
    if let Some(instrument) = instrument {
        properties.insert("instrument".to_owned(), instrument.to_owned());
    }
    if let Some(note) = note {
        properties.insert("note".to_owned(), note.to_string());
    }
    let state = BlockState { id, properties };
    if !check_written_state(member, "instrument", &state, ctx.registry, diagnostics) {
        return Ok(false);
    }
    // Interned only once the cell exists, so a refusal leaves no palette entry.
    if canvas.try_paint(cell, || palette.intern(state)) {
        Ok(true)
    } else {
        Err(Box::new(outside_the_array(NOTE_BLOCK, member)))
    }
}
