//! The furniture library a body's `place` builds from when the file declares no `def` of the name
//! (`spec/components-editing-sites` "The furniture library").

use std::sync::OnceLock;

use crate::intent::{DefIr, IntentModule, ThemeIr, lower};
use crate::parse::parse;

/// The library's source, as the specification prints it.
pub const SOURCE: &str = include_str!("furniture/library.crn");

/// The library's theme and defs, parsed once.
fn library() -> &'static IntentModule {
    static LIBRARY: OnceLock<IntentModule> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        let module = parse(SOURCE).expect("the furniture library parses");
        lower(&module)
    })
}

/// The library's `def` named `name`.
#[must_use]
pub fn def(name: &str) -> Option<&'static DefIr> {
    library().defs.iter().find(|def| def.name == name)
}

/// Every `def` the library ships, in source order.
#[must_use]
pub fn defs() -> &'static [DefIr] {
    &library().defs
}

/// The theme a library def falls back on for a slot the body's theme does not bind.
///
/// # Panics
///
/// When the library declares no theme, which its tests rule out.
#[must_use]
pub fn theme() -> &'static ThemeIr {
    library()
        .themes
        .first()
        .expect("the furniture library declares its theme")
}

#[cfg(test)]
mod tests;
