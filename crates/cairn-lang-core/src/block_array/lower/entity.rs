//! What the entity members share: the cell a line places one in, the way it faces, where in the
//! cell it stands or hangs, and the values its keys read (`spec/entities` "Two tiers").

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::intent::Member;
use crate::suggest::{did_you_mean, nearest_match};

use crate::block_array::args::{UnreadArgument, diag_invalid_member, read_or_ignore};
use crate::block_array::geometry::Declared;
use crate::block_array::joins::Face;
use crate::block_array::material::AbsentEntity;
use crate::block_array::{Bbox, Entity, EntityData, Item};

use super::canvas::MemberCanvas;
use super::fixture::{Fixture, Refusal, fixture_cell, outside_the_array};
use super::members::StructCtx;
use super::part::{COMPASS_FACES, Stand, stand_of, triple_cell};

/// How far a hanging entity's point sits from its cell's middle toward the face it hangs on, in
/// blocks: the game's own inset, 15/32.
const HANGING_INSET: f64 = 0.468_75;

/// How an entity keeps its place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Hold {
    /// On the face of the block behind it, `faces` naming the ways it may face at a triple.
    Hangs { faces: &'static [Face] },
    /// On its cell's floor.
    Stands,
}

/// Where a line places an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Placed {
    /// The cell it stands or hangs in.
    pub(super) cell: (u32, u32, u32),
    /// The way it faces.
    pub(super) facing: Face,
    /// The face it is set against, as `spec/entities` "Anchor conventions" names one.
    pub(super) host_face: Option<Face>,
}

/// The six faces an item frame hangs on.
pub(super) const EVERY_FACE: [Face; 6] = [
    Face::North,
    Face::South,
    Face::East,
    Face::West,
    Face::Up,
    Face::Down,
];

/// The cell and facing `member`'s line places it at, or the refusal of a line that places none;
/// a `facing=` of another kind than a word joins `unread`.
pub(super) fn place(
    fixture: Fixture,
    hold: Hold,
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    unread: &mut Vec<UnreadArgument>,
) -> Result<Placed, Refusal> {
    let stand = stand_of(fixture, fixture.noun, member)?;
    let written = read_or_ignore(
        member,
        "facing",
        |kind| match kind {
            ValueKind::Ident(word) => Some(word.as_str()),
            _ => None,
        },
        "a face such as `north`, at a coordinate triple (by a wall the entity takes none)",
    )
    .unwrap_or_else(|argument| {
        unread.push(argument);
        None
    });
    if stand == Stand::ByWall {
        if let Some(word) = written {
            return Err(Box::new(diag_invalid_member(
                member,
                format!(
                    "{} `facing={word}` by a wall: it {} the wall and faces away from it, so \
                     `facing=` has nothing to choose",
                    fixture.keyword,
                    match hold {
                        Hold::Hangs { .. } => "hangs on",
                        Hold::Stands => "stands by",
                    },
                ),
                vec!["drop `facing=`, or place it at a coordinate triple, which takes one".into()],
            )));
        }
        let (anchor, cell) = fixture_cell(fixture, member, y_offset, ctx)?;
        let away = anchor.host_face();
        return Ok(Placed {
            cell,
            facing: away,
            host_face: Some(away),
        });
    }
    let cell = triple_cell(fixture, fixture.noun, member, y_offset, ctx)?;
    let faces: &[Face] = match hold {
        Hold::Hangs { faces } => faces,
        Hold::Stands => &COMPASS_FACES,
    };
    let facing = match written {
        Some(word) => {
            let Some(face) = Face::from_name(word).filter(|face| faces.contains(face)) else {
                let names: Vec<&str> = faces.iter().map(|face| face.as_str()).collect();
                let mut notes = vec![format!("`facing=` takes {}", names.join(", "))];
                notes.extend(
                    nearest_match(word, names.iter().copied())
                        .map(|near| did_you_mean(&format!("facing={near}"))),
                );
                return Err(Box::new(diag_invalid_member(
                    member,
                    format!("unknown {} `facing={word}`", fixture.keyword),
                    notes,
                )));
            };
            face
        }
        None if hold == Hold::Stands => Face::South,
        None => {
            return Err(Box::new(diag_invalid_member(
                member,
                format!(
                    "{} at a coordinate triple has no `facing=`, so nothing says which block it \
                     hangs on",
                    fixture.keyword
                ),
                vec![format!(
                    "write `facing=` with the way it faces, one of {}: it hangs on the block \
                     behind it",
                    faces
                        .iter()
                        .map(|face| face.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )],
            )));
        }
    };
    Ok(Placed {
        cell,
        facing,
        host_face: match hold {
            Hold::Hangs { .. } => Some(facing),
            Hold::Stands => None,
        },
    })
}

/// The middle of `cell`'s floor, where a standing entity stands.
pub(super) fn standing_point((x, y, z): (u32, u32, u32)) -> (f64, f64, f64) {
    (f64::from(x) + 0.5, f64::from(y), f64::from(z) + 0.5)
}

/// The point of `cell` a hanging entity facing `facing` hangs by: its middle, moved toward the
/// face behind it.
pub(super) fn hanging_point((x, y, z): (u32, u32, u32), facing: Face) -> (f64, f64, f64) {
    let (dx, dy, dz) = facing.step();
    (
        f64::from(x) + 0.5 - f64::from(dx) * HANGING_INSET,
        f64::from(y) + 0.5 - f64::from(dy) * HANGING_INSET,
        f64::from(z) + 0.5 - f64::from(dz) * HANGING_INSET,
    )
}

/// Declare and place a one-cell entity of `id` holding `data` where `placed` says, at `pos`.
pub(super) fn place_one(
    member: &Member,
    canvas: &mut MemberCanvas<'_>,
    placed: Placed,
    id: &str,
    pos: (f64, f64, f64),
    data: EntityData,
    unread: &mut Vec<UnreadArgument>,
) {
    canvas.declare(Declared::cell(placed.cell, placed.host_face));
    canvas.place_entity(
        member.span.start,
        Entity {
            id: id.to_owned(),
            pos,
            block_pos: placed.cell,
            facing: placed.facing,
            data,
            nbt: nbt_of(member, unread),
        },
    );
}

/// Declare `cells` as `member`'s box, or refuse a box that leaves the array.
pub(super) fn declare_box(
    fixture: Fixture,
    member: &Member,
    ctx: &StructCtx<'_>,
    canvas: &mut MemberCanvas<'_>,
    anchor: (u32, u32, u32),
    far: (u32, u32, u32),
    host_face: Option<Face>,
) -> Result<(), Refusal> {
    if ctx.dims.index(far.0, far.1, far.2).is_none() {
        return Err(Box::new(outside_the_array(fixture, member)));
    }
    canvas.declare(Declared {
        anchor,
        bbox: Bbox::spanning(anchor, far),
        host_face,
    });
    Ok(())
}

/// The compound the line's `nbt=` writes, as written; a value of another kind joins `unread`.
pub(super) fn nbt_of(member: &Member, unread: &mut Vec<UnreadArgument>) -> Option<String> {
    read_or_ignore(
        member,
        "nbt",
        |kind| match kind {
            ValueKind::Nbt(text) => Some(text.clone()),
            _ => None,
        },
        "an `nbt={...}` compound",
    )
    .unwrap_or_else(|argument| {
        unread.push(argument);
        None
    })
}

/// An item or enchantment id as a key writes it, namespaced: `diamond` or `minecraft:diamond`.
pub(super) fn id_value(kind: &ValueKind) -> Option<String> {
    match kind {
        ValueKind::Ident(name) => Some(format!("minecraft:{name}")),
        ValueKind::BlockId(id) if !id.contains('[') => Some(id.clone()),
        _ => None,
    }
}

/// What [`id_value`] reads, completing "`key=` must be …".
pub(super) const AN_ID: &str = "an id such as `diamond` or `minecraft:diamond`";

/// The item `key=` names, one of it with no enchantment; a value of another kind joins `unread`.
pub(super) fn item_of(
    member: &Member,
    key: &'static str,
    unread: &mut Vec<UnreadArgument>,
) -> Option<Item> {
    read_or_ignore(member, key, id_value, AN_ID)
        .unwrap_or_else(|argument| {
            unread.push(argument);
            None
        })
        .map(|id| Item {
            id,
            count: 1,
            enchantment: None,
        })
}

/// The scale `scale=` writes, above 0, or 1 when the line writes none; another value joins
/// `unread`.
pub(super) fn scale_of(member: &Member, unread: &mut Vec<UnreadArgument>) -> f64 {
    read_or_ignore(
        member,
        "scale",
        |kind| {
            let scale = match kind {
                ValueKind::Decimal(text) => text.parse::<f64>().ok()?,
                ValueKind::Int(whole) => i32::try_from(*whole).ok().map(f64::from)?,
                _ => return None,
            };
            (scale > 0.0 && scale.is_finite()).then_some(scale)
        },
        "a number above 0, such as `1.5`",
    )
    .unwrap_or_else(|argument| {
        unread.push(argument);
        None
    })
    .unwrap_or(1.0)
}

/// Each unread argument as its `W_IGNORED_ARGUMENT`, worded by `consequence`.
pub(super) fn report_unread(
    unread: Vec<UnreadArgument>,
    diagnostics: &mut Vec<Diagnostic>,
    consequence: impl Fn(&str) -> &'static str,
) {
    for argument in unread {
        let said = consequence(argument.key);
        diagnostics.push(argument.report(said));
    }
}

/// The `E_PARITY_UNSUPPORTED` on `member`, whose entity the target's edition lacks; `instead`
/// says what to write.
pub(super) fn diag_absent_entity(
    member: &Member,
    id: &str,
    absent: &AbsentEntity,
    instead: String,
) -> Diagnostic {
    let has = absent.declared_by;
    Diagnostic {
        code: DiagnosticCode::ParityUnsupported,
        span: member.span.clone(),
        primary: format!(
            "`{id}` is an entity only {} has, so `{}` cannot build it",
            has.title(),
            absent.registry,
        ),
        notes: vec![
            DiagnosticNote {
                span: None,
                message: absent.reason.clone(),
            },
            DiagnosticNote {
                span: None,
                message: instead,
            },
        ],
        data: Some(DiagnosticData::ParityUnsupported {
            id: id.to_owned(),
            registry: absent.registry.clone(),
            declared_by: has.as_str().to_owned(),
            reason: absent.reason.clone(),
        }),
    }
}
