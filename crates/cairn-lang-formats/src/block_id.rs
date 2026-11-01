//! The block-id shape both structure backends refuse to write without.

/// Whether `id` is a `namespace:path` id with both sides non-empty, not an unresolved token.
pub(crate) fn is_concrete_id(id: &str) -> bool {
    let mut parts = id.splitn(2, ':');
    let ns = parts.next().unwrap_or("");
    let path = parts.next();
    matches!(path, Some(p) if !ns.is_empty() && !p.is_empty())
}
