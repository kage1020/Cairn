//! A `site` body: each `place` resolved against its `def` and `theme=`, and unplaced `def`s.

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
    // Filled in source order, so `east_of=` and `north_of=` can name only a prior place.
    let mut seen_place_ids: IndexMap<String, Span> = IndexMap::new();
    // Ids refused with `E_INVALID_PLACE_ID`, each with its first row. A `connect` endpoint, being a
    // lexer `Ident`, cannot name one.
    let mut refused_place_ids: IndexMap<String, Span> = IndexMap::new();
    // `place_id` → `use=` def name, for a `connect` row's port lookup.
    let mut place_def: IndexMap<String, String> = IndexMap::new();

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
            // `check::member_scope` reports this as `E_MISPLACED_MEMBER`.
            continue;
        }

        // Recorded before any refusal of the row, so its `def` does not also earn `W_UNUSED_DEF`.
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

        // Before any cross-scope lookup, so the structural problem is reported first.
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

        // Already reported, by `incomplete_place_diag` or `check::type_mismatch`.
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

        // No default: the module's single theme stands in only for struct and def scopes.
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

        let resolution =
            resolve_struct_or_def(&def.members, Some(bound_theme.as_str()), &siblings, ctx);
        scopes
            .entry(place_scope_key(&site.name, place_id))
            .or_insert(resolution);
        place_def.insert(place_id.to_owned(), use_name.to_owned());
    }
}

/// Whether the row's origin selector is usable; on `false` at least one finding was pushed.
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

    // The target must come earlier in source order, so cycles cannot form.
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

/// An unresolved reference, with a nearest-match suggestion from `candidates`.
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

/// The id the rest of the loop keys on, or `None` once the row's faults are all reported.
///
/// The origin is checked on both paths out, so adding a missing `id=` surfaces no new error.
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

    // Before the id joins a `::` scope key and becomes a file stem, which `:` or `/` would break.
    if let Err(err) = PlaceId::new(place_id) {
        diagnostics.push(invalid_place_id_diag(
            place_id,
            site_name,
            member.span.clone(),
            &err,
        ));
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

/// Keys a `place` row must declare, with what each is for, in the message's order.
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

/// Whether the row carries `key=` at all; a hoisted `id=` lives on [`Member::id`].
fn member_declares(member: &Member, key: &str) -> bool {
    member.intent_state.contains_key(key) || (key == "id" && member.id.is_some())
}

/// The finding for a `place` row missing a required key, or `None` when it declares all three.
///
/// `id=` names the placement's output (`spec/components-editing-sites` "Output naming").
fn incomplete_place_diag(member: &Member, site_name: &str) -> Option<Diagnostic> {
    let missing: Vec<&(&str, &str)> = REQUIRED_PLACE_KEYS
        .iter()
        .filter(|(key, _)| !member_declares(member, key))
        .collect();
    let quoted: Vec<String> = missing.iter().map(|(key, _)| format!("`{key}=`")).collect();
    // `and_list`'s `None` is the "nothing missing" exit.
    let listed = and_list(&quoted)?;
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
        // For a quick-fix (`spec/lint` "Machine-readable payload").
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
