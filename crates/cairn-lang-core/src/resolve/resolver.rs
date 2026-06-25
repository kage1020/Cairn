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
//! Theme selection rule (kept deliberately narrow for M2-PR3):
//! - **0 themes in file** → no scope has a theme; every `mat_slot=` is left
//!   unresolved silently (the file may be intended as a library).
//! - **1 theme in file** → every scope picks that theme.
//! - **multiple themes** → struct/def scopes do not auto-pick (which one
//!   applies is decided at the `place ... theme=X` boundary in M3).
//!
//! Selectors are scoped to their **bound theme only**: a scope with
//! `bound_theme = None` gets no `selector_extras` (even if some theme in
//! the file has a selector that would syntactically match), and
//! `E_THEME_SELECTOR_UNMATCHED` is only reported for themes that bound to
//! at least one scope. This honours the per-theme DI contract from
//! `spec/materials-themes.md` §7 — a selector belongs to one theme, not
//! the union of all themes in the file.
//!
//! Site `place` lines are followed cross-scope: each `place` is resolved
//! against the referenced `def`'s members with the place's own `theme=`
//! argument applied, and lands under a dedicated `site::SITE::ID` scope
//! key. Missing or duplicate references fail loud with
//! `E_UNRESOLVED_PLACE_REF` / `E_UNRESOLVED_THEME_REF` /
//! `E_DUPLICATE_PLACE_ID` / `E_INVALID_PLACE_ORIGIN`; defs that no site
//! ever references surface as `W_UNUSED_DEF`. Walkway (`connect`) bodies
//! are passed through to the lowering pass without port validation — the
//! port model and `E_UNRESOLVED_PORT` are not yet defined.
//!
//! The returned [`Resolution::diagnostics`] is in **resolver-emission
//! order**, not sorted by source span. The `check::check` pipeline runs
//! its findings through `DiagnosticSink::into_sorted` after merging, so
//! sorting here too would be redundant work.

use std::collections::HashSet;

use indexmap::IndexMap;
use serde::Serialize;

use crate::ast::{Value, ValueKind};
use crate::check::{Diagnostic, DiagnosticCode, DiagnosticNote, Severity};
use crate::error::Span;
use crate::intent::{
    DefIr, IntentModule, Member, MemberBody, MemberRole, SiteIr, StructIr, ThemeIr, ValueWithSpan,
    role_of,
};
use crate::suggest::nearest_match;

use super::binding::{SelectorMatch, ThemeBinding, TokenKind, classify_token};

/// Resolution of one whole [`IntentModule`].
///
/// Ownership: [`Resolution`] does **not** mutate the IR; it holds an
/// independent map keyed by scope name. The `members` map inside each
/// [`ScopeResolution`] is keyed by `member.span.start` so callers can
/// correlate without threading an index back through the original `Vec`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Resolution {
    /// One entry per theme defined in the module, keyed by theme name.
    pub themes: IndexMap<String, ThemeBinding>,
    /// One entry per struct/def/site, keyed by `kind::name` (e.g.
    /// `struct::cottage`, `def::cottage`, `site::hamlet`) so a file with a
    /// struct and a def of the same name still produces two distinct keys.
    pub scopes: IndexMap<String, ScopeResolution>,
    /// Diagnostics gathered during resolution. The `check` pipeline merges
    /// these with the rest of `cairn check`'s output.
    #[serde(skip)]
    pub diagnostics: Vec<Diagnostic>,
}

/// Resolution outcome for a single struct/def/site body.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScopeResolution {
    /// Which theme governed this scope, when the resolver could pick one.
    /// `None` when the file has 0 or >1 themes (see the module-level
    /// "theme selection rule").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound_theme: Option<String>,
    /// Per-member resolved binding, keyed by `member.span.start`.
    pub members: IndexMap<usize, ResolvedMemberBinding>,
}

/// Resolved data attached to a single [`Member`].
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct ResolvedMemberBinding {
    /// The value bound to this member's `mat_slot=` via the applied theme,
    /// when both ends matched. `None` if the member has no `mat_slot=`, no
    /// theme was bound to the scope, or the slot was not declared in the
    /// theme (in which case `E_UNRESOLVED_SLOT` was emitted).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slot_value: Option<ValueWithSpan>,
    /// Extra `key=value` bindings injected by a matching theme selector,
    /// merged left-to-right in source order (later selector wins on key
    /// collision).
    #[serde(skip_serializing_if = "IndexMap::is_empty")]
    pub selector_extras: IndexMap<String, ValueWithSpan>,
}

/// Resolve theme bindings over the given Intent IR.
///
/// Always returns a [`Resolution`] — every theme appears in `.themes`, every
/// struct/def/site appears in `.scopes`, and any problems encountered are
/// collected into `.diagnostics`.
#[must_use]
pub fn resolve(ir: &IntentModule) -> Resolution {
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut themes: IndexMap<String, ThemeBinding> = IndexMap::new();
    for theme in &ir.themes {
        let binding = build_theme_binding(theme);
        // Last-write-wins on duplicate theme names (the `duplicate` pass in
        // `check` is the authority on flagging the collision). Keeping the
        // map insertion-ordered means downstream consumers see the same
        // theme order the source declared.
        themes.insert(binding.name.clone(), binding);
    }

    let single_theme_name = single_theme(&themes);
    let mut scopes: IndexMap<String, ScopeResolution> = IndexMap::new();
    let mut applied_themes: HashSet<String> = HashSet::new();

    for s in &ir.structs {
        let resolution = resolve_struct_or_def(
            &s.members,
            single_theme_name.as_deref(),
            &mut themes,
            &mut applied_themes,
            &mut diagnostics,
        );
        scopes.insert(struct_key(s), resolution);
    }
    for d in &ir.defs {
        let resolution = resolve_struct_or_def(
            &d.members,
            single_theme_name.as_deref(),
            &mut themes,
            &mut applied_themes,
            &mut diagnostics,
        );
        scopes.insert(def_key(d), resolution);
    }
    let mut used_defs: HashSet<String> = HashSet::new();
    for site in &ir.sites {
        resolve_site_placements(
            site,
            &ir.defs,
            &mut themes,
            &mut applied_themes,
            &mut scopes,
            &mut used_defs,
            &mut diagnostics,
        );
    }
    check_unused_defs(&ir.defs, &used_defs, &mut diagnostics);

    check_slot_targets(&themes, &mut diagnostics);
    check_unmatched_selectors(&themes, &applied_themes, &mut diagnostics);

    Resolution {
        themes,
        scopes,
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

fn single_theme(themes: &IndexMap<String, ThemeBinding>) -> Option<String> {
    if themes.len() == 1 {
        themes.keys().next().cloned()
    } else {
        None
    }
}

fn struct_key(s: &StructIr) -> String {
    format!("struct::{}", s.name)
}

fn def_key(d: &DefIr) -> String {
    format!("def::{}", d.name)
}

/// IR-side key for a single `place` inside a `site`.
///
/// Embedding the site name (`site::hamlet::home1` rather than
/// `place::home1`) lets multiple sites in one module own non-clashing place
/// ids — the IR key shape stays unambiguous even before
/// [`crate::block_array::output_filename`] flattens the leaf for the
/// per-file `.nbt` name.
#[must_use]
pub fn place_scope_key(site_name: &str, place_id: &str) -> String {
    format!("site::{site_name}::{place_id}")
}

fn resolve_site_placements(
    site: &SiteIr,
    defs: &[DefIr],
    themes: &mut IndexMap<String, ThemeBinding>,
    applied_themes: &mut HashSet<String>,
    scopes: &mut IndexMap<String, ScopeResolution>,
    used_defs: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Local index lets each `east_of=ID` / `north_of=ID` lookup name a
    // *prior* place in source order — re-walking `site.placements` per
    // lookup would be quadratic and would also let a later place forward-
    // reference an earlier one's mistakes.
    let mut seen_place_ids: IndexMap<String, Span> = IndexMap::new();

    // Pre-built name lists so `nearest_match` candidates are stable per site
    // rather than re-allocated per place.
    let def_names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    let theme_names: Vec<String> = themes.keys().cloned().collect();

    for member in &site.placements {
        if !matches!(member.role, MemberRole::Place) {
            // `connect` lines do not register a scope and are left for the
            // lowering pass to surface as `W_DEFERRED_MEMBER`. Other site-
            // local members (logic, assert) are also out of scope here.
            continue;
        }

        // Unnamed place lines (no `id=`) cannot be referenced by `east_of` /
        // `north_of` and have no scope key to register, so they are skipped
        // silently here; the upstream syntactic check will surface a
        // distinct error if id absence becomes a structural failure.
        let Some(place_id) = member.id.as_deref() else {
            continue;
        };

        if let Some(first) = seen_place_ids.get(place_id) {
            diagnostics.push(duplicate_place_id_diag(
                &site.name,
                place_id,
                first,
                &member.span,
            ));
            continue;
        }
        seen_place_ids.insert(place_id.to_owned(), member.span.clone());

        // Validate origin selectors before any cross-scope lookup so the
        // user sees the structural problem first. An invalid origin makes
        // the rest of the placement unsalvageable — skip the def/theme
        // resolution and the scope insert so the lowering pass does not
        // emit a `.nbt` for a structurally rejected placement.
        if !validate_place_origin(member, &site.name, place_id, &seen_place_ids, diagnostics) {
            continue;
        }

        let use_target = member
            .intent_state
            .get("use")
            .and_then(|v| v.value.as_label_str());
        let theme_target = member
            .intent_state
            .get("theme")
            .and_then(|v| v.value.as_label_str());

        let Some(use_name) = use_target else {
            continue;
        };
        let def = defs.iter().find(|d| d.name == use_name);
        if def.is_none() {
            diagnostics.push(unresolved_place_ref_diag(
                &format!("`use={use_name}` references an unknown def"),
                member.span.clone(),
                use_name,
                def_names.iter().copied(),
            ));
            continue;
        }
        let def = def.expect("checked is_none above");
        used_defs.insert(def.name.clone());

        let Some(theme_name) = theme_target else {
            continue;
        };
        if !themes.contains_key(theme_name) {
            diagnostics.push(unresolved_theme_ref_diag(
                theme_name,
                member.span.clone(),
                theme_names.iter().map(String::as_str),
            ));
            continue;
        }

        // Cross-scope resolve: run the def's members under the picked theme,
        // even when the file has multiple themes (the per-place `theme=`
        // wins over the single-theme heuristic).
        let resolution = resolve_struct_or_def(
            &def.members,
            Some(theme_name),
            themes,
            applied_themes,
            diagnostics,
        );
        scopes.insert(place_scope_key(&site.name, place_id), resolution);
    }
}

/// Returns `true` when the placement passes every origin-selector check, in
/// which case `resolve_site_placements` may proceed with the cross-scope
/// def / theme resolution and register a scope. `false` means at least one
/// `E_INVALID_PLACE_ORIGIN` or `E_UNRESOLVED_PLACE_REF` (target reference)
/// was emitted — the caller must skip the rest of the placement so the
/// lowering pass does not voxelise a structurally rejected `place`.
fn validate_place_origin(
    member: &Member,
    site_name: &str,
    place_id: &str,
    seen_place_ids: &IndexMap<String, Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let at = member.intent_state.get("at");
    let east_of = member.intent_state.get("east_of");
    let north_of = member.intent_state.get("north_of");
    let selector_count =
        usize::from(at.is_some()) + usize::from(east_of.is_some()) + usize::from(north_of.is_some());

    if selector_count == 0 {
        diagnostics.push(invalid_place_origin_diag(
            place_id,
            member.span.clone(),
            "`place` is missing an origin selector; add `at=origin`, `east_of=ID`, or `north_of=ID`",
        ));
        return false;
    }
    if selector_count > 1 {
        diagnostics.push(invalid_place_origin_diag(
            place_id,
            member.span.clone(),
            "`place` carries more than one origin selector; keep exactly one of `at`, `east_of`, `north_of`",
        ));
        return false;
    }
    if let Some(value) = at
        && !matches!(&value.value.kind, ValueKind::Ident(s) if s == "origin")
    {
        diagnostics.push(invalid_place_origin_diag(
            place_id,
            value.span.clone(),
            "`at=` only accepts `origin`; use `east_of=ID` or `north_of=ID` for relative placement",
        ));
        return false;
    }

    // Cross-place reference validation: the target must appear before this
    // place in source order so cycles cannot form.
    let mut ok = true;
    for (key, value) in [("east_of", east_of), ("north_of", north_of)] {
        let Some(value) = value else {
            continue;
        };
        let Some(target) = value.value.as_label_str() else {
            diagnostics.push(invalid_place_origin_diag(
                place_id,
                value.span.clone(),
                &format!("`{key}=` expects a place id label"),
            ));
            ok = false;
            continue;
        };
        if !seen_place_ids.contains_key(target) || target == place_id {
            // Suggestion pool is *prior* place ids only — pointing at a
            // later place would let cycles slip in. The same-site exclusion
            // keeps `east_of=self` from showing up as a viable suggestion.
            let prior: Vec<&str> = seen_place_ids
                .keys()
                .filter(|id| id.as_str() != place_id)
                .map(String::as_str)
                .collect();
            diagnostics.push(unresolved_place_ref_diag_with_ordering_note(
                &format!(
                    "`{key}={target}` in site `{site_name}` does not name a prior place id",
                ),
                value.span.clone(),
                target,
                prior.iter().copied(),
            ));
            ok = false;
        }
    }
    ok
}

fn check_unused_defs(
    defs: &[DefIr],
    used: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for def in defs {
        if used.contains(&def.name) {
            continue;
        }
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::UnusedDef,
            severity: Severity::Warning,
            span: def.span.clone(),
            primary: format!(
                "def `{name}` is never referenced by a `place use={name}`",
                name = def.name,
            ),
            notes: vec![DiagnosticNote {
                span: None,
                message: "remove the def, or place an instance via `site ... place use=...`"
                    .to_owned(),
            }],
        });
    }
}

fn unresolved_place_ref_diag<'a>(
    primary: &str,
    span: Span,
    typo: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Diagnostic {
    let mut notes = Vec::new();
    if let Some(suggested) = nearest_match(typo, candidates) {
        notes.push(DiagnosticNote {
            span: None,
            message: format!("did you mean `{suggested}`?"),
        });
    }
    Diagnostic {
        code: DiagnosticCode::UnresolvedPlaceRef,
        severity: Severity::Error,
        span,
        primary: primary.to_owned(),
        notes,
    }
}

/// Same as [`unresolved_place_ref_diag`] but appends an ordering-only note
/// for `east_of=` / `north_of=` failures. The nearest-match candidate pool
/// is restricted to *earlier* place ids in the same site so cycles cannot
/// form; without the note an ordering miss looks like the suggestion engine
/// just gave up.
fn unresolved_place_ref_diag_with_ordering_note<'a>(
    primary: &str,
    span: Span,
    typo: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Diagnostic {
    let mut diag = unresolved_place_ref_diag(primary, span, typo, candidates);
    diag.notes.push(DiagnosticNote {
        span: None,
        message:
            "later places in the same site cannot be referenced; declare the target above this line"
                .to_owned(),
    });
    diag
}

fn unresolved_theme_ref_diag<'a>(
    theme: &str,
    span: Span,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Diagnostic {
    let mut notes = Vec::new();
    if let Some(suggested) = nearest_match(theme, candidates) {
        notes.push(DiagnosticNote {
            span: None,
            message: format!("did you mean `{suggested}`?"),
        });
    }
    Diagnostic {
        code: DiagnosticCode::UnresolvedThemeRef,
        severity: Severity::Error,
        span,
        primary: format!("`theme={theme}` is not a declared theme"),
        notes,
    }
}

fn duplicate_place_id_diag(
    site_name: &str,
    place_id: &str,
    first: &Span,
    second: &Span,
) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::DuplicatePlaceId,
        severity: Severity::Error,
        span: second.clone(),
        primary: format!("duplicate `id={place_id}` in site `{site_name}`"),
        notes: vec![DiagnosticNote {
            span: Some(first.clone()),
            message: "first declared here".to_owned(),
        }],
    }
}

fn invalid_place_origin_diag(place_id: &str, span: Span, message: &str) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::InvalidPlaceOrigin,
        severity: Severity::Error,
        span,
        primary: format!("invalid origin selector on `place id={place_id}`: {message}"),
        notes: vec![],
    }
}

fn resolve_struct_or_def(
    members: &[Member],
    single_theme_name: Option<&str>,
    themes: &mut IndexMap<String, ThemeBinding>,
    applied_themes: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> ScopeResolution {
    let (theme_name, theme_slots) = match single_theme_name {
        Some(name) => themes.get(name).map_or((None, None), |t| {
            (Some(name.to_owned()), Some(t.slots.clone()))
        }),
        None => (None, None),
    };

    if let Some(name) = &theme_name {
        applied_themes.insert(name.clone());
    }

    let mut resolution = ScopeResolution {
        bound_theme: theme_name.clone(),
        members: IndexMap::new(),
    };
    resolve_members(
        members,
        theme_slots.as_ref(),
        theme_name.as_deref(),
        themes,
        &mut resolution,
        diagnostics,
    );
    resolution
}

fn resolve_members(
    members: &[Member],
    theme_slots: Option<&IndexMap<String, ValueWithSpan>>,
    theme_name: Option<&str>,
    themes: &mut IndexMap<String, ThemeBinding>,
    out: &mut ScopeResolution,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for member in members {
        let mut binding = ResolvedMemberBinding::default();

        // 1. mat_slot resolution against the scope's applied theme.
        if let Some(slot_name) = &member.mat_slot
            && let (Some(slots), Some(tname)) = (theme_slots, theme_name)
        {
            match slots.get(slot_name) {
                Some(v) => binding.slot_value = Some(v.clone()),
                None => diagnostics.push(unresolved_slot_diag(slot_name, tname, member, slots)),
            }
        }

        // 2. Selector matching — scoped to the bound theme only. A scope
        //    with `bound_theme=None` (multi-theme file, no auto-pick)
        //    gets no selector_extras, matching the per-theme DI contract.
        if let Some(tname) = theme_name
            && let Some(theme_binding) = themes.get_mut(tname)
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
            resolve_members(children, theme_slots, theme_name, themes, out, diagnostics);
        }
    }
}

fn selector_matches(sel: &SelectorMatch, member: &Member) -> bool {
    if !keyword_matches_role(&sel.keyword, &member.role) {
        return false;
    }
    sel.attrs
        .iter()
        .all(|(key, expected)| member_attr_matches(member, key, &expected.value))
}

fn keyword_matches_role(keyword: &str, role: &MemberRole) -> bool {
    matches!(
        (keyword, role),
        ("floor", MemberRole::Floor)
            | ("walls", MemberRole::Walls)
            | ("door", MemberRole::Door)
            | ("window", MemberRole::Window)
            | ("roof", MemberRole::Roof)
            | ("stair", MemberRole::Stair)
            | ("level", MemberRole::Level)
            | ("pressure_plate", MemberRole::PressurePlate)
            | ("circuit", MemberRole::Circuit)
            | ("place", MemberRole::Place)
            | ("connect", MemberRole::Connect)
    ) || matches!(role, MemberRole::Other(other) if other == keyword)
}

/// Compare a selector attribute's expected value against the corresponding
/// member field. `id`, `class`, and `mat_slot` live as their own
/// `Option<String>` fields on [`Member`] so the comparison is string-vs-
/// `Ident`/`Str`; everything else is a generic `key=value` arg that lives
/// in [`crate::intent::IntentState`] and compares by [`ValueKind`].
fn member_attr_matches(member: &Member, key: &str, expected: &Value) -> bool {
    match key {
        "id" => member
            .id
            .as_deref()
            .is_some_and(|v| value_eq_label(expected, v)),
        "class" => member
            .class
            .as_deref()
            .is_some_and(|v| value_eq_label(expected, v)),
        "mat_slot" => member
            .mat_slot
            .as_deref()
            .is_some_and(|v| value_eq_label(expected, v)),
        _ => member
            .intent_state
            .get(key)
            .is_some_and(|actual| actual.value.kind == expected.kind),
    }
}

fn value_eq_label(expected: &Value, raw: &str) -> bool {
    matches!(&expected.kind,
        ValueKind::Ident(s) | ValueKind::Str(s) if s == raw,
    )
}

fn unresolved_slot_diag(
    slot: &str,
    theme_name: &str,
    member: &Member,
    available_slots: &IndexMap<String, ValueWithSpan>,
) -> Diagnostic {
    // Suggestion goes ahead of the generic remediation so a top-down reader
    // sees the targeted fix first. The suggestion pool is the *applied
    // theme's* declared slots only — a `mat_slot=` cannot bind across
    // themes, and proposing a slot from a different theme would point the
    // user at code that wouldn't help.
    let mut notes = Vec::with_capacity(2);
    if let Some(suggested) = nearest_match(slot, available_slots.keys().map(String::as_str)) {
        notes.push(DiagnosticNote {
            span: None,
            message: format!("did you mean `{suggested}`?"),
        });
    }
    notes.push(DiagnosticNote {
        span: None,
        message: format!(
            "add `slot {slot} -> @...` to theme `{theme_name}` or correct the slot name",
        ),
    });
    Diagnostic {
        code: DiagnosticCode::UnresolvedSlot,
        severity: Severity::Error,
        span: member.span.clone(),
        primary: format!("`mat_slot={slot}` is not declared in theme `{theme_name}`"),
        notes,
    }
}

fn check_slot_targets(themes: &IndexMap<String, ThemeBinding>, diagnostics: &mut Vec<Diagnostic>) {
    for theme in themes.values() {
        for (slot_name, v) in &theme.slots {
            if classify_token(&v.value) == TokenKind::NotAToken {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::UnknownSlotTarget,
                    severity: Severity::Warning,
                    span: v.span.clone(),
                    primary: format!(
                        "slot `{slot}` in theme `{theme}` does not bind to a canonical or abstract material token",
                        slot = slot_name,
                        theme = theme.name,
                    ),
                    notes: vec![DiagnosticNote {
                        span: None,
                        message: "expected a `@canonical_block` or `@abstract.material` value"
                            .to_owned(),
                    }],
                });
            }
        }
    }
}

fn check_unmatched_selectors(
    themes: &IndexMap<String, ThemeBinding>,
    applied_themes: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for theme in themes.values() {
        // Skip themes that were never bound to a scope — their selectors
        // are vacuously unmatched in M2-PR3 because the resolver can't
        // pick which struct/def they apply to (multi-theme files defer
        // the decision to M3's `place ... theme=X` boundary). Warning
        // about every selector in such a theme would be noise.
        if !applied_themes.contains(&theme.name) {
            continue;
        }
        for sel in &theme.selectors {
            // Skip selectors whose keyword is itself unknown — the
            // `keyword_allowlist` pass already flagged that with
            // `E_UNKNOWN_KEYWORD`, and pointing at the same span a second
            // time with "selector doesn't match" reads as noise.
            if matches!(role_of(&sel.keyword), MemberRole::Other(_)) {
                continue;
            }
            if sel.matched_member_spans.is_empty() {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::ThemeSelectorUnmatched,
                    severity: Severity::Warning,
                    span: sel.source_span.clone(),
                    primary: format!(
                        "theme selector `{kw}[...]` in `{theme}` does not match any member",
                        kw = sel.keyword,
                        theme = theme.name,
                    ),
                    notes: vec![DiagnosticNote {
                        span: None,
                        message: "remove the selector or relax its attribute filters".to_owned(),
                    }],
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lower, parse};

    fn ir(source: &str) -> IntentModule {
        let module = parse(source).expect("parse");
        lower(&module)
    }

    #[test]
    fn single_theme_file_resolves_struct_slots() {
        let src = "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
        let r = resolve(&ir(src));
        let scope = r.scopes.get("struct::s").expect("scope present");
        assert_eq!(scope.bound_theme.as_deref(), Some("t"));
        let bound = scope.members.values().next().expect("member binding");
        assert!(bound.slot_value.is_some(), "wall slot should resolve");
        assert!(
            r.diagnostics.is_empty(),
            "no diagnostics expected, got {:?}",
            r.diagnostics,
        );
    }

    #[test]
    fn unresolved_slot_emits_diagnostic() {
        let src = "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=4x4\n  walls mat_slot=floor height=3\n";
        let r = resolve(&ir(src));
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::UnresolvedSlot && d.severity == Severity::Error),
            "expected E_UNRESOLVED_SLOT, got {:?}",
            r.diagnostics,
        );
    }

    #[test]
    fn multiple_themes_leave_struct_unbound() {
        let src = "theme a:\n  slot wall -> @cobblestone\ntheme b:\n  slot wall -> @stone\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
        let r = resolve(&ir(src));
        let scope = r.scopes.get("struct::s").unwrap();
        assert!(scope.bound_theme.is_none());
        // No E_UNRESOLVED_SLOT because no theme was applied.
        assert!(
            !r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
        );
    }

    #[test]
    fn multi_theme_unbound_scope_gets_no_selector_extras() {
        // Regression: an earlier version walked `themes.values_mut()`
        // unconditionally and wrote every matching selector into
        // `selector_extras`, even when the scope's `bound_theme` was None.
        // That violated the per-theme DI contract from §7.
        let src = "theme a:\n  walls[class=outer] -> trim=@a_trim\ntheme b:\n  walls[class=outer] -> trim=@b_trim\n\nstruct s size=4x4\n  walls class=outer height=3\n";
        let r = resolve(&ir(src));
        let scope = r.scopes.get("struct::s").unwrap();
        assert!(scope.bound_theme.is_none());
        let bound = scope.members.values().next().unwrap();
        assert!(
            bound.selector_extras.is_empty(),
            "unbound scope must not absorb selectors from any theme, got {:?}",
            bound.selector_extras,
        );
    }

    #[test]
    fn unmatched_selector_warning_is_suppressed_for_unapplied_themes() {
        // Regression: in a multi-theme file, no theme is applied to the
        // struct/def, so warning on every theme selector would be noise.
        // M3 will pick the theme via `place ... theme=X`; until then the
        // selectors are not "unmatched", they're "not yet bound".
        let src = "theme a:\n  walls[class=outer] -> trim=@a\ntheme b:\n  walls[class=outer] -> trim=@b\n\nstruct s size=4x4\n  walls class=outer height=3\n";
        let r = resolve(&ir(src));
        assert!(
            !r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ThemeSelectorUnmatched),
            "no warnings expected, got {:?}",
            r.diagnostics,
        );
    }

    #[test]
    fn selector_match_adds_extras_and_marks_matched() {
        let src = "theme t:\n  slot wall -> @cobblestone\n  walls[class=outer] -> trim=@spruce_log\n\nstruct s size=4x4\n  walls class=outer mat_slot=wall height=3\n";
        let r = resolve(&ir(src));
        let scope = r.scopes.get("struct::s").unwrap();
        let bound = scope.members.values().next().unwrap();
        assert!(
            bound.selector_extras.contains_key("trim"),
            "extras: {:?}",
            bound.selector_extras,
        );
        assert!(
            !r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ThemeSelectorUnmatched),
        );
    }

    #[test]
    fn unmatched_selector_emits_warning() {
        let src = "theme t:\n  slot wall -> @cobblestone\n  walls[class=does_not_exist] -> trim=@spruce_log\n\nstruct s size=4x4\n  walls class=outer mat_slot=wall height=3\n";
        let r = resolve(&ir(src));
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ThemeSelectorUnmatched
                    && d.severity == Severity::Warning),
        );
    }

    #[test]
    fn unresolved_slot_attaches_did_you_mean_note() {
        // `mat_slot=wal` is one deletion away from the declared `wall` slot;
        // the resolver must surface that as a targeted suggestion so the
        // fix is unambiguous.
        let src = "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=4x4\n  walls mat_slot=wal height=3\n";
        let r = resolve(&ir(src));
        let diag = r
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::UnresolvedSlot)
            .unwrap_or_else(|| panic!("no E_UNRESOLVED_SLOT, got {:?}", r.diagnostics));
        assert!(
            diag.notes
                .iter()
                .any(|n| n.message.contains("did you mean `wall`")),
            "expected `did you mean` note, got {:#?}",
            diag.notes,
        );
    }

    #[test]
    fn unresolved_slot_skips_suggestion_when_pool_is_far_away() {
        // No theme slot is close to `quartz` — the resolver must not invent
        // a guess. The remediation note stays as the lone follow-up.
        let src = "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=4x4\n  walls mat_slot=quartz height=3\n";
        let r = resolve(&ir(src));
        let diag = r
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::UnresolvedSlot)
            .unwrap_or_else(|| panic!("no E_UNRESOLVED_SLOT, got {:?}", r.diagnostics));
        assert!(
            !diag
                .notes
                .iter()
                .any(|n| n.message.contains("did you mean")),
            "no suggestion expected, got {:#?}",
            diag.notes,
        );
    }

    #[test]
    fn unknown_slot_target_emits_warning() {
        let src = "theme t:\n  slot wall -> plain_ident\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
        let r = resolve(&ir(src));
        assert!(r.diagnostics.iter().any(
            |d| d.code == DiagnosticCode::UnknownSlotTarget && d.severity == Severity::Warning
        ),);
    }
}
