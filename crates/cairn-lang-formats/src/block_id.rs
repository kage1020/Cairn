//! The block-id shape both structure backends refuse to write without.

/// Whether `id` is a concrete `namespace:path` id rather than an abstract
/// token the registry pack should have resolved.
pub(crate) fn is_concrete_id(id: &str) -> bool {
    // A concrete Minecraft id always carries exactly one `:` separating a
    // non-empty namespace from a non-empty path. Abstract tokens lowered
    // from `@cobblestone` look like `@cobblestone` — no colon at all — and
    // a stray empty side (`:foo` / `foo:`) is treated as abstract too so a
    // typo never reaches the NBT layer silently.
    let mut parts = id.splitn(2, ':');
    let ns = parts.next().unwrap_or("");
    let path = parts.next();
    matches!(path, Some(p) if !ns.is_empty() && !p.is_empty())
}
