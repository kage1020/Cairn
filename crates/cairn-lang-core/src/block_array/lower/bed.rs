//! Laying a `bed` member's two blocks, its head in the cell its anchor names and its foot behind
//! it (`spec/blockstate` "Derive by default, promote on override").

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::{Member, MemberRole};
use crate::suggest::{did_you_mean, nearest_match};

use crate::block_array::args::{UnreadArgument, diag_invalid_member, ident_or_ignore};
use crate::block_array::geometry::Declared;
use crate::block_array::joins::Face;
use crate::block_array::{Bbox, BlockEntityData, BlockState, DyeColor, TargetRegistry};

use super::canvas::{Canvas, MemberCanvas, ScopePalette};
use super::cell_box::Cell;
use super::fixture::{
    Anchor, Fixture, Refusal, Toward, fixture_cell, outside_the_array, written_facing,
};
use super::geometry_material::{Family, GeometryMemberDescription, geometry_material_id, path_of};
use super::members::{MemberState, StructCtx, default_block_id, resolve_member_state};
use super::stand::{COMPASS_FACES, Stand, stand_of, triple_cell};

/// Catalog token of the bed a `bed` is built from when no `mat_slot=` binds one.
const BED_TOKEN: &str = "bed.default";

/// Bed id used when the pack cannot supply one, in the base edition's spelling.
const BED_BASE_ID: &str = "minecraft:red_bed";

/// How a bed's findings name it.
const BED: Fixture = Fixture {
    keyword: "bed",
    noun: "bed",
};

/// The note on a compass `facing=`, saying which way each word turns a bed.
const BED_COMPASS: &str = "`facing=` takes one of north, south, east, west, the way the bed's \
                           head points: north toward the `back` wall and east toward the `right` \
                           one";

/// Lay a `bed`, its head toward the wall it stands by unless its line turns it.
pub(super) fn fill_bed(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut unread: Option<UnreadArgument> = None;
    let facing = ident_or_ignore(
        member,
        "facing",
        "the way the bed's head points, `in`, `out`, `left` or `right` by a wall and `north`, \
         `south`, `east` or `west` at a triple",
    )
    .unwrap_or_else(|argument| {
        unread = Some(argument);
        None
    });
    let written = Facing {
        word: facing,
        unreadable: unread.is_some(),
    };
    let stand = place_bed(member, written, y_offset, ctx, palette, canvas, diagnostics)
        .unwrap_or_else(|refusal| {
            diagnostics.push(*refusal);
            None
        });
    if let Some(argument) = unread {
        diagnostics.push(argument.report(match stand {
            Some(Stand::ByWall) => "the bed lies with its head to the wall it stands by",
            _ => "this bed is not built either way — see the finding on the same line",
        }));
    }
}

/// The `facing=` a bed's line writes.
#[derive(Debug, Clone, Copy)]
struct Facing<'m> {
    /// The word it names, `None` where the line writes none the pass can read.
    word: Option<&'m str>,
    /// Whether the line writes one of a kind other than a word.
    unreadable: bool,
}

/// [`fill_bed`], answering how the bed's `at=` named its head's cell, `None` for a bed its
/// binding's refused state literal leaves unbuilt, or the finding that refuses it.
fn place_bed(
    member: &Member,
    written: Facing<'_>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<Stand>, Refusal> {
    let facing = written.word;
    let stand = stand_of(BED, "bed", member)?;
    let (head, face, lies_in, host_face) = match stand {
        Stand::Triple => {
            let face = compass_facing(member, written)?;
            (
                triple_cell(BED, "bed", member, y_offset, ctx)?,
                face,
                Region::Footprint,
                None,
            )
        }
        Stand::ByWall => {
            let (anchor, head) = fixture_cell(BED, member, y_offset, ctx)?;
            let face = facing_by_the_wall(member, facing, anchor)?;
            let lies_in = match anchor {
                Anchor::Inside(_) => Region::Interior,
                Anchor::Outside(_) => Region::Exterior,
            };
            (head, face, lies_in, Some(anchor.host_face()))
        }
    };
    let (dx, _, dz) = face.step();
    let foot = head
        .0
        .checked_add_signed(-dx)
        .zip(head.2.checked_add_signed(-dz))
        .map(|(x, z)| (x, head.1, z))
        .filter(|&foot| lies_in.holds(foot, ctx));
    let Some(foot) = foot else {
        let turned = facing.map_or_else(String::new, |word| format!(" `facing={word}`"));
        return Err(Box::new(diag_invalid_member(
            member,
            format!(
                "bed{turned} puts its foot, the cell behind its head, {}",
                lies_in.outside(),
            ),
            vec![
                "turn the bed with `facing=`, or move it, so the cell behind its head is free"
                    .to_owned(),
            ],
        )));
    };
    let Some(BedBlock { id, entity }) = bed_block(member, ctx, diagnostics)? else {
        return Ok(None);
    };
    if [head, foot]
        .iter()
        .any(|&(x, y, z)| ctx.dims.index(x, y, z).is_none())
    {
        return Err(Box::new(outside_the_array(BED, member)));
    }
    canvas.declare(Declared {
        anchor: head,
        bbox: Bbox::spanning(head, foot),
        host_face,
    });
    for (cell, part) in [(head, "head"), (foot, "foot")] {
        let mut properties = IndexMap::new();
        properties.insert("facing".to_owned(), face.as_str().to_owned());
        properties.insert("part".to_owned(), part.to_owned());
        let state = BlockState {
            id: id.clone(),
            properties,
        };
        canvas.paint_with_entity(cell, || palette.intern(state), entity.clone());
    }
    Ok(Some(stand))
}

/// The block a bed is built from, with what each half's block entity holds.
struct BedBlock {
    id: String,
    /// The colour each half keeps beside its block, which a writer saves where its edition keeps
    /// the colour there.
    entity: BlockEntityData,
}

/// Where a bed's foot may lie, as its head's anchor places the head.
#[derive(Debug, Clone, Copy)]
enum Region {
    /// The struct's footprint, for a head at a triple.
    Footprint,
    /// Strictly inside the wall ring, for a head inside a wall.
    Interior,
    /// The block array outside the footprint, for a head outside a wall.
    Exterior,
}

impl Region {
    /// Whether `cell` lies in this region of the struct.
    fn holds(self, (x, y, z): Cell, ctx: &StructCtx<'_>) -> bool {
        let within = |at: u32, inset: u32, width: u32| {
            at >= ctx.overhang + inset && at + inset < ctx.overhang + width
        };
        let footprint = within(x, 0, ctx.interior_w) && within(z, 0, ctx.interior_h);
        match self {
            Self::Footprint => footprint,
            Self::Interior => within(x, 1, ctx.interior_w) && within(z, 1, ctx.interior_h),
            Self::Exterior => !footprint && ctx.dims.index(x, y, z).is_some(),
        }
    }

    /// Where a cell outside this region lies, completing "puts its foot …".
    fn outside(self) -> &'static str {
        match self {
            Self::Footprint => "outside the struct",
            Self::Interior => "in the struct's walls",
            Self::Exterior => "in the wall or past the struct's overhang",
        }
    }
}

/// The way a bed at a triple points its head, or the refusal of a `facing=` that is missing or no
/// compass word.
fn compass_facing(member: &Member, facing: Facing<'_>) -> Result<Face, Refusal> {
    let Some(word) = facing.word else {
        let primary = if facing.unreadable {
            "bed at a coordinate triple has a `facing=` that is no compass word, so nothing says \
             which way it lies"
        } else {
            "bed at a coordinate triple has no `facing=`, so nothing says which way it lies"
        };
        return Err(Box::new(diag_invalid_member(
            member,
            primary.to_owned(),
            vec![
                BED_COMPASS.to_owned(),
                "no wall stands by a bed at a triple for its head to lie against".to_owned(),
            ],
        )));
    };
    if let Some(face) = Face::from_name(word).filter(|face| COMPASS_FACES.contains(face)) {
        return Ok(face);
    }
    let mut notes = vec![BED_COMPASS.to_owned()];
    if Toward::HORIZONTAL
        .iter()
        .any(|toward| toward.name() == word)
    {
        notes.push(format!(
            "`{word}` names a way from a wall, and a bed at a coordinate triple stands by none; \
             anchor it by a wall with `at=<side>.outside` or `at=inside.<side>` to turn it that \
             way"
        ));
    } else {
        notes.extend(
            nearest_match(word, COMPASS_FACES.map(Face::as_str))
                .map(|near| did_you_mean(&format!("facing={near}"))),
        );
    }
    Err(Box::new(diag_invalid_member(
        member,
        format!("unknown bed `facing={word}` at a coordinate triple"),
        notes,
    )))
}

/// The way a bed by the wall of `anchor` points its head: toward the wall, or the way its line
/// turns it along the wall; the refusal of a `facing=` away from it, which puts its foot there.
fn facing_by_the_wall(
    member: &Member,
    facing: Option<&str>,
    anchor: Anchor,
) -> Result<Face, Refusal> {
    let away = Toward::away_from_the_wall(anchor);
    let to_the_wall = match away {
        Toward::In => Toward::Out,
        _ => Toward::In,
    };
    let toward = written_facing(BED, member, facing, &Toward::HORIZONTAL)?;
    if toward == Some(away) {
        return Err(Box::new(diag_invalid_member(
            member,
            format!(
                "bed `facing={}` points its head away from the wall it stands by, which puts its \
                 foot in that wall",
                away.name()
            ),
            vec![format!(
                "a bed by a wall lies with its head to the wall, `facing={}`, which it takes when \
                 its line writes none, or along it, `facing=left` or `facing=right`",
                to_the_wall.name()
            )],
        )));
    }
    Ok(toward.unwrap_or(to_the_wall).face(anchor.side()))
}

/// The bed block a bed is built from, its `mat_slot=` or the pack's default; `None` for a binding
/// whose state literal the target refused, which builds no bed.
fn bed_block(
    member: &Member,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<Option<BedBlock>, Refusal> {
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
    let id = geometry_material_id(
        member,
        ctx.scope,
        ctx.registry,
        resolved.as_ref(),
        || {
            fell_back = true;
            default_block_id(member, ctx.registry, BED_TOKEN, BED_BASE_ID)
        },
        &GeometryMemberDescription {
            subject: "bed".to_owned(),
            states_from: "the wall it lies against and the way its head points",
            requires: Some(Family::Bed),
        },
        diagnostics,
    )?;
    let token = if fell_back {
        Some(BED_TOKEN)
    } else {
        ctx.scope
            .and_then(|scope| scope.members.get(&member.span.start))
            .and_then(|binding| binding.slot_value.as_ref())
            .and_then(|slot| match &slot.value.kind {
                ValueKind::Token(token) => Some(token.as_str()),
                _ => None,
            })
    };
    let color = bed_color(&id, token, ctx.registry);
    Ok(Some(BedBlock {
        id,
        entity: BlockEntityData::Bed { color },
    }))
}

/// The dye of the dyed bed a bed of `id`, named by `token`, is or stands for; `None` for one whose
/// block names no dye and whose token stands for no dyed bed.
fn bed_color(
    id: &str,
    token: Option<&str>,
    registry: Option<&dyn TargetRegistry>,
) -> Option<DyeColor> {
    let canonical = token
        .and_then(|token| registry?.canonical_block(token))
        .unwrap_or_else(|| id.to_owned());
    path_of(&canonical)
        .strip_suffix("_bed")
        .and_then(DyeColor::from_name)
}

/// The members of `flattened` that laid a bed and hold one of its two cells on `canvas`.
pub(super) fn split_beds(flattened: &[(u32, &Member)], canvas: &Canvas) -> Vec<u32> {
    (0_u32..)
        .zip(flattened)
        .filter(|(_, (_, member))| matches!(member.role, MemberRole::Bed))
        .filter_map(|(index, _)| {
            let bbox = canvas.declared(index)?.bbox;
            let held = [bbox.min(), bbox.max()]
                .into_iter()
                .filter(|&cell| canvas.owner_at(cell) == Some(index))
                .count();
            (held == 1).then_some(index)
        })
        .collect()
}
