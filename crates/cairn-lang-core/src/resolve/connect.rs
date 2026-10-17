//! `connect` rows, recorded as [`ValidatedConnect`]s when both ports and the `path=` material
//! resolve.

use indexmap::IndexMap;

use crate::ast::{Value, ValueKind};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::ids::{PlaceId, PortId, SiteName};
use crate::intent::{ConnectEnd, DefIr, Member, MemberRole};
use crate::suggest::{did_you_mean_note, nearest_match};

use super::resolution::{PortRef, ValidatedConnect};
use super::unresolved_ref::unresolved_ref_diag;

/// The place ids of a site's rows above a `connect`, each with its first row.
#[derive(Clone, Copy)]
pub(super) struct PlaceIds<'a> {
    /// Ids that passed the id rule.
    pub(super) seen: &'a IndexMap<String, Span>,
    /// Ids refused with `E_INVALID_PLACE_ID`.
    pub(super) refused: &'a IndexMap<String, Span>,
}

/// Records a [`ValidatedConnect`] for `member` when both ends and its `path=` material resolve.
pub(super) fn resolve_connect_row(
    member: &Member,
    site_name: &str,
    defs: &[DefIr],
    place_ids: PlaceIds<'_>,
    place_def: &IndexMap<String, String>,
    connects: &mut Vec<ValidatedConnect>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // INVARIANT(upstream-diagnosed): `check::connect_arity` reports exactly the rows skipped here.
    let [from_value, separator, to_value] = member.positional.as_slice() else {
        return;
    };
    if !matches!(&separator.kind, ValueKind::Ident(keyword) if keyword == "to") {
        return;
    }
    // Both ends before short-circuiting, so two broken halves earn two findings.
    let from = port_ref_from_value(from_value, ConnectEnd::From, place_ids, diagnostics);
    let to = port_ref_from_value(to_value, ConnectEnd::To, place_ids, diagnostics);
    let (Some(from), Some(to)) = (from, to) else {
        return;
    };

    let mut all_ports_valid = true;
    if !validate_port(&from, defs, place_def, diagnostics) {
        all_ports_valid = false;
    }
    if !validate_port(&to, defs, place_def, diagnostics) {
        all_ports_valid = false;
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
    // No other pass sees `path=`, yet `resolve_block_state` assumes a non-token was reported.
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
    if !all_ports_valid {
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

/// The [`PortRef`] a `<place>.<port>` value names; `None` for any other shape, an unknown place
/// or one refused for its id.
fn port_ref_from_value(
    raw: &Value,
    end: ConnectEnd,
    place_ids: PlaceIds<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<PortRef> {
    // INVARIANT(upstream-diagnosed): `check::connect_arity` reports every non-`<place>.<port>` end.
    let ValueKind::DotRef(dot) = &raw.kind else {
        return None;
    };
    let [port] = dot.tail() else {
        return None;
    };
    let place_str = dot.head();
    let port_str = port.as_str();
    if let Some(refused) = place_ids.refused.get(place_str) {
        diagnostics.push(deferred_connect_to_refused_id_diag(
            &format!("{place_str}.{port_str}"),
            raw.span.clone(),
            refused,
        ));
        return None;
    }
    if !place_ids.seen.contains_key(place_str) {
        let prior: Vec<&str> = place_ids.seen.keys().map(String::as_str).collect();
        diagnostics.push(unresolved_ref_diag(
            DiagnosticCode::UnresolvedPlaceRef,
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
    // INVARIANT(structural): `seen` holds only ids `PlaceId::new` accepted.
    let place = PlaceId::new(place_str).expect("a seen place id is a valid PlaceId");
    // INVARIANT(upstream-diagnosed): every lexer `Ident` is a valid port id.
    let port = PortId::new(port_str).expect("surface lexer enforces PortId invariants");
    Some(PortRef {
        place,
        port,
        span: raw.span.clone(),
    })
}

/// `W_DEFERRED_CONNECT`: an endpoint names a row refused for its id, which `refused` spans.
fn deferred_connect_to_refused_id_diag(endpoint: &str, span: Span, refused: &Span) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::DeferredConnect,
        span,
        primary: format!(
            "`connect` target `{endpoint}` names a `place` row refused for its id; no walkway laid"
        ),
        notes: vec![DiagnosticNote {
            span: Some(refused.clone()),
            message: "that row carries its own diagnostic saying what is wrong with its id; \
                      renaming it, and this endpoint with it, lays this walkway"
                .to_owned(),
        }],
        data: None,
    }
}

/// Whether exactly one of the def body's own members carries `port.port`; if not, pushes why.
fn validate_port(
    port: &PortRef,
    defs: &[DefIr],
    place_def: &IndexMap<String, String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let Some(def_name) = place_def.get(port.place.as_str()) else {
        // The `place` row reports its own fault; this names the walkway lost with it.
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
        // INVARIANT(structural): `resolve_site_placements` fills `place_def` only from `defs`.
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

    // Only the body's own members are ports: `block_array::ports` assumes no `level y=N` has
    // shifted a port's member, so widening this lookup needs changes there too.
    let count = def
        .members
        .iter()
        .filter(|m| m.id.as_deref() == Some(port.port.as_str()))
        .count();
    match count {
        0 => {
            let refusal = match level_holding(&def.members, port.port.as_str()) {
                Some(scoped) => diag_level_scoped_port(port, def, scoped),
                None => diag_undeclared_port(port, def),
            };
            diagnostics.push(refusal);
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

fn diag_undeclared_port(port: &PortRef, def: &DefIr) -> Diagnostic {
    let pool: Vec<&str> = def.members.iter().filter_map(|m| m.id.as_deref()).collect();
    let mut notes = Vec::with_capacity(2);
    notes.extend(
        nearest_match(port.port.as_str(), pool.iter().copied())
            .map(|suggested| did_you_mean_note(&format!("{}.{suggested}", port.place))),
    );
    notes.push(DiagnosticNote {
        span: None,
        message: format!(
            "add `id={port_id}` to a member of `def {def_name}` (e.g. `door id={port_id} ...`)",
            port_id = port.port,
            def_name = def.name,
        ),
    });
    Diagnostic {
        code: DiagnosticCode::UnresolvedPort,
        span: port.span.clone(),
        primary: format!(
            "port `{port_id}` is not declared by `def {def_name}` (used by `place {place_id}`)",
            port_id = port.port,
            def_name = def.name,
            place_id = port.place,
        ),
        notes,
        data: None,
    }
}

/// A port's member indented under a `level`.
#[derive(Clone, Copy)]
struct LevelScoped<'m> {
    /// The `level` the member sits directly under.
    level: &'m Member,
    /// The member that carries the port's `id=`.
    member: &'m Member,
}

fn diag_level_scoped_port(port: &PortRef, def: &DefIr, scoped: LevelScoped<'_>) -> Diagnostic {
    let level_label = scoped
        .level
        .id
        .as_deref()
        .map_or_else(|| "a `level`".to_owned(), |id| format!("`level id={id}`"));
    Diagnostic {
        code: DiagnosticCode::UnresolvedPort,
        span: port.span.clone(),
        primary: format!(
            "port `{port_id}` of `def {def_name}` (used by `place {place_id}`) is declared under \
             {level_label}, and a member under a `level` cannot be a port yet",
            port_id = port.port,
            def_name = def.name,
            place_id = port.place,
        ),
        notes: vec![DiagnosticNote {
            span: Some(scoped.member.span.clone()),
            message: format!(
                "a port has to be a door or window declared directly in the body of \
                 `def {def_name}`: use one of those as the endpoint; level-scoped ports are a \
                 future extension",
                def_name = def.name,
            ),
        }],
        data: None,
    }
}

/// A member carrying `id=port_id` under a `level` in `members`, with the innermost such `level`,
/// the one `E_UNSUPPORTED_NESTING` also points at.
fn level_holding<'m>(members: &'m [Member], port_id: &str) -> Option<LevelScoped<'m>> {
    members
        .iter()
        .filter(|m| matches!(m.role, MemberRole::Level))
        .find_map(|level| {
            level_holding(&level.children.members, port_id).or_else(|| {
                level
                    .children
                    .members
                    .iter()
                    .find(|m| m.id.as_deref() == Some(port_id))
                    .map(|member| LevelScoped { level, member })
            })
        })
}
