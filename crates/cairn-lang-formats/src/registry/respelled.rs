//! How a version writes a canonical token whose id its `blocks` table does not declare, trying the
//! pack's tables in the order `spec/materials-themes` "Mappings across version and edition" gives.

use super::aliases::AliasIndex;
use super::blocks::BlocksIndex;
use super::states::StateMapIndex;

/// How a version writes a canonical block it does not declare under the token's own id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Respelled<'a> {
    /// The version declares this other spelling of the block's `aliases` group.
    Renamed(&'a str),
    /// The version's state map writes the Java block as Bedrock blocks of its own.
    Apart,
}

/// How `version` writes the canonical block `id` names: under the one spelling of its alias group
/// the version declares, or else apart through the version's state map; `None` when the version
/// declares `id`, has no `blocks` table, or writes it neither way.
pub(crate) fn respelled_at<'a>(
    blocks: &'a BlocksIndex,
    aliases: &'a AliasIndex,
    states: Option<&StateMapIndex>,
    version: &str,
    id: &str,
) -> Option<Respelled<'a>> {
    let ids = blocks.ids_for(version)?;
    if ids.binary_search_by(|known| known.as_str().cmp(id)).is_ok() {
        return None;
    }
    if let Some(spelling) = aliases.renamed(blocks, ids, id) {
        return Some(Respelled::Renamed(spelling));
    }
    states?
        .at(version)?
        .writes_apart(id)
        .then_some(Respelled::Apart)
}
