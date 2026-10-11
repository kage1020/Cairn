//! [`resolve`] and the per-scope walk behind it. With one logical theme every scope picks it; with
//! several, a `struct` or `def` picks none and waits for a `place ... theme=X`.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::diagnostic::Diagnostic;
use crate::edition::Edition;
use crate::intent::{DefIr, IntentModule, Member, MemberBody, StructIr, ThemeIr, ValueWithSpan};

use super::binding::{SelectorMatch, ThemeBinding};
use super::output_names::check_output_names;
use super::resolution::{Resolution, ResolvedMemberBinding, ScopeResolution, ValidatedConnect};
use super::selector::selector_matches;
use super::site::{check_unused_defs, resolve_site_placements};
use super::slots::{check_slot_targets, check_unmatched_selectors, unresolved_slot_diag};
use super::theme_reference::{
    any_member_reads_a_slot, pick_variant, sibling_slot_names, single_logical_theme,
    theme_variant_missing_diag,
};

/// The state every scope resolution reads from and adds to.
pub(super) struct ResolveCtx<'a> {
    /// Theme bindings by name; selector matching records its members here.
    pub(super) themes: &'a mut IndexMap<String, ThemeBinding>,
    /// Theme names some scope applied.
    applied_themes: &'a mut HashSet<String>,
    /// Everything the resolution found, in the order it found it.
    pub(super) diagnostics: &'a mut Vec<Diagnostic>,
    /// Logical themes already reported as unbindable under the pin, so each is said once.
    pub(super) reported_missing: &'a mut HashSet<String>,
    /// `(member position, slot, theme)` triples already reported as `E_UNRESOLVED_SLOT`. Written
    /// only where one is pushed: another walk of the same body may judge the slot differently.
    diagnosed: &'a mut HashSet<(usize, String, String)>,
}

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

pub(super) fn struct_key(s: &StructIr) -> String {
    format!("struct::{}", s.name)
}

fn def_key(d: &DefIr) -> String {
    format!("def::{}", d.name)
}

/// IR-side key for a single `place` inside a `site`, `site::SITE::ID`.
#[must_use]
pub fn place_scope_key(site_name: &str, place_id: &str) -> String {
    format!("site::{site_name}::{place_id}")
}

/// One struct, def or place scope resolved under `picked_theme_name`, a variant's own name.
pub(super) fn resolve_struct_or_def(
    members: &[Member],
    picked_theme_name: Option<&str>,
    sibling_slots: &HashSet<String>,
    ctx: &mut ResolveCtx<'_>,
) -> ScopeResolution {
    // INVARIANT(structural): every name reaching here came from `themes.keys()`.
    let bound = if let Some(name) = picked_theme_name {
        if let Some(t) = ctx.themes.get(name) {
            Some((name.to_owned(), t.slots.clone()))
        } else {
            debug_assert!(
                false,
                "resolve_struct_or_def: picked theme `{name}` is not in the themes map; \
                 pick_variant should only surface names it read from themes.keys()",
            );
            None
        }
    } else {
        None
    };

    if let Some((name, _)) = &bound {
        ctx.applied_themes.insert(name.clone());
    }

    let mut resolution = ScopeResolution {
        bound_theme: bound.as_ref().map(|(name, _)| name.clone()),
        members: IndexMap::new(),
    };
    resolve_members(
        members,
        bound.as_ref().map(|(name, slots)| (name.as_str(), slots)),
        sibling_slots,
        &mut resolution,
        ctx,
    );
    resolution
}

fn resolve_members(
    members: &[Member],
    bound: Option<(&str, &IndexMap<String, ValueWithSpan>)>,
    sibling_slots: &HashSet<String>,
    out: &mut ScopeResolution,
    ctx: &mut ResolveCtx<'_>,
) {
    for member in members {
        let mut binding = ResolvedMemberBinding::default();

        // INVARIANT(upstream-diagnosed): `check::arguments` reports a `mat_slot=` its role ignores.
        if let Some(slot_name) = member.read_slot()
            && let Some((tname, slots)) = bound
        {
            match slots.get(slot_name) {
                Some(v) => binding.slot_value = Some(v.clone()),
                None if sibling_slots.contains(slot_name) => {}
                // INVARIANT(already-reported): an identical triple is already in `ctx.diagnostics`.
                None => {
                    let said = (member.span.start, slot_name.to_owned(), tname.to_owned());
                    if ctx.diagnosed.insert(said) {
                        ctx.diagnostics
                            .push(unresolved_slot_diag(slot_name, tname, member, slots));
                    }
                }
            }
        }

        if let Some((tname, _)) = bound
            && let Some(theme_binding) = ctx.themes.get_mut(tname)
        {
            for sel in &mut theme_binding.selectors {
                if selector_matches(sel, member) {
                    sel.matched_member_spans.push(member.span.clone());
                    for (k, v) in &sel.bindings {
                        binding.selector_extras.insert(k.clone(), v.clone());
                    }
                }
            }
        }

        out.members.insert(member.span.start, binding);

        let MemberBody {
            members: children, ..
        } = &member.children;
        if !children.is_empty() {
            resolve_members(children, bound, sibling_slots, out, ctx);
        }
    }
}
