//! Theme-binding resolution over an [`IntentModule`].
//!
//! The single entry point is [`resolve`]. It walks every theme, def, struct,
//! and site once, building a [`Resolution`] that records:
//!
//! - one [`ThemeBinding`] per theme (verbatim slots + per-selector match
//!   tracking),
//! - one [`ScopeResolution`] per struct/def/site (the theme chosen for that
//!   scope plus the resolved binding per member),
//! - and the diagnostics emitted along the way (`E_UNRESOLVED_SLOT`,
//!   `E_UNKNOWN_SLOT_TARGET`, `E_THEME_SELECTOR_UNMATCHED`).
//!
//! Theme selection rule (kept deliberately narrow):
//! - **0 themes in file** → no scope has a theme; every `mat_slot=` is left
//!   unresolved silently (the file may be intended as a library).
//! - **1 theme in file** → every scope picks that theme.
//! - **multiple themes** → struct/def scopes do not auto-pick; the choice
//!   is deferred to the `place ... theme=X` boundary on the site side.
//!
//! Selectors are scoped to their **bound theme only**: a scope with
//! `bound_theme = None` gets no `selector_extras` (even if some theme in
//! the file has a selector that would syntactically match), and
//! `E_THEME_SELECTOR_UNMATCHED` is only reported for themes that bound to
//! at least one scope. This honours the per-theme DI contract from
//! `spec/materials-themes` — a selector belongs to one theme, not
//! the union of all themes in the file.
//!
//! Site `place` lines are followed cross-scope: each `place` is resolved
//! against the referenced `def`'s members with the place's own `theme=`
//! argument applied, and lands under a dedicated `site::SITE::ID` scope
//! key. Missing or duplicate references fail loud with
//! `E_UNRESOLVED_PLACE_REF` / `E_UNRESOLVED_THEME_REF` /
//! `E_DUPLICATE_PLACE_ID` / `E_INVALID_PLACE_ORIGIN`; defs that no site
//! ever references surface as `W_UNUSED_DEF`.
//!
//! `connect` rows resolve at the same layer: the `from.port` / `to.port`
//! `DotRef`s on either side of the `to` keyword are matched against the
//! referenced placement's `def` body, producing one [`ValidatedConnect`]
//! per row. A port id that names no member of that body fails loud with
//! `E_UNRESOLVED_PORT`, which says so when the id is on a member under a
//! `level` and otherwise carries a nearest-match note when one fits the
//! spell cap; a `def` exposing the same `id=` on more than one
//! member raises `E_AMBIGUOUS_PORT`; an absent `path=` triggers
//! `E_MISSING_PATH_MATERIAL`. The walkway voxeliser (under
//! `block_array`) consumes the resolved connects without re-walking the
//! `DotRef`s.
//!
//! Once every scope is resolved, the artifact names they would be written
//! under are compared: two scopes that share one file, ignoring case, are
//! `E_OUTPUT_NAME_COLLISION` (see [`check_output_names`]).
//!
//! The returned [`Resolution::diagnostics`] is in **resolver-emission
//! order**, not sorted by source span. The `check::check` pipeline runs
//! its findings through `DiagnosticSink::into_sorted` after merging, so
//! sorting here too would be redundant work.

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
///
/// Carried as one struct rather than threaded as five positional
/// arguments, for the reason [`crate::block_array`]'s `StructCtx` and
/// `cairn-lang-redstone`'s `LoweringCtx` give for the same shape: a new
/// per-resolution field lands as one field instead of touching every
/// signature between [`resolve`] and the walk that needs it.
///
/// It is also where the two say-it-once ledgers sit together. They are one
/// device at two levels — a cause with several emission sites, reported
/// once between them — and keeping them apart made that hard to see.
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
    /// A def body is resolved once as its own scope and once more per
    /// `place` that instantiates it, so without this the same finding is
    /// reported once per placement — plus once more when the module can
    /// auto-pick a theme for the def's own scope. That count is a fact
    /// about the placement list rather than about the source the author
    /// has to fix.
    ///
    /// The triple is the finding's identity: its span is the member's, and
    /// its message names the slot and the theme. Two placements naming two
    /// themes resolve the same `mat_slot=` against two slot maps and earn
    /// two findings, and two members reading one missing slot earn two as
    /// well; only a repeat of all three says the same thing twice. The
    /// slot name is redundant while [`Member::mat_slot`] holds at most one
    /// name, which is a fact about the member rather than about the rule.
    ///
    /// Written where a diagnostic is pushed, never where a body is walked.
    /// Two resolutions of one body can bind the same theme and still judge
    /// a slot differently — sibling-variant softening applies to a
    /// reference that names the logical theme and not to one that names a
    /// variant — so a resolution that said nothing must leave the next one
    /// free to speak.
    diagnosed: &'a mut HashSet<(usize, String, String)>,
}

/// Resolve theme bindings over the given Intent IR.
///
/// Always returns a [`Resolution`] — every distinct theme name appears in
/// `.themes`, every distinct scope key appears in `.scopes`, and any
/// problems encountered are collected into `.diagnostics`.
///
/// INVARIANT(`FIRST_BINDING_WINS`): when two declarations produce the
/// same binding key, the first one binds and the later ones are skipped.
///
/// The binding key is not always the item name. It is the name for
/// `theme`, `struct::NAME` for a struct and `def::NAME` for a def — but
/// `site::NAME::PLACE_ID` for a placement, which includes the place id.
/// Two `site` blocks of one name therefore do not shadow each other:
/// their places land in one shared namespace and every place with a
/// distinct `id=` binds. Only a place id repeated across those blocks
/// collides, and there the first wins like everywhere else.
///
/// The direction has to be picked because the collision is real, and
/// picking it uniformly is what keeps the resolution readable — a `def`
/// whose placement is sized from one body and whose members resolve
/// from another is a wrong build, not a stylistic difference. `first`
/// is the one `defs.iter().find` (the `place use=` lookup) already
/// takes, and the one `E_DUPLICATE_ITEM` tells the author about.
/// `tests/check/duplicate.rs` records the shape that made the choice
/// necessary.
///
/// INVARIANT(upstream-diagnosed): a skipped binding is not reported from
/// here. `check::duplicate` has already pushed `E_DUPLICATE_ITEM` into
/// the same sink for every repeated name, so a user running `cairn
/// check` — or any build command, all of which gate on it — sees a
/// position-anchored signal. Re-pushing would report one mistake twice,
/// and the pass that owns it can anchor on the name token, which the IR
/// no longer carries. This is the same division of labour the silent
/// arms in `resolve_connect_row` follow, and it is pinned the same
/// way, by `tests/resolve/silent_skip_arms.rs` for the resolver-only path
/// and `tests/check/duplicate.rs` for the full pipeline. A
/// `debug_assert` on the skip is deliberately absent: the condition is
/// ordinary malformed input, and aborting on it would take down the LSP
/// on a file the author is halfway through editing.
///
/// Skipping is only about the *binding*. Each duplicate body is still
/// walked and still contributes its own body-local diagnostics, so an
/// author fixing the collision sees the problems inside both bodies in
/// the same run.
///
/// The `edition` argument drives per-edition theme-variant selection
/// (`spec/versioning-editions` "Java / Bedrock portability", hierarchy #2):
/// when the file declares two themes whose names share a base and differ only
/// by an `_java` / `_bedrock` suffix, `Some(Edition::Java)` picks the `_java`
/// variant and `Some(Edition::Bedrock)` picks the `_bedrock` variant. `None`
/// is the "no edition has been picked yet" case (typically `cairn check`
/// without `--edition`) — the resolver unions slot names across variants of
/// the same logical theme so a `mat_slot=NAME` reference that only one variant
/// declares does not spuriously fire `E_UNRESOLVED_SLOT`.
#[must_use]
pub fn resolve(ir: &IntentModule, edition: Option<Edition>) -> Resolution {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut themes: IndexMap<String, ThemeBinding> = IndexMap::new();
    // Every declared body, including one whose name lost the binding.
    // The map holds one entry per name, so it is not the right thing to
    // walk for findings that are local to a body — a bad slot value in a
    // shadowed `theme` is still a bad slot value, and the author should
    // not have to fix the name first to be told about it.
    let mut declared: Vec<ThemeBinding> = Vec::with_capacity(ir.themes.len());
    for theme in &ir.themes {
        let binding = build_theme_binding(theme);
        declared.push(binding.clone());
        // First-write-wins on duplicate names, as everywhere else in this
        // function (see `FIRST_BINDING_WINS`). Keeping the map
        // insertion-ordered means downstream consumers see the same theme
        // order the source declared.
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
            // A refusal here is the module-level half of the same finding
            // the site path reports per `place`: the module declares this
            // theme and the pin can bind none of its variants. Left
            // unreported it was silent — `bound_theme` stayed `None`, every
            // `mat_slot=` skipped the branch that would have named a theme,
            // and the build wrote the requested extent out of air.
            //
            // Which is also the reason it is conditional on a `mat_slot=`
            // existing to be starved. A module that declares one edition's
            // theme and never reads a slot from it emits no air, so a pin
            // the theme cannot satisfy costs that module nothing — and the
            // build it would have produced is byte-identical either way.
            //
            // `reported_missing` carries the finding across to the site
            // loop, where every `place` naming this theme would otherwise
            // repeat it verbatim against a different span.
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
                // Sibling slots only gate `E_UNRESOLVED_SLOT` under the
                // no-edition-yet case — a Some(edition) compile binds one
                // variant authoritatively and cross-variant slots must not
                // soften diagnostics for it.
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
    // A def is a template, and this walk resolves it against the theme the
    // module auto-picks even though a `place` below will walk the same body
    // under the placement's own theme. It stays: a file of defs and no
    // `site` is a file worth checking, and a placement can be abandoned
    // before it reaches the body — an absent `theme=`, an origin that does
    // not resolve — which leaves this the only walk able to report what is
    // inside. Where a placement does bind the same theme, `diagnosed` makes
    // the second report cost nothing.
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

/// IR-side key for a single `place` inside a `site`.
///
/// Embedding the site name (`site::hamlet::home1` rather than
/// `place::home1`) keeps the keys of one `id=` placed in two sites apart,
/// so each placement resolves and lowers on its own. The file a placement
/// is written to is named by [`artifact_stem`](crate::ids::artifact_stem),
/// after its `id=` alone, so those two placements still name one file;
/// that is `E_OUTPUT_NAME_COLLISION`.
#[must_use]
pub fn place_scope_key(site_name: &str, place_id: &str) -> String {
    format!("site::{site_name}::{place_id}")
}

/// Resolve one struct / def / place scope against a specific picked
/// theme name.
///
/// `picked_theme_name` is a fully-qualified theme name (e.g. `shop_java`),
/// not a logical name — variant selection is the caller's job. `sibling_slots`
/// carries the union of slot names in sibling variants of the same logical
/// theme, used to gate `E_UNRESOLVED_SLOT` under the edition = `None` case;
/// pass an empty set when no siblings apply (single-variant file, or a
/// site-side explicit `theme=X` binding).
pub(super) fn resolve_struct_or_def(
    members: &[Member],
    picked_theme_name: Option<&str>,
    sibling_slots: &HashSet<String>,
    ctx: &mut ResolveCtx<'_>,
) -> ScopeResolution {
    // Structural invariant: every name that reaches here came out of
    // `themes.keys()`. The module-level auto-pick gets it from
    // `pick_variant`, which builds its candidates by iterating those keys;
    // the site-side branch gets it from `bind_place_theme`, which returns
    // only what `resolve_theme_reference` read from the same map — a
    // user-supplied `theme=X` label that names nothing there is refused
    // up-slope instead. Reaching the `None` arm of `themes.get` would
    // mean one of those guarantees broke — asymmetric with `validate_port`,
    // which uses the same shape of loud fallback. `debug_assert!(false)`
    // trips in dev / test builds; a release build silently degrades to
    // the unbound-theme path rather than panicking on user data.
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

/// Walk `members` under the scope's bound theme, filling `out` and
/// pushing whatever the walk finds into `diagnostics`.
///
/// `bound` carries the theme's name and its slot map together because a
/// scope has both or neither: an unbound scope resolves no `mat_slot=` and
/// matches no selector, and splitting the pair let a caller ask for one
/// half.
///
fn resolve_members(
    members: &[Member],
    bound: Option<(&str, &IndexMap<String, ValueWithSpan>)>,
    sibling_slots: &HashSet<String>,
    out: &mut ScopeResolution,
    ctx: &mut ResolveCtx<'_>,
) {
    for member in members {
        let mut binding = ResolvedMemberBinding::default();

        // 1. mat_slot resolution against the scope's applied theme.
        //    A slot the picked variant does not declare is still "known"
        //    when a sibling variant of the same logical theme declares it,
        //    which suppresses `E_UNRESOLVED_SLOT` for the edition-variant
        //    themes of `spec/versioning-editions` "Java / Bedrock
        //    portability" under `cairn check` without an `--edition` pin.
        //    `slot_value` stays `None` in that case — the concrete binding is
        //    edition-specific and comes into scope only once the compile picks
        //    a variant.
        //
        //    INVARIANT(upstream-diagnosed): a `mat_slot=` on a role whose
        //    `MemberRole::unread_arguments` lists it is not looked up at
        //    all, and nothing is pushed for it here. Inside the top-level
        //    `check` pipeline, `check::arguments` has already pushed
        //    `W_IGNORED_ARGUMENT` for it into the same sink, and a slot name
        //    the theme lacks changes nothing on such a role. A library
        //    caller that runs `resolve` without `check` gets no signal;
        //    `tests/resolve/silent_skip_arms.rs` carries the arm in its matrix.
        if let Some(slot_name) = member.read_slot()
            && let Some((tname, slots)) = bound
        {
            match slots.get(slot_name) {
                Some(v) => binding.slot_value = Some(v.clone()),
                None if sibling_slots.contains(slot_name) => {}
                // INVARIANT(already-reported): a repeat of the same
                // finding is dropped, never the finding itself — an
                // identical triple can only be here because a copy is
                // already in `ctx.diagnostics`, which is built and
                // returned by the same call. See
                // [`ResolveCtx::diagnosed`]; `tests/resolve/silent_skip_arms.rs`
                // carries it in the matrix of resolver arms that drop
                // something without reporting it.
                None => {
                    let said = (member.span.start, slot_name.to_owned(), tname.to_owned());
                    if ctx.diagnosed.insert(said) {
                        ctx.diagnostics
                            .push(unresolved_slot_diag(slot_name, tname, member, slots));
                    }
                }
            }
        }

        // 2. Selector matching — scoped to the bound theme only. A scope
        //    with `bound_theme=None` (multi-theme file, no auto-pick)
        //    gets no selector_extras, matching the per-theme DI contract.
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

        // 3. Recurse into nested members (e.g. `level y=...` blocks).
        let MemberBody {
            members: children, ..
        } = &member.children;
        if !children.is_empty() {
            resolve_members(children, bound, sibling_slots, out, ctx);
        }
    }
}
