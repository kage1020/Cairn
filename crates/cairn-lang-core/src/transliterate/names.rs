//! The names a transliteration gives its bodies and the chunks it cuts them into.

use std::collections::HashSet;

use crate::ast::ItemKind;
use crate::block_array::Dims;
use crate::ids::is_windows_device_name;

use super::CHUNK_FOOTPRINT;

/// A body name for each of `units`, a raw name with the extents of its grid, in order:
/// lower-case, every run of characters other than letters and digits one `_`, and none a
/// top-level keyword, a device name Windows reserves, or a name an earlier body or its chunks
/// took, nor one whose own chunks would take such a name.
#[must_use]
pub fn unit_names<'a>(units: impl IntoIterator<Item = (&'a str, Dims)>) -> Vec<String> {
    let mut taken: HashSet<String> = HashSet::new();
    let mut out = Vec::new();
    for (raw, dims) in units {
        let base = identifier(raw);
        let mut candidate = base.clone();
        let mut n = 2;
        while taken.contains(&candidate)
            || chunk_names(&candidate, dims).any(|c| taken.contains(&c))
        {
            candidate = format!("{base}_{n}");
            n += 1;
        }
        taken.extend(chunk_names(&candidate, dims));
        taken.insert(candidate.clone());
        out.push(candidate);
    }
    out
}

/// How many chunks a grid of `dims` is cut into along `x` and along `z`, `None` when it is
/// written as one body.
pub(super) fn chunk_counts(dims: Dims) -> Option<(u32, u32)> {
    (dims.x > CHUNK_FOOTPRINT || dims.z > CHUNK_FOOTPRINT).then(|| {
        (
            dims.x.div_ceil(CHUNK_FOOTPRINT),
            dims.z.div_ceil(CHUNK_FOOTPRINT),
        )
    })
}

/// The name of the chunk of `unit` that is `i`-th along `x` and `j`-th along `z`.
pub(super) fn chunk_name(unit: &str, i: u32, j: u32) -> String {
    format!("{unit}_x{i}_z{j}")
}

/// The names of the chunks `unit`'s grid of `dims` is cut into.
fn chunk_names(unit: &str, dims: Dims) -> impl Iterator<Item = String> + '_ {
    let (columns, rows) = chunk_counts(dims).unwrap_or((0, 0));
    (0..columns).flat_map(move |i| (0..rows).map(move |j| chunk_name(unit, i, j)))
}

/// `name` as an identifier no body name rule refuses.
fn identifier(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let trimmed = out.trim_matches('_');
    let mut out = if trimmed.is_empty() {
        "structure".to_owned()
    } else if trimmed.starts_with(|c: char| c.is_ascii_digit()) {
        format!("structure_{trimmed}")
    } else {
        trimmed.to_owned()
    };
    let keyword = ItemKind::ALL.iter().any(|kind| kind.keyword() == out);
    if keyword || is_windows_device_name(&out) {
        out.push_str("_structure");
    }
    out
}
