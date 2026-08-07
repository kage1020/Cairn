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
//! `spec/materials-themes.md` §7 — a selector belongs to one theme, not
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
//! per row. Missing port ids fail loud with `E_UNRESOLVED_PORT` (with a
//! nearest-match note); a `def` exposing the same `id=` on more than one
//! member raises `E_AMBIGUOUS_PORT`; an absent `path=` triggers
//! `E_MISSING_PATH_MATERIAL`. The walkway voxeliser (under
//! `block_array`) consumes the resolved connects without re-walking the
//! `DotRef`s.
//!
//! The returned [`Resolution::diagnostics`] is in **resolver-emission
//! order**, not sorted by source span. The `check::check` pipeline runs
//! its findings through `DiagnosticSink::into_sorted` after merging, so
//! sorting here too would be redundant work.

use std::collections::HashSet;

use indexmap::IndexMap;
use serde::Serialize;

use crate::ast::{Value, ValueKind};
use crate::check::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::edition::Edition;
use crate::error::Span;
use crate::ids::{IdError, PlaceId, PortId, SiteName};
use crate::intent::{
    ConnectEnd, DefIr, IntentModule, Member, MemberBody, MemberRole, SiteIr, StructIr, ThemeIr,
    ValueWithSpan, role_of,
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
    /// One entry per successfully-resolved `connect` row, in source order.
    /// Rows that failed validation (`E_UNRESOLVED_PORT`,
    /// `E_AMBIGUOUS_PORT`, `E_MISSING_PATH_MATERIAL`,
    /// `E_UNRESOLVED_PLACE_REF` on the place half) are dropped here so the
    /// walkway voxeliser does not lay a strip against a broken reference.
    /// Empty for any file that declares no `connect` lines, matching the
    /// `placements` shape on [`crate::block_array::BlockArrayIr`].
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub connects: Vec<ValidatedConnect>,
    /// Diagnostics gathered during resolution. The `check` pipeline merges
    /// these with the rest of `cairn check`'s output.
    #[serde(skip)]
    pub diagnostics: Vec<Diagnostic>,
}

/// One end of a `connect a.PORT to b.PORT` row, resolved into a
/// `(place, port)` pair the block-array lowering can look up directly.
///
/// `place` is the `id=` of the named place; `port` is the `id=` of the
/// matching member inside that place's `def`. The span points at the
/// originating `DotRef` value in source so the resolver-side
/// diagnostics (`E_UNRESOLVED_PORT`, `E_AMBIGUOUS_PORT`,
/// `E_UNRESOLVED_PLACE_REF` from the connect path) can underline the
/// exact token the user wrote, not the whole `connect` line. Block-array
/// side diagnostics (`W_WALKWAY_BLOCKED`, `W_DUPLICATE_WALKWAY`, the
/// endpoint-cascade `W_DEFERRED_MEMBER`) describe the whole walkway and
/// therefore anchor at `ValidatedConnect::span` instead.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PortRef {
    /// `place id=` value the port belongs to.
    pub place: PlaceId,
    /// Member `id=` exposed by the place's def.
    pub port: PortId,
    /// Byte range of the `place.port` token in source.
    #[serde(skip)]
    pub span: Span,
}

impl std::fmt::Display for PortRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.place, self.port)
    }
}

/// One `connect from.port to to.port path=@MATERIAL` row, validated.
///
/// Carries everything the walkway voxeliser needs (both port refs, the
/// path material as a `ValueWithSpan` so `resolve_block_state` can
/// process it uniformly with `mat_slot=` values, the originating site
/// name, and the row's span for follow-up diagnostics).
///
/// The `path` is intentionally still a [`ValueWithSpan`] (and not a
/// lifted `BlockState`) at this layer: per-edition material resolution
/// is the responsibility of the next maturity tier (see
/// [`crate::resolve`] module docs), and lifting here would invert the
/// `core` → `formats` dependency edge that owns the registry pack.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ValidatedConnect {
    /// Bare site name (no `site::` IR-key prefix).
    pub site: SiteName,
    /// First port — `from.port`.
    pub from: PortRef,
    /// Second port — `to.port`.
    pub to: PortRef,
    /// `path=@MATERIAL` value. Carried as [`ValueWithSpan`] so the
    /// walkway lowering can run it through the same
    /// `resolve_block_state` pipeline `mat_slot=` uses, lifting both
    /// canonical and abstract tokens through the registry pack with one
    /// code path.
    pub path: ValueWithSpan,
    /// Byte range of the originating `connect ...` line.
    #[serde(skip)]
    pub span: Span,
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
/// `tests/check_duplicate_items.rs` records the shape that made the
/// choice necessary.
///
/// INVARIANT(upstream-diagnosed): a skipped binding is not reported from
/// here. `check::duplicate` has already pushed `E_DUPLICATE_ITEM` into
/// the same sink for every repeated name, so a user running `cairn
/// check` — or any build command, all of which gate on it — sees a
/// position-anchored signal. Re-pushing would report one mistake twice,
/// and the pass that owns it can anchor on the name token, which the IR
/// no longer carries. This is the same division of labour the silent
/// arms in [`resolve_connect_row`] follow, and it is pinned the same
/// way, by `tests/silent_skip_arms.rs` for the resolver-only path and
/// `tests/check_duplicate_items.rs` for the full pipeline. A
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
/// (spec versioning-editions §10.7 hierarchy #2): when the file declares
/// two themes whose names share a base and differ only by an `_java` /
/// `_bedrock` suffix, `Some(Edition::Java)` picks the `_java` variant and
/// `Some(Edition::Bedrock)` picks the `_bedrock` variant. `None` is the
/// "no edition has been picked yet" case (typically `cairn check` without
/// `--edition`) — the resolver unions slot names across variants of the
/// same logical theme so a `mat_slot=NAME` reference that only one variant
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

    let (auto_picked, auto_siblings) = match single_logical.as_deref() {
        Some(logical) => {
            let picked = pick_variant(&themes, logical, edition).map(str::to_owned);
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

    for s in &ir.structs {
        let resolution = resolve_struct_or_def(
            &s.members,
            auto_picked.as_deref(),
            &auto_siblings,
            &mut themes,
            &mut applied_themes,
            &mut diagnostics,
        );
        scopes.entry(struct_key(s)).or_insert(resolution);
    }
    for d in &ir.defs {
        let resolution = resolve_struct_or_def(
            &d.members,
            auto_picked.as_deref(),
            &auto_siblings,
            &mut themes,
            &mut applied_themes,
            &mut diagnostics,
        );
        scopes.entry(def_key(d)).or_insert(resolution);
    }
    let mut used_defs: HashSet<String> = HashSet::new();
    let mut connects: Vec<ValidatedConnect> = Vec::new();
    for site in &ir.sites {
        resolve_site_placements(
            site,
            &ir.defs,
            &mut themes,
            &mut applied_themes,
            &mut scopes,
            &mut used_defs,
            &mut connects,
            &mut diagnostics,
        );
    }
    check_unused_defs(&ir.defs, &used_defs, &mut diagnostics);

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

/// Strip an `_java` / `_bedrock` suffix from a theme name, returning the
/// logical name plus the variant marker.
///
/// A theme declared as `theme shop_java:` reports `("shop", Some(Java))`;
/// `theme medieval:` reports `("medieval", None)`. The suffix set is
/// closed (matches [`Edition`]) so a future edition adds one arm here.
fn strip_edition_suffix(name: &str) -> (&str, Option<Edition>) {
    if let Some(base) = name.strip_suffix("_java") {
        (base, Some(Edition::Java))
    } else if let Some(base) = name.strip_suffix("_bedrock") {
        (base, Some(Edition::Bedrock))
    } else {
        (name, None)
    }
}

/// Return the sole *logical* theme name in the file, ignoring per-edition
/// variant suffixes.
///
/// A file with `theme shop_java` + `theme shop_bedrock` reports
/// `Some("shop")` because both are variants of one logical theme — this
/// keeps the auto-pick rule intact when the author uses spec §10.7
/// variants. A file with `theme cottage` + `theme keep` reports `None`
/// because the two names are genuinely distinct logical themes.
fn single_logical_theme(themes: &IndexMap<String, ThemeBinding>) -> Option<String> {
    let mut logical: Option<String> = None;
    for name in themes.keys() {
        let (l, _) = strip_edition_suffix(name);
        match &logical {
            None => logical = Some(l.to_owned()),
            Some(seen) if seen == l => {}
            Some(_) => return None,
        }
    }
    logical
}

/// Pick the theme name to bind for `logical` under `edition`.
///
/// Order of preference:
///
/// - `Some(Java)`    → `<logical>_java` → unsuffixed `<logical>` → **unbound**
/// - `Some(Bedrock)` → `<logical>_bedrock` → unsuffixed `<logical>` → **unbound**
/// - `None`          → unsuffixed `<logical>` → `<logical>_java` → `<logical>_bedrock`
///
/// Under a `Some(edition)` compile the fallback deliberately **stops at
/// the unsuffixed variant** rather than cross over to the opposite
/// edition's variant. Binding, say, a `_bedrock` theme under
/// `--edition java` would silently route Bedrock-only slot values into a
/// Java `.nbt`; leaving the scope unbound instead surfaces the mismatch
/// through `E_UNRESOLVED_SLOT` on any `mat_slot=X` reference, which is
/// the loud outcome spec versioning-editions §10.4 requires.
///
/// The `None` case still tolerates a partial file (only one variant
/// declared): it prefers the unsuffixed theme, then Java, then Bedrock —
/// a deterministic order that avoids leaking source-order into
/// diagnostics.
fn pick_variant<'a>(
    themes: &'a IndexMap<String, ThemeBinding>,
    logical: &str,
    edition: Option<Edition>,
) -> Option<&'a str> {
    let mut unsuffixed: Option<&str> = None;
    let mut java: Option<&str> = None;
    let mut bedrock: Option<&str> = None;
    for name in themes.keys() {
        let (l, variant) = strip_edition_suffix(name);
        if l != logical {
            continue;
        }
        match variant {
            None => unsuffixed = Some(name.as_str()),
            Some(Edition::Java) => java = Some(name.as_str()),
            Some(Edition::Bedrock) => bedrock = Some(name.as_str()),
        }
    }
    match edition {
        Some(Edition::Java) => java.or(unsuffixed),
        Some(Edition::Bedrock) => bedrock.or(unsuffixed),
        None => unsuffixed.or(java).or(bedrock),
    }
}

/// Slot names present in *sibling* variants of the same logical theme.
///
/// Used to gate `E_UNRESOLVED_SLOT` under the edition = `None` case: a
/// `mat_slot=X` should not error when `X` is declared by any variant of the
/// picked scope's logical theme. Returns an empty set when no siblings exist
/// (single-variant file, or edition-bound compile) — the caller then falls
/// back to the picked variant's slots alone.
fn sibling_slot_names(
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

#[allow(clippy::too_many_arguments)]
fn resolve_site_placements(
    site: &SiteIr,
    defs: &[DefIr],
    themes: &mut IndexMap<String, ThemeBinding>,
    applied_themes: &mut HashSet<String>,
    scopes: &mut IndexMap<String, ScopeResolution>,
    used_defs: &mut HashSet<String>,
    connects: &mut Vec<ValidatedConnect>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Local index lets each `east_of=ID` / `north_of=ID` lookup name a
    // *prior* place in source order — re-walking `site.placements` per
    // lookup would be quadratic and would also let a later place forward-
    // reference an earlier one's mistakes.
    let mut seen_place_ids: IndexMap<String, Span> = IndexMap::new();
    // `place_id` → `use=DEF_NAME` so `connect` rows can find the def whose
    // body exposes the named port without re-walking the placement list.
    let mut place_def: IndexMap<String, String> = IndexMap::new();

    // Pre-built name lists so `nearest_match` candidates are stable per site
    // rather than re-allocated per place.
    let def_names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    let theme_names: Vec<String> = themes.keys().cloned().collect();

    for member in &site.placements {
        if matches!(member.role, MemberRole::Connect) {
            resolve_connect_row(
                member,
                &site.name,
                defs,
                &seen_place_ids,
                &place_def,
                connects,
                diagnostics,
            );
            continue;
        }
        if !matches!(member.role, MemberRole::Place) {
            // Everything that reaches here is a member a `site` body has no
            // reader for — `logic` and `assert` are not members at all and
            // live in `SiteIr`'s own fields. `check::member_scope` reports
            // this row as `E_MISPLACED_MEMBER`; this arm is why it had to.
            continue;
        }

        // Report every key the row is short of at once, before any of the
        // per-key work, so the author fixes one line once instead of
        // re-running the compiler to discover the next omission. The row is
        // still walked past this point when it has an `id=`: registering it
        // in `seen_place_ids` is what keeps a later `east_of=` or a
        // `connect` naming it behaving as it did — an incomplete row is a
        // row that failed, not a row that was never written.
        if let Some(diagnostic) = incomplete_place_diag(member, &site.name) {
            diagnostics.push(diagnostic);
        }

        // An unnamed placement has no scope key to register, cannot be
        // referenced by `east_of` / `north_of`, and is unreachable from any
        // `connect` row (a dot-ref needs a name on the left side), so there
        // is nothing further to do with it. The finding above is what makes
        // that a report rather than a silent skip.
        let Some(place_id) = member.id.as_deref() else {
            continue;
        };

        // Validate before the id becomes half of a scope key. `PlaceId`
        // states the invariants, and `place_scope_key` joins on `::`, so an
        // id carrying `.` or `:` produces a key nothing can parse back —
        // which is where the lowering pass used to `expect` and panic.
        if let Err(err) = PlaceId::new(place_id) {
            diagnostics.push(invalid_place_id_diag(
                place_id,
                &site.name,
                member.span.clone(),
                &err,
            ));
            continue;
        }
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

        // Both inputs that reach this arm are already reported, and by
        // different owners: an absent `use=` by `incomplete_place_diag`
        // above, a present-but-not-label-shaped one (`use=3`, which
        // `as_label_str` also answers `None` for) by
        // `check::type_mismatch`'s `E_TYPE_MISMATCH_LABEL`. Calling the
        // second one missing would be a lie, which is why the completeness
        // check keys on the *key* rather than on the lifted value.
        //
        // Skipping the rest of the pipeline prevents `place_def` and the
        // scope map from carrying a half-built entry, so the lowering pass
        // below does not emit a `.nbt` for a placement with no def.
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

        // Same split as `use=` above: absent is `incomplete_place_diag`'s,
        // mistyped is `check::type_mismatch`'s. The single-theme heuristic
        // in `resolve_struct_or_def` does not rescue an omitted `theme=`
        // here — it defaults a *scope*, and this arm returns before any
        // scope is built.
        //
        // Skipping the rest of the pipeline here leaves `place_def` unset
        // for this place; a downstream `connect` targeting it additionally
        // earns the `W_DEFERRED_CONNECT` cascade in `validate_port`, the
        // same pairing an unresolved `use=DEF` already produces.
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
        // wins over the single-theme heuristic). Sibling-variant slot union
        // does not apply here — the author explicitly named one theme via
        // `theme=`, so unresolved slots on that specific theme are real
        // errors, not the multi-variant softening the top-level scope loop
        // uses under `cairn check`.
        let no_siblings: HashSet<String> = HashSet::new();
        let resolution = resolve_struct_or_def(
            &def.members,
            Some(theme_name),
            &no_siblings,
            themes,
            applied_themes,
            diagnostics,
        );
        scopes
            .entry(place_scope_key(&site.name, place_id))
            .or_insert(resolution);
        // Record this place's def so a later `connect` row can look up
        // the def's members without re-walking the site body.
        place_def.insert(place_id.to_owned(), use_name.to_owned());
    }
}

/// Validate one `connect from.port to to.port path=@MAT` row and push a
/// matching [`ValidatedConnect`] when both ends and the path material
/// pass every check.
///
/// Failures all skip the push so the walkway voxeliser only sees rows it
/// can lay safely. Each diagnostic kind is emitted at most once per
/// failure mode so a single broken row never reads as multiple unrelated
/// problems:
///
/// - `E_UNRESOLVED_PLACE_REF` — `from.place_id` or `to.place_id` does
///   not name a prior `place` in the same site;
/// - `E_UNRESOLVED_PORT` — the `port_id` half is not declared by any
///   member of the referenced def, with a nearest-match note when one
///   sits within the spell cap;
/// - `E_AMBIGUOUS_PORT` — multiple members of the referenced def share
///   that `id=`; downstream lowering would have to pick one arbitrarily;
/// - `E_MISSING_PATH_MATERIAL` — the row has no `path=` argument so
///   walkway lowering has no material to lay.
fn resolve_connect_row(
    member: &Member,
    site_name: &str,
    defs: &[DefIr],
    seen_place_ids: &IndexMap<String, Span>,
    place_def: &IndexMap<String, String>,
    connects: &mut Vec<ValidatedConnect>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // Surface positional shape from the snapshot:
    //   positional[0] = DotRef(from.port)
    //   positional[1] = Ident("to")
    //   positional[2] = DotRef(to.port)
    //
    // INVARIANT(upstream-diagnosed): inside the top-level `check`
    // pipeline, `check::connect_arity` has already pushed
    // `E_CONNECT_ARITY` into the same sink for any row whose positional
    // shape is not `FROM.PORT to TO.PORT`, so a user running `cairn
    // check` always sees a position-anchored signal even when these
    // guards fire. The guards survive for library callers that invoke
    // `resolve(ir)` directly (LSP fast paths, ad-hoc tooling): the
    // silent return keeps walkway voxelisation from picking up a
    // half-formed or misshapen row instead of panicking on a partial
    // parse.
    //
    // The slice pattern rejects the missing-half cases and the
    // over-arity case together, and the separator test rejects
    // `connect a.entry xxx b.entry`. Matching an exact-length slice
    // rather than indexing is what keeps the count bound here: reading
    // `positional[0..3]` accepted `connect a.entry to b.entry c.exit`
    // and laid a walkway for a row `check` calls an error, which is a
    // disagreement about well-formedness between the two layers rather
    // than a difference in how loudly they say so. Pinned by
    // `tests/silent_skip_arms.rs` (resolver-only) and
    // `tests/check_connect_arity.rs` (full pipeline).
    let [from_value, separator, to_value] = member.positional.as_slice() else {
        return;
    };
    if !matches!(&separator.kind, ValueKind::Ident(keyword) if keyword == "to") {
        return;
    }
    // Lift both ends before short-circuiting so a row with two broken
    // halves earns two diagnostics, not just the first. `validate_port`
    // already follows the same accumulate-then-decide pattern below.
    let from = port_ref_from_value(from_value, ConnectEnd::From, seen_place_ids, diagnostics);
    let to = port_ref_from_value(to_value, ConnectEnd::To, seen_place_ids, diagnostics);
    let (Some(from), Some(to)) = (from, to) else {
        return;
    };

    let mut ok = true;
    if !validate_port(&from, defs, place_def, diagnostics) {
        ok = false;
    }
    if !validate_port(&to, defs, place_def, diagnostics) {
        ok = false;
    }

    let path = member.intent_state.get("path").cloned();
    let Some(path) = path else {
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::MissingPathMaterial,
            span: member.span.clone(),
            primary: "`connect` requires a `path=` material to lay the walkway".to_owned(),
            notes: vec![DiagnosticNote {
                span: None,
                message:
                    "add `path=@gravel` (or another `@token`) to the end of the `connect` line"
                        .to_owned(),
            }],
            data: None,
        });
        return;
    };
    // A bare label (`path=gravel`) or any other non-token kind would slip
    // through `resolve_block_state` as `MaterialDeferred::AlreadyDiagnosed`
    // — that arm is wired for theme slot values which the
    // `E_UNKNOWN_SLOT_TARGET` pass already flags. `connect.path` is not in
    // that pass's scope, so we fail loud here instead of letting the row
    // drop silently in the walkway voxeliser.
    if !matches!(path.value.kind, ValueKind::Token(_)) {
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::MissingPathMaterial,
            span: path.span.clone(),
            primary: format!(
                "`connect path=` must be a material token like `@gravel`, got {}",
                path.value.kind_name(),
            ),
            notes: vec![DiagnosticNote {
                span: None,
                message: "use `@TOKEN` (e.g. `path=@gravel`); bare labels and string literals are not material references"
                    .to_owned(),
            }],
            data: None,
        });
        return;
    }
    if !ok {
        return;
    }

    connects.push(ValidatedConnect {
        site: SiteName::new(site_name).expect("surface lexer enforces SiteName invariants"),
        from,
        to,
        path,
        span: member.span.clone(),
    });
}

/// Lift one positional [`Value`] into a [`PortRef`], emitting
/// `E_UNRESOLVED_PLACE_REF` if the head segment does not name a prior
/// place in the same site and returning `None` for any shape other than
/// a one-dot `<place>.<port>` reference. `end` goes into the primary
/// diagnostic so the user can tell the two ends apart at a glance.
fn port_ref_from_value(
    raw: &Value,
    end: ConnectEnd,
    seen_place_ids: &IndexMap<String, Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<PortRef> {
    // INVARIANT(upstream-diagnosed): inside the top-level `check`
    // pipeline, `check::connect_arity` has already pushed
    // `E_CONNECT_ARITY` into the same sink for any endpoint that is not
    // a one-dot reference, so a user running `cairn check` always sees
    // a position-anchored signal even when these guards fire. Re-pushing
    // here would report one mistake twice.
    //
    // The guards survive for library callers that invoke `resolve(ir)`
    // directly (LSP fast paths, ad-hoc tooling), mirroring the guards on
    // `resolve_connect_row` above: returning `None` keeps walkway
    // voxelisation from picking up a row whose endpoints name nothing.
    //
    // What each guard actually catches differs by origin. The parser
    // builds a `DotRef` only when a `.` follows the head token, so a
    // parsed reference always carries at least one tail segment: from
    // source, the first guard catches every non-reference value
    // (`connect a to b` arrives as `Ident`) and the second catches only
    // `place.port.extra`. An empty tail is reachable just by hand-built
    // IR, and takes the same silent path. An `E_UNRESOLVED_PORT` arm
    // used to sit on that empty-tail case with a "missing a port id"
    // message; it could not fire from parsed source, and the author
    // who writes `connect a to b` now gets that advice from
    // `check::connect_arity`, anchored on the value itself.
    //
    // Pinned by `tests/silent_skip_arms.rs` (resolver-only) and
    // `tests/check_connect_arity.rs` (full pipeline).
    let ValueKind::DotRef(dot) = &raw.kind else {
        return None;
    };
    let [port] = dot.tail() else {
        return None;
    };
    let place_str = dot.head();
    let port_str = port.as_str();
    if !seen_place_ids.contains_key(place_str) {
        let prior: Vec<&str> = seen_place_ids.keys().map(String::as_str).collect();
        diagnostics.push(unresolved_place_ref_diag(
            &format!(
                "the `{end}` endpoint `{place_str}.{port_str}` does not name a prior place in this site",
                end = end.label(),
            ),
            raw.span.clone(),
            place_str,
            prior.iter().copied(),
        ));
        return None;
    }
    // INVARIANT(upstream-diagnosed): the surface lexer's `Ident` rule
    // forbids `.`, `:`, and whitespace, so any `DotRef` segment that
    // reached this point is already a valid newtype payload — the
    // `.expect` failure mode would mean the lexer accepted a token the
    // surface grammar forbids. Cheaper than re-validating per row.
    let place = PlaceId::new(place_str).expect("surface lexer enforces PlaceId invariants");
    let port = PortId::new(port_str).expect("surface lexer enforces PortId invariants");
    Some(PortRef {
        place,
        port,
        span: raw.span.clone(),
    })
}

/// Walk the referenced def's members and decide whether `port.port_id`
/// is a valid port id, emitting the matching `E_UNRESOLVED_PORT` /
/// `E_AMBIGUOUS_PORT` diagnostic when not. The port-id ambient pool for
/// the suggestion is the def's set of member ids — pointing the user at
/// an id from a sibling def would just send them down a different broken
/// path.
fn validate_port(
    port: &PortRef,
    defs: &[DefIr],
    place_def: &IndexMap<String, String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let Some(def_name) = place_def.get(port.place.as_str()) else {
        // `port_ref_from_value` already gates the lift on
        // `seen_place_ids`, so reaching here means `port.place_id` is
        // registered but its `place_def` entry never landed. The arms
        // in `resolve_site_placements` that skip the insert split into
        // two camps:
        //
        // - upstream-diagnosed: `validate_place_origin` failure
        //   (`E_INVALID_PLACE_ORIGIN`), duplicate id
        //   (`E_DUPLICATE_PLACE_ID`), `use=` naming an unknown def
        //   (`E_UNRESOLVED_PLACE_REF`), `theme=` naming an unknown
        //   theme (`E_UNRESOLVED_THEME_REF`). The user already sees a
        //   targeted error for these.
        // - intentionally silent: missing `use=` / missing `theme=`
        //   (multi-theme files). The surface grammar accepts both and
        //   no `check` pass requires either today.
        //
        // Either way the walkway would vanish from the build. Emit a
        // cascade `W_DEFERRED_CONNECT` so the silent arms surface as
        // an explicit signal — mirroring the `W_DEFERRED_MEMBER` cascade
        // used for walkway endpoint cascades in `block_array::lower` —
        // and so a future refactor that drops a normal-path
        // `place_def.insert` cannot silently break every walkway.
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::DeferredConnect,
            span: port.span.clone(),
            primary: format!(
                "`connect` target `{place_id}.{port_id}` references a place with no resolved \
                 def/theme; no walkway laid",
                place_id = port.place,
                port_id = port.port,
            ),
            notes: vec![DiagnosticNote {
                span: None,
                message: "add `use=DEF` and `theme=NAME` to the referenced `place`, or remove \
                          this `connect` row"
                    .to_owned(),
            }],
            data: None,
        });
        return false;
    };
    let Some(def) = defs.iter().find(|d| d.name == *def_name) else {
        // INVARIANT(structural): `resolve_site_placements` only inserts
        // a `use_name` into `place_def` *after* `defs.iter().find(|d|
        // d.name == use_name)` already returned `Some` in its
        // `unresolved_place_ref_diag` arm (which `continue`s on miss).
        // By construction, every `def_name` reachable here is therefore
        // present in `defs`. A miss is a contract break in
        // `resolve_site_placements`, not an upstream-diagnosed input —
        // fail loud in debug builds so a future refactor that violates
        // the construction invariant surfaces immediately.
        debug_assert!(
            false,
            "validate_port: def `{def_name}` (used by `place {place_id}`) is in `place_def` but \
             absent from `defs` — `resolve_site_placements` was supposed to gate `place_def` \
             insertion on def presence",
            def_name = def_name,
            place_id = port.place,
        );
        return false;
    };

    let matches: Vec<&Member> = def
        .members
        .iter()
        .filter(|m| m.id.as_deref() == Some(port.port.as_str()))
        .collect();
    match matches.len() {
        0 => {
            let pool: Vec<&str> = def.members.iter().filter_map(|m| m.id.as_deref()).collect();
            let mut notes = Vec::with_capacity(2);
            if let Some(suggested) = nearest_match(port.port.as_str(), pool.iter().copied()) {
                notes.push(DiagnosticNote {
                    span: None,
                    message: format!("did you mean `{}.{suggested}`?", port.place),
                });
            }
            notes.push(DiagnosticNote {
                span: None,
                message: format!(
                    "add `id={port_id}` to a member of `def {def_name}` (e.g. `door id={port_id} ...`)",
                    port_id = port.port,
                    def_name = def_name,
                ),
            });
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::UnresolvedPort,
                span: port.span.clone(),
                primary: format!(
                    "port `{port_id}` is not declared by `def {def_name}` (used by `place {place_id}`)",
                    port_id = port.port,
                    def_name = def_name,
                    place_id = port.place,
                ),
                notes,
                data: None,
            });
            false
        }
        1 => true,
        n => {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::AmbiguousPort,
                span: port.span.clone(),
                primary: format!(
                    "port `{port_id}` matches {n} members of `def {def_name}`; the reference is ambiguous",
                    port_id = port.port,
                    def_name = def_name,
                ),
                notes: vec![DiagnosticNote {
                    span: None,
                    message: "rename the duplicate `id=` so each port is uniquely addressable"
                        .to_owned(),
                }],
                data: None,
            });
            false
        }
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
    let selector_count = usize::from(at.is_some())
        + usize::from(east_of.is_some())
        + usize::from(north_of.is_some());

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
                &format!("`{key}={target}` in site `{site_name}` does not name a prior place id"),
                value.span.clone(),
                target,
                prior.iter().copied(),
            ));
            ok = false;
        }
    }
    ok
}

fn check_unused_defs(defs: &[DefIr], used: &HashSet<String>, diagnostics: &mut Vec<Diagnostic>) {
    for def in defs {
        if used.contains(&def.name) {
            continue;
        }
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::UnusedDef,
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
            data: None,
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
        span,
        primary: primary.to_owned(),
        notes,
        data: None,
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
        span,
        primary: format!("`theme={theme}` is not a declared theme"),
        notes,
        data: None,
    }
}

/// Report a `place` row missing any of the keys it needs to become a
/// placement, or `None` when it has all three.
///
/// `id=` names the `.nbt` the compiler writes for this placement
/// (`spec/components-editing-sites.md` §9.3.4) and is the second half of
/// the scope key `site::SITE::PLACE` that `east_of=` and `connect` parse
/// back out — so it cannot be auto-addressed the way §5.5 auto-addresses a
/// geometry member, whose address derives from its role and position
/// rather than naming an output file. `use=` names the `def` the placement
/// instantiates and `theme=` the theme its `mat_slot=` members resolve
/// against; without either there is no volume to voxelise.
///
/// Absence is read off the surface keys, not off the lifted values:
/// `member.id` is `None` both for an absent `id=` and for an `id=3` that
/// `intent::lower` declined to hoist, and only the first is a missing key.
fn incomplete_place_diag(member: &Member, site_name: &str) -> Option<Diagnostic> {
    let absent =
        |key: &str, lifted_present: bool| !lifted_present && !member.intent_state.contains_key(key);
    let missing: Vec<&str> = [
        ("id", member.id.is_some()),
        ("use", false),
        ("theme", false),
    ]
    .into_iter()
    .filter(|(key, lifted)| absent(key, *lifted))
    .map(|(key, _)| key)
    .collect();
    if missing.is_empty() {
        return None;
    }
    let quoted: Vec<String> = missing.iter().map(|key| format!("`{key}=`")).collect();
    // "a, b, and c" rather than "a, b, c": the list is read aloud in a
    // sentence, and up to three keys fit without wrapping.
    let listed = match quoted.split_last() {
        Some((last, [])) => last.clone(),
        Some((last, head)) => format!("{}, and {last}", head.join(", ")),
        None => unreachable!("the empty case returned above"),
    };
    let mut notes = Vec::new();
    for key in &missing {
        notes.push(DiagnosticNote {
            span: None,
            message: match *key {
                "id" => "`id=` names the `.nbt` this placement is written to, and is the half of the scope key that `east_of=` and `connect` refer to — the compiler has no name to invent for it".to_owned(),
                "use" => "`use=DEF` names the `def` this placement instantiates".to_owned(),
                _ => "`theme=NAME` names the theme this placement's `mat_slot=` members resolve against".to_owned(),
            },
        });
    }
    Some(Diagnostic {
        code: DiagnosticCode::IncompletePlace,
        span: member.span.clone(),
        primary: format!(
            "`place` in site `{site_name}` is missing {listed}, so no placement is built for it",
        ),
        notes,
        data: None,
    })
}

fn duplicate_place_id_diag(
    site_name: &str,
    place_id: &str,
    first: &Span,
    second: &Span,
) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::DuplicatePlaceId,
        span: second.clone(),
        primary: format!("duplicate `id={place_id}` in site `{site_name}`"),
        notes: vec![DiagnosticNote {
            span: Some(first.clone()),
            message: "first declared here".to_owned(),
        }],
        data: None,
    }
}

fn invalid_place_id_diag(place_id: &str, site_name: &str, span: Span, err: &IdError) -> Diagnostic {
    let reason = match err {
        IdError::Empty => "it is empty".to_owned(),
        IdError::ForbiddenChar { ch, .. } => format!("it contains `{ch}`"),
    };
    Diagnostic {
        code: DiagnosticCode::InvalidPlaceId,
        span,
        primary: format!(
            "`place id={place_id}` in site `{site_name}` is not a usable id: {reason}"
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "a place id becomes part of the `site::<site>::<place>` scope key, \
                      so it must be non-empty and free of `.`, `:`, and whitespace"
                .to_owned(),
        }],
        data: None,
    }
}

fn invalid_place_origin_diag(place_id: &str, span: Span, message: &str) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::InvalidPlaceOrigin,
        span,
        primary: format!("invalid origin selector on `place id={place_id}`: {message}"),
        notes: vec![],
        data: None,
    }
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
fn resolve_struct_or_def(
    members: &[Member],
    picked_theme_name: Option<&str>,
    sibling_slots: &HashSet<String>,
    themes: &mut IndexMap<String, ThemeBinding>,
    applied_themes: &mut HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> ScopeResolution {
    // Structural invariant: `pick_variant` (the only caller producing an
    // auto-picked name here) iterates `themes.keys()` to build its
    // candidates, so any name it returns is guaranteed to be in `themes`.
    // The site-side branch that hits this with a user-supplied `theme=X`
    // label filters through `themes.contains_key(theme_name)` up-slope
    // before calling in. Reaching the `None` arm of `themes.get` would
    // mean one of those guarantees broke — asymmetric with `validate_port`,
    // which uses the same shape of loud fallback. `debug_assert!(false)`
    // trips in dev / test builds; a release build silently degrades to
    // the unbound-theme path rather than panicking on user data.
    let (theme_name, theme_slots) = if let Some(name) = picked_theme_name {
        if let Some(t) = themes.get(name) {
            (Some(name.to_owned()), Some(t.slots.clone()))
        } else {
            debug_assert!(
                false,
                "resolve_struct_or_def: picked theme `{name}` is not in the themes map; \
                 pick_variant should only surface names it read from themes.keys()",
            );
            (None, None)
        }
    } else {
        (None, None)
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
        sibling_slots,
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
    sibling_slots: &HashSet<String>,
    themes: &mut IndexMap<String, ThemeBinding>,
    out: &mut ScopeResolution,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for member in members {
        let mut binding = ResolvedMemberBinding::default();

        // 1. mat_slot resolution against the scope's applied theme.
        //    A slot the picked variant does not declare is still "known"
        //    when a sibling variant of the same logical theme declares it,
        //    which suppresses `E_UNRESOLVED_SLOT` for spec §10.7's
        //    edition-variant themes under `cairn check` without an
        //    `--edition` pin. `slot_value` stays `None` in that case — the
        //    concrete binding is edition-specific and comes into scope only
        //    once the compile picks a variant.
        if let Some(slot_name) = &member.mat_slot
            && let (Some(slots), Some(tname)) = (theme_slots, theme_name)
        {
            match slots.get(slot_name) {
                Some(v) => binding.slot_value = Some(v.clone()),
                None if sibling_slots.contains(slot_name) => {}
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
            resolve_members(
                children,
                theme_slots,
                theme_name,
                sibling_slots,
                themes,
                out,
                diagnostics,
            );
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

/// Whether a selector's keyword names this member's role.
///
/// `MemberRole::keyword` is the inverse of `intent::role_of`, so the
/// comparison is the identity it looks like — including the `Other`
/// case, where the role carries the author's own word.
fn keyword_matches_role(keyword: &str, role: &MemberRole) -> bool {
    role.keyword() == keyword
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
        span: member.span.clone(),
        primary: format!("`mat_slot={slot}` is not declared in theme `{theme_name}`"),
        notes,
        data: None,
    }
}

/// Flag slot values that are not material tokens.
///
/// Takes every *declared* theme rather than the bound map: the check is
/// local to one body and needs no resolution, so a body whose name lost
/// the binding still gets its findings. Selector matching is the
/// opposite case — see [`check_unmatched_selectors`].
fn check_slot_targets(themes: &[ThemeBinding], diagnostics: &mut Vec<Diagnostic>) {
    for theme in themes {
        for (slot_name, v) in &theme.slots {
            if classify_token(&v.value) == TokenKind::NotAToken {
                diagnostics.push(Diagnostic {
                    code: DiagnosticCode::UnknownSlotTarget,
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
                    data: None,
                });
            }
        }
    }
}

/// Flag theme selectors that matched no member.
///
/// Takes the bound map, not every declared body. "Matched nothing" is a
/// statement about a resolution this selector took part in, and a body
/// whose name lost the binding never got to take part — reporting its
/// selectors would blame them for a name collision reported elsewhere.
/// The body-local counterpart is [`check_slot_targets`].
fn check_unmatched_selectors(
    themes: &IndexMap<String, ThemeBinding>,
    applied_themes: &HashSet<String>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for theme in themes.values() {
        // Skip themes that were never bound to a scope — their selectors
        // are vacuously unmatched because the resolver can't pick which
        // struct/def they apply to (multi-theme files defer the decision
        // to the `place ... theme=X` boundary on the site side). Warning
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
                    data: None,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::Severity;
    use crate::{lower, parse};

    fn ir(source: &str) -> IntentModule {
        let module = parse(source).expect("parse");
        lower(&module)
    }

    #[test]
    fn single_theme_file_resolves_struct_slots() {
        let src = "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
        let r = resolve(&ir(src), None);
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
        let r = resolve(&ir(src), None);
        assert!(
            r.diagnostics.iter().any(
                |d| d.code == DiagnosticCode::UnresolvedSlot && d.severity() == Severity::Error
            ),
            "expected E_UNRESOLVED_SLOT, got {:?}",
            r.diagnostics,
        );
    }

    #[test]
    fn multiple_themes_leave_struct_unbound() {
        let src = "theme a:\n  slot wall -> @cobblestone\ntheme b:\n  slot wall -> @stone\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
        let r = resolve(&ir(src), None);
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
        let r = resolve(&ir(src), None);
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
        // A theme is picked at the `place ... theme=X` boundary; until
        // that binding runs, the selectors are not "unmatched", they're
        // "not yet bound".
        let src = "theme a:\n  walls[class=outer] -> trim=@a\ntheme b:\n  walls[class=outer] -> trim=@b\n\nstruct s size=4x4\n  walls class=outer height=3\n";
        let r = resolve(&ir(src), None);
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
        let r = resolve(&ir(src), None);
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
        let r = resolve(&ir(src), None);
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ThemeSelectorUnmatched
                    && d.severity() == Severity::Warning),
        );
    }

    #[test]
    fn unresolved_slot_attaches_did_you_mean_note() {
        // `mat_slot=wal` is one deletion away from the declared `wall` slot;
        // the resolver must surface that as a targeted suggestion so the
        // fix is unambiguous.
        let src = "theme t:\n  slot wall -> @cobblestone\n\nstruct s size=4x4\n  walls mat_slot=wal height=3\n";
        let r = resolve(&ir(src), None);
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
        let r = resolve(&ir(src), None);
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
    fn connect_resolves_to_port_refs_with_path_material() {
        // Two cottages connected by gravel — the canonical village shape.
        // The resolver must build one `ValidatedConnect` carrying both
        // `(place, port)` pairs plus the `path=@gravel` value, with
        // no diagnostics.
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def cottage size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "\n",
            "site s:\n",
            "  place id=a use=cottage theme=t at=origin\n",
            "  place id=b use=cottage theme=t east_of=a gap=2\n",
            "  connect a.entry to b.entry path=@gravel\n",
        );
        let r = resolve(&ir(src), None);
        assert!(
            !r.diagnostics
                .iter()
                .any(|d| d.severity() == Severity::Error),
            "no errors expected, got {:?}",
            r.diagnostics,
        );
        assert_eq!(r.connects.len(), 1, "exactly one connect resolved");
        let c = &r.connects[0];
        assert_eq!(c.site, "s");
        assert_eq!(c.from.place, "a");
        assert_eq!(c.from.port, "entry");
        assert_eq!(c.to.place, "b");
        assert_eq!(c.to.port, "entry");
    }

    #[test]
    fn connect_unknown_port_emits_e_unresolved_port() {
        // Port `entry` is exposed by the def, but the user mistyped it.
        // The resolver must surface E_UNRESOLVED_PORT with a nearest-
        // match note pointing at the correct `place.port` shape.
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def cottage size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "\n",
            "site s:\n",
            "  place id=a use=cottage theme=t at=origin\n",
            "  place id=b use=cottage theme=t east_of=a gap=2\n",
            "  connect a.entr to b.entry path=@gravel\n",
        );
        let r = resolve(&ir(src), None);
        let diag = r
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::UnresolvedPort)
            .unwrap_or_else(|| panic!("expected E_UNRESOLVED_PORT, got {:?}", r.diagnostics));
        assert_eq!(diag.severity(), Severity::Error);
        assert!(
            diag.notes.iter().any(|n| n.message.contains("a.entry")),
            "expected nearest-match note pointing at a.entry, got {:?}",
            diag.notes,
        );
        // Anchor: the span must point at the `a.entr` DotRef, not at
        // the whole `connect` row. Mirrors the to-side assertion in
        // `connect_unknown_port_on_to_side_emits_e_unresolved_port`.
        let typo_start = src.find("a.entr").expect("typo present");
        let typo_end = typo_start + "a.entr".len();
        assert_eq!(diag.span.start, typo_start);
        assert_eq!(diag.span.end, typo_end);
        // The failed connect must not surface as resolved — walkway
        // voxelisation only sees rows it can lay safely.
        assert!(r.connects.is_empty(), "broken connect must not resolve");
    }

    #[test]
    fn connect_ambiguous_port_emits_e_ambiguous_port() {
        // Two members of the def carry `id=entry`. The reference is
        // ambiguous; the resolver must say so loudly rather than picking
        // one silently.
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def cottage size=5x5:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "  window id=entry side=back y=1 offset=1 size=1x1 mat_slot=wall\n",
            "\n",
            "site s:\n",
            "  place id=a use=cottage theme=t at=origin\n",
            "  place id=b use=cottage theme=t east_of=a gap=2\n",
            "  connect a.entry to b.entry path=@gravel\n",
        );
        let r = resolve(&ir(src), None);
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::AmbiguousPort),
            "expected E_AMBIGUOUS_PORT, got {:?}",
            r.diagnostics,
        );
    }

    #[test]
    fn connect_missing_path_emits_e_missing_path_material() {
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def cottage size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "\n",
            "site s:\n",
            "  place id=a use=cottage theme=t at=origin\n",
            "  place id=b use=cottage theme=t east_of=a gap=2\n",
            "  connect a.entry to b.entry\n",
        );
        let r = resolve(&ir(src), None);
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::MissingPathMaterial),
            "expected E_MISSING_PATH_MATERIAL, got {:?}",
            r.diagnostics,
        );
        assert!(r.connects.is_empty());
    }

    #[test]
    fn connect_non_token_path_emits_e_missing_path_material() {
        // `path=plain_ident` and `path="gravel"` slip through the
        // material resolver as `MaterialDeferred::AlreadyDiagnosed` (the
        // theme-slot path). The resolver must surface that as
        // E_MISSING_PATH_MATERIAL with a "must be an @ token" note,
        // otherwise the row drops silently in the walkway voxeliser.
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def cottage size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "\n",
            "site s:\n",
            "  place id=a use=cottage theme=t at=origin\n",
            "  place id=b use=cottage theme=t east_of=a gap=2\n",
            "  connect a.entry to b.entry path=plain_ident\n",
        );
        let r = resolve(&ir(src), None);
        let diag = r
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::MissingPathMaterial)
            .unwrap_or_else(|| panic!("expected E_MISSING_PATH_MATERIAL, got {:?}", r.diagnostics));
        assert_eq!(diag.severity(), Severity::Error);
        assert!(
            diag.primary.contains("token") && diag.primary.contains("identifier"),
            "expected the message to call out the kind mismatch, got: {}",
            diag.primary,
        );
        assert!(r.connects.is_empty(), "non-token path must not resolve");
    }

    #[test]
    fn connect_unknown_place_emits_e_unresolved_place_ref() {
        // `ghost` is not a known place id. Re-uses the existing
        // E_UNRESOLVED_PLACE_REF code so a single diagnostic family
        // covers every "unknown place" path (origin selectors and now
        // connect refs).
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def cottage size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "\n",
            "site s:\n",
            "  place id=a use=cottage theme=t at=origin\n",
            "  connect a.entry to ghost.entry path=@gravel\n",
        );
        let r = resolve(&ir(src), None);
        let diag = r
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::UnresolvedPlaceRef && d.primary.contains("ghost"))
            .unwrap_or_else(|| {
                panic!(
                    "expected E_UNRESOLVED_PLACE_REF mentioning ghost, got {:?}",
                    r.diagnostics,
                )
            });
        // Anchor: span underlines the `ghost.entry` reference itself.
        let bad_start = src.find("ghost.entry").expect("bad ref present");
        let bad_end = bad_start + "ghost.entry".len();
        assert_eq!(diag.span.start, bad_start);
        assert_eq!(diag.span.end, bad_end);
    }

    #[test]
    fn connect_with_both_endpoints_unknown_emits_both_e_unresolved_place_ref() {
        // Pin the "fail both halves" contract: a row whose `from` and
        // `to` both name absent places must surface *two* diagnostics
        // (one per offending DotRef), not just the first one. Catches a
        // regression where short-circuit on the `from` half would let
        // the `to` typo slip through silently.
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def cottage size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "\n",
            "site s:\n",
            "  place id=a use=cottage theme=t at=origin\n",
            "  connect ghost.entry to phantom.entry path=@gravel\n",
        );
        let r = resolve(&ir(src), None);
        let ghost_hit = r
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedPlaceRef && d.primary.contains("ghost"));
        let phantom_hit = r
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedPlaceRef && d.primary.contains("phantom"));
        assert!(
            ghost_hit && phantom_hit,
            "expected E_UNRESOLVED_PLACE_REF for both `ghost` and `phantom`, got {:?}",
            r.diagnostics,
        );
        assert!(r.connects.is_empty(), "broken connect must not resolve");
    }

    #[test]
    fn connect_unknown_port_on_to_side_emits_e_unresolved_port() {
        // Symmetric to `connect_unknown_port_emits_e_unresolved_port`: the
        // bad port is on the `to` half (`b.entr`) instead of the `from`
        // half. The diagnostic must anchor at the offending token, not at
        // the whole `connect` line.
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def cottage size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "\n",
            "site s:\n",
            "  place id=a use=cottage theme=t at=origin\n",
            "  place id=b use=cottage theme=t east_of=a gap=2\n",
            "  connect a.entry to b.entr path=@gravel\n",
        );
        let r = resolve(&ir(src), None);
        let diag = r
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::UnresolvedPort)
            .unwrap_or_else(|| panic!("expected E_UNRESOLVED_PORT, got {:?}", r.diagnostics));
        assert_eq!(diag.severity(), Severity::Error);
        assert!(
            diag.notes.iter().any(|n| n.message.contains("b.entry")),
            "expected nearest-match note pointing at b.entry, got {:?}",
            diag.notes,
        );
        // The span must point at the `b.entr` token itself so a renderer
        // can underline the typo rather than the whole `connect` row.
        let typo_start = src.find("b.entr").expect("typo present");
        let typo_end = typo_start + "b.entr".len();
        assert_eq!(
            diag.span.start, typo_start,
            "span should anchor at the `b.entr` DotRef, got {:?}",
            diag.span,
        );
        assert_eq!(
            diag.span.end, typo_end,
            "span should end at the `b.entr` DotRef, got {:?}",
            diag.span,
        );
        assert!(r.connects.is_empty(), "broken connect must not resolve");
    }

    #[test]
    fn connect_unknown_place_on_from_side_emits_e_unresolved_place_ref() {
        // Symmetric to `connect_unknown_place_emits_e_unresolved_place_ref`:
        // the unknown place id is on the `from` half instead of the `to`
        // half. A single E_UNRESOLVED_PLACE_REF code must continue to
        // cover both directions.
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def cottage size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "\n",
            "site s:\n",
            "  place id=a use=cottage theme=t at=origin\n",
            "  connect ghost.entry to a.entry path=@gravel\n",
        );
        let r = resolve(&ir(src), None);
        let diag = r
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::UnresolvedPlaceRef && d.primary.contains("ghost"))
            .unwrap_or_else(|| {
                panic!(
                    "expected E_UNRESOLVED_PLACE_REF mentioning ghost, got {:?}",
                    r.diagnostics,
                )
            });
        // The span must underline the `ghost.entry` reference so the
        // renderer points at the bad token rather than the whole row.
        let bad_start = src.find("ghost.entry").expect("bad ref present");
        let bad_end = bad_start + "ghost.entry".len();
        assert_eq!(
            diag.span.start, bad_start,
            "span should anchor at the `ghost.entry` DotRef, got {:?}",
            diag.span,
        );
        assert_eq!(
            diag.span.end, bad_end,
            "span should end at the `ghost.entry` DotRef, got {:?}",
            diag.span,
        );
        assert!(r.connects.is_empty(), "broken connect must not resolve");
    }

    /// A slot bound to something that is not a material token is an
    /// error, not advisory: `walls mat_slot=wall` below lowers to air, so
    /// a `cairn check` that exited 0 would certify a hollow build.
    #[test]
    fn unknown_slot_target_emits_error() {
        let src = "theme t:\n  slot wall -> plain_ident\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
        let r = resolve(&ir(src), None);
        assert!(r.diagnostics.iter().any(
            |d| d.code == DiagnosticCode::UnknownSlotTarget && d.severity() == Severity::Error
        ),);
    }

    // ------------------------------------------------------------------
    // Per-edition theme fallback (spec versioning-editions §10.7 #2).
    // The following tests pin the AC set that keeps the resolver honest
    // about which variant it bound and when the sibling-slot union kicks in.
    // ------------------------------------------------------------------

    fn per_edition_variants_src() -> String {
        // One logical theme `t` split into `_java` / `_bedrock` variants.
        // Each variant has a private slot (`java_only` / `bedrock_only`) so
        // the variant-picking behaviour is observable via `slot_value`, and
        // a shared slot (`floor`) confirms both variants participate in
        // `single_logical_theme` grouping.
        [
            "theme t_java:",
            "  slot floor -> @oak_planks",
            "  slot java_only -> @spruce_planks",
            "",
            "theme t_bedrock:",
            "  slot floor -> @oak_planks",
            "  slot bedrock_only -> @dark_oak_planks",
            "",
            "struct s size=4x4",
            "  floor  mat_slot=floor",
            "",
        ]
        .join("\n")
    }

    #[test]
    fn per_edition_java_picks_java_variant() {
        let r = resolve(&ir(&per_edition_variants_src()), Some(Edition::Java));
        let scope = r.scopes.get("struct::s").expect("scope present");
        assert_eq!(scope.bound_theme.as_deref(), Some("t_java"));
        assert!(
            r.diagnostics.is_empty(),
            "no diagnostics expected for a slot both variants declare, got {:?}",
            r.diagnostics,
        );
    }

    #[test]
    fn per_edition_bedrock_picks_bedrock_variant() {
        let r = resolve(&ir(&per_edition_variants_src()), Some(Edition::Bedrock));
        let scope = r.scopes.get("struct::s").expect("scope present");
        assert_eq!(scope.bound_theme.as_deref(), Some("t_bedrock"));
        assert!(r.diagnostics.is_empty(), "got {:?}", r.diagnostics);
    }

    #[test]
    fn per_edition_variant_binds_only_its_own_slot_under_compile() {
        // Under Some(Java), a member referencing a slot that only the
        // Bedrock variant declares must fail loud — sibling-slot union is
        // reserved for the edition = None case.
        let src = [
            "theme t_java:",
            "  slot floor -> @oak_planks",
            "",
            "theme t_bedrock:",
            "  slot floor -> @oak_planks",
            "  slot bedrock_only -> @dark_oak_planks",
            "",
            "struct s size=4x4",
            "  floor mat_slot=bedrock_only",
            "",
        ]
        .join("\n");
        let r = resolve(&ir(&src), Some(Edition::Java));
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
            "expected E_UNRESOLVED_SLOT under --edition java for a Bedrock-only slot, got {:?}",
            r.diagnostics,
        );
    }

    #[test]
    fn per_edition_check_unions_slots_across_variants() {
        // AC10: resolve(ir, None) on a file with `_java` + `_bedrock` variants
        // must accept a `mat_slot=X` when X is declared by *either* variant.
        let src = [
            "theme t_java:",
            "  slot floor -> @oak_planks",
            "  slot java_only -> @spruce_planks",
            "",
            "theme t_bedrock:",
            "  slot floor -> @oak_planks",
            "  slot bedrock_only -> @dark_oak_planks",
            "",
            "struct s size=4x4",
            "  floor mat_slot=java_only",
            "  floor mat_slot=bedrock_only",
            "",
        ]
        .join("\n");
        let r = resolve(&ir(&src), None);
        assert!(
            !r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
            "sibling-slot union should suppress E_UNRESOLVED_SLOT for either variant's slot, got {:?}",
            r.diagnostics,
        );
    }

    #[test]
    fn per_edition_check_still_errors_on_slot_absent_from_all_variants() {
        // The union softens `mat_slot=X` only when X is declared *somewhere*.
        // A slot declared by no variant of the logical theme is still an
        // error, so a genuine typo is still caught under `cairn check`.
        let src = [
            "theme t_java:",
            "  slot floor -> @oak_planks",
            "",
            "theme t_bedrock:",
            "  slot floor -> @oak_planks",
            "",
            "struct s size=4x4",
            "  floor mat_slot=totally_bogus",
            "",
        ]
        .join("\n");
        let r = resolve(&ir(&src), None);
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
            "expected E_UNRESOLVED_SLOT for a slot no variant declares, got {:?}",
            r.diagnostics,
        );
    }

    #[test]
    fn per_edition_falls_back_to_unsuffixed_when_variant_missing() {
        // `theme t:` (no variant suffix) is the shared-default form used
        // by every existing example. It must resolve for both editions so
        // the current corpus keeps compiling untouched.
        let src = [
            "theme t:",
            "  slot floor -> @oak_planks",
            "",
            "struct s size=4x4",
            "  floor mat_slot=floor",
            "",
        ]
        .join("\n");
        for edition in [Some(Edition::Java), Some(Edition::Bedrock), None] {
            let r = resolve(&ir(&src), edition);
            let scope = r.scopes.get("struct::s").unwrap();
            assert_eq!(
                scope.bound_theme.as_deref(),
                Some("t"),
                "edition={edition:?}"
            );
            assert!(
                r.diagnostics.is_empty(),
                "edition={edition:?}: got {:?}",
                r.diagnostics,
            );
        }
    }

    #[test]
    fn per_edition_java_preferred_over_bedrock_when_unsuffixed_absent() {
        // `edition = None` prefers the unsuffixed variant, then Java, then
        // Bedrock — the deterministic order avoids leaking source order
        // into diagnostics when only variants exist.
        let src = [
            "theme t_bedrock:",
            "  slot floor -> @dark_oak_planks",
            "",
            "theme t_java:",
            "  slot floor -> @oak_planks",
            "",
            "struct s size=4x4",
            "  floor mat_slot=floor",
            "",
        ]
        .join("\n");
        let r = resolve(&ir(&src), None);
        let scope = r.scopes.get("struct::s").unwrap();
        assert_eq!(scope.bound_theme.as_deref(), Some("t_java"));
    }

    #[test]
    fn distinct_logical_themes_still_leave_scope_unbound() {
        // Two truly distinct logical themes (`cottage` and `keep`) remain
        // the multi-theme deferred-selection case — the `_java`/`_bedrock`
        // grouping must not collapse them into one.
        let src = [
            "theme cottage:",
            "  slot floor -> @oak_planks",
            "",
            "theme keep:",
            "  slot floor -> @stone_bricks",
            "",
            "struct s size=4x4",
            "  floor mat_slot=floor",
            "",
        ]
        .join("\n");
        let r = resolve(&ir(&src), Some(Edition::Java));
        let scope = r.scopes.get("struct::s").unwrap();
        assert!(
            scope.bound_theme.is_none(),
            "two distinct logical themes must remain unbound, got {:?}",
            scope.bound_theme,
        );
    }

    #[test]
    fn per_edition_java_does_not_fall_back_to_bedrock_variant() {
        // Silent misrouting guard: a file with only `theme t_bedrock:` must
        // leave the scope unbound under `Some(Edition::Java)` rather than
        // silently binding the Bedrock variant. Any `mat_slot=` reference
        // then surfaces as `E_UNRESOLVED_SLOT`, which is the loud outcome
        // spec §10.4 requires — binding across editions would route
        // Bedrock-only slot values into a Java `.nbt`.
        let src = [
            "theme t_bedrock:",
            "  slot floor -> @dark_oak_planks",
            "",
            "struct s size=4x4",
            "  floor mat_slot=floor",
            "",
        ]
        .join("\n");
        let r = resolve(&ir(&src), Some(Edition::Java));
        let scope = r.scopes.get("struct::s").unwrap();
        assert!(
            scope.bound_theme.is_none(),
            "Some(Java) with only `_bedrock` variant must not bind, got {:?}",
            scope.bound_theme,
        );
        // The unresolved-slot diagnostic is skipped here because no theme
        // is bound at all — matching the multi-theme "no auto-pick" branch
        // in `multiple_themes_leave_struct_unbound`. That branch is the
        // authority on unbound-scope semantics; the AC to pin under a
        // downstream compile is that lowering treats an unbound scope's
        // `mat_slot` as an unresolved abstract material.
        assert!(
            !r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
        );
    }

    #[test]
    fn per_edition_bedrock_does_not_fall_back_to_java_variant() {
        // Symmetric guard for the opposite direction — a file with only
        // `theme t_java:` must not bind under `Some(Edition::Bedrock)`.
        let src = [
            "theme t_java:",
            "  slot floor -> @oak_planks",
            "",
            "struct s size=4x4",
            "  floor mat_slot=floor",
            "",
        ]
        .join("\n");
        let r = resolve(&ir(&src), Some(Edition::Bedrock));
        let scope = r.scopes.get("struct::s").unwrap();
        assert!(scope.bound_theme.is_none());
    }

    #[test]
    fn strip_edition_suffix_recognises_both_editions() {
        assert_eq!(
            strip_edition_suffix("shop_java"),
            ("shop", Some(Edition::Java))
        );
        assert_eq!(
            strip_edition_suffix("shop_bedrock"),
            ("shop", Some(Edition::Bedrock)),
        );
        assert_eq!(strip_edition_suffix("medieval"), ("medieval", None));
        // Names that happen to end with a similar substring but are not
        // suffixed with the closed edition set remain unsuffixed.
        assert_eq!(strip_edition_suffix("javanese"), ("javanese", None));
    }
}
