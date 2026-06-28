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
use crate::check::{Diagnostic, DiagnosticCode, DiagnosticNote, Severity};
use crate::error::Span;
use crate::ids::{PlaceId, PortId, SiteName};
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

    check_slot_targets(&themes, &mut diagnostics);
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
            // Other site-local members (logic, assert) are out of scope here.
            continue;
        }

        // INVARIANT(structural): `id=` is intentionally optional on `place`.
        // An unnamed placement has no scope key to register, cannot be
        // referenced by `east_of` / `north_of`, and is unreachable from
        // any `connect` row (dot-ref needs a name on the left side). No
        // `check` pass — `keyword_allowlist`, `duplicate`, or
        // `type_mismatch` — currently treats a missing `id=` as a
        // structural failure, so the silent skip is by design rather
        // than a missing upstream diagnostic. If `id=` ever becomes
        // mandatory the new `check` pass owns the error and this arm
        // becomes unreachable.
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

        // INVARIANT(structural): `use=` is intentionally optional in the
        // surface grammar — a `place` row may declare just an `id=`
        // without committing to a def (e.g. as a deliberate
        // work-in-progress marker). No `check` pass currently requires
        // `use=`, so emitting a resolver-level error here would tighten
        // the language without an M3 motivation. Skipping the rest of
        // the pipeline prevents `place_def` and the scope map from
        // carrying a half-built entry, so the lowering pass below does
        // not emit a `.nbt` for a placement with no def. Downstream
        // `connect` rows that target this place surface the gap via
        // `validate_port`'s `place_def` miss arm, which emits
        // `W_DEFERRED_CONNECT` so the user sees *why* the walkway was
        // not laid instead of watching it vanish silently.
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

        // INVARIANT(structural): `theme=` is intentionally optional on a
        // per-place basis only for single-theme files — the site-wide
        // heuristic in `resolve_struct_or_def` defaults to the lone
        // theme there, so the user can omit `theme=` without losing
        // coverage. On multi-theme files an omitted `theme=` is a known
        // silent gap: no `check` pass requires it and no targeted
        // diagnostic exists yet (an `E_PLACE_THEME_REQUIRED` pass would
        // be the correct long-term home and is tracked separately).
        // Skipping the rest of the pipeline here leaves `place_def`
        // unset for this place; downstream `connect` rows targeting it
        // surface the gap via the `W_DEFERRED_CONNECT` cascade in
        // `validate_port`, which keeps the multi-theme silent skip
        // visible until the dedicated check pass lands.
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
        // Record this place's def so a later `connect` row can look up
        // the def's members without re-walking the site body.
        place_def.insert(place_id.to_owned(), use_name.to_owned());
    }
}

/// Validate one `connect from.port to to.port path=@MAT` row and push a
/// matching [`ResolvedConnect`] when both ends and the path material
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
    let from_value = member.positional.first();
    let to_value = member.positional.get(2);
    let (Some(from_value), Some(to_value)) = (from_value, to_value) else {
        // INVARIANT(upstream-diagnosed): `check::connect_arity` emits
        // `E_CONNECT_ARITY` for any `connect` row whose positional
        // shape is not `FROM.PORT to TO.PORT`, so the top-level
        // `check` pipeline never reaches this arm with a well-formed
        // module. Library callers that invoke `resolve(ir)` directly
        // (LSP fast paths, ad-hoc tooling) still benefit from the
        // silent return: it keeps walkway voxelisation from picking up
        // a half-formed row instead of panicking on a partial parse.
        // Pinned by `tests/silent_skip_arms.rs` (resolver-only) and
        // `tests/check_connect_arity.rs` (full pipeline).
        return;
    };
    // Lift both ends before short-circuiting so a row with two broken
    // halves earns two diagnostics, not just the first. `validate_port`
    // already follows the same accumulate-then-decide pattern below.
    let from = port_ref_from_value(from_value, "from", seen_place_ids, diagnostics);
    let to = port_ref_from_value(to_value, "to", seen_place_ids, diagnostics);
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
            severity: Severity::Error,
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
            severity: Severity::Error,
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
/// place in the same site and returning `None` on any non-`DotRef`
/// shape. `side` ("from" / "to") goes into the primary diagnostic so
/// the user can tell the two ends apart at a glance.
fn port_ref_from_value(
    raw: &Value,
    side: &str,
    seen_place_ids: &IndexMap<String, Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<PortRef> {
    let ValueKind::DotRef(dot) = &raw.kind else {
        // INVARIANT(upstream-diagnosed): `connect <from> to <to>` only
        // accepts a `Value` whose `ValueKind` is `DotRef`. Any other shape
        // — bare literal, token, call — fails the surface grammar at parse
        // time or the connect-member discriminator in `intent::lower`,
        // which emits its own structural diagnostic before this row
        // reaches the resolver. Re-pushing here would double-count a
        // single malformed row. Verification of the upstream emission is
        // parser-level and cannot be checked from inside this
        // `Vec<Diagnostic>`-scoped pass, so the contract is enforced by
        // the doc comment plus the grammar tests in `tests/parse_*.rs`
        // (a debug_assert here would tangle the resolver's API with the
        // parser's diagnostic timing, which is intentionally kept
        // outside this pass's responsibility).
        return None;
    };
    if dot.tail().is_empty() {
        // `connect home1 to ...` — the user named a place but no port.
        // Treat it as an unresolved port so the diagnostic carries the
        // same `did you mean` shape a typo would have on the right side
        // of the dot. Suggestion pool is empty because we have no def
        // yet (the head place itself may still be unknown), so the user
        // just gets the missing-port message.
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::UnresolvedPort,
            severity: Severity::Error,
            span: raw.span.clone(),
            primary: format!(
                "`{side}` is missing a port id: write `{place}.PORT` to name a member of the placed def",
                place = dot.head(),
            ),
            notes: vec![],
            data: None,
        });
        return None;
    }
    let place_str = dot.head();
    let port_str = dot.tail()[0].as_str();
    if !seen_place_ids.contains_key(place_str) {
        let prior: Vec<&str> = seen_place_ids.keys().map(String::as_str).collect();
        diagnostics.push(unresolved_place_ref_diag(
            &format!("`{side}={place_str}.{port_str}` does not name a prior place in this site"),
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
            severity: DiagnosticCode::DeferredConnect.severity(),
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
                severity: Severity::Error,
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
                severity: Severity::Error,
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
        severity: Severity::Error,
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
        severity: Severity::Error,
        span,
        primary: format!("`theme={theme}` is not a declared theme"),
        notes,
        data: None,
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
        data: None,
    }
}

fn invalid_place_origin_diag(place_id: &str, span: Span, message: &str) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::InvalidPlaceOrigin,
        severity: Severity::Error,
        span,
        primary: format!("invalid origin selector on `place id={place_id}`: {message}"),
        notes: vec![],
        data: None,
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
        data: None,
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
                    data: None,
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
                    data: None,
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
        // A theme is picked at the `place ... theme=X` boundary; until
        // that binding runs, the selectors are not "unmatched", they're
        // "not yet bound".
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
        let r = resolve(&ir(src));
        assert!(
            !r.diagnostics.iter().any(|d| d.severity == Severity::Error),
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
        let r = resolve(&ir(src));
        let diag = r
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::UnresolvedPort)
            .unwrap_or_else(|| panic!("expected E_UNRESOLVED_PORT, got {:?}", r.diagnostics));
        assert_eq!(diag.severity, Severity::Error);
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
        let r = resolve(&ir(src));
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
        let r = resolve(&ir(src));
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
        let r = resolve(&ir(src));
        let diag = r
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::MissingPathMaterial)
            .unwrap_or_else(|| panic!("expected E_MISSING_PATH_MATERIAL, got {:?}", r.diagnostics));
        assert_eq!(diag.severity, Severity::Error);
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
        let r = resolve(&ir(src));
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
        let r = resolve(&ir(src));
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
        let r = resolve(&ir(src));
        let diag = r
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::UnresolvedPort)
            .unwrap_or_else(|| panic!("expected E_UNRESOLVED_PORT, got {:?}", r.diagnostics));
        assert_eq!(diag.severity, Severity::Error);
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
        let r = resolve(&ir(src));
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

    #[test]
    fn unknown_slot_target_emits_warning() {
        let src = "theme t:\n  slot wall -> plain_ident\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
        let r = resolve(&ir(src));
        assert!(r.diagnostics.iter().any(
            |d| d.code == DiagnosticCode::UnknownSlotTarget && d.severity == Severity::Warning
        ),);
    }
}
