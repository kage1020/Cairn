//! Setting a `chest` member's chest block against the wall it stands by (`spec/blockstate`
//! "Derive by default, promote on override").

use indexmap::IndexMap;

use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::BlockState;
use crate::block_array::args::{UnreadArgument, diag_invalid_member, ident_or_ignore};

use super::canvas::{MemberCanvas, ScopePalette};
use super::fixture::{Fixture, Refusal, Toward, fixture_cell, outside_the_array, written_facing};
use super::geometry_material::{Family, GeometryMemberDescription, geometry_material_id};
use super::members::{MemberState, StructCtx, default_block_id, resolve_member_state};
use super::waterlogged::{
    WrittenWaterlogged, keep_waterlogged, read_waterlogged, waterlogged_state,
};

/// Catalog token a `chest`'s default material comes from; the pack spells it per edition.
pub(super) const CHEST_TOKEN: &str = "chest.default";

/// Chest id used when the pack cannot supply one: the Java spelling, Java being the base edition
/// (`spec/versioning-editions` "Backend = data tables").
pub(super) const CHEST_BASE_ID: &str = "minecraft:chest";

/// How a chest's findings name it.
const CHEST: Fixture = Fixture {
    keyword: "chest",
    noun: "chest",
};

/// The `facing=`, `size=` and `waterlogged=` a chest's line writes, read before anything can
/// refuse it.
#[derive(Debug, Clone, Copy)]
struct WrittenStates<'m> {
    facing: Option<&'m str>,
    size: Option<&'m str>,
    waterlogged: Option<WrittenWaterlogged<'m>>,
}

/// Stand a `chest`'s chest block against its wall, facing away from it unless its line turns it.
pub(super) fn fill_chest(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Read before anything can refuse the chest, so every unreadable state is reported now.
    let mut unread: Vec<(UnreadArgument, &str)> = Vec::new();
    let mut read = |key: &'static str, expected: &'static str, consequence: &'static str| {
        ident_or_ignore(member, key, expected).unwrap_or_else(|argument| {
            unread.push((argument, consequence));
            None
        })
    };
    let facing = read(
        "facing",
        Toward::HORIZONTAL_WORDS,
        "the chest faces away from the wall it stands by",
    );
    let size = read(
        "size",
        "`single`",
        "the chest pairs with the chest beside it, as it does without `size=`",
    );
    let waterlogged = read_waterlogged(member).unwrap_or_else(|argument| {
        unread.push((
            argument,
            "the chest holds water where a water `fill` overlaps it, as with `waterlogged=auto`",
        ));
        None
    });
    let written = WrittenStates {
        facing,
        size,
        waterlogged,
    };
    let built = match place_chest(member, written, y_offset, ctx, palette, canvas, diagnostics) {
        Ok(()) => true,
        Err(refusal) => {
            diagnostics.push(*refusal);
            false
        }
    };
    for (argument, consequence) in unread {
        diagnostics.push(argument.report(if built {
            consequence
        } else {
            "this chest is not built either way — see the finding on the same line"
        }));
    }
}

/// [`fill_chest`], answering the finding that refuses the chest.
fn place_chest(
    member: &Member,
    written: WrittenStates<'_>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), Refusal> {
    let toward = written_facing(CHEST, member, written.facing, &Toward::HORIZONTAL);
    let single = match written.size {
        None => Ok(false),
        Some("single") => Ok(true),
        Some(other) => Err(Box::new(diag_invalid_member(
            member,
            format!("unknown chest `size={other}`"),
            vec![
                "`size=` takes `single`, which keeps the chest from pairing; leave it out to let \
                 the chest pair with the one beside it"
                    .to_owned(),
            ],
        ))),
    };
    let waterlogged = waterlogged_state(member, written.waterlogged);
    let (toward, single, waterlogged) = match (toward, single, waterlogged) {
        (Ok(toward), Ok(single), Ok(waterlogged)) => (toward, single, waterlogged),
        (toward, single, waterlogged) => {
            let mut refusals: Vec<Refusal> = [toward.err(), single.err(), waterlogged.err()]
                .into_iter()
                .flatten()
                .collect();
            // The caller reports the one returned, after these.
            let last = refusals.pop();
            diagnostics.extend(refusals.into_iter().map(|refusal| *refusal));
            return last.map_or(Ok(()), Err);
        }
    };
    let (anchor, cell) = fixture_cell(CHEST, member, y_offset, ctx)?;
    let Some(id) = chest_block_id(member, ctx, diagnostics)? else {
        return Ok(());
    };
    let side = anchor.side();
    let facing = toward
        .unwrap_or_else(|| Toward::away_from_the_wall(anchor))
        .face(side);
    let mut properties = IndexMap::new();
    properties.insert("facing".to_owned(), facing.as_str().to_owned());
    // A written `type` keeps the chest out of every pair (`spec/blockstate` "Connections").
    if single {
        properties.insert("type".to_owned(), "single".to_owned());
    }
    keep_waterlogged(&mut properties, waterlogged);
    let state = BlockState { id, properties };
    // Interned only once the cell exists, so a refusal leaves no palette entry.
    if canvas.try_paint(cell, || palette.intern(state)) {
        Ok(())
    } else {
        Err(Box::new(outside_the_array(CHEST, member)))
    }
}

/// The block a chest is built from: its `mat_slot=`, or the pack's default chest; `None` for a
/// binding whose state literal the target refused, which builds no chest.
fn chest_block_id(
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
        || default_block_id(member, ctx.registry, CHEST_TOKEN, CHEST_BASE_ID),
        &GeometryMemberDescription {
            subject: "chest".to_owned(),
            states_from: "the wall it stands by and the chest beside it",
            requires: Some(Family::Chest),
        },
        diagnostics,
    )
    .map(Some)
}
