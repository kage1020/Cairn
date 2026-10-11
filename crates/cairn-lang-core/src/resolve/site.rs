//! A `site` body: each `place` row checked, then resolved against the
//! `def` it instantiates under the theme it names, and the `def`s no row
//! ever places.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::edition::Edition;
use crate::error::Span;
use crate::ids::{FORBIDDEN_IDENT_PUNCTUATION, IdError, PlaceId, shown_char};
use crate::intent::{DefIr, Member, MemberRole, SiteIr};
use crate::prose::and_list;
use crate::suggest::{did_you_mean_note, nearest_match};

use super::connect::resolve_connect_row;
use super::resolution::{ScopeResolution, ValidatedConnect};
use super::resolver::{ResolveCtx, place_scope_key, resolve_struct_or_def};
use super::theme_reference::bind_place_theme;

pub(super) fn resolve_site_placements(
    site: &SiteIr,
    defs: &[DefIr],
    edition: Option<Edition>,
    scopes: &mut IndexMap<String, ScopeResolution>,
    used_defs: &mut HashSet<String>,
    connects: &mut Vec<ValidatedConnect>,
    ctx: &mut ResolveCtx<'_>,
) {
    // Local index lets each `east_of=ID` / `north_of=ID` lookup name a
    // *prior* place in source order — re-walking `site.placements` per
    // lookup would be quadratic and would also let a later place forward-
    // reference an earlier one's mistakes.
    let mut seen_place_ids: IndexMap<String, Span> = IndexMap::new();
    // Rows refused with `E_INVALID_PLACE_ID`, by id, each with the span of
    // the first row refused under it. Such a row is never registered in
    // `seen_place_ids`, but a later `east_of=` / `north_of=` naming it still
    // means that row, so the reference is reported as the cascade
    // `W_DEFERRED_PLACE`, pointing here, rather than as an unknown id. A
    // second row refused under an id already here is `E_DUPLICATE_PLACE_ID`,
    // as a second row under an id in `seen_place_ids` is.
    // Filled in source order, like `seen_place_ids`: a reference *above* the
    // refused row names no prior place and is `E_UNRESOLVED_PLACE_REF`.
    //
    // A `connect` endpoint needs no such lookup: it is a `DotRef` whose head
    // is a lexer `Ident`, which no refused id is, so it cannot name one. If
    // `connect` ever takes a string, it needs this map too.
    let mut refused_place_ids: IndexMap<String, Span> = IndexMap::new();
    // `place_id` → `use=DEF_NAME` so `connect` rows can find the def whose
    // body exposes the named port without re-walking the placement list.
    let mut place_def: IndexMap<String, String> = IndexMap::new();

    // Pre-built name lists so `nearest_match` candidates are stable per site
    // rather than re-allocated per place.
    let def_names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
    let theme_names: Vec<String> = ctx.themes.keys().cloned().collect();

    for member in &site.placements {
        if matches!(member.role, MemberRole::Connect) {
            resolve_connect_row(
                member,
                &site.name,
                defs,
                &seen_place_ids,
                &place_def,
                connects,
                ctx.diagnostics,
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

        // `use=` is read once, here, for both of its readers: the reference
        // recorded just below and the def lookup further down, so the two
        // cannot read it differently. A row whose `use=` names a declared
        // def references it, whatever refuses the row; every refusal of a
        // `place` row comes after this record. `W_UNUSED_DEF` is "a `def`
        // no `place use=` references", and on a refused row the finding
        // with the repair is the row's own — advising the author to delete
        // the def would send them to a line that is correct. A `use=` that
        // is not label-shaped (`use=3`) names no def, so it records
        // nothing.
        let use_target = member
            .intent_state
            .get("use")
            .and_then(|v| v.value.as_label_str());
        let use_def = use_target.and_then(|name| defs.iter().find(|d| d.name == name));
        if let Some(def) = use_def {
            used_defs.insert(def.name.clone());
        }

        let Some(place_id) = usable_place_id(
            member,
            &site.name,
            &seen_place_ids,
            &mut refused_place_ids,
            ctx.diagnostics,
        ) else {
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
            &refused_place_ids,
            ctx.diagnostics,
        ) {
            continue;
        }

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
        let Some(def) = use_def else {
            ctx.diagnostics.push(unresolved_ref_diag(
                DiagnosticCode::UnresolvedPlaceRef,
                &format!("`use={use_name}` references an unknown def"),
                member.span.clone(),
                use_name,
                def_names.iter().copied(),
            ));
            continue;
        };

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
            ctx.themes,
            &theme_names,
            ctx.reported_missing,
            ctx.diagnostics,
        ) else {
            continue;
        };

        // Cross-scope resolve: run the def's members under the picked theme,
        // even when the file has multiple themes (the per-place `theme=`
        // wins over the single-theme heuristic).
        let resolution =
            resolve_struct_or_def(&def.members, Some(bound_theme.as_str()), &siblings, ctx);
        scopes
            .entry(place_scope_key(&site.name, place_id))
            .or_insert(resolution);
        // Record this place's def so a later `connect` row can look up
        // the def's members without re-walking the site body.
        place_def.insert(place_id.to_owned(), use_name.to_owned());
    }
}

/// Returns `true` when the placement passes every origin-selector check, in
/// which case `resolve_site_placements` may proceed with the cross-scope
/// def / theme resolution and register a scope. `false` means at least one
/// `E_INVALID_PLACE_ORIGIN`, `E_UNRESOLVED_PLACE_REF` (target reference) or
/// `W_DEFERRED_PLACE` was emitted — the caller must skip the rest of the
/// placement so the lowering pass does not voxelise a structurally rejected
/// `place`.
/// `place_id` is `None` for a row that declared no `id=`. Such a row is
/// already reported as incomplete and can never be referenced, but its
/// origin selector is checked anyway so every problem on the line surfaces
/// together — the same reason the missing-key finding lists all three keys
/// at once.
///
/// A target in `refused_place_ids` names a prior row already refused with
/// `E_INVALID_PLACE_ID`. That row has no position to step from, so the
/// answer is `false`, and the reference is reported as `W_DEFERRED_PLACE`
/// rather than `E_UNRESOLVED_PLACE_REF`: the id does name a row, and the
/// repair is on that row.
fn validate_place_origin(
    member: &Member,
    site_name: &str,
    place_id: Option<&str>,
    seen_place_ids: &IndexMap<String, Span>,
    refused_place_ids: &IndexMap<String, Span>,
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
    let mut origin_is_usable = true;
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
            origin_is_usable = false;
            continue;
        };
        if let Some(refused) = refused_place_ids.get(target) {
            diagnostics.push(deferred_place_diag(
                key,
                target,
                site_name,
                value.span.clone(),
                refused,
            ));
            origin_is_usable = false;
            continue;
        }
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
            origin_is_usable = false;
        }
    }
    origin_is_usable
}

pub(super) fn check_unused_defs(
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

/// An unresolved reference to a declared name, with a nearest-match
/// suggestion drawn from `candidates`.
pub(super) fn unresolved_ref_diag<'a>(
    code: DiagnosticCode,
    primary: &str,
    span: Span,
    typo: &str,
    candidates: impl IntoIterator<Item = &'a str>,
) -> Diagnostic {
    let mut notes = Vec::new();
    notes.extend(nearest_match(typo, candidates).map(did_you_mean_note));
    Diagnostic {
        code,
        span,
        primary: primary.to_owned(),
        notes,
        data: None,
    }
}

/// `E_UNRESOLVED_PLACE_REF` with an appended ordering-only note
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
    let mut diag = unresolved_ref_diag(
        DiagnosticCode::UnresolvedPlaceRef,
        primary,
        span,
        typo,
        candidates,
    );
    diag.notes.push(DiagnosticNote {
        span: None,
        message:
            "later places in the same site cannot be referenced; declare the target above this line"
                .to_owned(),
    });
    diag
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
///
/// A row refused with `E_INVALID_PLACE_ID` has its id and span added to
/// `refused_place_ids`, so a later row naming it is reported as the
/// cascade `W_DEFERRED_PLACE` rather than as an id that names nothing. A
/// second row refused under the same id is also `E_DUPLICATE_PLACE_ID`
/// against the first, as it would be under a usable id, for the reason
/// above: otherwise renaming both rows to one usable id would surface the
/// duplicate for the first time on the lines the author had only now fixed.
fn usable_place_id<'a>(
    member: &'a Member,
    site_name: &str,
    seen_place_ids: &IndexMap<String, Span>,
    refused_place_ids: &mut IndexMap<String, Span>,
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
        validate_place_origin(
            member,
            site_name,
            None,
            seen_place_ids,
            refused_place_ids,
            diagnostics,
        );
        return None;
    };

    // Validate before the id becomes half of a scope key. `PlaceId` states
    // the invariants, and `place_scope_key` joins on `::`, so an id carrying
    // `.` or `:` produces a key nothing can parse back — which is where the
    // lowering pass used to `expect` and panic. The id is also the stem of
    // the artifact's file name, so a `/` or `\` in it used to put that file
    // outside `--out`.
    if let Err(err) = PlaceId::new(place_id) {
        diagnostics.push(invalid_place_id_diag(
            place_id,
            site_name,
            member.span.clone(),
            &err,
        ));
        // First wins, as for `seen_place_ids`: the note on a reference
        // points at the row refused first under that id, and a later row
        // refused under it is a duplicate of that row.
        if let Some(first) = refused_place_ids.get(place_id) {
            diagnostics.push(duplicate_place_id_diag(
                site_name,
                place_id,
                first,
                &member.span,
            ));
        } else {
            refused_place_ids.insert(place_id.to_owned(), member.span.clone());
        }
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
fn member_declares(member: &Member, key: &str) -> bool {
    member.intent_state.contains_key(key) || (key == "id" && member.id.is_some())
}

/// Report a `place` row missing any of the keys it needs to become a
/// placement, or `None` when it declares all three.
///
/// `id=` names the `.nbt` the compiler writes for this placement
/// (`spec/components-editing-sites` "Output naming") and is the name
/// `east_of=` and `connect` refer to — so it cannot be auto-assigned the way
/// that chapter's "Editing model" auto-assigns a geometry
/// member's address, which derives from parent / role / side / level /
/// offset and names nothing outside the body it sits in. `use=` names the
/// `def` the placement instantiates and `theme=` the theme its `mat_slot=`
/// members resolve against; without either there is no volume to voxelise.
fn incomplete_place_diag(member: &Member, site_name: &str) -> Option<Diagnostic> {
    let missing: Vec<&(&str, &str)> = REQUIRED_PLACE_KEYS
        .iter()
        .filter(|(key, _)| !member_declares(member, key))
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
        // The key set is what a quick-fix needs, and `spec/lint`
        // "Machine-readable payload" asks consumers to match on
        // `(code, data.kind)` rather than parse the prose it is also rendered
        // into.
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
        IdError::ForbiddenChar { ch, .. } => format!("it contains `{}`", shown_char(*ch)),
    };
    // Built from the list the rule itself reads, so the advice cannot name a
    // character the rule lets through or leave out one it refuses.
    let marks: Vec<String> = FORBIDDEN_IDENT_PUNCTUATION
        .iter()
        .map(|mark| format!("`{mark}`"))
        .collect();
    Diagnostic {
        code: DiagnosticCode::InvalidPlaceId,
        span,
        primary: format!(
            "`place id={place_id}` in site `{site_name}` is not a usable id: {reason}"
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: format!(
                "a place id becomes part of the `site::<site>::<place>` scope key and \
                 the stem of the artifact file written into `--out`, so it must be \
                 non-empty and free of {marks}, whitespace, and control characters; \
                 rename it with letters, digits, and `_` (`home1`, `north_tower`)",
                marks = marks.join(", "),
            ),
        }],
        data: None,
    }
}

/// `W_DEFERRED_PLACE`: `key=target` names a row refused for its id.
///
/// The cascade shape `W_DEFERRED_CONNECT` uses, one layer up: the row
/// naming the refused id is correct as written and is not placed only
/// because what it is placed against has no position. The note carries the
/// refused row's span, so the author is sent to the row that has the
/// repair. Renaming that row's id renames what this reference has to say,
/// which the note says too rather than promising the row places on its own.
fn deferred_place_diag(
    key: &str,
    target: &str,
    site_name: &str,
    span: Span,
    refused: &Span,
) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::DeferredPlace,
        span,
        primary: format!(
            "`{key}={target}` in site `{site_name}` names a `place` row refused for its id, \
             so this row is not placed"
        ),
        notes: vec![DiagnosticNote {
            span: Some(refused.clone()),
            message: "that row carries its own diagnostic saying what is wrong with its id; \
                      renaming it, and this reference with it, places this row too"
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
