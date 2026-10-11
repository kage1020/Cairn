//! The block-id shape both structure backends refuse to write without.

/// Whether `id` is a concrete `namespace:path` id rather than an abstract
/// token the registry pack should have resolved.
pub(crate) fn is_concrete_id(id: &str) -> bool {
    // An abstract token (`@cobblestone`) has no colon, and an empty side
    // (`:foo`, `foo:`) counts as abstract so a typo never reaches the NBT
    // layer.
    let mut parts = id.splitn(2, ':');
    let ns = parts.next().unwrap_or("");
    let path = parts.next();
    matches!(path, Some(p) if !ns.is_empty() && !p.is_empty())
}
