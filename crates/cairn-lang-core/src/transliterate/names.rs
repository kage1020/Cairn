//! The names a transliteration gives its bodies.

use crate::ast::ItemKind;
use crate::ids::is_windows_device_name;

/// A body name for each of `raw`, in order: lower-case, every run of characters other than
/// letters and digits one `_`, and none a top-level keyword, a device name Windows reserves, or
/// a name an earlier one took.
#[must_use]
pub fn unit_names<'a>(raw: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut taken: Vec<String> = Vec::new();
    for name in raw {
        let base = identifier(name);
        let mut candidate = base.clone();
        let mut n = 2;
        while taken.contains(&candidate) {
            candidate = format!("{base}_{n}");
            n += 1;
        }
        taken.push(candidate);
    }
    taken
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
