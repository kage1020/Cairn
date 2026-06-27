//! Semantic layer for the Intent IR.
//!
//! Given a lowered [`crate::intent::IntentModule`], `resolve` walks every
//! `theme`, `def`, `struct`, and `site` and produces a [`Resolution`] that
//! records:
//!
//! 1. **slot bindings** — each `mat_slot=NAME` on a member is paired with
//!    `theme.slots[NAME]` from the applied theme;
//! 2. **selector matches** — each `KEYWORD[attrs] -> bindings` row in a theme
//!    is paired with the member(s) it actually matches in the file;
//! 3. **diagnostics** — `E_UNRESOLVED_SLOT` (Error) for `mat_slot=NAME` that
//!    no theme provides, `E_UNKNOWN_SLOT_TARGET` (Warning) for theme slot
//!    targets that look like neither a canonical nor an abstract material
//!    token, and `E_THEME_SELECTOR_UNMATCHED` (Warning) for selectors that
//!    don't bind to anything.
//!
//! The resolved data here is consumed by:
//!
//! - `crate::check` — merges `Resolution.diagnostics` into the full `cairn
//!   check` output so a single CLI run reports both syntactic and
//!   theme-binding problems together;
//! - `cairn info` — re-uses the same [`Resolution`] when computing the three
//!   version axes (`compute_axes`).
//!
//! Per-edition resolved blockstate (the `Lifted` semantic level) is the
//! next maturity tier's responsibility — `Member.resolved_state` is left
//! untouched here.
//!
//! See `spec/materials-themes.md` §7 (slots, selectors, canonical vs.
//! abstract tokens) and `spec/versioning-editions.md` §10.5 (the three info
//! axes) for the rules this module enforces.

mod binding;
mod requires_parse;
mod resolver;
mod version_axes;

pub use binding::{SelectorMatch, ThemeBinding, TokenKind, classify_token};
pub use requires_parse::{compare_versions, parse_min_version};
pub use resolver::{
    PortRef, Resolution, ResolvedConnect, ResolvedMemberBinding, ScopeResolution, place_scope_key,
    resolve,
};
pub use version_axes::{
    EditionPortability, RegistryRange, SemanticSensitiveFinding, VersionAxes, compute_axes,
};
