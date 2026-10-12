//! Which theme a scope binds, judged against the module's [`ThemeBinding`]s. The name-only rule
//! is [`super::theme_variant`]'s.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::edition::Edition;
use crate::error::Span;
use crate::intent::{IntentModule, Member};

use super::binding::ThemeBinding;
use super::theme_variant::{
    bound_theme_name, pick_variant as pick_variant_by_name, strip_edition_suffix,
};
use super::unresolved_ref::unresolved_ref_diag;

/// [`super::theme_variant::pick_variant`] over the module's binding map.
pub(super) fn pick_variant<'a>(
    themes: &'a IndexMap<String, ThemeBinding>,
    logical: &str,
    edition: Option<Edition>,
) -> Option<&'a str> {
    pick_variant_by_name(themes.keys().map(String::as_str), logical, edition)
}

/// [`super::theme_variant::single_logical_theme`] over the module's binding map.
pub(super) fn single_logical_theme(themes: &IndexMap<String, ThemeBinding>) -> Option<String> {
    super::theme_variant::single_logical_theme(themes.keys().map(String::as_str)).map(str::to_owned)
}

/// What a `place ... theme=NAME` reference resolves to under `edition`.
enum ThemeReference<'a> {
    /// Bind this theme.
    Bound {
        /// The theme actually bound — not necessarily the name written.
        name: &'a str,
        /// How the reference was spelled.
        spelling: Spelling,
    },
    /// Variants of this logical theme exist, but none binds under this edition.
    NoVariantForEdition(Edition),
    /// No declared theme matches the name.
    Unknown,
}

/// How a `theme=` reference was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spelling {
    /// No edition suffix: names the logical theme, whose siblings may soften its slot findings.
    Logical,
    /// An edition suffix; `rebound_under` is the pin that bound a different variant instead.
    Variant { rebound_under: Option<Edition> },
}

/// Resolves a `place ... theme=NAME` reference, selecting variants as the module-level pick does.
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
        // Unpinned, this is an undeclared suffixed name: a misspelling, not the edition's fault.
        return match edition {
            Some(pinned) => ThemeReference::NoVariantForEdition(pinned),
            None => ThemeReference::Unknown,
        };
    };
    ThemeReference::Bound {
        name,
        spelling: match written_variant {
            None => Spelling::Logical,
            // Only a pin rebinds; unpinned, a suffixed name binds verbatim or not at all.
            Some(_) if name != written => Spelling::Variant {
                rebound_under: edition,
            },
            Some(_) => Spelling::Variant {
                rebound_under: None,
            },
        },
    }
}

/// The theme `theme=NAME` binds and the sibling slots that soften it, or `None` once reported.
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
            // Said once per logical theme; each placement is still refused.
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
            // Unpinned and logical only: an author who named one variant asked about its slots.
            let siblings = if edition.is_none() && spelling == Spelling::Logical {
                sibling_slot_names(themes, logical, &name)
            } else {
                HashSet::new()
            };
            Some((name, siblings))
        }
    }
}

/// Whether any struct or def member reads a `mat_slot=` through [`Member::read_slot`].
///
/// Sites are not walked: a `place` binds its theme whether or not a slot is read
/// (`spec/versioning-editions` "A part may declare its own floor").
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

/// Slot names declared by the variants of `logical` other than `picked`.
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
