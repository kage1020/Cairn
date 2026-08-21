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
use crate::check::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::edition::Edition;
use crate::error::Span;
use crate::ids::{IdError, PlaceId, PortId, SiteName};
use crate::intent::{
    ConnectEnd, DefIr, IntentModule, Member, MemberBody, MemberRole, SelectorRule, SiteIr,
    StructIr, ThemeIr, ValueWithSpan, role_of,
};
use crate::prose::{and_list, selector_text};
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
    // Logical themes already reported as unbindable under the pin, so the
    // module-level pick and every `place` naming one say it once between
    // them rather than once each.
    let mut reported_missing: HashSet<String> = HashSet::new();

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
            edition,
            &mut reported_missing,
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
/// Java `.nbt`. Returning `None` instead is reported as
/// `E_THEME_VARIANT_MISSING` by both callers — not as `E_UNRESOLVED_SLOT`,
/// which needs a bound theme to say the slot is missing from and would
/// blame a slot that is declared and spelled correctly.
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

/// What a `place ... theme=NAME` reference resolves to under `edition`.
enum ThemeReference<'a> {
    /// Bind this theme.
    Bound {
        /// The theme actually bound — not necessarily the name written.
        name: &'a str,
        /// How the reference was spelled, which is what decides whether the
        /// author asked about one variant or about the logical theme.
        spelling: Spelling,
    },
    /// Variants of this logical theme are declared, but none can bind under
    /// the edition this carries.
    NoVariantForEdition(Edition),
    /// No theme in the module shares this name's logical part — or the name
    /// carries a suffix, nothing is declared under it, and no edition is
    /// pinned to justify picking another variant.
    Unknown,
}

/// How a `theme=` reference was written.
///
/// Two booleans said this before — one for "carried a suffix", one for its
/// negation — which left `rebound && logical_spelling` expressible and made
/// the edition behind a rebind reachable only through an `expect`. It was
/// not a sound one: a suffixed name nothing declares reached it with no
/// edition pinned, and `cairn check` panicked on a typo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spelling {
    /// Written without an edition suffix, so it names the logical theme.
    /// Sibling variants may soften its slot diagnostics while no edition is
    /// picked.
    Logical,
    /// Written with an edition suffix, so it names one variant. The author
    /// asked about that variant's slots and no sibling softens them.
    /// `rebound_under` is `Some(edition)` when that pin bound a different
    /// variant than the one named.
    Variant { rebound_under: Option<Edition> },
}

/// Resolve a `place ... theme=NAME` reference against the pinned edition.
///
/// The site path used to bind `NAME` verbatim whenever the module declared
/// it, which made it the one route into a scope that [`pick_variant`] did
/// not guard: `theme=shop_bedrock` bound under `--edition java` and wrote
/// Bedrock-only slot values into a Java `.nbt`. Every reference now goes
/// through the same variant selection the module-level auto-pick uses, so a
/// pin means the same thing wherever the theme was chosen.
///
/// A reference is read as naming the *logical* theme, which is what spec
/// versioning-editions §10.7 asks the semantic layer to name. `theme=shop`
/// consequently resolves in a module that declares only `shop_java` and
/// `shop_bedrock` — before this it was `E_UNRESOLVED_THEME_REF`, so the
/// spelling the spec prescribes was the one spelling that did not work.
///
/// Without a pin, nothing re-picks a variant the author named. A declared
/// name binds verbatim; a *suffixed* name nothing declares is unknown, the
/// same answer a misspelled theme has always had. Substituting a sibling
/// there would swap a variant on `cairn lower`'s say-so, which is exactly
/// what this function exists to stop a pin from doing silently.
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
    if edition.is_none() {
        return match themes.get_key_value(written) {
            Some((name, _)) => ThemeReference::Bound {
                name: name.as_str(),
                spelling: match written_variant {
                    Some(_) => Spelling::Variant {
                        rebound_under: None,
                    },
                    None => Spelling::Logical,
                },
            },
            // A logical name still resolves with no pin — that is the
            // spelling §10.7 asks for, and `pick_variant`'s unpinned order
            // is deterministic. A suffixed one does not: it names a variant
            // the module does not have.
            None if written_variant.is_some() => ThemeReference::Unknown,
            None => match pick_variant(themes, logical, None) {
                Some(name) => ThemeReference::Bound {
                    name,
                    spelling: Spelling::Logical,
                },
                // Unreachable: the guard above found a variant of `logical`,
                // and the unpinned arm of `pick_variant` accepts all three.
                None => ThemeReference::Unknown,
            },
        };
    }
    let pinned = edition.expect("the unpinned case returned above");
    match pick_variant(themes, logical, edition) {
        Some(name) => ThemeReference::Bound {
            name,
            spelling: match written_variant {
                Some(_) if name != written => Spelling::Variant {
                    rebound_under: Some(pinned),
                },
                Some(_) => Spelling::Variant {
                    rebound_under: None,
                },
                None => Spelling::Logical,
            },
        },
        None => ThemeReference::NoVariantForEdition(pinned),
    }
}

/// Resolve a `place ... theme=NAME` to the theme to bind and the sibling
/// slots that may soften its diagnostics, reporting whichever way it failed.
///
/// `None` means the placement cannot be built and the reason is already in
/// `diagnostics`; the caller skips the scope so the lowering pass does not
/// emit an artifact for a placement whose materials were never decided.
fn bind_place_theme(
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
            diagnostics.push(unresolved_theme_ref_diag(
                written,
                span.clone(),
                declared_names.iter().map(String::as_str),
            ));
            None
        }
        ThemeReference::NoVariantForEdition(pinned) => {
            // One cause, one report — every `place` naming this theme, and
            // the module-level pick before them, ask the author for the same
            // edit in the same `theme` block. The placement is still refused;
            // what is deduplicated is the sentence, not the consequence.
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
            // Sibling-variant slot union applies under the same edition
            // condition the top-level scope loop uses, and for the same
            // reason — it softens `E_UNRESOLVED_SLOT` only while no edition
            // has been picked — plus one this path adds: the reference must
            // name the logical theme. Having named one variant, the author
            // asked about that variant's slots, and softening them against a
            // sibling answers a question they did not ask.
            let siblings = if edition.is_none() && spelling == Spelling::Logical {
                sibling_slot_names(themes, logical, &name)
            } else {
                HashSet::new()
            };
            Some((name, siblings))
        }
    }
}

/// Whether any struct or def member anywhere in the module reads a
/// `mat_slot=`.
///
/// The module-level auto-pick binds a theme for every struct and def scope,
/// but a scope only *needs* one to read a slot from. Without this, declaring
/// a `_bedrock` theme and never using it made `--edition java` a hard error
/// on a module whose output does not contain a single block of air.
fn any_member_reads_a_slot(ir: &IntentModule) -> bool {
    fn any(members: &[Member]) -> bool {
        members
            .iter()
            .any(|m| m.mat_slot.is_some() || any(&m.children.members))
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
fn theme_variant_missing_diag(
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
                          spec/versioning-editions.md §10.4 forbids"
                    .to_owned(),
            },
        ],
        data: None,
    }
}

/// A `theme=` named one variant and the pin bound another.
///
/// `declared` separates the two ways that happens: the named variant exists
/// and the pin preferred its own, or the named variant does not exist at all
/// and the pin fell back to what it could reach (the unsuffixed theme). The
/// second reads as a plain mistake and should not be described as a choice
/// between variants.
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
                "write `theme={logical}` — spec/versioning-editions.md §10.7 keeps the semantic \
                 layer edition-neutral and lets the variant follow the build",
            ),
        }],
        data: None,
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
    edition: Option<Edition>,
    reported_missing: &mut HashSet<String>,
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

        let Some(place_id) = usable_place_id(member, &site.name, &seen_place_ids, diagnostics)
        else {
            continue;
        };
        seen_place_ids.insert(place_id.to_owned(), member.span.clone());

        // Validate origin selectors before any cross-scope lookup so the
        // user sees the structural problem first. An invalid origin makes
        // the rest of the placement unsalvageable — skip the def/theme
        // resolution and the scope insert so the lowering pass does not
        // emit a `.nbt` for a structurally rejected placement.
        if !validate_place_origin(
            member,
            &site.name,
            Some(place_id),
            &seen_place_ids,
            diagnostics,
        ) {
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
        let Some(def) = defs.iter().find(|d| d.name == use_name) else {
            diagnostics.push(unresolved_place_ref_diag(
                &format!("`use={use_name}` references an unknown def"),
                member.span.clone(),
                use_name,
                def_names.iter().copied(),
            ));
            continue;
        };
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
        let Some((bound_theme, siblings)) = bind_place_theme(
            theme_name,
            &member.span,
            edition,
            themes,
            &theme_names,
            reported_missing,
            diagnostics,
        ) else {
            continue;
        };

        // Cross-scope resolve: run the def's members under the picked theme,
        // even when the file has multiple themes (the per-place `theme=`
        // wins over the single-theme heuristic).
        let resolution = resolve_struct_or_def(
            &def.members,
            Some(bound_theme.as_str()),
            &siblings,
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
        // registered but its `place_def` entry never landed. Every arm in
        // `resolve_site_placements` that skips the insert reports the row
        // itself: an absent `use=` / `theme=` (`E_INCOMPLETE_PLACE`), a
        // mistyped one (`E_TYPE_MISMATCH_LABEL`), a failed origin selector
        // (`E_INVALID_PLACE_ORIGIN`), a `use=` naming an unknown def
        // (`E_UNRESOLVED_PLACE_REF`), a `theme=` naming an unknown theme
        // (`E_UNRESOLVED_THEME_REF`). There is no silent camp left.
        //
        // The cascade is not a duplicate of any of them. Each says which
        // row is broken; this says which walkway went with it, which
        // nothing else does — mirroring the `W_DEFERRED_MEMBER` cascade
        // used for walkway endpoints in `block_array::lower`. It also keeps
        // a future refactor that drops a normal-path `place_def.insert`
        // from silently breaking every walkway.
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::DeferredConnect,
            span: port.span.clone(),
            primary: format!(
                "`connect` target `{place_id}.{port_id}` names a `place` row that did not \
                 resolve; no walkway laid",
                place_id = port.place,
                port_id = port.port,
            ),
            notes: vec![DiagnosticNote {
                span: None,
                message: "that row carries its own diagnostic saying what is wrong with it; \
                          fixing it is what lays this walkway"
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
/// `place_id` is `None` for a row that declared no `id=`. Such a row is
/// already reported as incomplete and can never be referenced, but its
/// origin selector is checked anyway so every problem on the line surfaces
/// together — the same reason the missing-key finding lists all three keys
/// at once.
fn validate_place_origin(
    member: &Member,
    site_name: &str,
    place_id: Option<&str>,
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
        if !seen_place_ids.contains_key(target) || Some(target) == place_id {
            // Suggestion pool is *prior* place ids only — pointing at a
            // later place would let cycles slip in. The same-site exclusion
            // keeps `east_of=self` from showing up as a viable suggestion.
            let prior: Vec<&str> = seen_place_ids
                .keys()
                .filter(|id| Some(id.as_str()) != place_id)
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

/// Validate everything about a `place` row that can be judged from the row
/// alone, and yield the id the rest of the loop keys on.
///
/// `None` means the row cannot become a placement and every reason has been
/// reported. Split out from `resolve_site_placements` because it is the one
/// stretch that needs nothing but the row and the ids seen before it — the
/// cross-scope work below needs the def list, the theme map, and three
/// output maps besides.
///
/// Ordering is load-bearing. The completeness check runs first so the
/// author sees every key the row is short of before any consequence of one
/// of them, and the origin selector is checked on both paths out: without
/// that, adding the `id=` this function just asked for would surface a
/// *new* error on the line the author had only now fixed, which is the
/// re-run cycle listing all three keys at once exists to avoid.
fn usable_place_id<'a>(
    member: &'a Member,
    site_name: &str,
    seen_place_ids: &IndexMap<String, Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<&'a str> {
    if let Some(diagnostic) = incomplete_place_diag(member, site_name) {
        diagnostics.push(diagnostic);
    }

    // An unnamed placement has nothing to register, cannot be referenced by
    // `east_of` / `north_of`, and is unreachable from any `connect` row (a
    // dot-ref needs a name on the left side), so there is nothing further to
    // do with it once its own line has been judged.
    let Some(place_id) = member.id.as_deref() else {
        validate_place_origin(member, site_name, None, seen_place_ids, diagnostics);
        return None;
    };

    // Validate before the id becomes half of a scope key. `PlaceId` states
    // the invariants, and `place_scope_key` joins on `::`, so an id carrying
    // `.` or `:` produces a key nothing can parse back — which is where the
    // lowering pass used to `expect` and panic.
    if let Err(err) = PlaceId::new(place_id) {
        diagnostics.push(invalid_place_id_diag(
            place_id,
            site_name,
            member.span.clone(),
            &err,
        ));
        return None;
    }
    if let Some(first) = seen_place_ids.get(place_id) {
        diagnostics.push(duplicate_place_id_diag(
            site_name,
            place_id,
            first,
            &member.span,
        ));
        return None;
    }
    Some(place_id)
}

/// Keys a `place` row must declare, paired with what each one is for.
///
/// Ordered as an author writes them, and the order is load-bearing twice
/// over: it fixes both the sentence the message builds and the `missing`
/// list in the structured payload, so the same source always renders the
/// same text.
///
/// Carrying the purpose here rather than in a `match` on the key is what
/// keeps a fourth required key (`at=` is the realistic candidate) from
/// inheriting some other key's note through a wildcard arm.
const REQUIRED_PLACE_KEYS: &[(&str, &str)] = &[
    (
        "id",
        "`id=` is the name every `east_of=` and `connect` refers to, and the name this placement's `.nbt` is written under — the compiler has no name to invent for it",
    ),
    (
        "use",
        "`use=DEF` names the `def` this placement instantiates",
    ),
    (
        "theme",
        "`theme=NAME` names the theme this placement's `mat_slot=` members resolve against",
    ),
];

/// Whether the row carries `key=` at all, regardless of what its value is.
///
/// `intent::lower` hoists a label-shaped `id=` out of `intent_state` onto
/// [`Member::id`], so `id=b` lands in one of the two and `id=3` — not
/// label-shaped, so not hoisted — lands in the other. Every other `place`
/// key stays in `intent_state` either way. Asking both is what keeps a
/// mistyped key from being reported as an absent one, which would send the
/// author to add a key already on the line.
fn declares(member: &Member, key: &str) -> bool {
    member.intent_state.contains_key(key) || (key == "id" && member.id.is_some())
}

/// Report a `place` row missing any of the keys it needs to become a
/// placement, or `None` when it declares all three.
///
/// `id=` names the `.nbt` the compiler writes for this placement
/// (`spec/components-editing-sites.md` §9.3.4) and is the name `east_of=`
/// and `connect` refer to — so it cannot be auto-assigned the way
/// `spec/components-editing-sites.md` §9.2 auto-assigns a geometry
/// member's address, which derives from parent / role / side / level /
/// offset and names nothing outside the body it sits in. `use=` names the
/// `def` the placement instantiates and `theme=` the theme its `mat_slot=`
/// members resolve against; without either there is no volume to voxelise.
fn incomplete_place_diag(member: &Member, site_name: &str) -> Option<Diagnostic> {
    let missing: Vec<&(&str, &str)> = REQUIRED_PLACE_KEYS
        .iter()
        .filter(|(key, _)| !declares(member, key))
        .collect();
    let quoted: Vec<String> = missing.iter().map(|(key, _)| format!("`{key}=`")).collect();
    // Returning through `and_list`'s `None` rather than an early `is_empty`
    // guard keeps the "nothing missing" exit and the "nothing to join" exit
    // as one branch — and keeps a `place` constructor from panicking while
    // reporting somebody else's error.
    let listed = and_list(&quoted)?;
    // Named when the row has one, matching `E_INVALID_PLACE_ORIGIN` and
    // `E_DUPLICATE_PLACE_ID`: two incomplete rows in one site would
    // otherwise render byte-identical primaries.
    let subject = member
        .id
        .as_deref()
        .map_or_else(|| "`place`".to_owned(), |id| format!("`place id={id}`"));
    Some(Diagnostic {
        code: DiagnosticCode::IncompletePlace,
        span: member.span.clone(),
        primary: format!(
            "{subject} in site `{site_name}` is missing {listed}, so no placement is built for it",
        ),
        notes: missing
            .iter()
            .map(|(_, purpose)| DiagnosticNote {
                span: None,
                message: (*purpose).to_owned(),
            })
            .collect(),
        // The key set is what a quick-fix needs, and `spec/lint.md` §11.2
        // asks consumers to match on `(code, data.kind)` rather than parse
        // the prose it is also rendered into.
        data: Some(DiagnosticData::IncompletePlace {
            missing: missing.iter().map(|(key, _)| (*key).to_owned()).collect(),
        }),
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

fn invalid_place_origin_diag(place_id: Option<&str>, span: Span, message: &str) -> Diagnostic {
    // A row with no `id=` is already reported as incomplete; it still gets
    // this finding, and quoting a name it does not have would be worse than
    // quoting none.
    let subject = place_id.map_or_else(
        || "`place`".to_owned(),
        |place_id| format!("`place id={place_id}`"),
    );
    Diagnostic {
        code: DiagnosticCode::InvalidPlaceOrigin,
        span,
        primary: format!("invalid origin selector on {subject}: {message}"),
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

/// Whether two selector rows pick out the same members — in this file and
/// in any other.
///
/// [`selector_matches`] cannot tell two rows apart when they carry the same
/// keyword (the comparison is string equality against `MemberRole::keyword`)
/// and attribute maps with the same keys whose values are interchangeable
/// under [`member_attr_matches`]. Interchangeability is per key rather than
/// per value: `class=small` and `class="small"` name one label and select
/// alike, while `side=front` and `side="front"` are two [`ValueKind`]s and
/// select disjoint sets.
///
/// The relation is symmetric and transitive but **not** reflexive: a
/// label key holding a non-label value takes the `false` arm of
/// [`attr_values_select_alike`], so `select_the_same_members(r, r)` is
/// false for `window[id=5]`. A partial equivalence is all the grouping in
/// `check::duplicate` needs — symmetry and transitivity are what let a new
/// row be compared against one representative instead of every row in the
/// group, and a row unrelated to itself opens a group nothing ever joins,
/// which reports nothing. A reader who takes "equivalence" at face value
/// and optimises on it (swapping the representative for an arbitrary
/// member, short-circuiting the self-comparison) would be relying on a
/// property this does not have.
///
/// Lives beside the matcher rather than in the pass that reports duplicate
/// rows, so the answer stays derived from the rule it is about.
pub(crate) fn select_the_same_members(a: &SelectorRule, b: &SelectorRule) -> bool {
    a.keyword == b.keyword
        && a.attrs.len() == b.attrs.len()
        && a.attrs.iter().all(|(key, value)| {
            b.attrs
                .get(key)
                .is_some_and(|other| attr_values_select_alike(key, &value.value, &other.value))
        })
}

/// Whether swapping one selector attribute value for the other leaves
/// [`member_attr_matches`] answering the same for every member under `key`.
///
/// The split is [`LABEL_ATTRS`], the same table that function reads.
/// A label attribute goes through [`value_eq_label`], which takes an
/// `Ident` or a `Str` carrying the same text; every other key is compared
/// against an [`crate::intent::IntentState`] entry by [`ValueKind`], where
/// the two spellings are different values.
/// `ds_*_value_form_matters_exactly_where_the_matcher_says_it_does` in
/// `tests/check_duplicate_selector.rs` pins that pair of answers from the
/// outside, by checking `E_THEME_SELECTOR_UNMATCHED` alongside the
/// finding.
///
/// The `false` for a non-label value under a label key is the accurate
/// answer rather than a conservative one: [`value_eq_label`] rejects such a
/// value for every member, so neither row binds anything the other could
/// take over. Those rows are already an error by a different scope —
/// `check::type_mismatch` covers the same three keys and reports
/// `E_TYPE_MISMATCH_LABEL` on each of them — so the gap says nothing that
/// goes unsaid. `ds_*_a_label_key_holding_a_non_label_value_pairs_with_nothing`
/// pins the pairing. (Named by its half rather than its number: the `ds_`
/// numbering in that file is not in file order.)
///
/// The [`ValueKind`] comparison is by value at every depth, including
/// inside a `ValueKind::List`: [`Value`]'s equality is its kind's, so two
/// lists spelled identically on two lines select alike. That is the same
/// answer [`member_attr_matches`] gives, which is what makes "these two
/// rows select the same members" and "this row selects this member" agree
/// about what a value is.
fn attr_values_select_alike(key: &str, a: &Value, b: &Value) -> bool {
    if label_attr(key).is_none() {
        return a.kind == b.kind;
    }
    match (&a.kind, &b.kind) {
        (
            ValueKind::Ident(left) | ValueKind::Str(left),
            ValueKind::Ident(right) | ValueKind::Str(right),
        ) => left == right,
        _ => false,
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

/// Reads the [`Member`] field one label selector attribute filters on.
/// The elided lifetime is the higher-ranked one, so the borrow of the
/// returned label is the borrow of the member it came off.
type LabelField = fn(&Member) -> Option<&str>;

/// The selector attributes lowering lifts out of
/// [`crate::intent::IntentState`] and onto [`Member`]'s own fields, paired
/// with the field each one filters on.
///
/// One table rather than one list per reader. [`member_attr_matches`] wants
/// the accessor and [`attr_values_select_alike`] wants the membership; a key
/// added to only one of them would let the duplicate check disagree with the
/// matcher it is derived from. (`check::type_mismatch`'s `LABEL_KEYS` is a
/// third list and a deliberate superset — it also covers `use=` and
/// `theme=`, which are not selector attributes at all.)
const LABEL_ATTRS: [(&str, LabelField); 3] = [
    ("id", |member| member.id.as_deref()),
    ("class", |member| member.class.as_deref()),
    ("mat_slot", |member| member.mat_slot.as_deref()),
];

/// The accessor for `key`, or `None` when `key` is an ordinary
/// `key=value` living in [`crate::intent::IntentState`].
fn label_attr(key: &str) -> Option<LabelField> {
    LABEL_ATTRS
        .iter()
        .find(|(name, _)| *name == key)
        .map(|(_, field)| *field)
}

/// Compare a selector attribute's expected value against the corresponding
/// member field. A [`LABEL_ATTRS`] key is compared string-vs-`Ident`/`Str`;
/// everything else is a generic `key=value` arg that lives in
/// [`crate::intent::IntentState`] and compares by [`ValueKind`].
fn member_attr_matches(member: &Member, key: &str, expected: &Value) -> bool {
    let Some(field) = label_attr(key) else {
        return member
            .intent_state
            .get(key)
            .is_some_and(|actual| actual.value.kind == expected.kind);
    };
    field(member).is_some_and(|actual| value_eq_label(expected, actual))
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
                        "theme selector `{selector}` in `{theme}` does not match any member",
                        selector = selector_text(&sel.keyword, &sel.attrs),
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

    /// The warning names the row's attributes rather than eliding them to
    /// `[...]`. One theme can hold several rows on one keyword, and a
    /// message that cannot tell them apart makes the reader match spans by
    /// hand. `check::duplicate` renders the same way, through the same
    /// helper, so two findings on one row spell the selector alike.
    #[test]
    fn unmatched_selector_names_the_attributes_it_filtered_on() {
        let src = "theme t:\n  slot wall -> @cobblestone\n  \
             walls[class=does_not_exist] -> trim=@a\n  \
             walls[class=\"nor_this\",side=front] -> trim=@b\n\n\
             struct s size=4x4\n  walls class=outer mat_slot=wall height=3\n";
        let r = resolve(&ir(src), None);
        let primaries: Vec<&str> = r
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::ThemeSelectorUnmatched)
            .map(|d| d.primary.as_str())
            .collect();
        assert_eq!(
            primaries,
            [
                "theme selector `walls[class=does_not_exist]` in `t` does not match any member",
                "theme selector `walls[class=\"nor_this\",side=front]` in `t` does not match any member",
            ],
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
        // silently binding the Bedrock variant, which would route
        // Bedrock-only slot values into a Java `.nbt`. The loud outcome
        // spec §10.4 requires is `E_THEME_VARIANT_MISSING`, asserted below.
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
        // Not `E_UNRESOLVED_SLOT`: the slot is declared and spelled
        // correctly, and that message has to name the theme the slot is
        // missing from — of which there is none, because the pin refused
        // the only one. The theme is what cannot be honoured, so the
        // finding names the theme. Reporting nothing at all is what let a
        // build write the requested extent out of air at exit 0.
        assert!(
            !r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
        );
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ThemeVariantMissing),
            "the refusal must be reported, got {:?}",
            r.diagnostics,
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
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ThemeVariantMissing),
            "got {:?}",
            r.diagnostics,
        );
    }

    // ------------------------------------------------------------------
    // A pin the module cannot satisfy, and a pin the site path ignored.
    // ------------------------------------------------------------------

    /// One logical theme `shop` in the given variants, plus a def and a
    /// site that places it under `theme={reference}`.
    fn placed_under(reference: &str, variants: &[&str]) -> String {
        use std::fmt::Write as _;

        let mut declarations = String::new();
        for variant in variants {
            let value = match *variant {
                "_bedrock" => "dark_oak_planks",
                "_java" => "spruce_planks",
                _ => "oak_planks",
            };
            write!(
                declarations,
                "theme shop{variant}:\n  slot floor -> @{value}\n\n"
            )
            .expect("writing to a String cannot fail");
        }
        format!(
            "{declarations}def hut size=4x4:\n  floor mat_slot=floor\n\nsite s:\n  \
             place id=home use=hut theme={reference} at=origin\n"
        )
    }

    fn placed_scope(r: &Resolution) -> &ScopeResolution {
        r.scopes.get("site::s::home").expect("place scope present")
    }

    /// No finding about variant selection, and no error of any kind.
    ///
    /// Narrower than "no diagnostics at all", which these tests used to
    /// assert: an advisory added later for an unrelated reason would break
    /// them while saying nothing about the theme reference they are about.
    fn nothing_said_about_variants(r: &Resolution) -> bool {
        !r.diagnostics.iter().any(|d| {
            d.severity() == Severity::Error
                || matches!(
                    d.code,
                    DiagnosticCode::ThemeVariantRebound | DiagnosticCode::ThemeVariantMissing
                )
        })
    }

    #[test]
    fn refuses_a_module_whose_only_variant_is_for_the_other_edition() {
        let src = [
            "theme shop_bedrock:",
            "  slot floor -> @dark_oak_planks",
            "",
            "struct s size=4x4",
            "  floor mat_slot=floor",
            "",
        ]
        .join("\n");
        let r = resolve(&ir(&src), Some(Edition::Java));
        let diag = r
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::ThemeVariantMissing)
            .unwrap_or_else(|| panic!("expected the refusal, got {:?}", r.diagnostics));
        assert_eq!(diag.severity(), Severity::Error);
        assert!(
            diag.primary.contains("shop") && diag.primary.contains("java"),
            "the message must name the theme and the pin: {}",
            diag.primary,
        );
        let notes = diag
            .notes
            .iter()
            .map(|n| n.message.as_str())
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            notes.contains("shop_bedrock"),
            "the notes must say which variants do exist: {notes}",
        );
        assert!(
            notes.contains("shop_java") && notes.contains("drop the suffix"),
            "the notes must give both fixes: {notes}",
        );
    }

    #[test]
    fn a_module_variant_is_only_refused_once_however_many_scopes_read_it() {
        // The auto-pick is one decision for the whole module, so repeating
        // the finding per struct would report one cause N times.
        let src = [
            "theme shop_bedrock:",
            "  slot floor -> @dark_oak_planks",
            "",
            "struct a size=4x4",
            "  floor mat_slot=floor",
            "",
            "struct b size=4x4",
            "  floor mat_slot=floor",
            "",
        ]
        .join("\n");
        let r = resolve(&ir(&src), Some(Edition::Java));
        assert_eq!(
            r.diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::ThemeVariantMissing)
                .count(),
            1,
        );
    }

    #[test]
    fn an_unpinned_resolve_refuses_nothing_about_variants() {
        // The same module is fine for `cairn check` and `cairn lower`: with
        // no edition named there is nothing the variant fails to satisfy.
        let src = [
            "theme shop_bedrock:",
            "  slot floor -> @dark_oak_planks",
            "",
            "struct s size=4x4",
            "  floor mat_slot=floor",
            "",
        ]
        .join("\n");
        let r = resolve(&ir(&src), None);
        assert_eq!(
            r.scopes
                .get("struct::s")
                .expect("scope")
                .bound_theme
                .as_deref(),
            Some("shop_bedrock"),
        );
        assert!(
            !r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ThemeVariantMissing),
            "got {:?}",
            r.diagnostics,
        );
    }

    #[test]
    fn a_place_naming_the_other_editions_variant_binds_this_editions() {
        // The defect: `theme=shop_bedrock` bound verbatim under a Java
        // build, so Bedrock-only slot values reached a Java `.nbt` while
        // `pick_variant` — the guard written to prevent exactly that — was
        // never consulted on this path.
        let src = placed_under("shop_bedrock", &["_java", "_bedrock"]);
        let r = resolve(&ir(&src), Some(Edition::Java));
        assert_eq!(placed_scope(&r).bound_theme.as_deref(), Some("shop_java"));
    }

    #[test]
    fn a_rebound_place_theme_says_which_variant_it_bound() {
        let src = placed_under("shop_bedrock", &["_java", "_bedrock"]);
        let r = resolve(&ir(&src), Some(Edition::Java));
        let diag = r
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::ThemeVariantRebound)
            .unwrap_or_else(|| panic!("expected the notice, got {:?}", r.diagnostics));
        assert_eq!(diag.severity(), Severity::Warning);
        assert!(
            diag.primary.contains("shop_bedrock") && diag.primary.contains("shop_java"),
            "both the written and the bound name belong in the message: {}",
            diag.primary,
        );
        assert!(
            diag.notes.iter().any(|n| n.message.contains("theme=shop")),
            "the note must offer the neutral spelling: {:?}",
            diag.notes,
        );
    }

    #[test]
    fn a_place_naming_this_editions_variant_binds_it_without_comment() {
        let src = placed_under("shop_bedrock", &["_java", "_bedrock"]);
        let r = resolve(&ir(&src), Some(Edition::Bedrock));
        assert_eq!(
            placed_scope(&r).bound_theme.as_deref(),
            Some("shop_bedrock")
        );
        assert!(nothing_said_about_variants(&r), "got {:?}", r.diagnostics);
    }

    #[test]
    fn a_place_naming_the_logical_theme_binds_the_pinned_variant() {
        // The spelling spec versioning-editions §10.7 asks for. Before the
        // reference went through variant selection it was the one spelling
        // that did not resolve, because no theme is named plain `shop`.
        let src = placed_under("shop", &["_java", "_bedrock"]);
        for (edition, expected) in [
            (Edition::Java, "shop_java"),
            (Edition::Bedrock, "shop_bedrock"),
        ] {
            let r = resolve(&ir(&src), Some(edition));
            assert_eq!(placed_scope(&r).bound_theme.as_deref(), Some(expected));
            assert!(nothing_said_about_variants(&r), "got {:?}", r.diagnostics);
        }
    }

    #[test]
    fn a_place_naming_no_declared_theme_is_still_unresolved() {
        let src = placed_under("barn", &["_java", "_bedrock"]);
        let r = resolve(&ir(&src), Some(Edition::Java));
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::UnresolvedThemeRef),
            "a name no variant shares is a typo, not an edition problem: {:?}",
            r.diagnostics,
        );
    }

    #[test]
    fn a_place_theme_binds_verbatim_without_a_pin() {
        // Nothing argues for another variant here, and re-picking would let
        // `cairn lower` silently swap the variant the author named.
        let src = placed_under("shop_bedrock", &["_java", "_bedrock"]);
        let r = resolve(&ir(&src), None);
        assert_eq!(
            placed_scope(&r).bound_theme.as_deref(),
            Some("shop_bedrock")
        );
        assert!(nothing_said_about_variants(&r), "got {:?}", r.diagnostics);
    }

    #[test]
    fn a_place_is_refused_when_the_pin_has_no_variant_to_bind() {
        let src = placed_under("shop_bedrock", &["_bedrock"]);
        let r = resolve(&ir(&src), Some(Edition::Java));
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ThemeVariantMissing),
            "got {:?}",
            r.diagnostics,
        );
        assert!(
            r.scopes.get("site::s::home").is_none(),
            "a placement whose theme cannot bind must not become a scope",
        );
    }

    #[test]
    fn an_unsuffixed_theme_binds_under_every_pin_and_says_nothing() {
        let src = placed_under("shop", &[""]);
        for edition in [None, Some(Edition::Java), Some(Edition::Bedrock)] {
            let r = resolve(&ir(&src), edition);
            assert_eq!(
                placed_scope(&r).bound_theme.as_deref(),
                Some("shop"),
                "edition {edition:?}",
            );
            assert!(
                nothing_said_about_variants(&r),
                "edition {edition:?}: {:?}",
                r.diagnostics,
            );
        }
    }

    /// A def whose member reads a slot only the Bedrock variant declares,
    /// placed under `theme={reference}`.
    ///
    /// The unrelated `barn` theme is load-bearing. A def is resolved twice —
    /// once as its own top-level scope, once per placement — so with `shop`
    /// as the module's only logical theme the def's own scope auto-picks a
    /// variant and reports the slot itself. Every assertion below would then
    /// hold whatever the placement path did with its siblings. A second
    /// logical theme suppresses the auto-pick, leaving the `theme=` on the
    /// `place` as the only thing that can bind this member at all.
    fn placed_reading_a_bedrock_only_slot(reference: &str) -> String {
        format!(
            "theme shop_java:\n  slot floor -> @oak_planks\n\n\
             theme shop_bedrock:\n  slot floor -> @oak_planks\n  \
             slot bedrock_only -> @dark_oak_planks\n\n\
             theme barn:\n  slot floor -> @hay_block\n\n\
             def hut size=4x4:\n  floor mat_slot=bedrock_only\n\nsite s:\n  \
             place id=home use=hut theme={reference} at=origin\n"
        )
    }

    #[test]
    fn a_logical_place_theme_is_softened_by_its_siblings_without_a_pin() {
        // Opening the site path to logical names re-opens the door the
        // sibling union exists to hold: with no edition picked, a slot only
        // one variant declares must not error, or a file that compiles
        // cleanly for Bedrock fails `cairn check`.
        let r = resolve(&ir(&placed_reading_a_bedrock_only_slot("shop")), None);
        // The logical name has to bind first, or "no unresolved slot" would
        // hold for the uninteresting reason that no scope was built.
        assert_eq!(placed_scope(&r).bound_theme.as_deref(), Some("shop_java"));
        assert!(
            !r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
            "got {:?}",
            r.diagnostics,
        );
    }

    #[test]
    fn a_pin_makes_a_logical_place_theme_authoritative() {
        let r = resolve(
            &ir(&placed_reading_a_bedrock_only_slot("shop")),
            Some(Edition::Java),
        );
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
            "a pin binds one variant and its slots are the whole answer: {:?}",
            r.diagnostics,
        );
    }

    #[test]
    fn naming_a_variant_explicitly_keeps_its_slots_strict_without_a_pin() {
        // The author asked about `shop_java`'s slots. Softening them
        // against a sibling would answer a question they did not ask.
        let r = resolve(&ir(&placed_reading_a_bedrock_only_slot("shop_java")), None);
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
            "got {:?}",
            r.diagnostics,
        );
    }

    #[test]
    fn a_variant_the_module_does_not_declare_is_unresolved_without_a_pin() {
        // The spelling and the declaration set disagree, and no edition is
        // named to settle it. Re-picking a sibling here would swap a variant
        // on `cairn lower`'s say-so — the very thing the pinned path is
        // written to stop doing silently — so the answer is the one a
        // misspelled theme has always had.
        //
        // This shape was missing from the helpers entirely: every source
        // they built spelled a reference that the variant list contained.
        for (reference, variants) in [
            ("shop_java", ["_bedrock"].as_slice()),
            ("shop_bedrock", ["_java"].as_slice()),
            ("shop_java", [""].as_slice()),
        ] {
            let r = resolve(&ir(&placed_under(reference, variants)), None);
            assert!(
                r.diagnostics
                    .iter()
                    .any(|d| d.code == DiagnosticCode::UnresolvedThemeRef),
                "theme={reference} against {variants:?}: {:?}",
                r.diagnostics,
            );
            assert!(
                r.scopes.get("site::s::home").is_none(),
                "theme={reference} against {variants:?}: no scope may be built",
            );
        }
    }

    #[test]
    fn a_pin_falls_back_to_the_unsuffixed_theme_and_says_the_name_was_not_declared() {
        // `theme=shop_java` with only `theme shop:` declared. The pin has no
        // `_java` variant to prefer, so it binds the unsuffixed theme — the
        // rebind is real, but describing it as "names one edition's variant"
        // would be wrong: that variant is not declared anywhere.
        let src = placed_under("shop_java", &[""]);
        let r = resolve(&ir(&src), Some(Edition::Java));
        assert_eq!(placed_scope(&r).bound_theme.as_deref(), Some("shop"));
        let diag = r
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::ThemeVariantRebound)
            .unwrap_or_else(|| panic!("expected the notice, got {:?}", r.diagnostics));
        assert!(
            diag.primary.contains("is not a declared theme"),
            "the message must not call an undeclared name a variant: {}",
            diag.primary,
        );
    }

    #[test]
    fn a_pin_prefers_the_unsuffixed_theme_over_the_other_editions_variant() {
        // The §10.4 fallback order, exercised through the site path: with
        // `shop` and `shop_bedrock` declared, a Java build binds `shop` and
        // does not cross to `shop_bedrock`.
        let src = placed_under("shop", &["", "_bedrock"]);
        let r = resolve(&ir(&src), Some(Edition::Java));
        assert_eq!(placed_scope(&r).bound_theme.as_deref(), Some("shop"));
        let r = resolve(&ir(&src), Some(Edition::Bedrock));
        assert_eq!(
            placed_scope(&r).bound_theme.as_deref(),
            Some("shop_bedrock")
        );
    }

    #[test]
    fn one_unbindable_theme_is_one_finding_however_many_places_read_it() {
        // The module-level pick and both placements ask for the same edit in
        // the same `theme` block, and the three messages were byte-identical
        // apart from their span.
        let src = concat!(
            "theme shop_bedrock:\n  slot floor -> @dark_oak_planks\n\n",
            "def hut size=4x4:\n  floor mat_slot=floor\n\n",
            "site s:\n",
            "  place id=a use=hut theme=shop at=origin\n",
            "  place id=b use=hut theme=shop east_of=a gap=2\n",
        );
        let r = resolve(&ir(src), Some(Edition::Java));
        assert_eq!(
            r.diagnostics
                .iter()
                .filter(|d| d.code == DiagnosticCode::ThemeVariantMissing)
                .count(),
            1,
            "got {:?}",
            r.diagnostics,
        );
        // Deduplicating the sentence must not smuggle a placement into the
        // build: both are still refused.
        assert!(r.scopes.get("site::s::a").is_none());
        assert!(r.scopes.get("site::s::b").is_none());
    }

    #[test]
    fn a_theme_no_member_reads_a_slot_from_is_not_refused_under_a_pin() {
        // Nothing is starved here — every material is concrete — so the pin
        // costs this module nothing and the build is byte-identical with or
        // without it. Refusing would fail a theme-library file, or any
        // module that does not use `mat_slot=`, on a `--edition` CI job.
        let src = [
            "theme shop_bedrock:",
            "  slot floor -> @dark_oak_planks",
            "",
            "struct s size=4x4",
            "  floor mat=@stone",
            "",
        ]
        .join("\n");
        let r = resolve(&ir(&src), Some(Edition::Java));
        assert!(
            !r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ThemeVariantMissing),
            "got {:?}",
            r.diagnostics,
        );

        // The same module with one `mat_slot=` is refused, so the gate is
        // the slot and not something else about the source.
        let reading = src.replace("floor mat=@stone", "floor mat_slot=floor");
        let r = resolve(&ir(&reading), Some(Edition::Java));
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ThemeVariantMissing),
            "got {:?}",
            r.diagnostics,
        );
    }

    #[test]
    fn a_slot_read_from_a_nested_member_counts_as_reading_one() {
        // `level y=N` groups members, so the only `mat_slot=` in a module
        // can sit one level down. A scan that stops at the top level would
        // let this file build its floor out of air under a pin that cannot
        // bind the theme, which is the outcome the finding exists to stop.
        let src = [
            "theme shop_bedrock:",
            "  slot floor -> @dark_oak_planks",
            "",
            "struct s size=4x4",
            "  level y=1",
            "    floor mat_slot=floor",
            "",
        ]
        .join(
            "
",
        );
        let module = crate::parse(&src).expect("parses");
        let intent = crate::lower(&module);
        assert!(
            any_member_reads_a_slot(&intent),
            "premise: the nested member is the only reader in the module",
        );
        let r = resolve(&intent, Some(Edition::Java));
        assert!(
            r.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::ThemeVariantMissing),
            "got {:?}",
            r.diagnostics,
        );
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
