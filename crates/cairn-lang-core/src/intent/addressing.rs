//! Giving each member of a `struct` or `def` body its address, from where its line puts it
//! (`spec/syntax` "IDs, classes, addresses").

use crate::ast::{Value, ValueKind};
use crate::edition::Edition;

use super::{AddressSegment, Member, MemberAddress, Size, ValueWithSpan, patched_actuator};

/// One coordinate of a [`Position`]; a value the line writes in a shape no step reads sorts last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Coordinate {
    Read(u64),
    Unread,
}

/// Where a member stands among its siblings of one role and side, compared field by field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    /// Along its wall: `offset=`, or the column an `at=` door anchor names.
    column: Coordinate,
    /// `y=`.
    row: Coordinate,
    /// The low corner of the cell a coordinate `at=` names, or of the box `from=` and `to=` span,
    /// `(y, z, x)`.
    low: [Coordinate; 3],
    /// The high corner of the same.
    high: [Coordinate; 3],
}

/// What a line writes other than its `id=`, `class=` and `[selector]`, each value spelled by
/// [`Value::spelling`], compared field by field.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Written {
    /// Each `key=value`, sorted.
    pairs: Vec<(String, String)>,
    /// The bare values, in the order written.
    bare: Vec<String>,
    /// The value after `->`.
    binding: Option<String>,
    /// The edition of the `@edition` guard the line stands under.
    guard: Option<&'static str>,
}

/// The order a body's members are counted in, compared field by field.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Key {
    role: String,
    side: Option<String>,
    position: Position,
    written: Written,
    /// The keys of the members nested under it that take an address, sorted.
    nested: Vec<Key>,
}

/// Give each of `members`, and each member nested under one, its address under `parent`, or at
/// the top of the body when there is none, in a body of `footprint`.
pub(super) fn assign_addresses(
    members: &mut [Member],
    footprint: Option<&Size>,
    parent: Option<&MemberAddress>,
) {
    let mut keyed: Vec<(usize, Key)> = members
        .iter()
        .enumerate()
        .filter_map(|(i, member)| Some((i, key(member, footprint)?)))
        .collect();
    // Stable, so only members that tie at every step keep the order their lines are written in.
    keyed.sort_by(|(_, a), (_, b)| a.cmp(b));
    let mut previous: Option<(String, Option<String>)> = None;
    let mut index = 0;
    for (i, key) in keyed {
        let group = (key.role, key.side);
        if previous.as_ref() == Some(&group) {
            index += 1;
        } else {
            index = 0;
        }
        let segment = AddressSegment::new(group.0.clone(), group.1.clone(), index);
        members[i].address = Some(match parent {
            Some(parent) => parent.child(segment),
            None => MemberAddress::new(segment),
        });
        previous = Some(group);
    }
    for member in members.iter_mut() {
        if let Some(address) = member.address.clone() {
            assign_addresses(&mut member.children.members, footprint, Some(&address));
        }
    }
}

/// Where `member` comes in the count, or `None` for an actuator patch, which stands nowhere: it
/// acts on the member its `[selector]` picks.
fn key(member: &Member, footprint: Option<&Size>) -> Option<Key> {
    if patched_actuator(member).is_some() {
        return None;
    }
    let side = side(member);
    let position = position(member, side.as_deref(), footprint);
    let mut nested: Vec<Key> = member
        .children
        .members
        .iter()
        .filter_map(|child| key(child, footprint))
        .collect();
    nested.sort();
    Some(Key {
        role: member.role.keyword().to_owned(),
        side,
        position,
        written: written(member),
        nested,
    })
}

/// The wall a member's line puts it on (`side=front`) or against (`at=inside.front`), or the
/// opening a beam rests on (`at=entry.top`).
fn side(member: &Member) -> Option<String> {
    let wall = |key| match &member.written(key)?.value.kind {
        ValueKind::Ident(side) if key == "side" => Some(side.clone()),
        ValueKind::DotRef(wall) => Some(wall.to_string()),
        _ => None,
    };
    wall("side").or_else(|| wall("at"))
}

fn position(member: &Member, side: Option<&str>, footprint: Option<&Size>) -> Position {
    let (low, high) = corners(member);
    Position {
        column: column(member, side, footprint),
        row: member
            .written("y")
            .map_or(Coordinate::Read(0), whole_number),
        low,
        high,
    }
}

/// A non-negative integer as a [`Coordinate`], and anything else as [`Coordinate::Unread`].
fn whole_number(value: &ValueWithSpan) -> Coordinate {
    match value.value.kind {
        ValueKind::Int(n) => u64::try_from(n).map_or(Coordinate::Unread, Coordinate::Read),
        _ => Coordinate::Unread,
    }
}

fn column(member: &Member, side: Option<&str>, footprint: Option<&Size>) -> Coordinate {
    if let Some(offset) = member.written("offset") {
        return whole_number(offset);
    }
    let Some(at) = member.written("at") else {
        return Coordinate::Read(0);
    };
    match &at.value.kind {
        ValueKind::Ident(anchor) => anchor_column(anchor, side, footprint),
        // A wall, an opening or a cell, which name no column.
        ValueKind::DotRef(_) | ValueKind::Coord { .. } => Coordinate::Read(0),
        _ => Coordinate::Unread,
    }
}

/// The column a door anchor stands in on `side`, as `spec/syntax` "Selectors" places it.
fn anchor_column(anchor: &str, side: Option<&str>, footprint: Option<&Size>) -> Coordinate {
    let length = match (side, footprint) {
        (Some("front" | "back"), Some(footprint)) => Some(u64::from(footprint.w.get())),
        (Some("left" | "right"), Some(footprint)) => Some(u64::from(footprint.h.get())),
        _ => None,
    };
    match (anchor, length) {
        ("left", _) => Coordinate::Read(0),
        ("center", Some(length)) => Coordinate::Read(length / 2),
        ("right", Some(length)) => Coordinate::Read(length - 1),
        _ => Coordinate::Unread,
    }
}

/// The corners of the cell a coordinate `at=` names, or else of the box `from=` and `to=` span,
/// as `(y, z, x)`.
fn corners(member: &Member) -> ([Coordinate; 3], [Coordinate; 3]) {
    let cell = |value: Option<&ValueWithSpan>| match value.map(|value| &value.value.kind) {
        None => [Coordinate::Read(0); 3],
        Some(ValueKind::Coord { x, y, z }) => [
            Coordinate::Read(*y),
            Coordinate::Read(*z),
            Coordinate::Read(*x),
        ],
        Some(_) => [Coordinate::Unread; 3],
    };
    if let Some(at) = member
        .written("at")
        .filter(|at| matches!(at.value.kind, ValueKind::Coord { .. }))
    {
        let at = cell(Some(at));
        return (at, at);
    }
    let (from, to) = (cell(member.written("from")), cell(member.written("to")));
    let low = std::array::from_fn(|i| from[i].min(to[i]));
    let high = std::array::from_fn(|i| from[i].max(to[i]));
    (low, high)
}

fn written(member: &Member) -> Written {
    let mut pairs: Vec<(String, String)> = member
        .written_fields()
        .filter(|(key, _)| !matches!(*key, "id" | "class"))
        .map(|(key, value)| (key.to_owned(), value.value.spelling()))
        .collect();
    if let Some(slot) = &member.mat_slot {
        pairs.push(("mat_slot".to_owned(), slot.name.clone()));
    }
    pairs.sort();
    Written {
        pairs,
        bare: member.positional.iter().map(Value::spelling).collect(),
        binding: member.binding.as_ref().map(Value::spelling),
        guard: member.edition_guard.map(Edition::as_str),
    }
}
