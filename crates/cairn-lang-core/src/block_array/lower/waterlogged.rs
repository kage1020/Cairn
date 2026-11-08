//! Reading the `waterlogged=` a `stair` or `chest` line writes, which keeps its block's state from
//! a water fill (`spec/blockstate` "`waterlogged`").

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::args::{UnreadArgument, diag_invalid_member, read_or_ignore};

/// The block state a water fill sets on a block it overlaps, and a line may keep.
pub(super) const WATERLOGGED: &str = "waterlogged";

/// The note on a `waterlogged=` refused for its word, naming the ones it takes.
const TAKES: &str = "`waterlogged=` takes `auto`, which lets a water `fill` set it, or `true` or \
                     `false`, which keep it";

/// A `waterlogged=` as the line writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WrittenWaterlogged<'m> {
    /// `auto`, which leaves the state to a water fill as no `waterlogged=` does.
    Auto,
    /// `true` or `false`, kept whatever water overlaps the block.
    Kept(bool),
    /// Any other word, which the member refuses.
    Unknown(&'m str),
}

/// Read a member's `waterlogged=`: `Ok(None)` when absent, `Err` when its value is neither a word
/// nor a boolean.
pub(super) fn read_waterlogged(
    member: &Member,
) -> Result<Option<WrittenWaterlogged<'_>>, UnreadArgument> {
    read_or_ignore(
        member,
        "waterlogged",
        |kind| match kind {
            ValueKind::Bool(kept) => Some(WrittenWaterlogged::Kept(*kept)),
            ValueKind::Ident(word) if word == "auto" => Some(WrittenWaterlogged::Auto),
            ValueKind::Ident(word) => Some(WrittenWaterlogged::Unknown(word)),
            _ => None,
        },
        "`auto`, `true` or `false`",
    )
}

/// The `waterlogged` the member's block state writes, `None` to leave it to a water fill, or the
/// refusal of a word the key does not take.
pub(super) fn waterlogged_state(
    member: &Member,
    written: Option<WrittenWaterlogged<'_>>,
) -> Result<Option<bool>, Box<Diagnostic>> {
    match written {
        None | Some(WrittenWaterlogged::Auto) => Ok(None),
        Some(WrittenWaterlogged::Kept(kept)) => Ok(Some(kept)),
        Some(WrittenWaterlogged::Unknown(word)) => Err(Box::new(diag_invalid_member(
            member,
            format!("unknown {} `waterlogged={word}`", member.role.keyword()),
            vec![TAKES.to_owned()],
        ))),
    }
}

/// Write `kept` into `properties` as the block's `waterlogged`, when the line keeps one.
pub(super) fn keep_waterlogged(properties: &mut IndexMap<String, String>, kept: Option<bool>) {
    if let Some(kept) = kept {
        properties.insert(WATERLOGGED.to_owned(), kept.to_string());
    }
}
