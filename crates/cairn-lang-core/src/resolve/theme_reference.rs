//! Which theme a scope binds: the module-level auto-pick and a `place ...
//! theme=NAME` reference, judged against the module's [`ThemeBinding`]s,
//! and the findings either can raise.
//!
//! The rule both apply speaks only names and is
//! [`super::theme_variant`]'s; this is the half that needs the bindings.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::edition::Edition;
use crate::error::Span;
use crate::intent::{IntentModule, Member};

use super::binding::ThemeBinding;
use super::site::unresolved_ref_diag;
use super::theme_variant::{
    bound_theme_name, pick_variant as pick_variant_by_name, strip_edition_suffix,
};

/// Pick the theme name to bind for `logical` under `edition`, out of the
/// module's binding map. See [`super::theme_variant::pick_variant`].
pub(super) fn pick_variant<'a>(
    themes: &'a IndexMap<String, ThemeBinding>,
    logical: &str,
    edition: Option<Edition>,
) -> Option<&'a str> {
    pick_variant_by_name(themes.keys().map(String::as_str), logical, edition)
}

/// The sole logical theme of the module, or `None`. See
/// [`super::theme_variant::single_logical_theme`].
pub(super) fn single_logical_theme(themes: &IndexMap<String, ThemeBinding>) -> Option<String> {
    super::theme_variant::single_logical_theme(themes.keys().map(String::as_str)).map(str::to_owned)
}

/// What a `place ... theme=NAME` reference resolves to under `edition`.
enum ThemeReference<'a> {
    /// Bind this theme.
    Bound {
        /// The theme actually bound — not necessarily the name written.
        name: &'a str,
        /// How the reference was spelled, which is what decides whether the
        /// author asked about one variant or about the logical theme.
        spelling: Spelling,
    },
    /// Variants of this logical theme are declared, but none can bind under
    /// the edition this carries.
    NoVariantForEdition(Edition),
    /// No theme in the module shares this name's logical part — or the name
    /// carries a suffix, nothing is declared under it, and no edition is
    /// pinned to justify picking another variant.
    Unknown,
}

/// How a `theme=` reference was written.
///
/// One enum rather than two flags, so a logical spelling cannot also be
/// rebound and the edition behind a rebind travels with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spelling {
    /// Written without an edition suffix, so it names the logical theme.
    /// Sibling variants may soften its slot diagnostics while no edition is
    /// picked.
    Logical,
    /// Written with an edition suffix, so it names one variant. The author
    /// asked about that variant's slots and no sibling softens them.
    /// `rebound_under` is `Some(edition)` when that pin bound a different
    /// variant than the one named.
    Variant { rebound_under: Option<Edition> },
}

/// Resolve a `place ... theme=NAME` reference against the pinned edition.
///
/// The name rule is [`bound_theme_name`]'s, which selects variants as the module-level auto-pick
/// does, so a pin means the same thing wherever the theme was chosen: `theme=shop_bedrock` under
/// `--edition java` never writes Bedrock-only slot values into a Java `.nbt`.
fn resolve_theme_reference<'a>(
    themes: &'a IndexMap<String, ThemeBinding>,
    written: &str,
    edition: Option<Edition>,
) -> ThemeReference<'a> {
    let (logical, written_variant) = strip_edition_suffix(written);
    if !themes
        .keys()
        .any(|name| strip_edition_suffix(name).0 == logical)
    {
        return ThemeReference::Unknown;
    }
    let Some(name) = bound_theme_name(themes.keys().map(String::as_str), written, edition) else {
        // A variant of this logical theme is declared and none fits. Only a
        // pin can rule them all out; unpinned, this is a *suffixed* name
        // nothing declares, a misspelling and not the edition's fault.
        return match edition {
            Some(pinned) => ThemeReference::NoVariantForEdition(pinned),
            None => ThemeReference::Unknown,
        };
    };
    ThemeReference::Bound {
        name,
        spelling: match written_variant {
            None => Spelling::Logical,
            // Only a pin rebinds; unpinned, a suffixed name binds verbatim
            // or not at all.
            Some(_) if name != written => Spelling::Variant {
                rebound_under: edition,
            },
            Some(_) => Spelling::Variant {
                rebound_under: None,
            },
        },
    }
}

/// Resolve a `place ... theme=NAME` to the theme to bind and the sibling
/// slots that may soften its diagnostics, reporting whichever way it failed.
///
/// `None` means the placement cannot be built and the reason is already in
/// `diagnostics`; the caller skips the scope so the lowering pass does not
/// emit an artifact for a placement whose materials were never decided.
pub(super) fn bind_place_theme(
    written: &str,
    span: &Span,
    edition: Option<Edition>,
    themes: &IndexMap<String, ThemeBinding>,
    declared_names: &[String],
    reported_missing: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(String, HashSet<String>)> {
    let (logical, _) = strip_edition_suffix(written);
    match resolve_theme_reference(themes, written, edition) {
        ThemeReference::Unknown => {
            diagnostics.push(unresolved_ref_diag(
                DiagnosticCode::UnresolvedThemeRef,
                &format!("`theme={written}` is not a declared theme"),
                span.clone(),
                written,
                declared_names.iter().map(String::as_str),
            ));
            None
        }
        ThemeReference::NoVariantForEdition(pinned) => {
            // One cause, one report: every `place` naming this theme, and the
            // module-level pick, ask for the same edit to the same `theme`
            // block. Each placement is still refused.
            if reported_missing.insert(logical.to_owned()) {
                diagnostics.push(theme_variant_missing_diag(
                    logical,
                    pinned,
                    themes,
                    span.clone(),
                ));
            }
            None
        }
        ThemeReference::Bound { name, spelling } => {
            let name = name.to_owned();
            if let Spelling::Variant {
                rebound_under: Some(pinned),
            } = spelling
            {
                diagnostics.push(theme_variant_rebound_diag(
                    written,
                    &name,
                    logical,
                    pinned,
                    themes.contains_key(written),
                    span.clone(),
                ));
            }
            // Siblings soften `E_UNRESOLVED_SLOT` only while no edition is
            // picked, as in the top-level scope loop, and only for a logical
            // spelling: an author who named one variant asked about its slots.
            let siblings = if edition.is_none() && spelling == Spelling::Logical {
                sibling_slot_names(themes, logical, &name)
            } else {
                HashSet::new()
            };
            Some((name, siblings))
        }
    }
}

/// Whether any struct or def member anywhere in the module reads a
/// `mat_slot=`.
///
/// The module-level auto-pick binds a theme for every struct and def scope,
/// but a scope only *needs* one to read a slot from: a module that declares
/// only a `_bedrock` theme and reads no slot still builds under
/// `--edition java`, since no block of its output comes from the theme. A
/// member reads a slot through [`Member::read_slot`], so a `mat_slot=` on a
/// role that reads none does not count.
///
/// Sites are not walked, and [`bind_place_theme`] has no such gate: the
/// lockfile records a placement's theme (`Placement::theme`), and its
/// version floor applies once bound whether or not a slot is read
/// (`spec/versioning-editions` "A part may declare its own floor"). A
/// placement built with no theme would record one it was not built from,
/// so such a `place` is refused whatever its def reads.
pub(super) fn any_member_reads_a_slot(ir: &IntentModule) -> bool {
    fn any(members: &[Member]) -> bool {
        members
            .iter()
            .any(|m| m.read_slot().is_some() || any(&m.children.members))
    }
    ir.structs.iter().any(|s| any(&s.members)) || ir.defs.iter().any(|d| any(&d.members))
}

/// Every declared variant of `logical`, in declaration order.
fn declared_variants<'a>(
    themes: &'a IndexMap<String, ThemeBinding>,
    logical: &str,
) -> Vec<&'a str> {
    themes
        .keys()
        .filter(|name| strip_edition_suffix(name).0 == logical)
        .map(String::as_str)
        .collect()
}

/// The pinned edition has no variant of `logical` it can bind.
pub(super) fn theme_variant_missing_diag(
    logical: &str,
    edition: Edition,
    themes: &IndexMap<String, ThemeBinding>,
    span: Span,
) -> Diagnostic {
    let declared = declared_variants(themes, logical);
    let listed = declared.join("`, `");
    Diagnostic {
        code: DiagnosticCode::ThemeVariantMissing,
        span,
        primary: format!(
            "theme `{logical}` has no variant that can bind for `{}`",
            edition.as_str(),
        ),
        notes: vec![
            DiagnosticNote {
                span: None,
                message: format!("the module declares `{listed}`"),
            },
            DiagnosticNote {
                span: None,
                message: format!(
                    "add `theme {logical}_{}:`, or drop the suffix from a variant that is \
                     edition-neutral so `{logical}` binds for either edition",
                    edition.as_str(),
                ),
            },
            DiagnosticNote {
                span: None,
                message: "binding the other edition's variant would route its slot values into \
                          this edition's output, which is the silent substitution \
                          spec/versioning-editions \"Fail-loud and minimum-version inference\" \
                          forbids"
                    .to_owned(),
            },
        ],
        data: None,
    }
}

/// A `theme=` named one variant and the pin bound another.
///
/// `declared` separates the two ways that happens: the named variant exists
/// and the pin preferred its own, or it does not exist and the pin fell back
/// to the unsuffixed theme, which is a plain mistake rather than a choice
/// between variants.
fn theme_variant_rebound_diag(
    written: &str,
    bound: &str,
    logical: &str,
    edition: Edition,
    declared: bool,
    span: Span,
) -> Diagnostic {
    let named = if declared {
        format!("`theme={written}` names one edition's variant")
    } else {
        format!("`theme={written}` is not a declared theme")
    };
    Diagnostic {
        code: DiagnosticCode::ThemeVariantRebound,
        span,
        primary: format!("{named}; this `{}` build bound `{bound}`", edition.as_str()),
        notes: vec![DiagnosticNote {
            span: None,
            message: format!(
                "write `theme={logical}` — the semantic layer stays edition-neutral and lets \
                 the variant follow the build, per spec/versioning-editions \
                 \"Java / Bedrock portability\"",
            ),
        }],
        data: None,
    }
}

/// Slot names present in *sibling* variants of the same logical theme.
///
/// While no edition is pinned, a `mat_slot=X` is not `E_UNRESOLVED_SLOT`
/// when any variant of the picked scope's logical theme declares `X`.
pub(super) fn sibling_slot_names(
    themes: &IndexMap<String, ThemeBinding>,
    logical: &str,
    picked: &str,
) -> HashSet<String> {
    let mut out = HashSet::new();
    for (name, binding) in themes {
        if name == picked {
            continue;
        }
        let (l, _) = strip_edition_suffix(name);
        if l != logical {
            continue;
        }
        for slot in binding.slots.keys() {
            out.insert(slot.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests;
