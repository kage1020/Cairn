//! Giving each member of a `struct` or `def` body its address, from where its line puts it
//! (`spec/syntax` "IDs, classes, addresses").

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::ast::ValueKind;

use super::{AddressSegment, Member, MemberAddress, Size, ValueWithSpan};

/// One coordinate of a [`Position`]; a value the line writes in a shape no pass reads sorts last.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Coordinate {
    Read(u64),
    Unread,
}

/// Where a member stands among its siblings of one role and side, compared field by field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    /// Along its wall: `offset=`, or the column a door's `at=` names.
    column: Coordinate,
    /// `y=`.
    row: Coordinate,
    /// The low corner of the cell or box its `at=`, `from=` and `to=` name, `(y, z, x)`.
    low: [Coordinate; 3],
    /// The high corner of the same.
    high: [Coordinate; 3],
}

/// Everything a line writes, key by key, as text, which orders two members at one position.
type Written = BTreeMap<String, String>;

/// Give each of `members`, and each member nested under one, its address under `parent`, in a
/// body of `footprint`.
pub(super) fn assign_addresses(
    members: &mut [Member],
    footprint: Option<&Size>,
    parent: &MemberAddress,
) {
    let keys: Vec<(String, Option<String>, Position, Written)> = members
        .iter()
        .map(|member| {
            let side = side(member);
            let position = position(member, side.as_deref(), footprint);
            (
                member.role.keyword().to_owned(),
                side,
                position,
                written(member),
            )
        })
        .collect();
    // An actuator patch stands nowhere: it acts on the member its `[selector]` picks.
    let mut order: Vec<usize> = (0..members.len())
        .filter(|&i| members[i].selector.is_none())
        .collect();
    // Stable, so only lines that write the same thing keep the order they are written in.
    order.sort_by(|&a, &b| keys[a].cmp(&keys[b]));
    let mut taken: HashMap<(&str, Option<&str>), u32> = HashMap::new();
    for i in order {
        let (role, side, _, _) = &keys[i];
        let index = taken.entry((role.as_str(), side.as_deref())).or_default();
        members[i].address = Some(parent.child(AddressSegment {
            role: role.clone(),
            side: side.clone(),
            index: *index,
        }));
        *index += 1;
    }
    for member in members.iter_mut() {
        if let Some(address) = member.address.clone() {
            assign_addresses(&mut member.children.members, footprint, &address);
        }
    }
}

/// The wall a member's line puts it on (`side=front`) or against (`at=inside.front`).
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
        ValueKind::Int(_) => whole_number(at),
        _ => Coordinate::Read(0),
    }
}

/// The column a door's named anchor stands in on `side`, as `spec/syntax` "Selectors" places it.
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

/// The corners of the cell `at=` names, or of the box `from=` and `to=` name, as `(y, z, x)`.
fn corners(member: &Member) -> ([Coordinate; 3], [Coordinate; 3]) {
    let cell = |key: &str| match member.written(key).map(|value| &value.value.kind) {
        None => [Coordinate::Read(0); 3],
        Some(ValueKind::Coord { x, y, z }) => [
            Coordinate::Read(*y),
            Coordinate::Read(*z),
            Coordinate::Read(*x),
        ],
        Some(_) => [Coordinate::Unread; 3],
    };
    if member.written("at").is_some() {
        let at = cell("at");
        return (at, at);
    }
    let (from, to) = (cell("from"), cell("to"));
    let low = std::array::from_fn(|i| from[i].min(to[i]));
    let high = std::array::from_fn(|i| from[i].max(to[i]));
    (low, high)
}

fn written(member: &Member) -> Written {
    let mut written: Written = member
        .written_fields()
        .map(|(key, value)| (key.to_owned(), text(&value.value)))
        .collect();
    let labels = [
        ("id", member.id.as_deref()),
        ("class", member.class.as_deref()),
        (
            "mat_slot",
            member.mat_slot.as_ref().map(|slot| slot.name.as_str()),
        ),
    ];
    for (key, label) in labels {
        if let Some(label) = label {
            written.insert(key.to_owned(), label.to_owned());
        }
    }
    if !member.positional.is_empty() {
        written.insert(" ".to_owned(), text(&member.positional));
    }
    if let Some(binding) = &member.binding {
        written.insert("->".to_owned(), text(binding));
    }
    if let Some(edition) = member.edition_guard {
        written.insert("@edition".to_owned(), edition.as_str().to_owned());
    }
    written
}

fn text(value: &impl Serialize) -> String {
    serde_json::to_string(value).expect("a value the parser read serializes")
}
