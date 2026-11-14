//! Hanging a `trapdoor` by a wall, or setting it at a coordinate triple, with the `facing`, `half`
//! and `open` its line writes (`spec/blockstate` "Derive by default, promote on override").

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;
use crate::suggest::{did_you_mean, nearest_match};

use crate::block_array::BlockState;
use crate::block_array::args::{
    UnreadArgument, diag_invalid_member, ident_or_ignore, read_or_ignore,
};
use crate::block_array::geometry::Declared;
use crate::block_array::joins::Face;

use super::canvas::{MemberCanvas, ScopePalette};
use super::fixture::{Fixture, Refusal, Toward, fixture_cell, outside_the_array, written_facing};
use super::geometry_material::{Family, GeometryMemberDescription, geometry_material_id};
use super::members::{MemberState, StructCtx, default_block_id, resolve_member_state};
use super::part::{COMPASS, COMPASS_FACES, Stand, stand_of, triple_cell, word};
use super::waterlogged::{
    WrittenWaterlogged, keep_waterlogged, read_waterlogged, waterlogged_state,
};

/// Catalog token a `trapdoor`'s default material comes from; the pack spells it per edition.
const TRAPDOOR_TOKEN: &str = "trapdoor.default";

/// Trapdoor id used when the pack cannot supply one, in the base edition's spelling.
const TRAPDOOR_BASE_ID: &str = "minecraft:oak_trapdoor";

/// How a trapdoor's findings name it.
const TRAPDOOR: Fixture = Fixture {
    keyword: "trapdoor",
    noun: "trapdoor",
};

/// The states a trapdoor's line writes, read before anything can refuse it.
#[derive(Debug, Clone, Copy)]
struct WrittenStates<'m> {
    facing: Option<&'m str>,
    half: Option<&'m str>,
    open: Option<bool>,
    waterlogged: Option<WrittenWaterlogged<'m>>,
}

/// Set a `trapdoor` where its `at=` names, closed in the bottom half of its cell unless its line
/// says otherwise.
pub(super) fn fill_trapdoor(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Read before anything can refuse the trapdoor, so every unreadable state is reported now.
    let mut unread: Vec<UnreadArgument> = Vec::new();
    let facing = ident_or_ignore(
        member,
        "facing",
        "a way from the wall, `in`, `out`, `left` or `right`, by a wall and `north`, `south`, \
         `east` or `west` at a triple",
    )
    .unwrap_or_else(|argument| {
        unread.push(argument);
        None
    });
    let half = ident_or_ignore(member, "half", "`top` or `bottom`").unwrap_or_else(|argument| {
        unread.push(argument);
        None
    });
    let open = read_or_ignore(
        member,
        "open",
        |kind| match kind {
            ValueKind::Bool(open) => Some(*open),
            _ => None,
        },
        "`true` or `false`",
    )
    .unwrap_or_else(|argument| {
        unread.push(argument);
        None
    });
    let waterlogged = read_waterlogged(member).unwrap_or_else(|argument| {
        unread.push(argument);
        None
    });
    let written = WrittenStates {
        facing,
        half,
        open,
        waterlogged,
    };
    let stand = match place_trapdoor(member, written, y_offset, ctx, palette, canvas, diagnostics) {
        Ok(stand) => Some(stand),
        Err(refusal) => {
            diagnostics.push(*refusal);
            None
        }
    };
    for argument in unread {
        let consequence = match (stand, argument.key) {
            (None, _) => "this trapdoor is not built either way — see the finding on the same line",
            (Some(Stand::ByWall), "facing") => {
                "the trapdoor faces away from the wall it hangs by, hinged on that wall"
            }
            (Some(Stand::Triple), "facing") => "the trapdoor takes the block's own `facing`",
            (_, "half") => "the trapdoor is built in the bottom half of its cell, as `half=bottom`",
            (_, "open") => "the trapdoor is built closed, as `open=false`",
            _ => {
                "the trapdoor holds water where a water `fill` overlaps it, as with \
                 `waterlogged=auto`"
            }
        };
        diagnostics.push(argument.report(consequence));
    }
}

/// [`fill_trapdoor`], answering how its `at=` named its cell, or the finding that refuses it.
fn place_trapdoor(
    member: &Member,
    written: WrittenStates<'_>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Stand, Refusal> {
    let stand = stand_of(TRAPDOOR, TRAPDOOR.noun, member)?;
    let toward = match stand {
        Stand::ByWall => {
            written_facing(TRAPDOOR, member, written.facing, &Toward::HORIZONTAL).map(Look::Wall)
        }
        Stand::Triple => compass_facing(member, written.facing).map(Look::Compass),
    };
    let half = word(member, "half", written.half, &["top", "bottom"]);
    let waterlogged = waterlogged_state(member, written.waterlogged);
    let (toward, half, waterlogged) = match (toward, half, waterlogged) {
        (Ok(toward), Ok(half), Ok(waterlogged)) => (toward, half, waterlogged),
        (toward, half, waterlogged) => {
            let mut refusals: Vec<Refusal> = [toward.err(), half.err(), waterlogged.err()]
                .into_iter()
                .flatten()
                .collect();
            // The caller reports the one returned, after these.
            let last = refusals.pop();
            diagnostics.extend(refusals.into_iter().map(|refusal| *refusal));
            return last.map_or(Ok(stand), Err);
        }
    };
    let (cell, facing, host_face) = match toward {
        Look::Wall(toward) => {
            let (anchor, cell) = fixture_cell(TRAPDOOR, member, y_offset, ctx)?;
            let way = toward.unwrap_or_else(|| Toward::away_from_the_wall(anchor));
            (
                cell,
                Some(way.face(anchor.side())),
                Some(anchor.host_face()),
            )
        }
        Look::Compass(facing) => (
            triple_cell(TRAPDOOR, TRAPDOOR.noun, member, y_offset, ctx)?,
            facing,
            None,
        ),
    };
    let Some(id) = trapdoor_block_id(member, ctx, diagnostics)? else {
        return Ok(stand);
    };
    let mut properties = IndexMap::new();
    if let Some(facing) = facing {
        properties.insert("facing".to_owned(), facing.as_str().to_owned());
    }
    properties.insert("half".to_owned(), half.unwrap_or("bottom").to_owned());
    properties.insert("open".to_owned(), written.open.unwrap_or(false).to_string());
    keep_waterlogged(&mut properties, waterlogged);
    let state = BlockState { id, properties };
    canvas.declare(Declared::cell(cell, host_face));
    // Interned only once the cell exists, so a refusal leaves no palette entry.
    if canvas.try_paint(cell, || palette.intern(state)) {
        Ok(stand)
    } else {
        Err(Box::new(outside_the_array(TRAPDOOR, member)))
    }
}

/// Which way a trapdoor's `facing=` turns it, by the form of its `at=`.
#[derive(Debug, Clone, Copy)]
enum Look {
    /// A way from the wall it hangs by, `None` to face away from it.
    Wall(Option<Toward>),
    /// A compass face at a triple, `None` for the block's own.
    Compass(Option<Face>),
}

/// The compass face a trapdoor at a triple names, `None` when its line writes none, or the
/// refusal of any other word.
fn compass_facing(member: &Member, written: Option<&str>) -> Result<Option<Face>, Refusal> {
    let Some(word) = written else {
        return Ok(None);
    };
    if let Some(face) = Face::from_name(word).filter(|face| COMPASS_FACES.contains(face)) {
        return Ok(Some(face));
    }
    let mut notes = vec![COMPASS.to_owned()];
    if Toward::HORIZONTAL
        .iter()
        .any(|toward| toward.name() == word)
    {
        notes.push(format!(
            "`{word}` names a way from a wall, and a trapdoor at a coordinate triple hangs by \
             none; set it by a wall with `at=<side>.outside` or `at=inside.<side>` to turn it \
             that way"
        ));
    } else {
        let names = COMPASS_FACES.map(Face::as_str);
        notes
            .extend(nearest_match(word, names).map(|near| did_you_mean(&format!("facing={near}"))));
    }
    Err(Box::new(diag_invalid_member(
        member,
        format!("unknown trapdoor `facing={word}` at a coordinate triple"),
        notes,
    )))
}

/// The block a trapdoor is built from: its `mat_slot=`, or the pack's default trapdoor; `None`
/// for a binding whose state literal the target refused, which builds no trapdoor.
fn trapdoor_block_id(
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
        || default_block_id(member, ctx.registry, TRAPDOOR_TOKEN, TRAPDOOR_BASE_ID),
        &GeometryMemberDescription {
            subject: "trapdoor".to_owned(),
            states_from: "the wall it hangs by and its own line",
            requires: Some(Family::Trapdoor),
        },
        diagnostics,
    )
    .map(Some)
}
