//! The per-scope walk behind [`resolve`](super::resolve()), one struct, def or place at a time.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::diagnostic::Diagnostic;
use crate::intent::{Member, MemberBody, ValueWithSpan};

use super::binding::ThemeBinding;
use super::resolution::{ResolvedMemberBinding, ScopeResolution};
use super::selector::selector_matches;
use super::slots::unresolved_slot_diag;

/// The state every scope resolution reads from and adds to.
pub(super) struct ResolveCtx<'a> {
    /// Theme bindings by name; selector matching records its members here.
    pub(super) themes: &'a mut IndexMap<String, ThemeBinding>,
    /// Theme names some scope applied.
    pub(super) applied_themes: &'a mut HashSet<String>,
    /// Everything the resolution found, in the order it found it.
    pub(super) diagnostics: &'a mut Vec<Diagnostic>,
    /// Logical themes already reported as unbindable under the pin, so each is said once.
    pub(super) reported_missing: &'a mut HashSet<String>,
    /// `(member position, slot, theme)` triples already reported as `E_UNRESOLVED_SLOT`. Written
    /// only where one is pushed: another walk of the same body may judge the slot differently.
    pub(super) diagnosed: &'a mut HashSet<(usize, String, String)>,
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
                        ctx.diagnostics.push(unresolved_slot_diag(
                            slot_name,
                            tname,
                            member,
                            slots,
                            sibling_slots,
                        ));
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
