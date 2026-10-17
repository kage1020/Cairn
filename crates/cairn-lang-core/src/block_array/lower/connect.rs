//! Lowering the `connect` rows of every `site` to walkway strips.

use std::collections::HashSet;
use std::hash::BuildHasher;

use indexmap::IndexMap;

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::ids::{PlaceId, PortId, SiteName, WalkwayEndpoint, WalkwayScopeKey};
use crate::intent::DefIr;
use crate::prose::and_list;
use crate::resolve::{Resolution, place_scope_key};

use crate::block_array::material::{
    MaterialDeferred, TargetRegistry, is_stand_in, resolve_block_state,
};
use crate::block_array::ports::port_world_position;
use crate::block_array::walkway::{
    BlockedIndex, ROUTE_AREA_CAP, RoutePathError, SearchRect, WalkwayLayout, build_walkway_array,
    l_path, l_path_area, route_path,
};
use crate::block_array::{BlockArray, BlockState, Dims, Footprint, Placement, Walkway};

use super::site_floor::{FloorExtent, FloorPlan, PlacedBody};
use super::token_diag::{
    TokenSite, diag_abstract_token, diag_state_literal_unchecked, diag_unknown_abstract_token,
    diag_unknown_id,
};

/// A world cell, `(x, y, z)`.
type Cell = (i32, i32, i32);

/// What laying the walkways reads: the whole finished site at once.
pub(super) struct ConnectInputs<'a> {
    pub(super) resolution: &'a Resolution,
    pub(super) defs: &'a [DefIr],
    pub(super) registry: Option<&'a dyn TargetRegistry>,
    /// Every `place` row that lowered, under its `site::SITE::PLACE_ID` key.
    pub(super) placed: &'a IndexMap<String, PlacedBody>,
    /// Placement floors on the walk plane, and each placement's floor extent.
    pub(super) floor: &'a FloorPlan,
}

/// Lower every resolved `connect` row into a walkway array and its [`Walkway`] record.
///
/// Every finding anchors on the row's own span, so the final dedup never merges two rows.
pub(super) fn lower_connects(
    inputs: &ConnectInputs<'_>,
    structures: &mut IndexMap<String, BlockArray>,
    walkways: &mut IndexMap<WalkwayScopeKey, Walkway>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let &ConnectInputs {
        resolution,
        registry,
        placed,
        floor,
        ..
    } = inputs;
    let blocked = &floor.cells;
    let mut seen_pairs: HashSet<(SiteName, PlaceId, PortId, PlaceId, PortId)> = HashSet::new();
    // Indexed once for every row, so many colliding rows cannot multiply a linear scan.
    let blocked_index = BlockedIndex::new(blocked);

    for connect in &resolution.connects {
        let from_key = place_scope_key(connect.site.as_str(), connect.from.place.as_str());
        let to_key = place_scope_key(connect.site.as_str(), connect.to.place.as_str());
        let Some((from_pos, to_pos)) =
            place_ports(connect, (&from_key, &to_key), inputs, diagnostics)
        else {
            continue;
        };

        // A pair is recorded only once its strip is laid, so a refused earlier row is no duplicate.
        let dedup_key = connect.walkway_pair();
        if seen_pairs.contains(&dedup_key) {
            diagnostics.push(diag_duplicate_walkway(connect));
            continue;
        }

        let Some(material) = walkway_material(connect, registry, diagnostics) else {
            continue;
        };
        let material_id = material.id.clone();
        let Some((path, route_failure)) = walkway_path(
            connect,
            (from_pos, to_pos),
            blocked,
            &blocked_index,
            diagnostics,
        ) else {
            continue;
        };
        let routed = route_failure.is_none();
        let Some((from_endpoint, to_endpoint, scope_key)) = walkway_ends(connect, diagnostics)
        else {
            continue;
        };
        let WalkwayLayout {
            array,
            origin,
            blocked_count: skipped,
        } = build_walkway_array(&path, material, blocked, &scope_key);
        if routed {
            debug_assert_eq!(
                skipped, 0,
                "walkway `{scope_key}` laid a routed path with {skipped} collisions",
            );
        }
        if skipped > 0
            && let Some(note) = walkway_blocked_note(
                connect,
                route_failure,
                &BlockedEndpoints {
                    from: (from_pos, &from_key),
                    to: (to_pos, &to_key),
                },
                &SiteFloor {
                    structures,
                    placed,
                    floor,
                },
            )
        {
            diagnostics.push(diag_walkway_blocked(connect, skipped, note));
        }
        let footprint = walkway_footprint(array.dims);
        // Bound outside the `debug_assert!` so release builds still report a replaced key.
        let replaced = structures.insert(scope_key.as_str().to_owned(), array);
        debug_assert!(
            replaced.is_none(),
            "walkway `{scope_key}` replaced an existing structure; two `connect` rows \
             encoded to one scope key",
        );
        if replaced.is_some() {
            diagnostics.push(diag_walkway_replaced(connect, &scope_key));
        }
        seen_pairs.insert(dedup_key);
        walkways.insert(
            scope_key,
            Walkway {
                site: connect.site.clone(),
                from: from_endpoint,
                to: to_endpoint,
                origin,
                footprint,
                path_material: material_id,
            },
        );
    }
}

/// The world cells of a `connect` row's two ports, or `None` after a `W_DEFERRED_MEMBER`.
fn place_ports(
    connect: &crate::resolve::ValidatedConnect,
    (from_key, to_key): (&str, &str),
    inputs: &ConnectInputs<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(Cell, Cell)> {
    let from_body = inputs.placed.get(from_key);
    let to_body = inputs.placed.get(to_key);
    let (Some(from_body), Some(to_body)) = (from_body, to_body) else {
        // A placement was refused upstream though the connect resolved; say which side.
        diagnostics.push(diag_walkway_endpoint_skipped(
            connect,
            from_body.is_none(),
            to_body.is_none(),
        ));
        return None;
    };
    let source_def = |body: &PlacedBody| {
        inputs
            .defs
            .iter()
            .find(|d| d.name == body.placement.source_def)
    };
    let (Some(from_def), Some(to_def)) = (source_def(from_body), source_def(to_body)) else {
        // INVARIANT: `lower_site` inserts a `Placement` only after finding its `use=DEF` in `defs`.
        debug_assert!(
            false,
            "connect `{}` to `{}` references placements whose source def is missing from `defs`",
            connect.from.place, connect.to.place,
        );
        return None;
    };

    let from_pos = port_world_position(
        from_body.placement.origin,
        from_body.placement.dims,
        from_def,
        &connect.from.port,
        &from_body.walls,
        &from_body.cut,
    );
    let to_pos = port_world_position(
        to_body.placement.origin,
        to_body.placement.dims,
        to_def,
        &connect.to.port,
        &to_body.walls,
        &to_body.cut,
    );
    let (notes, noun, (from_refused, to_refused)) = match (from_pos, to_pos) {
        (Ok(from_pos), Ok(to_pos)) => return Some((from_pos, to_pos)),
        (Err(from_err), Err(to_err)) => (
            vec![
                from_err.note(&connect.from.to_string()),
                to_err.note(&connect.to.to_string()),
            ],
            "ports",
            (true, true),
        ),
        (Err(from_err), Ok(_)) => (
            vec![from_err.note(&connect.from.to_string())],
            "port",
            (true, false),
        ),
        (Ok(_), Err(to_err)) => (
            vec![to_err.note(&connect.to.to_string())],
            "port",
            (false, true),
        ),
    };
    let unplaceable = blamed_endpoints(connect, from_refused, to_refused);
    diagnostics.push(Diagnostic {
        code: DiagnosticCode::DeferredMember,
        span: connect.span.clone(),
        primary: format!(
            "walkway `{from} ↔ {to}` was skipped because {noun} {unplaceable} could not be placed",
            from = connect.from,
            to = connect.to,
        ),
        notes,
        data: None,
    });
    None
}

/// A `connect` row's two ends and scope key, or `None` after a `W_INVALID_WALKWAY_IDENT`.
fn walkway_ends(
    connect: &crate::resolve::ValidatedConnect,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(WalkwayEndpoint, WalkwayEndpoint, WalkwayScopeKey)> {
    let from_endpoint = WalkwayEndpoint {
        place: connect.from.place.clone(),
        port: connect.from.port.clone(),
    };
    let to_endpoint = WalkwayEndpoint {
        place: connect.to.place.clone(),
        port: connect.to.port.clone(),
    };
    match WalkwayScopeKey::from_parts(&connect.site, &from_endpoint, &to_endpoint) {
        Ok(scope_key) => Some((from_endpoint, to_endpoint, scope_key)),
        Err(e) => {
            diagnostics.push(diag_walkway_invalid_ident(connect, &e));
            None
        }
    }
}

/// The [`Footprint`] of a one-block-thick walkway array.
fn walkway_footprint(dims: Dims) -> Footprint {
    debug_assert_eq!(
        dims.y, 1,
        "walkway block array must be 1 block thick; build_walkway_array's contract \
         pins y = 1 and the lockfile relies on this when re-attaching the implicit y \
         via Footprint::to_dims_y1",
    );
    Footprint {
        x: dims.x,
        z: dims.z,
    }
}

/// A `connect` row whose port pair an earlier row in the same site already laid.
fn diag_duplicate_walkway(connect: &crate::resolve::ValidatedConnect) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::DuplicateWalkway,
        span: connect.span.clone(),
        primary: format!(
            "duplicate walkway `{from} ↔ {to}` in site `{site}`; the second row was dropped",
            from = connect.from,
            to = connect.to,
            site = connect.site,
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "remove the duplicate or rewrite it to connect a different port pair"
                .to_owned(),
        }],
        data: None,
    }
}

/// The block a `connect` row's `path=` lays, or `None` after the finding that says why.
fn walkway_material(
    connect: &crate::resolve::ValidatedConnect,
    registry: Option<&dyn TargetRegistry>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<BlockState> {
    match resolve_block_state(&connect.path, registry) {
        Ok(state) => {
            diagnostics.extend(diag_state_literal_unchecked(&connect.path, &state));
            Some(state)
        }
        Err(MaterialDeferred::Abstract(token)) => {
            diagnostics.push(diag_abstract_token(
                connect.path.span.clone(),
                &token,
                TokenSite::WalkwayPath,
            ));
            None
        }
        Err(MaterialDeferred::UnknownAbstract { token, suggestion }) => {
            diagnostics.push(diag_unknown_abstract_token(
                connect.path.span.clone(),
                &token,
                suggestion.as_deref(),
                TokenSite::WalkwayPath,
            ));
            None
        }
        Err(MaterialDeferred::UnknownId(unknown)) => {
            diagnostics.push(diag_unknown_id(connect.path.span.clone(), &unknown));
            None
        }
        Err(MaterialDeferred::AlreadyDiagnosed) => {
            // INVARIANT(upstream-diagnosed): `resolve_connect_row` refuses a non-token `path=` with
            // `E_MISSING_PATH_MATERIAL`.
            debug_assert!(
                false,
                "connect `{from}` to `{to}` in site `{site}` returned AlreadyDiagnosed for path; \
                 expected E_MISSING_PATH_MATERIAL upstream",
                from = connect.from,
                to = connect.to,
                site = connect.site,
            );
            None
        }
    }
}

/// The cells a `connect` row's strip runs through, and the router's error if it fell back to
/// the blocked L; `None` after a `W_WALKWAY_BLOCKED` past the router's cap.
fn walkway_path<S: BuildHasher>(
    connect: &crate::resolve::ValidatedConnect,
    (from_pos, to_pos): (Cell, Cell),
    blocked: &HashSet<Cell, S>,
    blocked_index: &BlockedIndex<'_, S>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(Vec<Cell>, Option<RoutePathError>)> {
    // Measured before it is built: an unobstructed L never reaches the router's own cap check.
    let straight_area = l_path_area(from_pos, to_pos);
    if straight_area > ROUTE_AREA_CAP {
        diagnostics.push(Diagnostic {
            code: DiagnosticCode::WalkwayBlocked,
            span: connect.span.clone(),
            primary: format!(
                "walkway `{from} ↔ {to}` spans {straight_area} cells, past the \
                 {ROUTE_AREA_CAP}-cell router cap, and was not laid",
                from = connect.from,
                to = connect.to,
            ),
            notes: vec![DiagnosticNote {
                span: None,
                message: format!(
                    "the walkway search area ({straight_area} cells) exceeds the router's \
                     cap of {ROUTE_AREA_CAP} cells; place the two structures closer together",
                ),
            }],
            data: None,
        });
        return None;
    }
    let straight = l_path(from_pos, to_pos);
    Some(if straight.iter().any(|cell| blocked.contains(cell)) {
        match route_path(from_pos, to_pos, blocked_index) {
            Ok(detour) => (detour, None),
            Err(e) => (straight, Some(e)),
        }
    } else {
        (straight, None)
    })
}

/// A strip laid with `skipped` cells dropped where they overlapped a placement.
fn diag_walkway_blocked(
    connect: &crate::resolve::ValidatedConnect,
    skipped: usize,
    note: String,
) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::WalkwayBlocked,
        span: connect.span.clone(),
        primary: format!(
            "walkway `{from} ↔ {to}` skipped {skipped} cells that overlapped an existing structure",
            from = connect.from,
            to = connect.to,
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: note,
        }],
        data: Some(DiagnosticData::WalkwayBlocked {
            skipped: skipped as u64,
        }),
    }
}

/// A walkway that replaced an earlier row's walkway under the same scope key.
fn diag_walkway_replaced(
    connect: &crate::resolve::ValidatedConnect,
    scope_key: &WalkwayScopeKey,
) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::InvalidWalkwayIdent,
        span: connect.span.clone(),
        primary: format!(
            "walkway `{from} ↔ {to}` encodes to the scope key `{scope_key}` of an \
             earlier `connect` row, whose walkway it replaced",
            from = connect.from,
            to = connect.to,
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "two different endpoint pairs must never share a scope key, so \
                      this is a compiler bug; rename a place or port on one of the two \
                      rows to keep both walkways"
                .to_owned(),
        }],
        data: None,
    }
}

/// The two ports a `W_WALKWAY_BLOCKED` note names: each one's world cell and placement key.
struct BlockedEndpoints<'a> {
    from: ((i32, i32, i32), &'a str),
    to: ((i32, i32, i32), &'a str),
}

/// The finished floor plan a `W_WALKWAY_BLOCKED` note reads to name what is in the way.
struct SiteFloor<'a> {
    structures: &'a IndexMap<String, BlockArray>,
    placed: &'a IndexMap<String, PlacedBody>,
    floor: &'a FloorPlan,
}

/// Note text for a `W_WALKWAY_BLOCKED` row, matched to why the router could not detour, or
/// `None` when it would name a stand-in, a block only the target's pack can name.
fn walkway_blocked_note(
    connect: &crate::resolve::ValidatedConnect,
    route_failure: Option<RoutePathError>,
    ends: &BlockedEndpoints<'_>,
    site: &SiteFloor<'_>,
) -> Option<String> {
    let note = match route_failure {
        Some(RoutePathError::EndpointBlocked {
            from_blocked,
            to_blocked,
        }) => {
            let mut clauses = Vec::new();
            if from_blocked {
                clauses.push(buried_port_clause(&connect.from, ends.from, connect, site)?);
            }
            if to_blocked {
                clauses.push(buried_port_clause(&connect.to, ends.to, connect, site)?);
            }
            clauses.join("; ")
        }
        Some(RoutePathError::AreaCapExceeded { area, cap, rect }) => {
            area_cap_note(area, cap, rect, ends, connect, site)
        }
        Some(RoutePathError::CoordinateOverflow) => {
            "the walkway endpoints sit at the edge of the representable coordinate space; \
             move the site closer to the origin"
                .to_owned()
        }
        // `None` with skipped cells cannot happen; the shared arm keeps the note true if it does.
        Some(RoutePathError::TargetUnreachable) | None => {
            "no unobstructed route exists between the two ports; widen the placement gap so \
             the walkway can round the obstacle"
                .to_owned()
        }
    };
    Some(note)
}

/// The note for a router whose [`SearchRect`] passed the area cap, naming what stretched it.
fn area_cap_note(
    area: u64,
    cap: u64,
    rect: SearchRect,
    ends: &BlockedEndpoints<'_>,
    connect: &crate::resolve::ValidatedConnect,
    site: &SiteFloor<'_>,
) -> String {
    let (from, to) = (ends.from.0, ends.to.0);
    let head = format!(
        "the router searches the box spanning both ports and every placement floor on the walk \
         plane, here {area} cells, past its cap of {cap} cells"
    );
    let ports_area = margin_box_area(from, to);
    if ports_area > cap {
        return format!(
            "{head}; the two ports alone span {ports_area} of those cells, so place the two \
             structures closer together"
        );
    }
    let labels: Vec<String> = edge_pushers(rect, from, to, &site.floor.extents)
        .into_iter()
        .filter_map(|key| site.placed.get(key))
        .map(|body| placement_label(&body.placement, connect))
        .collect();
    if labels.is_empty() {
        debug_assert!(
            false,
            "the search rectangle {rect:?} passed the cap, but no floor stretches it past the \
             ports {from:?} and {to:?}",
        );
        return format!("{head}; bring the outlying placements closer together");
    }
    let (floors, verb, pronoun) = if labels.len() == 1 {
        ("the floor of placement", "stretches", "that placement")
    } else {
        ("the floors of placements", "stretch", "those placements")
    };
    format!(
        "{head}; the two ports span only {ports_area} of those cells, and {floors} {list} \
         {verb} the box past them, so bring {pronoun} closer to the two ports",
        list = and_list(&labels).unwrap_or_default(),
    )
}

/// Cells in the box around `from` and `to` with the router's one-cell margin.
fn margin_box_area(from: (i32, i32, i32), to: (i32, i32, i32)) -> u64 {
    let span = |a: i32, b: i32| u64::from(a.abs_diff(b)) + 3;
    span(from.0, to.0).saturating_mul(span(from.2, to.2))
}

/// The keys of the placements whose floor sets an edge of `rect` beyond both ports.
pub(super) fn edge_pushers(
    rect: SearchRect,
    from: (i32, i32, i32),
    to: (i32, i32, i32),
    extents: &IndexMap<String, FloorExtent>,
) -> Vec<&str> {
    let y = from.1;
    let (port_min_x, port_max_x) = (from.0.min(to.0), from.0.max(to.0));
    let (port_min_z, port_max_z) = (from.2.min(to.2), from.2.max(to.2));
    let inner = |edge: i32, toward_centre: i64| i64::from(edge) + toward_centre;
    extents
        .iter()
        .filter(|(_, e)| e.y == y)
        .filter(|(_, e)| {
            (e.min_x < port_min_x && i64::from(e.min_x) == inner(rect.min_x, 1))
                || (e.max_x > port_max_x && i64::from(e.max_x) == inner(rect.max_x, -1))
                || (e.min_z < port_min_z && i64::from(e.min_z) == inner(rect.min_z, 1))
                || (e.max_z > port_max_z && i64::from(e.max_z) == inner(rect.max_z, -1))
        })
        .map(|(key, _)| key.as_str())
        .collect()
}

/// A placement as a note names it: its `place id=`, plus its site when not the row's own.
fn placement_label(placement: &Placement, connect: &crate::resolve::ValidatedConnect) -> String {
    if placement.site == connect.site {
        format!("`{}`", placement.place_id)
    } else {
        format!("`{}` in site `{}`", placement.place_id, placement.site)
    }
}

/// One clause of a buried-endpoint note: what covers the port cell, and the remedy for each; `None`
/// when the block on the cell is a stand-in.
fn buried_port_clause(
    port: &crate::resolve::PortRef,
    (cell, own_key): ((i32, i32, i32), &str),
    connect: &crate::resolve::ValidatedConnect,
    site: &SiteFloor<'_>,
) -> Option<String> {
    let mut own_block: Option<Option<&str>> = None;
    let mut own_label = None;
    let mut others: Vec<String> = Vec::new();
    for owner in site.floor.owners.get(&cell).into_iter().flatten() {
        let Some((key, body)) = site.placed.get_index(owner.placement) else {
            // INVARIANT: `collect_floor_cells` records indices into this same, unmutated `placed`.
            debug_assert!(false, "floor owner {owner:?} indexes past `placed`");
            continue;
        };
        if key == own_key {
            let block = site
                .structures
                .get(key)
                .and_then(|ba| ba.palette.entries.get(usize::from(owner.voxel.0)));
            if block.is_none() {
                // INVARIANT: the voxel was read from this placement's own array and palette.
                debug_assert!(
                    false,
                    "voxel {owner:?} of `{key}` names no entry of its palette",
                );
            }
            if block.is_some_and(is_stand_in) {
                return None;
            }
            own_block = Some(block.map(|state| state.id.as_str()));
            own_label = Some(placement_label(&body.placement, connect));
        } else {
            others.push(placement_label(&body.placement, connect));
        }
    }
    let own = own_block.zip(own_label).map(|(block, own)| match block {
        Some(block) => format!("the `{block}` its own placement {own} lays on that cell"),
        None => format!("a block its own placement {own} lays on that cell"),
    });
    let others = (!others.is_empty()).then(|| {
        let noun = if others.len() == 1 {
            "placement"
        } else {
            "placements"
        };
        format!(
            "the floor of {noun} {}",
            and_list(&others).unwrap_or_default()
        )
    });
    let clause = match (own, others) {
        (Some(own), None) => {
            format!("port `{port}` sits on {own}; move that block or the door/window")
        }
        (None, Some(others)) => format!(
            "port `{port}` is buried inside {others}; move that door/window to an \
             unobstructed wall or pull the placements apart"
        ),
        (Some(own), Some(others)) => format!(
            "port `{port}` is buried inside {others} and sits on {own}; move that block, and \
             move that door/window to an unobstructed wall or pull the placements apart"
        ),
        (None, None) => {
            // INVARIANT: every cell the router reports blocked was recorded with its owners.
            debug_assert!(
                false,
                "port `{port}` at {cell:?} is blocked but has no owner"
            );
            format!(
                "port `{port}` is buried inside another placement's floor; move that \
                 door/window to an unobstructed wall or pull the placements apart"
            )
        }
    };
    Some(clause)
}

fn diag_walkway_invalid_ident(
    connect: &crate::resolve::ValidatedConnect,
    err: &crate::ids::KeyConstructError,
) -> Diagnostic {
    use crate::ids::{KeyConstructError, KeySegmentRole};
    let (role, segment, problem, constraint, fix): (KeySegmentRole, _, _, _, _) = match err {
        KeyConstructError::ConsecutiveUnderscore { role, segment } => (
            *role,
            segment,
            "contains `__`, the separator between the walkway scope key's `from` and `to` \
             halves",
            "a walkway's site, place and port ids may not contain `__`, so no id can be \
             mistaken for the separator",
            "so it has no `__`, e.g. by replacing `__` with `_`",
        ),
        KeyConstructError::UnderscoreAtEdge { role, segment } => (
            (*role).into(),
            segment,
            "starts or ends with `_`",
            "a walkway's place and port ids may not start or end with `_` in either \
             position, so the rule does not depend on which way the row is written",
            "so it neither starts nor ends with `_`",
        ),
    };
    Diagnostic {
        code: DiagnosticCode::InvalidWalkwayIdent,
        span: connect.span.clone(),
        primary: format!(
            "walkway `{from} ↔ {to}` was dropped because the {role} id `{segment}` {problem}",
            from = connect.from,
            to = connect.to,
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: format!("{constraint}; rename the {role} {fix}"),
        }],
        data: None,
    }
}

/// The endpoint(s) of `connect` a diagnostic blames, each in backticks.
fn blamed_endpoints(
    connect: &crate::resolve::ValidatedConnect,
    from_at_fault: bool,
    to_at_fault: bool,
) -> String {
    let (from, to) = (&connect.from, &connect.to);
    match (from_at_fault, to_at_fault) {
        (true, true) => format!("`{from}` and `{to}`"),
        (true, false) => format!("`{from}`"),
        (false, true) => format!("`{to}`"),
        (false, false) => unreachable!("at least one endpoint must be at fault"),
    }
}

fn diag_walkway_endpoint_skipped(
    connect: &crate::resolve::ValidatedConnect,
    from_missing: bool,
    to_missing: bool,
) -> Diagnostic {
    let missing = blamed_endpoints(connect, from_missing, to_missing);
    let placements = if from_missing && to_missing {
        "placements"
    } else {
        "placement"
    };
    Diagnostic {
        code: DiagnosticCode::DeferredMember,
        span: connect.span.clone(),
        primary: format!(
            "walkway `{from} ↔ {to}` was skipped because the {missing} {placements} did not lower",
            from = connect.from,
            to = connect.to,
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "fix the upstream W_DEF_NO_SIZE / W_DEFERRED_MEMBER on the endpoint to \
                 bring the walkway back (the resolver drops the connect itself when \
                 E_UNRESOLVED_PLACE_REF fires, so the cascade never points there)"
                .to_owned(),
        }],
        data: None,
    }
}
