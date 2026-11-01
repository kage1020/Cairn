//! [`resolve`]: with one logical theme every scope picks it; with several, a `struct` or `def`
//! picks none and waits for a `place ... theme=X`.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::diagnostic::Diagnostic;
use crate::edition::Edition;
use crate::intent::{DefIr, IntentModule, ThemeIr};

use super::binding::{SelectorMatch, ThemeBinding};
use super::output_names::check_output_names;
use super::resolution::{Resolution, ScopeResolution, ValidatedConnect, struct_key};
use super::scope::{ResolveCtx, resolve_struct_or_def};
use super::site::{check_unused_defs, resolve_site_placements};
use super::slots::{check_slot_targets, check_unmatched_selectors};
use super::theme_reference::{
    any_member_reads_a_slot, pick_variant, sibling_slot_names, single_logical_theme,
    theme_variant_missing_diag,
};

/// The theme bindings, placements and `connect` rows of `ir` under `edition`'s theme variants.
///
/// INVARIANT(`FIRST_BINDING_WINS`): of two declarations with one binding key, the first binds.
/// INVARIANT(upstream-diagnosed): `check::duplicate` reports the others.
#[must_use]
pub fn resolve(ir: &IntentModule, edition: Option<Edition>) -> Resolution {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut themes: IndexMap<String, ThemeBinding> = IndexMap::new();
    // Every declared body, shadowed ones too, so a shadowed theme's bad slot value is reported.
    let mut declared: Vec<ThemeBinding> = Vec::with_capacity(ir.themes.len());
    for theme in &ir.themes {
        let binding = build_theme_binding(theme);
        declared.push(binding.clone());
        themes.entry(binding.name.clone()).or_insert(binding);
    }

    let single_logical = single_logical_theme(&themes);
    let mut scopes: IndexMap<String, ScopeResolution> = IndexMap::new();
    let mut applied_themes: HashSet<String> = HashSet::new();
    let mut reported_missing: HashSet<String> = HashSet::new();
    let mut diagnosed: HashSet<(usize, String, String)> = HashSet::new();

    let (auto_picked, auto_siblings) = match single_logical.as_deref() {
        Some(logical) => {
            let picked = pick_variant(&themes, logical, edition).map(str::to_owned);
            // Only when some member reads a slot, since otherwise the pin costs nothing.
            if picked.is_none()
                && let Some(pinned) = edition
                && let Some(first) = themes.values().next()
                && any_member_reads_a_slot(ir)
            {
                reported_missing.insert(logical.to_owned());
                diagnostics.push(theme_variant_missing_diag(
                    logical,
                    pinned,
                    &themes,
                    first.span.clone(),
                ));
            }
            let siblings = match (&picked, edition) {
                // Only unpinned: a pinned build's findings ignore other variants' slots.
                (Some(name), None) => sibling_slot_names(&themes, logical, name),
                _ => HashSet::new(),
            };
            (picked, siblings)
        }
        None => (None, HashSet::new()),
    };

    let mut ctx = ResolveCtx {
        themes: &mut themes,
        applied_themes: &mut applied_themes,
        diagnostics: &mut diagnostics,
        reported_missing: &mut reported_missing,
        diagnosed: &mut diagnosed,
    };

    for s in &ir.structs {
        let resolution =
            resolve_struct_or_def(&s.members, auto_picked.as_deref(), &auto_siblings, &mut ctx);
        scopes.entry(struct_key(s)).or_insert(resolution);
    }
    // Each `place` walks its def again, but a file with no `site` is still worth checking;
    // `diagnosed` drops the repeats.
    for d in &ir.defs {
        let resolution =
            resolve_struct_or_def(&d.members, auto_picked.as_deref(), &auto_siblings, &mut ctx);
        scopes.entry(def_key(d)).or_insert(resolution);
    }
    let mut used_defs: HashSet<String> = HashSet::new();
    let mut connects: Vec<ValidatedConnect> = Vec::new();
    for site in &ir.sites {
        resolve_site_placements(
            site,
            &ir.defs,
            edition,
            &mut scopes,
            &mut used_defs,
            &mut connects,
            &mut ctx,
        );
    }
    check_unused_defs(&ir.defs, &used_defs, &mut diagnostics);
    check_output_names(ir, &scopes, &connects, &mut diagnostics);

    check_slot_targets(&declared, &mut diagnostics);
    check_unmatched_selectors(&themes, &applied_themes, &mut diagnostics);

    Resolution {
        themes,
        scopes,
        connects,
        diagnostics,
    }
}

fn build_theme_binding(theme: &ThemeIr) -> ThemeBinding {
    let selectors = theme
        .selectors
        .iter()
        .map(|s| SelectorMatch {
            keyword: s.keyword.clone(),
            attrs: s.attrs.clone(),
            bindings: s.bindings.clone(),
            matched_member_spans: Vec::new(),
            source_span: s.span.clone(),
        })
        .collect();
    ThemeBinding {
        name: theme.name.clone(),
        slots: theme.slots.clone(),
        selectors,
        span: theme.span.clone(),
    }
}

fn def_key(d: &DefIr) -> String {
    format!("def::{}", d.name)
}
