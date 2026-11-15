//! Where a member set in one cell stands, and the words its line turns it with: the form of its
//! `at=`, the cell a coordinate triple names, and a state written as one of a few words.

use crate::ast::ValueKind;
use crate::intent::Member;
use crate::suggest::{did_you_mean, nearest_match};

use crate::block_array::args::diag_invalid_member;
use crate::block_array::joins::Face;

use super::cell_box::{Cell, Shape, written_cells};
use super::fixture::{Fixture, Refusal};
use super::members::StructCtx;

/// The note on a compass `facing=`, saying which way each word looks.
pub(super) const COMPASS: &str = "`facing=` takes one of north, south, east, west: north toward \
                                  the `back` wall and east toward the `right` one";

/// The four compass faces, in the order a finding lists them.
pub(super) const COMPASS_FACES: [Face; 4] = [Face::North, Face::South, Face::East, Face::West];

/// The note on an `at=` that is missing or neither form, saying what the two are, for `what`.
fn stands(what: &str) -> String {
    format!(
        "write `at=` as the coordinate triple `x,y,z` of the cell the {what} stands in, as in \
         `at=1,1,1`, or as a wall anchor, `at=<side>.outside` or `at=inside.<side>`"
    )
}

/// How a line's `at=` names the cell its member stands in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Stand {
    /// A coordinate triple.
    Triple,
    /// A wall anchor.
    ByWall,
}

/// `written` when it is one of `words`, `None` when the line writes none, or the refusal of any
/// other word.
pub(super) fn word<'m>(
    member: &Member,
    key: &str,
    written: Option<&'m str>,
    words: &[&'static str],
) -> Result<Option<&'m str>, Refusal> {
    let Some(name) = written else {
        return Ok(None);
    };
    if words.contains(&name) {
        return Ok(Some(name));
    }
    let mut notes = vec![format!("`{key}=` takes {}", words.join(" or "))];
    notes.extend(
        nearest_match(name, words.iter().copied())
            .map(|near| did_you_mean(&format!("{key}={near}"))),
    );
    Err(Box::new(diag_invalid_member(
        member,
        format!("unknown {} `{key}={name}`", member.role.keyword()),
        notes,
    )))
}

/// How the line's `at=` names its cell, or the refusal of an `at=` that is missing or neither form;
/// `what` is what the notes call the member.
pub(super) fn stand_of(fixture: Fixture, what: &str, member: &Member) -> Result<Stand, Refusal> {
    let keyword = fixture.keyword;
    let refuse =
        |primary: String| Box::new(diag_invalid_member(member, primary, vec![stands(what)]));
    let Some(raw) = member.arguments.get("at") else {
        return Err(refuse(format!(
            "{keyword} has no `at=`, so nothing says where it stands"
        )));
    };
    match raw.value.kind {
        ValueKind::Coord { .. } => Ok(Stand::Triple),
        ValueKind::DotRef(_) => Ok(Stand::ByWall),
        _ => Err(refuse(format!(
            "{keyword} `at=` must be a coordinate triple or a wall anchor, not {}",
            raw.value.describe(),
        ))),
    }
}

/// The cell a line's coordinate triple names, or the refusal of an `offset=` or `y=` beside it;
/// `what` is what the note calls the member.
pub(super) fn triple_cell(
    fixture: Fixture,
    what: &str,
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
) -> Result<Cell, Refusal> {
    let beside: Vec<String> = ["offset", "y"]
        .into_iter()
        .filter(|key| member.arguments.contains_key(*key))
        .map(|key| format!("`{key}=`"))
        .collect();
    if !beside.is_empty() {
        return Err(Box::new(diag_invalid_member(
            member,
            format!(
                "{} {} beside a coordinate triple, which names the cell whole",
                fixture.keyword,
                beside.join(" and "),
            ),
            vec![format!(
                "drop {}, or set the {} by a wall with `at=<side>.outside` or \
                 `at=inside.<side>`, which reads them",
                beside.join(" and "),
                what,
            )],
        )));
    }
    let cells = written_cells(member, Shape::Cell, y_offset, ctx)?;
    Ok(cells
        .cells()
        .next()
        .expect("a cell box holds at least one cell"))
}
