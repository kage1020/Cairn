//! Theme-binding resolution over an [`IntentModule`]: [`resolve`] and the
//! per-scope walk behind it.
//!
//! Theme selection is deliberately narrow. With no theme in the file, no
//! scope has one and every `mat_slot=` stays unresolved silently, since the
//! file may be a library. With one logical theme, every scope picks it. With
//! several, a `struct` or `def` picks none, and the choice waits for a
//! `place ... theme=X`.
//!
//! A selector belongs to one theme (`spec/materials-themes`), so a scope
//! with no bound theme gets no `selector_extras`, and
//! `E_THEME_SELECTOR_UNMATCHED` is reported only for a theme some scope
//! applied.
//!
//! Each `place` resolves the referenced `def`'s members under the place's
//! own `theme=`, in a `site::SITE::ID` scope, and each `connect` row
//! resolves into a [`ValidatedConnect`]. Once every scope is resolved,
//! [`check_output_names`] compares the artifact names they would be written
//! under.

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

/// The state every scope resolution reads from and adds to, including the
/// two say-it-once ledgers: a cause with several emission sites, reported
/// once between them.
pub(super) struct ResolveCtx<'a> {
    /// Theme bindings by name. Mutable because selector matching records
    /// the members each selector bound to.
    pub(super) themes: &'a mut IndexMap<String, ThemeBinding>,
    /// Theme names some scope applied, so [`check_unmatched_selectors`] can
    /// tell "matched nothing" apart from "never applied to anything".
    applied_themes: &'a mut HashSet<String>,
    /// Everything the resolution found, in the order it found it.
    pub(super) diagnostics: &'a mut Vec<Diagnostic>,
    /// Logical theme names already reported as unbindable under the pin, so
    /// the module-level pick and every `place` naming one say it once
    /// between them rather than once each.
    pub(super) reported_missing: &'a mut HashSet<String>,
    /// `(member position, slot name, theme name)` triples an
    /// `E_UNRESOLVED_SLOT` has already been pushed for.
    ///
    /// A def body is resolved as its own scope and again per `place`, so
    /// without this one finding would repeat per placement. The triple is
    /// the finding's identity: two placements naming two themes, or two
    /// members reading one missing slot, earn two findings.
    ///
    /// Written where a diagnostic is pushed, never where a body is walked:
    /// two resolutions binding the same theme can still judge a slot
    /// differently (sibling-variant softening applies only to a reference
    /// naming the logical theme), so a resolution that said nothing must
    /// leave the next free to speak.
    diagnosed: &'a mut HashSet<(usize, String, String)>,
}

/// Resolve theme bindings over the given Intent IR.
///
/// Always returns a [`Resolution`]: every distinct theme name is in
/// `themes`, every distinct scope key in `scopes`, and every problem in
/// `diagnostics`, in emission order.
///
/// INVARIANT(`FIRST_BINDING_WINS`): when two declarations produce the same
/// binding key, the first binds and the rest are skipped, as the
/// `place use=` lookup does and as `E_DUPLICATE_ITEM` tells the author. A
/// `def` sized from one body and resolved from another would be a wrong
/// build. The key is the name for a `theme`, `struct::NAME` or `def::NAME`,
/// and `site::NAME::PLACE_ID` for a placement, so two `site` blocks of one
/// name share a namespace and only a repeated place id collides.
///
/// INVARIANT(upstream-diagnosed): a skipped binding is not reported here;
/// `check::duplicate` reports it, anchored on the name token the IR no
/// longer carries. There is no `debug_assert` on the skip: it is ordinary
/// malformed input, and aborting would take down the language server
/// mid-edit. Each duplicate body is still walked for its own diagnostics.
///
/// `edition` selects theme variants (`spec/versioning-editions` "Java /
/// Bedrock portability"): of two themes named alike but for a `_java` /
/// `_bedrock` suffix, `Some(edition)` picks that edition's. `None` (no
/// `--edition` yet) unions slot names across a logical theme's variants, so
/// a `mat_slot=` only one variant declares does not raise
/// `E_UNRESOLVED_SLOT`.
#[must_use]
pub fn resolve(ir: &IntentModule, edition: Option<Edition>) -> Resolution {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut themes: IndexMap<String, ThemeBinding> = IndexMap::new();
    // Every declared body, including one whose name lost the binding, so
    // a shadowed theme's bad slot value is still reported.
    let mut declared: Vec<ThemeBinding> = Vec::with_capacity(ir.themes.len());
    for theme in &ir.themes {
        let binding = build_theme_binding(theme);
        declared.push(binding.clone());
        themes.entry(binding.name.clone()).or_insert(binding);
    }

    let single_logical = single_logical_theme(&themes);
    let mut scopes: IndexMap<String, ScopeResolution> = IndexMap::new();
    let mut applied_themes: HashSet<String> = HashSet::new();
    // See [`ResolveCtx::reported_missing`].
    let mut reported_missing: HashSet<String> = HashSet::new();
    // See [`ResolveCtx::diagnosed`].
    let mut diagnosed: HashSet<(usize, String, String)> = HashSet::new();

    let (auto_picked, auto_siblings) = match single_logical.as_deref() {
        Some(logical) => {
            let picked = pick_variant(&themes, logical, edition).map(str::to_owned);
            // The module-level half of the finding the site path reports
            // per `place`: no variant fits the pin, so every `mat_slot=`
            // would build air. Raised only when some member reads a slot,
            // since otherwise the pin costs nothing. `reported_missing`
            // stops each `place` naming the theme from repeating it.
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
                // Only without a pin: a pinned build binds one variant, and
                // other variants' slots must not soften its findings.
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
    // Each `place` walks a def again under its own theme, but a file of
    // defs and no `site` is worth checking, and a placement can be
    // abandoned before its body (no `theme=`, an unresolvable origin).
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
///
/// The site name keeps one `id=` placed in two sites apart in resolution
/// and lowering. The artifact is named by
/// [`artifact_stem`](crate::ids::artifact_stem) after the `id=` alone, so
/// those two placements still name one file: `E_OUTPUT_NAME_COLLISION`.
#[must_use]
pub fn place_scope_key(site_name: &str, place_id: &str) -> String {
    format!("site::{site_name}::{place_id}")
}

/// Resolve one struct / def / place scope against a specific picked
/// theme name.
///
/// `picked_theme_name` is a variant's own name (`shop_java`), not a logical
/// one: variant selection is the caller's job. `sibling_slots` is the union
/// of slot names in the other variants of the same logical theme, which
/// keeps `E_UNRESOLVED_SLOT` quiet when no edition is pinned; it is empty
/// when no siblings apply.
pub(super) fn resolve_struct_or_def(
    members: &[Member],
    picked_theme_name: Option<&str>,
    sibling_slots: &HashSet<String>,
    ctx: &mut ResolveCtx<'_>,
) -> ScopeResolution {
    // Structural invariant: every name reaching here came from
    // `themes.keys()`, through `pick_variant` or `bind_place_theme`, which
    // refuses an unknown `theme=X` earlier. A release build degrades to the
    // unbound path rather than panicking on user data.
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

/// Walk `members` under the scope's bound theme, filling `out` and pushing
/// whatever the walk finds into `ctx`.
///
/// `bound` pairs the theme's name with its slot map because a scope has
/// both or neither.
fn resolve_members(
    members: &[Member],
    bound: Option<(&str, &IndexMap<String, ValueWithSpan>)>,
    sibling_slots: &HashSet<String>,
    out: &mut ScopeResolution,
    ctx: &mut ResolveCtx<'_>,
) {
    for member in members {
        let mut binding = ResolvedMemberBinding::default();

        // A slot only a sibling variant declares raises nothing and binds
        // nothing until a pin picks a variant.
        //
        // INVARIANT(upstream-diagnosed): a `mat_slot=` its role does not
        // read is not looked up; `check::arguments` reports it as
        // `W_IGNORED_ARGUMENT`, and a library caller running `resolve`
        // alone gets no signal (`tests/resolve/silent_skip_arms.rs`).
        if let Some(slot_name) = member.read_slot()
            && let Some((tname, slots)) = bound
        {
            match slots.get(slot_name) {
                Some(v) => binding.slot_value = Some(v.clone()),
                None if sibling_slots.contains(slot_name) => {}
                // INVARIANT(already-reported): only a repeat is dropped; an
                // identical triple means a copy is already in
                // `ctx.diagnostics` (see `ResolveCtx::diagnosed`).
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
