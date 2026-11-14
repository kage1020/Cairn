//! A `raw_repeat`'s copies, and the rows an imported raw member sizes the volume to
//! (`spec/materials-themes` "Raw blocks").

use std::collections::HashMap;

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::{Member, MemberRole};

use crate::block_array::args::diag_invalid_member;

/// How many times a raw member is written, and how far each copy lies from the one before.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Repeat {
    count: u32,
    step: (u32, u32, u32),
}

/// The repeat of each raw member a `raw_repeat` holds, by the span start of the member's line.
pub(super) type Repeats = HashMap<usize, Repeat>;

impl Repeat {
    /// The line written once, as a raw member outside a `raw_repeat` is.
    pub(super) const ONCE: Self = Self {
        count: 1,
        step: (0, 0, 0),
    };

    /// How many copies are written.
    pub(super) fn count(self) -> u32 {
        self.count
    }

    /// Whether more than one copy is written.
    pub(super) fn repeats(self) -> bool {
        self.count > 1
    }

    /// How far the last copy lies from the first.
    pub(super) fn reach(self) -> (u64, u64, u64) {
        let copies = u64::from(self.count.saturating_sub(1));
        let (x, y, z) = self.step;
        (
            copies * u64::from(x),
            copies * u64::from(y),
            copies * u64::from(z),
        )
    }

    /// Each copy's shift from the first, the first's `(0, 0, 0)` included.
    pub(super) fn shifts(self) -> impl Iterator<Item = (u32, u32, u32)> {
        let (x, y, z) = self.step;
        (0..self.count).map(move |k| {
            (
                k.saturating_mul(x),
                k.saturating_mul(y),
                k.saturating_mul(z),
            )
        })
    }
}

/// The note on a `raw_repeat` refused for its `count=` or `step=`, saying what each is.
const REPEAT_FORM: &str = "write `count=N step=dx,dy,dz`: the raw members indented under the \
                           `raw_repeat` are written `N` times, each copy `dx,dy,dz` past the one \
                           before";

/// The copies a `raw_repeat`'s `count=` and `step=` ask for.
///
/// # Errors
///
/// The `E_INVALID_MEMBER` for a `count=` or `step=` that is missing or does not read, or for a
/// zero step between copies.
pub(super) fn read_repeat(member: &Member) -> Result<Repeat, Box<Diagnostic>> {
    let refuse = |primary: String, note: &str| {
        Box::new(diag_invalid_member(member, primary, vec![note.to_owned()]))
    };
    let count = match member.arguments.get("count") {
        None => {
            return Err(refuse(
                "raw_repeat has no `count=`, so nothing says how many copies it writes".to_owned(),
                REPEAT_FORM,
            ));
        }
        Some(raw) => match raw.value.kind {
            ValueKind::Int(n) if n >= 1 => u32::try_from(n).ok(),
            _ => None,
        }
        .ok_or_else(|| {
            refuse(
                format!(
                    "raw_repeat `count=` must be a positive integer that fits in u32, not {}",
                    raw.value.describe(),
                ),
                REPEAT_FORM,
            )
        })?,
    };
    let step = match member.arguments.get("step") {
        None => {
            return Err(refuse(
                "raw_repeat has no `step=`, so nothing says where each copy goes".to_owned(),
                REPEAT_FORM,
            ));
        }
        Some(raw) => match raw.value.kind {
            ValueKind::Coord { x, y, z } => u32::try_from(x)
                .ok()
                .zip(u32::try_from(y).ok())
                .zip(u32::try_from(z).ok())
                .map(|((x, y), z)| (x, y, z)),
            _ => None,
        }
        .ok_or_else(|| {
            refuse(
                format!(
                    "raw_repeat `step=` must be a coordinate triple whose components fit in u32, \
                     not {}",
                    raw.value.describe(),
                ),
                REPEAT_FORM,
            )
        })?,
    };
    if count > 1 && step == (0, 0, 0) {
        return Err(refuse(
            format!(
                "raw_repeat `step=0,0,0` puts each of its {count} copies in the cells of the first"
            ),
            "give `step=` the distance from one copy to the next, or write the raw members once \
             without a `raw_repeat`",
        ));
    }
    Ok(Repeat { count, step })
}

/// The refusal of a `raw_repeat` with no raw member indented under it.
pub(super) fn diag_empty_repeat(member: &Member) -> Diagnostic {
    diag_invalid_member(
        member,
        "raw_repeat has no `raw_block` or `raw_fill` indented under it, so it writes nothing"
            .to_owned(),
        vec![
            "indent the `raw_block` and `raw_fill` lines to repeat under the `raw_repeat`"
                .to_owned(),
        ],
    )
}

/// Whether a raw member's line carries `origin=imported`, the mark of a line `cairn import`
/// wrote.
pub(super) fn is_imported(member: &Member) -> bool {
    member.ident_value("origin") == Some("imported")
}

/// The refusal of a raw member's `origin=` that is not `imported`, or `None`.
pub(super) fn origin_refusal(member: &Member) -> Option<Diagnostic> {
    let written = member.arguments.get("origin")?;
    if is_imported(member) {
        return None;
    }
    Some(diag_invalid_member(
        member,
        format!(
            "{} `origin=` must be `imported`, not {}",
            member.role.keyword(),
            written.value.describe(),
        ),
        vec![
            "`origin=imported` marks a line `cairn import` wrote; leave `origin=` out of a line \
             written by hand"
                .to_owned(),
        ],
    ))
}

/// The `y` extent the imported raw members of `flattened` reach with every copy, `0` with none.
pub(super) fn imported_rows(flattened: &[(u32, &Member)], repeats: &Repeats) -> u32 {
    flattened
        .iter()
        .filter(|(_, member)| member.role.is_repeatable() && is_imported(member))
        .filter_map(|&(y_offset, member)| {
            let keys: &[&str] = match member.role {
                MemberRole::RawBlock => &["at"],
                _ => &["from", "to"],
            };
            let top = keys
                .iter()
                .map(|key| match member.arguments.get(*key)?.value.kind {
                    ValueKind::Coord { y, .. } => Some(y),
                    _ => None,
                })
                .collect::<Option<Vec<u64>>>()?
                .into_iter()
                .max()?;
            let repeat = repeats
                .get(&member.span.start)
                .copied()
                .unwrap_or(Repeat::ONCE);
            let rows = top
                .saturating_add(repeat.reach().1)
                .saturating_add(u64::from(y_offset))
                .saturating_add(1);
            Some(u32::try_from(rows).unwrap_or(u32::MAX))
        })
        .max()
        .unwrap_or(0)
}
