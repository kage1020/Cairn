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
/// pass every check.
///
/// Failures all skip the push so the walkway voxeliser only sees rows it
/// can lay safely. Each diagnostic kind is emitted at most once per
/// failure mode so a single broken row never reads as multiple unrelated
/// problems:
///
/// - `E_UNRESOLVED_PLACE_REF` — `from.place_id` or `to.place_id` does
///   not name a prior `place` in the same site;
/// - `W_DEFERRED_CONNECT` — `from.place_id` or `to.place_id` names a
///   prior `place` whose own row did not resolve, so there is no def to
///   look the port up in;
/// - `E_UNRESOLVED_PORT` — the `port_id` half names no member of the
///   referenced def's body. When a member under a `level` carries it, the
///   diagnostic says so, with a note at that member; otherwise it carries
///   a nearest-match note when one sits within the spell cap;
/// - `E_AMBIGUOUS_PORT` — multiple members of the referenced def share
///   that `id=`; downstream lowering would have to pick one arbitrarily;
/// - `E_MISSING_PATH_MATERIAL` — the row has no `path=` argument so
///   walkway lowering has no material to lay.
pub(super) fn resolve_connect_row(
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
    // INVARIANT(upstream-diagnosed): the surface lexer's `Ident` rule
    // (ASCII letters, digits and `_`) is a strict subset of what the id
    // newtypes accept, so any `DotRef` segment that
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

/// Look `port.port` up among the `id=`s of the referenced def's body — its
/// own members, not the ones indented under a `level` or any other member
/// — and return `true` when exactly one carries it. Otherwise push one
/// diagnostic and return `false`:
///
/// - the `place` row did not resolve, so there is no def to look in:
///   `W_DEFERRED_CONNECT`;
/// - no body member carries the id and a member under a `level` does
///   ([`level_holding`]): `E_UNRESOLVED_PORT` saying so, with a note at
///   that member ([`diag_level_scoped_port`]);
/// - no body member carries it otherwise: `E_UNRESOLVED_PORT` with a
///   did-you-mean note when one of the body's ids fits the spell cap, and
///   a note to add the `id=`. The pool is the body's ids, not the whole
///   file's — pointing the author at an id from a sibling def would send
///   them down a different broken path;
/// - more than one body member carries it: `E_AMBIGUOUS_PORT`.
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
        // `port_ref_from_value` already gates the lift on
        // `seen_place_ids`, so reaching here means `port.place_id` is
        // registered but its `place_def` entry never landed. Every arm in
        // `resolve_site_placements` that skips the insert reports the row
        // itself: an absent `use=` / `theme=` (`E_INCOMPLETE_PLACE`), a
        // mistyped one (`E_TYPE_MISMATCH_LABEL`), a failed origin selector
        // (`E_INVALID_PLACE_ORIGIN`), an `east_of=` / `north_of=` naming
        // no prior place (`E_UNRESOLVED_PLACE_REF`) or one refused for its
        // id (`W_DEFERRED_PLACE`), a `use=` naming an unknown def
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
        // `unresolved_ref_diag` arm (which `continue`s on miss).
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

    // Only the def body's own members are ports: this lookup does not
    // descend into a `level`, or into any other member. Widening the
    // lookup takes more than a change here. `port_world_position` looks
    // the port up in the body again, so a level-scoped id that resolved
    // here would first reach `PortRejection::UnknownMember`, which that
    // variant's doc calls unreachable because this lookup refuses the
    // miss. Past that, the lowering side reads a port's member as one no
    // `level y=N` has shifted, and each of these would go wrong without an
    // error: `walkway::DOOR_PORT_BASE_V` is `carve_door`'s `y_offset + 1`
    // only at `y_offset = 0`; a window port's `contains_rows(y, ..)` asks
    // about the rows `cut_window`'s `contains_rows(y + y_offset, ..)` cuts
    // only at `y_offset = 0` too; and the strip itself lies on the
    // placement's ground row, not on the base row of the member's
    // `level`: `port_world_position` returns `place_origin.1` for every
    // port, and `walkway::PORT_GROUND_V`, the `v` the window path maps
    // with, is the constant whose doc names it as the place a raised port
    // re-binds.
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
/// The primary names the `level` the member sits directly under. The note
/// points at the member itself and says what a port has to be and what to
/// name instead. It does not suggest moving the member out of the
/// `level`, which would change where it is built.
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

/// A member with `id=` `port_id` directly under a `level` in `members`, or
/// under a `level` nested in one at any depth, with the `level` it sits
/// directly under.
///
/// The search goes through `members` in source order and, inside each
/// `level`, asks the `level`s nested in it before its own members. So for
/// a member under a `level` that is itself under a `level`, the inner one
/// comes back: the one `E_UNSUPPORTED_NESTING` reports as "`level`
/// declared here", so the two findings send the reader to the same line.
///
/// Two choices about what this does not look at:
///
/// - The member's role. A `walls` under a `level` comes back too, so its
///   refusal says a member there "cannot be a port yet", though a `walls`
///   in the body is not a port either (`PortRejection::ReservedRole`).
/// - Nesting under anything but a `level`. A `door` indented under a
///   `circuit` is not found, so its port gets [`diag_undeclared_port`]'s
///   "not declared" message and its note to add the `id=`.
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
