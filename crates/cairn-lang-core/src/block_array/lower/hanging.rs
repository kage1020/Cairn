//! Hanging a `painting` or an `item_frame` on the face of the block behind it
//! (`spec/entities` "Hanging entities").

use std::collections::HashMap;
use std::num::NonZeroU32;

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::intent::Member;
use crate::suggest::{did_you_mean, nearest_match};

use crate::block_array::args::{UnreadArgument, diag_invalid_member, read_or_ignore};
use crate::block_array::material::PaintingLookup;
use crate::block_array::{BlockState, Compass, Entity, EntityData, PaletteIndex};

use super::canvas::{Canvas, MemberCanvas};
use super::constraints::{Grid, note, written};
use super::entity::{
    EVERY_FACE, Hold, declare_box, hanging_point, item_of, nbt_of, place, place_one, report_unread,
};
use super::fixture::{Fixture, Refusal};
use super::members::StructCtx;
use super::stand::COMPASS_FACES;

/// How a painting's findings name it.
const PAINTING: Fixture = Fixture {
    keyword: "painting",
    noun: "painting",
};

/// How an item frame's findings name it.
const ITEM_FRAME: Fixture = Fixture {
    keyword: "item_frame",
    noun: "item frame",
};

/// What becomes of an unread argument of an entity that is not built.
const NOT_BUILT: &str = "this entity is not built either way — see the finding on the same line";

/// Hang a `painting` of its `variant=` on the face its line names.
pub(super) fn fill_painting(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut unread = Vec::new();
    let built = match hang_painting(member, y_offset, ctx, canvas, &mut unread) {
        Ok(()) => true,
        Err(refusal) => {
            diagnostics.push(*refusal);
            false
        }
    };
    report_unread(unread, diagnostics, |_| {
        if built {
            "the painting is hung without it"
        } else {
            NOT_BUILT
        }
    });
}

/// [`fill_painting`], or the finding that refuses the painting.
fn hang_painting(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
    unread: &mut Vec<UnreadArgument>,
) -> Result<(), Refusal> {
    let name = variant_of(member)?;
    let placed = place(
        PAINTING,
        Hold::Hangs {
            faces: &COMPASS_FACES,
        },
        member,
        y_offset,
        ctx,
        unread,
    )?;
    let (spelling, width, height) = match ctx
        .registry
        .map_or(PaintingLookup::Unrecorded, |registry| {
            registry.painting(name)
        }) {
        PaintingLookup::Known(variant) => (Some(variant.spelling), variant.width, variant.height),
        PaintingLookup::Unknown {
            registry,
            known,
            elsewhere,
        } => {
            return Err(Box::new(unknown_variant(
                member, name, &registry, &known, &elsewhere,
            )));
        }
        PaintingLookup::Unrecorded => (None, NonZeroU32::MIN, NonZeroU32::MIN),
    };
    let facing = placed.compass();
    let right = facing.counter_clockwise().face();
    let at = |across: u32, up: u32| {
        let (dx, _, dz) = right.step();
        let (x, y, z) = placed.cell;
        let x = i64::from(x) + i64::from(dx) * i64::from(across);
        let z = i64::from(z) + i64::from(dz) * i64::from(across);
        Some((
            u32::try_from(x).ok()?,
            y.checked_add(up)?,
            u32::try_from(z).ok()?,
        ))
    };
    let outside = || Box::new(super::fixture::outside_the_array(PAINTING, member));
    let far = at(width.get() - 1, height.get() - 1).ok_or_else(outside)?;
    declare_box(
        PAINTING,
        member,
        ctx,
        canvas,
        placed.cell,
        far,
        placed.host_face,
    )?;
    let hung = at((width.get() - 1) / 2, (height.get() - 1) / 2).ok_or_else(outside)?;
    canvas.place_entity(
        member.span.start,
        Entity {
            id: "minecraft:painting".to_owned(),
            pos: painting_point(hung, facing, width, height),
            block_pos: hung,
            data: EntityData::Painting {
                facing,
                variant: name.to_owned(),
                spelling,
                width,
                height,
            },
            nbt: nbt_of(member, unread),
        },
    );
    Ok(())
}

/// The middle of the face a `width` by `height` painting facing `facing` covers, kept by `hung`:
/// `hung`'s point, moved half a block right for an even width and up for an even height.
fn painting_point(
    hung: (u32, u32, u32),
    facing: Compass,
    width: NonZeroU32,
    height: NonZeroU32,
) -> (f64, f64, f64) {
    let half = |side: NonZeroU32| {
        if side.get().is_multiple_of(2) {
            0.5
        } else {
            0.0
        }
    };
    let (x, y, z) = hanging_point(hung, facing.face());
    let (dx, _, dz) = facing.counter_clockwise().face().step();
    (
        x + f64::from(dx) * half(width),
        y + half(height),
        z + f64::from(dz) * half(width),
    )
}

/// The name a painting's `variant=` writes, or the refusal of a line that writes none.
fn variant_of(member: &Member) -> Result<&str, Refusal> {
    match member.written("variant").map(|raw| &raw.value.kind) {
        Some(ValueKind::Ident(name)) => Ok(name.as_str()),
        written => Err(Box::new(diag_invalid_member(
            member,
            match written {
                None => "painting has no `variant=`, so nothing says which painting hangs".into(),
                Some(_) => format!(
                    "painting `variant=` must be the name of a painting, not {}",
                    member
                        .written("variant")
                        .map(|raw| raw.value.describe())
                        .unwrap_or_default(),
                ),
            },
            vec!["write `variant=` with a painting the target has, such as `variant=kebab`".into()],
        ))),
    }
}

/// The `E_INVALID_MEMBER` for a variant `registry` has no painting of.
fn unknown_variant(
    member: &Member,
    name: &str,
    registry: &str,
    known: &[String],
    elsewhere: &[String],
) -> Diagnostic {
    let mut notes = Vec::new();
    if elsewhere.is_empty() {
        notes.extend(
            nearest_match(name, known.iter().map(String::as_str))
                .map(|near| did_you_mean(&format!("variant={near}"))),
        );
        notes.push(format!(
            "`{registry}` has the paintings {}",
            known.join(", ")
        ));
    } else {
        notes.push(format!(
            "`{name}` is a painting of {}: pass a `--target` that has it, or pick another",
            elsewhere.join(", ")
        ));
    }
    diag_invalid_member(
        member,
        format!("painting `variant={name}` is no painting `{registry}` has"),
        notes,
    )
}

/// Hang an `item_frame` on the face its line names, holding its `item=`.
pub(super) fn fill_item_frame(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut unread = Vec::new();
    let placed = place(
        ITEM_FRAME,
        Hold::Hangs { faces: &EVERY_FACE },
        member,
        y_offset,
        ctx,
        &mut unread,
    );
    let item = item_of(member, "item", &mut unread);
    let glowing = read_or_ignore(
        member,
        "glowing",
        |kind| match kind {
            ValueKind::Bool(glowing) => Some(*glowing),
            _ => None,
        },
        "`true` or `false`",
    )
    .unwrap_or_else(|argument| {
        unread.push(argument);
        None
    });
    let built = placed.is_ok();
    match placed {
        Ok(placed) => place_one(
            member,
            canvas,
            placed,
            if glowing == Some(true) {
                "minecraft:glow_item_frame"
            } else {
                "minecraft:item_frame"
            },
            hanging_point(placed.cell, placed.facing),
            EntityData::ItemFrame {
                facing: placed.facing,
                item,
            },
            &mut unread,
        ),
        Err(refusal) => diagnostics.push(*refusal),
    }
    report_unread(unread, diagnostics, |key| match (built, key) {
        (false, _) => NOT_BUILT,
        (true, "item") => "the frame is hung empty",
        (true, "glowing") => "the frame does not glow",
        (true, _) => "the frame is hung without it",
    });
}

/// Refuse, as a Bedrock build does, each item frame of `placed` whose cell of `grid` holds a block
/// or a frame earlier in `placed`, with an `E_FRAME_CELL_TAKEN`.
pub(super) fn refuse_taken_frame_cells(
    flattened: &[(u32, &Member)],
    canvas: &Canvas,
    grid: &Grid<'_>,
    placed: &mut Vec<(u32, Entity)>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let array = grid.array;
    let mut hung: HashMap<(u32, u32, u32), (&Member, String)> = HashMap::new();
    placed.retain(|(index, entity)| {
        if !matches!(entity.data, EntityData::ItemFrame { .. }) {
            return true;
        }
        let (_, member) = flattened[*index as usize];
        let cell = entity.block_pos;
        let held = array
            .dims
            .index(cell.0, cell.1, cell.2)
            .map_or(PaletteIndex::AIR, |i| array.voxels[i]);
        let taker = if held != PaletteIndex::AIR {
            Taker::Block(
                &array.palette.entries[usize::from(held.0)],
                canvas
                    .owner_at(cell)
                    .map(|owner| flattened[owner as usize].1),
            )
        } else if let Some((first, id)) = hung.get(&cell) {
            Taker::Frame(first, id)
        } else {
            hung.insert(cell, (member, entity.id.clone()));
            return true;
        };
        diagnostics.push(diag_frame_cell_taken(member, entity, grid, &taker));
        false
    });
}

/// What holds the cell a frame hangs in.
enum Taker<'a> {
    /// A block, with the member that built it.
    Block(&'a BlockState, Option<&'a Member>),
    /// A frame an earlier line hangs there, with its entity id.
    Frame(&'a Member, &'a str),
}

fn diag_frame_cell_taken(
    member: &Member,
    entity: &Entity,
    grid: &Grid<'_>,
    taker: &Taker<'_>,
) -> Diagnostic {
    let cell = grid.named(entity.block_pos);
    let (holds, by, fix) = match *taker {
        Taker::Block(state, by) => (
            format!("a cell that holds `{}`", written(state)),
            by.map(|by| {
                (
                    by,
                    format!(
                        "`{}` builds `{}` at {cell}",
                        by.role.keyword(),
                        written(state)
                    ),
                )
            }),
            "move the frame, or the block, to a cell of its own",
        ),
        Taker::Frame(first, id) => (
            "a cell another frame hangs in".to_owned(),
            Some((
                first,
                format!("`{}` hangs `{id}` at {cell} first", first.role.keyword()),
            )),
            "move one of the two frames to a cell of its own",
        ),
    };
    let there = by.map(|(by, message)| DiagnosticNote {
        span: Some(by.span.clone()),
        message,
    });
    Diagnostic {
        code: DiagnosticCode::FrameCellTaken,
        span: member.span.clone(),
        primary: format!(
            "`{}` hangs `{}` at {cell}, {holds}, and Bedrock builds an item frame as a block of \
             its cell",
            member.role.keyword(),
            entity.id,
        ),
        notes: there
            .into_iter()
            .chain([
                note("an item frame hangs only in a cell that holds air and no other frame"),
                note(fix),
            ])
            .collect(),
        data: None,
    }
}
