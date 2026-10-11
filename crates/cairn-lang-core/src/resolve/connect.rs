//! `connect` rows: both endpoints resolved to a port of a placed `def`,
//! and the `path=` material checked, before a [`ValidatedConnect`] is
//! recorded for the walkway voxeliser.

use indexmap::IndexMap;

use crate::ast::{Value, ValueKind};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::ids::{PlaceId, PortId, SiteName};
use crate::intent::{ConnectEnd, DefIr, Member, MemberRole};
use crate::suggest::{did_you_mean_note, nearest_match};

use super::resolution::{PortRef, ValidatedConnect};
use super::site::unresolved_ref_diag;

/// Validate one `connect from.port to to.port path=@MAT` row and push a
/// matching [`ValidatedConnect`] when both ends and the path material
/// pass, so the walkway voxeliser sees only rows it can lay.
///
/// Each failure is reported once:
///
/// - `E_UNRESOLVED_PLACE_REF`: an endpoint's place names no prior `place`
///   in the site;
/// - `W_DEFERRED_CONNECT`: it names a `place` whose own row did not
///   resolve, so there is no def to look the port up in;
/// - `E_UNRESOLVED_PORT`: the port names no member of that def's body;
/// - `E_AMBIGUOUS_PORT`: several members of the body carry that `id=`;
/// - `E_MISSING_PATH_MATERIAL`: the row has no `path=` material token.
pub(super) fn resolve_connect_row(
    member: &Member,
    site_name: &str,
    defs: &[DefIr],
    seen_place_ids: &IndexMap<String, Span>,
    place_def: &IndexMap<String, String>,
    connects: &mut Vec<ValidatedConnect>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // INVARIANT(upstream-diagnosed): `check::connect_arity` reports every
    // row not shaped `FROM.PORT to TO.PORT` as `E_CONNECT_ARITY`; these
    // guards stay silent for library callers that resolve without
    // checking. The exact-length slice keeps the two layers agreeing on
    // which rows are well-formed: indexing `positional[0..3]` would lay a
    // walkway for `connect a.entry to b.entry c.exit`, which `check`
    // refuses. Pinned by `tests/resolve/silent_skip_arms.rs`.
    let [from_value, separator, to_value] = member.positional.as_slice() else {
        return;
    };
    if !matches!(&separator.kind, ValueKind::Ident(keyword) if keyword == "to") {
        return;
    }
    // Both ends before short-circuiting, so two broken halves earn two
    // findings.
    let from = port_ref_from_value(from_value, ConnectEnd::From, seen_place_ids, diagnostics);
    let to = port_ref_from_value(to_value, ConnectEnd::To, seen_place_ids, diagnostics);
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
    // A non-token `path=` would reach `resolve_block_state`'s
    // `AlreadyDiagnosed` arm, which assumes `E_UNKNOWN_SLOT_TARGET` reported
    // it; that pass never sees `path=`, so it is reported here.
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

/// Lift one positional [`Value`] into a [`PortRef`]: `None` for any shape
/// but a one-dot `<place>.<port>` reference, and `E_UNRESOLVED_PLACE_REF`
/// when the place names no prior place in the site. `end` tells the two
/// ends apart in the message.
fn port_ref_from_value(
    raw: &Value,
    end: ConnectEnd,
    seen_place_ids: &IndexMap<String, Span>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<PortRef> {
    // INVARIANT(upstream-diagnosed): `check::connect_arity` reports every
    // endpoint that is not a one-dot reference; `None` keeps a library
    // caller's walkway from naming nothing. From parsed source the first
    // guard catches every non-reference and the second only
    // `place.port.extra`; an empty tail comes only from hand-built IR.
    // Pinned by `tests/resolve/silent_skip_arms.rs`.
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
    // INVARIANT(upstream-diagnosed): the lexer's `Ident` (ASCII letters,
    // digits, `_`) is a subset of what the id newtypes accept, so these
    // cannot fail on a parsed segment.
    let place = PlaceId::new(place_str).expect("surface lexer enforces PlaceId invariants");
    let port = PortId::new(port_str).expect("surface lexer enforces PortId invariants");
    Some(PortRef {
        place,
        port,
        span: raw.span.clone(),
    })
}

/// Look `port.port` up among the `id=`s of the referenced def body's own
/// members and return `true` when exactly one carries it. Otherwise push
/// one diagnostic and return `false`:
///
/// - the `place` row did not resolve: `W_DEFERRED_CONNECT`;
/// - only a member under a `level` carries it ([`level_holding`]):
///   [`diag_level_scoped_port`];
/// - no member carries it: [`diag_undeclared_port`], whose suggestions come
///   from this body's ids only, since an id from a sibling def would send
///   the author down another broken path;
/// - several members carry it: `E_AMBIGUOUS_PORT`.
///
/// The one `false` that pushes nothing is a `place_def` entry naming a def
/// missing from `defs`, which `resolve_site_placements` rules out and a
/// debug build asserts against.
fn validate_port(
    port: &PortRef,
    defs: &[DefIr],
    place_def: &IndexMap<String, String>,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let Some(def_name) = place_def.get(port.place.as_str()) else {
        // The place is registered but has no def: every arm of
        // `resolve_site_placements` that skips `place_def` reports the row
        // itself. This cascade says which walkway went with it, as
        // `block_array::lower`'s `W_DEFERRED_MEMBER` does for walkway
        // endpoints.
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
        // INVARIANT(structural): `resolve_site_placements` inserts into
        // `place_def` only after finding the def in `defs`.
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

    // Only the body's own members are ports. Widening this lookup takes
    // more than a change here: `port_world_position` looks the port up in
    // the body again and treats a miss as unreachable
    // (`PortRejection::UnknownMember`), and lowering assumes no `level y=N`
    // has shifted a port's member, so `block_array::ports::DOOR_PORT_BASE_V`,
    // a window port's `contains_rows` check and the strip's ground row
    // (`PORT_GROUND_V`, `place_origin.1`) would all go silently wrong.
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

/// `E_UNRESOLVED_PORT` for a port id that no member of the def's body
/// carries and no member under a `level` carries either: a did-you-mean
/// note when one of the body's ids fits the spell cap, then a note to add
/// the `id=`.
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

/// A member a port id names that is not in the def's body, because it
/// is indented under a `level`.
#[derive(Clone, Copy)]
struct LevelScoped<'m> {
    /// The `level` the member sits directly under.
    level: &'m Member,
    /// The member that carries the port's `id=`.
    member: &'m Member,
}

/// `E_UNRESOLVED_PORT` for a port whose `id=` is on a member under a
/// `level`, which is not in the def body a port is looked up in.
///
/// The note does not suggest moving the member out of the `level`, which
/// would change where it is built.
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

/// A member with `id=` `port_id` under a `level` in `members`, at any depth
/// of `level` nesting, with the `level` it sits directly under.
///
/// Nested `level`s are asked before a `level`'s own members, so the
/// innermost comes back: the one `E_UNSUPPORTED_NESTING` reports as
/// "`level` declared here", sending both findings to one line.
///
/// The member's role is not looked at, so a `walls` comes back too (in the
/// body it would be `PortRejection::ReservedRole`), and nesting under
/// anything but a `level` is not searched, so a `door` under a `circuit`
/// gets [`diag_undeclared_port`].
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
