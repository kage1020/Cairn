//! The per-scope walk behind [`resolve`](super::resolve()), one struct, def or place at a time.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::diagnostic::Diagnostic;
use crate::furniture::{self, Placed};
use crate::intent::{DefIr, Member, MemberBody, MemberRole, ValueWithSpan, levels_under};

use super::binding::{ThemeBinding, TokenKind, classify_token, member_form};
use super::resolution::{ResolvedMemberBinding, ScopeResolution};
use super::selector::selector_matches;
use super::slots::unresolved_slot_diag;

/// The state every scope resolution reads from and adds to.
pub(super) struct ResolveCtx<'a> {
    /// The file's own defs, which a body's `place` names ahead of the furniture library's.
    pub(super) defs: &'a [DefIr],
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
        placed: IndexMap::new(),
    };
    let bound = bound.as_ref().map(|(name, slots)| (name.as_str(), slots));
    resolve_members(members, &[], bound, sibling_slots, &mut resolution, ctx);
    resolve_placed_defs(members, bound, sibling_slots, &mut resolution, ctx);
    resolution
}

/// The name a library def's members are resolved under, which no theme of a file can have.
const LIBRARY_THEME: &str = "the furniture library's theme";

/// Resolve the `def` each `place` among `members` builds, under the scope's theme, into
/// `out.placed`; a library def falls back on the library's theme for a slot that one leaves
/// unbound (`spec/components-editing-sites` "A `def` placed in a body").
fn resolve_placed_defs(
    members: &[Member],
    bound: Option<(&str, &IndexMap<String, ValueWithSpan>)>,
    sibling_slots: &HashSet<String>,
    out: &mut ScopeResolution,
    ctx: &mut ResolveCtx<'_>,
) {
    for member in members {
        resolve_placed_defs(&member.children.members, bound, sibling_slots, out, ctx);
        if !matches!(member.role, MemberRole::Place) {
            continue;
        }
        let Some(def) = member
            .used_def()
            .and_then(|name| furniture::placed(name, ctx.defs))
        else {
            continue;
        };
        let mut placed = ScopeResolution {
            bound_theme: out.bound_theme.clone(),
            members: IndexMap::new(),
            placed: IndexMap::new(),
        };
        match def {
            Placed::File(def) => {
                resolve_members(&def.members, &[], bound, sibling_slots, &mut placed, ctx);
            }
            Placed::Library(def) => {
                let slots = library_slots(member, bound.map(|(_, slots)| slots));
                let none = HashSet::new();
                resolve_members(
                    &def.members,
                    &[],
                    Some((LIBRARY_THEME, &slots)),
                    &none,
                    &mut placed,
                    ctx,
                );
            }
        }
        out.placed.insert(member.span.start, placed);
    }
}

/// The library theme's slots, each one `theme` binds replaced by its binding, and each other
/// spanned at the `place` line that reads it.
fn library_slots(
    place: &Member,
    theme: Option<&IndexMap<String, ValueWithSpan>>,
) -> IndexMap<String, ValueWithSpan> {
    furniture::theme()
        .slots
        .iter()
        .map(|(slot, value)| {
            let bound = theme.and_then(|theme| theme.get(slot)).cloned();
            let value = bound.unwrap_or_else(|| {
                let mut at_the_line = value.clone();
                at_the_line.span.clone_from(&place.span);
                at_the_line.value.span.clone_from(&place.span);
                at_the_line
            });
            (slot.clone(), value)
        })
        .collect()
}

/// Resolve `members`, which stand under the `level`s whose ids are `levels`.
fn resolve_members(
    members: &[Member],
    levels: &[&str],
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
                Some(v) => {
                    binding.refused = classify_token(&v.value) == TokenKind::NotAToken
                        && member_form(&v.value).is_none();
                    binding.slot_value = Some(v.clone());
                    if let Some(arguments) = ctx
                        .themes
                        .get(tname)
                        .and_then(|theme| theme.slot_arguments.get(slot_name))
                    {
                        binding.slot_arguments.clone_from(arguments);
                    }
                }
                None if sibling_slots.contains(slot_name) => {}
                // INVARIANT(already-reported): an identical triple is already in `ctx.diagnostics`.
                None => {
                    binding.refused = true;
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
                if selector_matches(sel, member, levels) {
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
            let under = levels_under(levels, member);
            resolve_members(children, &under, bound, sibling_slots, out, ctx);
        }
    }
}
