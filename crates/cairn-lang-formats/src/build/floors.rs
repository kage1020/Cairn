//! The `@requires` floors a build is held to, the target they rule out, and how a message names
//! the floor.

use cairn_lang_core::resolve::{
    FloorOrigin, FloorPlacement, FloorVerdict, VersionFloor, VersionOrder,
};

use crate::target::ResolvedTarget;

/// The directive `floor` is written with, as a fix names it.
#[must_use]
pub fn floor_keyword(floor: &VersionFloor) -> &'static str {
    match floor.origin {
        FloorOrigin::Module => "`@requires`",
        FloorOrigin::Def(_) | FloorOrigin::Theme(_) => "`requires`",
    }
}

/// The line naming the `def` or `theme` that imposed a floor, or `None` for an `@requires` header.
#[must_use]
pub fn floor_origin_note(floor: &VersionFloor) -> Option<String> {
    let inherited_by = match &floor.origin {
        FloorOrigin::Module => return None,
        FloorOrigin::Def(name) => format!("every `place use={name}` inherits it"),
        FloorOrigin::Theme(_) => "every scope that binds the theme inherits it".to_owned(),
    };
    Some(format!(
        "{} declares it, and {inherited_by}",
        floor.declarer()
    ))
}

/// The `@requires` floors one edition's build is held to, with that edition's version order.
#[derive(Debug)]
pub struct DeclaredFloors {
    /// Never empty.
    pub floors: Vec<VersionFloor>,
    /// The target edition's `DataVersion` table.
    pub order: VersionOrder,
}

/// The floor that refuses a target, and on which ground.
#[derive(Debug, Clone, Copy)]
pub enum FloorRefusal<'a> {
    /// The edition's version table cannot place this floor.
    Unplaceable(&'a VersionFloor),
    /// The target sits below this floor.
    Below(&'a VersionFloor),
}

/// The floor that refuses `target`, or `None`; an unplaceable floor is named first, since raising
/// `--target` cannot satisfy it.
#[must_use]
pub fn floor_refusing_target<'a>(
    floors: &'a [VersionFloor],
    order: &VersionOrder,
    target: &ResolvedTarget,
) -> Option<FloorRefusal<'a>> {
    if let Some(floor) = floors
        .iter()
        .find(|floor| order.place(&floor.version) == FloorPlacement::Unplaceable)
    {
        return Some(FloorRefusal::Unplaceable(floor));
    }
    let target_key = target.order_key();
    floors
        .iter()
        .find(|floor| order.verdict(&floor.version, target_key) == FloorVerdict::Below)
        .map(FloorRefusal::Below)
}
