//! Setting a `chest` member's chest block against the wall it stands by (`spec/blockstate`
//! "Derive by default, promote on override").

use indexmap::IndexMap;

use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::BlockState;
use crate::block_array::args::{UnreadArgument, diag_invalid_member, ident_or_ignore};
use crate::block_array::joins::Face;
use crate::block_array::openings::WallSide;

use super::canvas::{MemberCanvas, ScopePalette};
use super::fixture::{Anchor, Fixture, Refusal, fixture_cell, outside_the_array};
use super::geometry_material::{Family, GeometryMemberDescription, geometry_material_id};
use super::members::{MemberState, StructCtx, default_block_id, resolve_member_state};

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

/// The values `facing=` takes, as a finding lists them.
const FACINGS: &str = "in, out, left, right";

/// Which way a chest's front looks, relative to the wall it stands by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Toward {
    /// Into the struct.
    In,
    /// Out of the struct.
    Out,
    /// Along the wall toward `offset=0`.
    Left,
    /// Along the wall away from `offset=0`.
    Right,
}

impl Toward {
    fn from_ident(name: &str) -> Option<Self> {
        Some(match name {
            "in" => Self::In,
            "out" => Self::Out,
            "left" => Self::Left,
            "right" => Self::Right,
            _ => return None,
        })
    }

    /// The face a chest by `side` looks toward.
    fn face(self, side: WallSide) -> Face {
        // The wall's outward normal, and the way its `offset=` grows.
        let (out, right) = match side {
            WallSide::Front => (Face::South, Face::East),
            WallSide::Back => (Face::North, Face::West),
            WallSide::Left => (Face::West, Face::South),
            WallSide::Right => (Face::East, Face::North),
        };
        match self {
            Self::In => out.opposite(),
            Self::Out => out,
            Self::Left => right.opposite(),
            Self::Right => right,
        }
    }
}

/// The `facing=` and `size=` a chest's line writes, read before anything can refuse it.
#[derive(Debug, Clone, Copy)]
struct WrittenStates<'m> {
    facing: Option<&'m str>,
    size: Option<&'m str>,
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
    let written = WrittenStates {
        facing: read(
            "facing",
            "`in`, `out`, `left` or `right`",
            "the chest faces away from the wall it stands by",
        ),
        size: read(
            "size",
            "`single`",
            "the chest pairs with the chest beside it, as it does without `size=`",
        ),
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
    let toward = match written.facing.map(|name| (name, Toward::from_ident(name))) {
        None => None,
        Some((_, Some(toward))) => Some(toward),
        Some((other, None)) => {
            return Err(Box::new(diag_invalid_member(
                member,
                format!("unknown chest `facing={other}`"),
                vec![format!("`facing=` takes one of {FACINGS}")],
            )));
        }
    };
    let single = match written.size {
        None => false,
        Some("single") => true,
        Some(other) => {
            return Err(Box::new(diag_invalid_member(
                member,
                format!("unknown chest `size={other}`"),
                vec![
                    "`size=` takes `single`, which keeps the chest from pairing; leave it out to \
                     let the chest pair with the one beside it"
                        .to_owned(),
                ],
            )));
        }
    };
    let (anchor, cell) = fixture_cell(CHEST, member, y_offset, ctx)?;
    let Some(id) = chest_block_id(member, ctx, diagnostics)? else {
        return Ok(());
    };
    let side = anchor.side();
    let facing = toward
        .unwrap_or(match anchor {
            Anchor::Inside(_) => Toward::In,
            Anchor::Outside(_) => Toward::Out,
        })
        .face(side);
    let mut properties = IndexMap::new();
    properties.insert("facing".to_owned(), facing.as_str().to_owned());
    // A written `type` keeps the chest out of every pair (`spec/blockstate` "Connections").
    if single {
        properties.insert("type".to_owned(), "single".to_owned());
    }
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

#[cfg(test)]
mod tests {
    use super::Toward;
    use crate::block_array::openings::WallSide;

    /// `out` is each wall's outward normal and `right` the way its `offset=` grows.
    #[test]
    fn a_chest_turns_by_the_walls_own_axes() {
        for side in [
            WallSide::Front,
            WallSide::Back,
            WallSide::Left,
            WallSide::Right,
        ] {
            let flat = |toward: Toward| {
                let (dx, _, dz) = toward.face(side).step();
                (dx, dz)
            };
            assert_eq!(flat(Toward::Out), side.outward_normal(), "{side:?}");
            assert_eq!(flat(Toward::Right), side.along(), "{side:?}");
            let (dx, dz) = side.outward_normal();
            assert_eq!(flat(Toward::In), (-dx, -dz), "{side:?}");
            let (dx, dz) = side.along();
            assert_eq!(flat(Toward::Left), (-dx, -dz), "{side:?}");
        }
    }
}
