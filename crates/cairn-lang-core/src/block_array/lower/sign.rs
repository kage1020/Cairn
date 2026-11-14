//! Setting a `sign` member's sign on the wall it hangs on or the block it stands on, with the text
//! its line writes (`spec/entities` "Two tiers").

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;
use crate::suggest::{did_you_mean, nearest_match};

use crate::block_array::args::{
    UnreadArgument, diag_invalid_member, ident_or_ignore, read_or_ignore, strings_or_ignore,
};
use crate::block_array::joins::Face;
use crate::block_array::material::is_stand_in;
use crate::block_array::{BlockEntityData, BlockState, SignText};

use super::canvas::{MemberCanvas, ScopePalette};
use super::fixture::{Fixture, Refusal, Toward, fixture_cell, outside_the_array};
use super::geometry_material::{
    Family, GeometryMemberDescription, geometry_material_id, wall_sign_of,
};
use super::members::{MemberState, StructCtx, default_block_id, resolve_member_state};
use super::part::{COMPASS, COMPASS_FACES, Stand, stand_of, triple_cell};

/// Catalog token of the standing sign a `sign` is built from when no `mat_slot=` binds one.
const STANDING_TOKEN: &str = "sign.oak";

/// Standing sign id used when the pack cannot supply one, in the base edition's spelling.
const STANDING_BASE_ID: &str = "minecraft:oak_sign";

/// Catalog token of the wall sign a `sign` hangs when no `mat_slot=` binds one.
const WALL_TOKEN: &str = "sign.oak_wall";

/// Wall sign id used when the pack cannot supply one, in the base edition's spelling.
const WALL_BASE_ID: &str = "minecraft:oak_wall_sign";

/// How a sign's findings name it.
const SIGN: Fixture = Fixture {
    keyword: "sign",
    noun: "sign",
};

/// The most lines a face of a sign holds.
const FACE_LINES: usize = 4;

/// What `text=` and `back=` take, completing "`key=` must be …".
const LINES: &str = "a string, or a list of up to four strings";

/// The `facing=`, text and `glowing=` a sign's line writes, read before anything can refuse it.
#[derive(Debug, Default)]
struct Written<'m> {
    facing: Option<&'m str>,
    front: Option<Vec<String>>,
    back: Option<Vec<String>>,
    glowing: Option<bool>,
}

/// Stand a `sign` on the block under it, or hang it on a wall, holding the text its line writes.
pub(super) fn fill_sign(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut unread: Vec<UnreadArgument> = Vec::new();
    let written = read_written(member, &mut unread);
    let stand = match place_sign(
        member,
        &written,
        y_offset,
        ctx,
        palette,
        canvas,
        diagnostics,
    ) {
        Ok(stand) => Some(stand),
        Err(refusal) => {
            diagnostics.push(*refusal);
            None
        }
    };
    for argument in unread {
        let consequence = match (argument.key, stand) {
            (_, None) => "this sign is not built either way — see the finding on the same line",
            ("facing", Some(Stand::Triple)) => {
                "the sign stands on the block under it, its front toward the `front` wall"
            }
            ("facing", Some(Stand::ByWall)) => {
                "the sign hangs on the wall it stands by, facing away from it"
            }
            ("text", _) => "the sign's front is left blank",
            ("back", _) => "the sign's back is left blank",
            _ => "the sign's text does not glow",
        };
        diagnostics.push(argument.report(consequence));
    }
}

/// Every key a sign reads off its line; a value of the wrong kind joins `unread` instead.
fn read_written<'m>(member: &'m Member, unread: &mut Vec<UnreadArgument>) -> Written<'m> {
    Written {
        facing: keep(
            unread,
            ident_or_ignore(
                member,
                "facing",
                "a compass face, `north`, `south`, `east` or `west`, at a triple (by a wall the sign \
             takes none)",
            ),
        ),
        front: keep(unread, strings_or_ignore(member, "text", LINES)),
        back: keep(unread, strings_or_ignore(member, "back", LINES)),
        glowing: keep(
            unread,
            read_or_ignore(
                member,
                "glowing",
                |kind| match kind {
                    ValueKind::Bool(glowing) => Some(*glowing),
                    _ => None,
                },
                "`true` or `false`",
            ),
        ),
    }
}

/// The value `read` holds, or `None` once its refusal joins `unread`.
fn keep<T>(unread: &mut Vec<UnreadArgument>, read: Result<Option<T>, UnreadArgument>) -> Option<T> {
    read.unwrap_or_else(|argument| {
        unread.push(argument);
        None
    })
}

/// [`fill_sign`], answering how the sign's `at=` named its cell, or the finding that refuses it.
fn place_sign(
    member: &Member,
    written: &Written<'_>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Stand, Refusal> {
    for (key, lines) in [("text", &written.front), ("back", &written.back)] {
        if let Some(lines) = lines.as_ref().filter(|lines| lines.len() > FACE_LINES) {
            return Err(Box::new(diag_invalid_member(
                member,
                format!(
                    "sign `{key}=` writes {} lines, more than a face of a sign holds",
                    lines.len()
                ),
                vec![format!(
                    "a face holds at most four lines; write `{key}=` as a list of up to four \
                     strings"
                )],
            )));
        }
    }
    let stand = stand_of(SIGN, "sign", member)?;
    let hung = hung_facing(member, written.facing, stand)?;
    let (cell, facing) = match stand {
        Stand::Triple => (triple_cell(SIGN, "sign", member, y_offset, ctx)?, hung),
        Stand::ByWall => {
            let (anchor, cell) = fixture_cell(SIGN, member, y_offset, ctx)?;
            let away = Toward::away_from_the_wall(anchor).face(anchor.side());
            (cell, Some(away))
        }
    };
    let Some(id) = sign_block_id(member, ctx, facing.is_some(), diagnostics)? else {
        return Ok(stand);
    };
    let mut properties = IndexMap::new();
    match facing {
        Some(face) => properties.insert("facing".to_owned(), face.as_str().to_owned()),
        // Written, since the block's own default turns between versions.
        None => properties.insert("rotation".to_owned(), "0".to_owned()),
    };
    let state = BlockState { id, properties };
    // Interned only once the cell exists, so a refusal leaves no palette entry.
    if !canvas.try_paint(cell, || palette.intern(state)) {
        return Err(Box::new(outside_the_array(SIGN, member)));
    }
    let face = |lines: &Option<Vec<String>>| SignText {
        lines: lines.clone().unwrap_or_default(),
        glowing: written.glowing.unwrap_or(false),
    };
    canvas.set_block_entity(
        cell,
        BlockEntityData::Sign {
            front: face(&written.front),
            back: face(&written.back),
        },
    );
    Ok(stand)
}

/// The way a sign at a triple hangs on the block behind it, `None` for one that stands on the
/// block under it, or the refusal of a `facing=` its anchor does not take there.
fn hung_facing(
    member: &Member,
    facing: Option<&str>,
    stand: Stand,
) -> Result<Option<Face>, Refusal> {
    let Some(word) = facing else {
        return Ok(None);
    };
    if stand == Stand::ByWall {
        return Err(Box::new(diag_invalid_member(
            member,
            format!(
                "sign `facing={word}` by a wall: the sign hangs on the wall's face, so `facing=` \
                 has nothing to choose"
            ),
            vec!["drop `facing=`; the sign faces away from the wall it hangs on".to_owned()],
        )));
    }
    if let Some(face) = Face::from_name(word).filter(|face| COMPASS_FACES.contains(face)) {
        return Ok(Some(face));
    }
    let mut notes = vec![COMPASS.to_owned()];
    if Toward::HORIZONTAL
        .iter()
        .any(|toward| toward.name() == word)
    {
        notes.push(format!(
            "`{word}` names a way from a wall, which a sign does not take: by a wall it hangs on \
             the wall and faces away from it, with no `facing=`"
        ));
    } else {
        notes.extend(
            nearest_match(word, COMPASS_FACES.map(Face::as_str))
                .map(|near| did_you_mean(&format!("facing={near}"))),
        );
    }
    Err(Box::new(diag_invalid_member(
        member,
        format!("unknown sign `facing={word}` at a coordinate triple"),
        notes,
    )))
}

/// The block a sign is built from: the standing sign its `mat_slot=` binds or the pack's default,
/// as the wall sign of that standing sign where it hangs; `None` for a binding whose state literal
/// the target refused, which builds no sign.
fn sign_block_id(
    member: &Member,
    ctx: &StructCtx<'_>,
    hangs: bool,
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
    let mut fell_back = false;
    let standing = geometry_material_id(
        member,
        ctx.scope,
        ctx.registry,
        resolved.as_ref(),
        || {
            fell_back = true;
            default_block_id(member, ctx.registry, STANDING_TOKEN, STANDING_BASE_ID)
        },
        &GeometryMemberDescription {
            subject: "sign".to_owned(),
            states_from: "the wall it hangs on or the block it stands on",
            requires: Some(Family::Sign),
        },
        diagnostics,
    )?;
    if !hangs || is_stand_in(&BlockState::bare(&standing)) {
        return Ok(Some(standing));
    }
    if fell_back {
        return default_block_id(member, ctx.registry, WALL_TOKEN, WALL_BASE_ID).map(Some);
    }
    Ok(Some(wall_sign_of(&standing)))
}
