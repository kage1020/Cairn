//! Intent IR → block-array IR lowering.
//!
//! The pass is total: every struct ends up in
//! [`BlockArrayIr::structures`], every issue surfaces as a warning on
//! [`BlockArrayIr::diagnostics`]. That keeps `cairn lower` useful even on
//! a half-finished module — the operator can see what voxels did lower,
//! and the diagnostic stream tells them what was skipped and why.
//!
//! ## Phase ordering
//!
//! `spec/compilation.md` §4.1 evaluates members in a fixed phase order
//! independent of source order:
//!
//! ```text
//! massing  (floor, walls)
//!   → envelope (roof, stair)
//!   → openings (door, window)
//!   → fixtures, logic_*, raw
//! ```
//!
//! The current pass implements the first three (massing, envelope,
//! openings). Members are bucketed by role and processed phase-by-phase;
//! within a phase source order wins (the last-wins rule for local
//! overrides). Roles outside the three implemented phases emit
//! `W_DEFERRED_MEMBER` and skip. `level y=N` blocks are flattened into
//! their children before bucketing so a nested `walls` / `door` /
//! `window` / `stair` reaches its phase with the level's `y=` applied
//! as an authored offset (see [`flatten_members`]).
//!
//! Defs are skipped at this layer: they only concretise via a `site`
//! `place ... use=def_name` reference, and site lowering arrives with the
//! multi-building pass. Sites themselves are also skipped for the same
//! reason.

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::ast::ValueKind;
use crate::check::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote, Severity};
use crate::error::Span;
use crate::ids::{PlaceId, PortId, SiteName, WalkwayEndpoint, WalkwayScopeKey};
use crate::intent::{
    DefIr, IntentModule, Member, MemberRole, SiteIr, Size, StructIr, ValueWithSpan,
};
use crate::resolve::{Resolution, ScopeResolution, place_scope_key};

use super::{Footprint, MAX_STRUCTURE_VOLUME, Placement, Walkway};

use super::material::{AbstractMaterialResolver, MaterialDeferred, resolve_block_state};
use super::openings::{WallSide, wall_length, wall_local_to_grid};
use super::roof::{
    Cardinal, GableVoxel, HipVoxel, RoofKind, STAIR_BASE_ID, ShedFace, ShedVoxel, StairFace,
    StairShape, flat_block_state, flat_extra_height, flat_voxels, gable_extra_height,
    gable_ridge_axis, gable_stair_state, gable_voxels, hip_extra_height, hip_stair_state,
    hip_voxels, shed_extra_height, shed_high_side, shed_slope_span, shed_stair_state, shed_voxels,
    stair_state,
};
use super::walkway::{
    BlockedIndex, ROUTE_AREA_CAP, RoutePathError, WalkwayLayout, build_walkway_array, l_path,
    l_path_area, port_world_position, route_path,
};
use super::{BlockArray, BlockArrayIr, BlockState, Dims, Palette, PaletteIndex};

/// Vanilla pressure plate id used by `pressure_plate` members that do not
/// resolve a `mat_slot=` binding. Species-specific plates (spruce, dark
/// oak, ...) land with the registry pack once the theme-slot table grows
/// a fixture axis; until then a bare `pressure_plate` lowers to
/// `oak_pressure_plate` and a `mat_slot=` that resolves to a different id
/// is honoured verbatim (mirroring [`STAIR_BASE_ID`]'s contract).
const PRESSURE_PLATE_BASE_ID: &str = "minecraft:oak_pressure_plate";

/// Lower every `struct` in `intent` into a [`BlockArray`].
///
/// Pairs each struct with its [`ScopeResolution`] from `resolution` so the
/// material lookups go through the same theme bindings `cairn check` and
/// `cairn info` already used. Members are processed in phase order
/// (massing → envelope → openings), so a `door` written before `walls` in
/// the source still cuts an opening through the resulting wall. Roles
/// outside the three implemented phases are reported via
/// `W_DEFERRED_MEMBER` and skipped.
///
/// `materials` is the registry-pack-backed abstract-token lifter. `Some`
/// turns `@floor.wood.broadleaf`-style tokens into concrete Java ids and
/// fail-loud on misses; `None` keeps the pre-PR2 behaviour where every
/// abstract token degrades to a `W_ABSTRACT_TOKEN_DEFERRED` warning so
/// library callers without a pack still get a partial build.
#[must_use]
pub fn lower_to_block_array(
    intent: &IntentModule,
    resolution: &Resolution,
    materials: Option<&dyn AbstractMaterialResolver>,
) -> BlockArrayIr {
    let mut structures: IndexMap<String, BlockArray> = IndexMap::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();

    for s in &intent.structs {
        let key = format!("struct::{}", s.name);
        let scope = resolution.scopes.get(&key);
        // `lower_struct` returns `None` only after it has already pushed a
        // diagnostic (no `size=`, etc.), so the skip here is silent on
        // purpose — diagnosing twice would teach a reader the struct had
        // two unrelated problems instead of one.
        if let Some(ba) = lower_struct(s, scope, materials, &mut diagnostics) {
            // First-write-wins on a duplicate name, matching
            // `resolve`'s `FIRST_BINDING_WINS`. `resolution.scopes` has
            // already bound the first body; taking the last here would
            // paint the second body's voxels with the first body's
            // resolved materials.
            structures.entry(key).or_insert(ba);
        }
    }

    let mut placements: IndexMap<String, Placement> = IndexMap::new();
    let mut walkways: IndexMap<WalkwayScopeKey, Walkway> = IndexMap::new();
    for site in &intent.sites {
        lower_site(
            site,
            &intent.defs,
            resolution,
            materials,
            &mut structures,
            &mut placements,
            &mut diagnostics,
        );
    }
    // Walkways are laid after every site has emitted its per-place
    // BlockArrays so the collision set already covers every floor tile
    // the strip might cross. Connects survive site boundaries — the
    // resolver tags each `ValidatedConnect` with the `site` name so we
    // can pair it back to the right `placements` lookup here.
    let blocked = collect_floor_cells(&structures, &placements);
    lower_connects(
        resolution,
        &intent.defs,
        materials,
        &placements,
        &blocked,
        &mut structures,
        &mut walkways,
        &mut diagnostics,
    );

    BlockArrayIr {
        structures,
        placements,
        walkways,
        diagnostics,
    }
}

/// World-space `(x, y, z)` of every non-air voxel on the y=0 plane of
/// every placement. The walkway voxeliser uses this set to skip cells
/// that would overwrite an existing floor tile — a strip ducking under
/// a corner of a building still completes, but the colliding cell stays
/// air and the row earns a `W_WALKWAY_BLOCKED` warning.
fn collect_floor_cells(
    structures: &IndexMap<String, BlockArray>,
    placements: &IndexMap<String, Placement>,
) -> HashSet<(i32, i32, i32)> {
    let mut out: HashSet<(i32, i32, i32)> = HashSet::new();
    for (key, placement) in placements {
        let Some(ba) = structures.get(key) else {
            continue;
        };
        // Only the y=0 plane matters: walkways sit at the ports' shared
        // Y (=0 for every example). 3D path search (staircases, multi-level
        // walkways) is intentionally out of scope so the port surface lands
        // in one piece.
        for z in 0..ba.dims.z {
            for x in 0..ba.dims.x {
                let Some(i) = ba.dims.index(x, 0, z) else {
                    continue;
                };
                if ba.voxels[i] == PaletteIndex::AIR {
                    continue;
                }
                let wx = placement
                    .origin
                    .0
                    .saturating_add(i32::try_from(x).unwrap_or(i32::MAX));
                let wz = placement
                    .origin
                    .2
                    .saturating_add(i32::try_from(z).unwrap_or(i32::MAX));
                out.insert((wx, placement.origin.1, wz));
            }
        }
    }
    out
}

/// Lower every resolved `connect` row into a walkway `BlockArray` and
/// a matching [`Walkway`] metadata record. Skips rows whose ports do
/// not resolve to a [`MemberRole::Door`] (other roles are not yet
/// modelled as ports) and emits a `W_DUPLICATE_WALKWAY` when the same
/// `(from, to)` pair has already been laid in the same site.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn lower_connects(
    resolution: &Resolution,
    defs: &[DefIr],
    materials: Option<&dyn AbstractMaterialResolver>,
    placements: &IndexMap<String, Placement>,
    blocked: &HashSet<(i32, i32, i32)>,
    structures: &mut IndexMap<String, BlockArray>,
    walkways: &mut IndexMap<WalkwayScopeKey, Walkway>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut seen_pairs: HashSet<(SiteName, PlaceId, PortId, PlaceId, PortId)> = HashSet::new();
    // Index the blocked set once for every row: the router needs the
    // per-plane bounding rectangle, and deriving it per row would
    // re-scan the whole set — a large site with many colliding rows
    // would multiply one linear scan into an effective DoS on user
    // input. See `BlockedIndex` for the cost contract.
    let blocked_index = BlockedIndex::new(blocked);

    for connect in &resolution.connects {
        let from_key = place_scope_key(connect.site.as_str(), connect.from.place.as_str());
        let to_key = place_scope_key(connect.site.as_str(), connect.to.place.as_str());
        let from_placement = placements.get(&from_key);
        let to_placement = placements.get(&to_key);
        let (Some(from_placement), Some(to_placement)) = (from_placement, to_placement) else {
            // At least one placement was rejected upstream (sizeless def,
            // unresolved theme, broken origin chain). The connect itself
            // resolved, so without a follow-up warning the walkway would
            // vanish silently — emit a cascade `W_DEFERRED_MEMBER` that
            // names the offending side so the user can see *why* the
            // strip was not laid.
            diagnostics.push(diag_walkway_endpoint_skipped(
                connect,
                from_placement.is_none(),
                to_placement.is_none(),
            ));
            continue;
        };
        let from_def = defs.iter().find(|d| d.name == from_placement.source_def);
        let to_def = defs.iter().find(|d| d.name == to_placement.source_def);
        let (Some(from_def), Some(to_def)) = (from_def, to_def) else {
            // Invariant: `lower_site` only inserts a `Placement` after
            // resolving its `use=DEF` against `defs`, so a placement
            // pointing at an absent def cannot exist here. Encode that
            // as a `debug_assert!` so a future refactor that breaks the
            // chain fails loud in tests instead of dropping the strip.
            debug_assert!(
                false,
                "connect `{}` to `{}` references placements whose source def is missing from `defs`",
                connect.from.place, connect.to.place,
            );
            continue;
        };

        let from_pos = port_world_position(
            from_placement.origin,
            from_placement.dims,
            from_def,
            &connect.from.port,
        );
        let to_pos = port_world_position(
            to_placement.origin,
            to_placement.dims,
            to_def,
            &connect.to.port,
        );
        let (Some(from_pos), Some(to_pos)) = (from_pos, to_pos) else {
            // The resolver already validated the port id, so this miss
            // means `port_world_position` rejected one of the member's
            // own properties: a missing / non-cardinal `side=`, a door
            // `at=` value outside `center | left | right`, a window
            // whose `offset + size.w` overflows the wall or whose
            // `y + size.h` overflows the walls `height=`, or a
            // stair / roof role for which port support is reserved.
            // Name the offending side so the user is not pointed at
            // the wrong half of the row.
            let from_label = connect.from.to_string();
            let to_label = connect.to.to_string();
            let unplaceable = match (from_pos.is_none(), to_pos.is_none()) {
                (true, true) => format!("`{from_label}` and `{to_label}`"),
                (true, false) => format!("`{from_label}`"),
                (false, true) => format!("`{to_label}`"),
                (false, false) => unreachable!("else arm requires at least one None"),
            };
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::DeferredMember,
                severity: Severity::Warning,
                span: connect.span.clone(),
                primary: format!(
                    "walkway `{from_label} ↔ {to_label}` was skipped because port {unplaceable} could not be placed",
                ),
                notes: vec![
                    DiagnosticNote {
                        span: None,
                        message:
                            "a `door` port requires `side=front|back|left|right` and `at=center|left|right`"
                                .to_owned(),
                    },
                    DiagnosticNote {
                        span: None,
                        message:
                            "a `window` port requires `side=front|back|left|right`, plus `offset=` / `y=` / `size=WxH` that fit inside the wall (`offset + size.w ≤ wall_length` and `y + size.h ≤ walls.height`)"
                                .to_owned(),
                    },
                    DiagnosticNote {
                        span: None,
                        message:
                            "stair / roof / other member roles cannot anchor a port yet — declare the port on a door or window instead"
                                .to_owned(),
                    },
                ],
                data: None,
            });
            continue;
        };

        // Duplicate guard: pin on (site, from_place, from_port,
        // to_place, to_port). Normalise the pair (sort the two ends)
        // so `a.entry → b.entry` and `b.entry → a.entry` count as the
        // same walkway — laying the strip both ways would be a silent
        // double-write.
        let mut endpoints = [
            (connect.from.place.clone(), connect.from.port.clone()),
            (connect.to.place.clone(), connect.to.port.clone()),
        ];
        endpoints.sort_unstable();
        let [(a_place, a_port), (b_place, b_port)] = endpoints;
        let dedup_key = (connect.site.clone(), a_place, a_port, b_place, b_port);
        if !seen_pairs.insert(dedup_key) {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::DuplicateWalkway,
                severity: Severity::Warning,
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
            });
            continue;
        }

        let material = match resolve_block_state(&connect.path, materials) {
            Ok(state) => state,
            Err(MaterialDeferred::Abstract(token)) => {
                diagnostics.push(diag_walkway_abstract_token(connect, &token));
                continue;
            }
            Err(MaterialDeferred::UnknownAbstract { token, suggestion }) => {
                diagnostics.push(diag_walkway_unknown_token(
                    connect,
                    &token,
                    suggestion.as_deref(),
                ));
                continue;
            }
            Err(MaterialDeferred::AlreadyDiagnosed) => {
                // INVARIANT(upstream-diagnosed): `resolve_block_state`
                // returns `AlreadyDiagnosed` only when the input value's
                // `ValueKind` is not a token (see
                // `material::resolve_block_state` /
                // `TokenKind::NotAToken`). The resolver's connect-row pass
                // (`resolve::resolver::resolve_connect_row`) rejects every
                // non-token `path=` shape with `E_MISSING_PATH_MATERIAL`
                // before the row enters `resolution.connects`, so we
                // cannot legitimately reach this arm. A future change that
                // bypasses that check would otherwise drop the strip
                // silently — fail loud in debug builds instead.
                debug_assert!(
                    false,
                    "connect `{from}` to `{to}` in site `{site}` returned AlreadyDiagnosed for path; \
                     expected E_MISSING_PATH_MATERIAL upstream",
                    from = connect.from,
                    to = connect.to,
                    site = connect.site,
                );
                continue;
            }
        };
        let material_id = material.id.clone();

        // Straight Manhattan L first — the cheap path, and identity for
        // every unobstructed row (existing lockfiles stay byte-stable).
        // Only when the L collides with a placement floor does the
        // ground-plane router search for a detour; a `RoutePathError`
        // (endpoint buried, target enclosed, area cap, coordinate
        // overflow) falls back to the L with skipped cells so the row
        // still lays and earns its `W_WALKWAY_BLOCKED` below, with a
        // note matched to the error.
        // Ask how long the L would be before building it. The cap has
        // always described this case — "two ports megametres apart" — but
        // only `route_path` consulted it, and `route_path` runs second and
        // only when something is in the way. An unobstructed pair walked
        // past the cap and materialised the whole strip: `gap=100000000`
        // spent 53 seconds on a 1.4 GB `Vec` before any check saw it.
        // Measure before building. `route_path` already refuses on this
        // quantity, but it runs second and only when the straight L is
        // obstructed — an unobstructed pair reached `build_walkway_array`
        // and sized a voxel buffer from the bounding box directly.
        let straight_area = l_path_area(from_pos, to_pos);
        if straight_area > ROUTE_AREA_CAP {
            let failure = RoutePathError::AreaCapExceeded {
                area: straight_area,
                cap: ROUTE_AREA_CAP,
            };
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::WalkwayBlocked,
                severity: Severity::Warning,
                span: connect.span.clone(),
                primary: format!(
                    "walkway `{from} ↔ {to}` spans {straight_area} cells, past the \
                     {ROUTE_AREA_CAP}-cell router cap, and was not laid",
                    from = connect.from,
                    to = connect.to,
                ),
                notes: vec![DiagnosticNote {
                    span: None,
                    message: walkway_blocked_note(connect, Some(failure)),
                }],
                data: None,
            });
            continue;
        }
        let straight = l_path(from_pos, to_pos);
        let (path, route_failure) = if straight.iter().any(|cell| blocked.contains(cell)) {
            match route_path(from_pos, to_pos, &blocked_index) {
                Ok(detour) => (detour, None),
                Err(e) => (straight, Some(e)),
            }
        } else {
            (straight, None)
        };
        let routed = route_failure.is_none();
        let from_endpoint = WalkwayEndpoint {
            place: connect.from.place.clone(),
            port: connect.from.port.clone(),
        };
        let to_endpoint = WalkwayEndpoint {
            place: connect.to.place.clone(),
            port: connect.to.port.clone(),
        };
        let scope_key =
            match WalkwayScopeKey::from_parts(&connect.site, &from_endpoint, &to_endpoint) {
                Ok(k) => k,
                Err(e) => {
                    diagnostics.push(diag_walkway_invalid_ident(connect, &e));
                    continue;
                }
            };
        let WalkwayLayout {
            array,
            origin,
            blocked_count: skipped,
        } = build_walkway_array(&path, material, blocked, &scope_key);
        if routed {
            // Both the collision-free straight L and a router detour
            // are collision-free by construction; a skipped cell here
            // means the router returned a path that crosses `blocked`,
            // which is an algorithm bug, not an input condition.
            debug_assert_eq!(
                skipped, 0,
                "walkway `{scope_key}` laid a routed path with {skipped} collisions",
            );
        }
        if skipped > 0 {
            diagnostics.push(Diagnostic {
                code: DiagnosticCode::WalkwayBlocked,
                severity: Severity::Warning,
                span: connect.span.clone(),
                primary: format!(
                    "walkway `{from} ↔ {to}` skipped {skipped} cells that overlapped an existing structure",
                    from = connect.from,
                    to = connect.to,
                ),
                notes: vec![DiagnosticNote {
                    span: None,
                    message: walkway_blocked_note(connect, route_failure),
                }],
                data: Some(DiagnosticData::WalkwayBlocked {
                    skipped: skipped as u64,
                }),
            });
        }
        let dims = array.dims;
        debug_assert_eq!(
            dims.y, 1,
            "walkway block array must be 1 block thick; build_walkway_array's contract \
             pins y = 1 and the lockfile relies on this when re-attaching the implicit y \
             via Footprint::to_dims_y1",
        );
        let footprint = Footprint {
            x: dims.x,
            z: dims.z,
        };
        structures.insert(scope_key.as_str().to_owned(), array);
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

/// Note text for a `W_WALKWAY_BLOCKED` warning, matched to why the
/// router could not detour. The remedies differ per cause — widening
/// the gap fixes an enclosed target but does nothing for a port buried
/// under another placement's floor or a site past the area cap — so a
/// single catch-all suggestion would misdirect the author on three of
/// the four arms.
fn walkway_blocked_note(
    connect: &crate::resolve::ValidatedConnect,
    route_failure: Option<RoutePathError>,
) -> String {
    match route_failure {
        Some(RoutePathError::EndpointBlocked {
            from_blocked,
            to_blocked,
        }) => {
            let buried = match (from_blocked, to_blocked) {
                (true, true) => format!(
                    "ports `{from}` and `{to}` are",
                    from = connect.from,
                    to = connect.to,
                ),
                (true, false) => format!("port `{from}` is", from = connect.from),
                (false, true) => format!("port `{to}` is", to = connect.to),
                (false, false) => {
                    unreachable!("EndpointBlocked carries at least one blocked side")
                }
            };
            format!(
                "{buried} buried inside another placement's floor; move that door/window to \
                 an unobstructed wall or pull the placements apart",
            )
        }
        Some(RoutePathError::AreaCapExceeded { area, cap }) => format!(
            "the walkway search area ({area} cells) exceeds the router's cap of {cap} cells; \
             place the two structures closer together",
        ),
        Some(RoutePathError::CoordinateOverflow) => {
            "the walkway endpoints sit at the edge of the representable coordinate space; \
             move the site closer to the origin"
                .to_owned()
        }
        // `TargetUnreachable` and `None` share the generic remedy: the
        // ports are fine but every route between them is walled off.
        // (`None` with skipped cells cannot happen — the router is only
        // bypassed when the straight L is collision-free — but the
        // catch-all keeps the note truthful if that wiring ever drifts.)
        Some(RoutePathError::TargetUnreachable) | None => {
            "no unobstructed route exists between the two ports; widen the placement gap so \
             the walkway can round the obstacle"
                .to_owned()
        }
    }
}

fn diag_walkway_invalid_ident(
    connect: &crate::resolve::ValidatedConnect,
    err: &crate::ids::KeyConstructError,
) -> Diagnostic {
    let crate::ids::KeyConstructError::ConsecutiveUnderscore { role, segment } = err;
    Diagnostic {
        code: DiagnosticCode::InvalidWalkwayIdent,
        severity: Severity::Warning,
        span: connect.span.clone(),
        primary: format!(
            "walkway `{from} ↔ {to}` was dropped because the {role} id `{segment}` \
             contains `__`, which collides with the walkway scope key's \
             `from`/`to` separator",
            from = connect.from,
            to = connect.to,
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "rename the offending id (e.g. replace `__` with `_`) so the \
                      lowered walkway scope key is unambiguous"
                .to_owned(),
        }],
        data: None,
    }
}

fn diag_walkway_endpoint_skipped(
    connect: &crate::resolve::ValidatedConnect,
    from_missing: bool,
    to_missing: bool,
) -> Diagnostic {
    let from_label = connect.from.to_string();
    let to_label = connect.to.to_string();
    let missing = match (from_missing, to_missing) {
        (true, true) => format!("`{from_label}` and `{to_label}` placements"),
        (true, false) => format!("`{from_label}` placement"),
        (false, true) => format!("`{to_label}` placement"),
        // Caller only invokes this helper when at least one side is
        // missing; the unreachable arm fails loud in tests if a future
        // refactor breaks that contract instead of emitting an empty
        // message at runtime.
        (false, false) => {
            unreachable!("diag_walkway_endpoint_skipped requires at least one side missing")
        }
    };
    Diagnostic {
        code: DiagnosticCode::DeferredMember,
        severity: Severity::Warning,
        span: connect.span.clone(),
        primary: format!(
            "walkway `{from_label} ↔ {to_label}` was skipped because the {missing} did not lower",
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

fn diag_walkway_abstract_token(
    connect: &crate::resolve::ValidatedConnect,
    token: &str,
) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::AbstractTokenDeferred,
        severity: Severity::Warning,
        span: connect.path.span.clone(),
        primary: format!(
            "abstract path token `@{token}` cannot be lowered without the registry pack; the walkway falls back to air",
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message:
                "use a canonical block token (e.g. `path=@gravel`) until the registry pack ships"
                    .to_owned(),
        }],
        data: None,
    }
}

fn diag_walkway_unknown_token(
    connect: &crate::resolve::ValidatedConnect,
    token: &str,
    suggestion: Option<&str>,
) -> Diagnostic {
    let mut notes = Vec::with_capacity(2);
    if let Some(s) = suggestion {
        notes.push(DiagnosticNote {
            span: None,
            message: format!("did you mean `@{s}`?"),
        });
    }
    notes.push(DiagnosticNote {
        span: None,
        message: "abstract path tokens must be declared in the pack's `materials` catalog"
            .to_owned(),
    });
    Diagnostic {
        code: DiagnosticCode::UnknownAbstractToken,
        severity: Severity::Error,
        span: connect.path.span.clone(),
        primary: format!(
            "abstract path token `@{token}` is not declared by the registry pack's materials catalog",
        ),
        notes,
        data: None,
    }
}

fn lower_struct<'a>(
    s: &StructIr,
    scope: Option<&'a ScopeResolution>,
    materials: Option<&'a dyn AbstractMaterialResolver>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<BlockArray> {
    let Some(size) = s.size.as_ref() else {
        diagnostics.push(diag_struct_no_size(s));
        return None;
    };
    lower_body_to_block_array(
        BodyDescriptor {
            kind: BodyKind::Struct,
            scope_label: &s.name,
            size,
            members: &s.members,
            header_span: &s.span,
            source_scope: format!("struct::{}", s.name),
        },
        scope,
        materials,
        diagnostics,
    )
}

/// Lower every `place` in `site` into its own per-place [`BlockArray`] and a
/// matching [`Placement`] record carrying the resolved world-space origin.
///
/// Cross-scope semantics: a place's `theme=` argument has already been
/// applied by the resolver (`place_scope_key` lookup), so the lowering pass
/// just walks the def's members under the prepared [`ScopeResolution`]. The
/// resolver emits every fail-loud diagnostic (`E_UNRESOLVED_PLACE_REF`,
/// `E_UNRESOLVED_THEME_REF`, `E_DUPLICATE_PLACE_ID`,
/// `E_INVALID_PLACE_ORIGIN`); this pass owns:
///
/// - the topological → absolute coordinate solver
///   (`at=origin`, `east_of=ID gap=N`, `north_of=ID gap=N`),
/// - the per-place IR emission into `structures` / `placements` under the
///   `site::SITE::PLACE_ID` key.
///
/// `connect` rows on the site body are handled by [`lower_connects`] (one
/// walkway `BlockArray` per row); members on a placed `def` whose role the
/// block-array lowering pass does not yet support surface as
/// `W_DEFERRED_MEMBER` from the def-walking step instead.
fn lower_site<'a>(
    site: &SiteIr,
    defs: &[DefIr],
    resolution: &'a Resolution,
    materials: Option<&'a dyn AbstractMaterialResolver>,
    structures: &mut IndexMap<String, BlockArray>,
    placements: &mut IndexMap<String, Placement>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for member in &site.placements {
        if matches!(member.role, MemberRole::Connect) {
            // `connect` rows are lowered after every placement lands so
            // walkway voxelisation can collide-check against the
            // finished floor plan. See `lower_connects` for the second
            // pass; nothing to do per-row here.
            continue;
        }
        if !matches!(member.role, MemberRole::Place) {
            continue;
        }
        let Some(place_id) = member.id.as_deref() else {
            continue;
        };
        // Build the typed ids once, here, rather than asserting the
        // invariant again at the bottom of the loop, which is where a
        // `place id="home.1"` used to `expect` and panic.
        //
        // The two ids fail for different reasons and so are handled
        // separately. `id=` takes a string literal, so nothing upstream of
        // the resolver's `E_INVALID_PLACE_ID` constrains its contents —
        // reachable, already reported, skip silently the way the
        // missing-scope arm below does.
        let Ok(placement_id) = PlaceId::new(place_id) else {
            continue;
        };
        // A site name is an identifier the lexer produced, so it cannot
        // carry `.`, `:`, or whitespace. Folding this into the arm above
        // would mean a future relaxation of the site-name grammar silently
        // dropped every place in the site; the `debug_assert!` convention
        // this file already uses for unreachable invariants fails loud in
        // tests instead.
        let Ok(placement_site) = SiteName::new(site.name.as_str()) else {
            debug_assert!(
                false,
                "site name `{}` is not a valid SiteName; the lexer is supposed to guarantee it",
                site.name,
            );
            continue;
        };

        let key = place_scope_key(&site.name, place_id);
        let Some(scope) = resolution.scopes.get(&key) else {
            // The resolver emitted `E_UNRESOLVED_PLACE_REF` /
            // `E_UNRESOLVED_THEME_REF` / `E_INVALID_PLACE_ORIGIN`; lowering
            // skips this place silently so the diagnostic count stays
            // honest.
            continue;
        };

        let use_name = member
            .intent_state
            .get("use")
            .and_then(|v| v.value.as_label_str());
        let theme_name = member
            .intent_state
            .get("theme")
            .and_then(|v| v.value.as_label_str());
        let (Some(use_name), Some(theme_name)) = (use_name, theme_name) else {
            continue;
        };
        let Some(def) = defs.iter().find(|d| d.name == use_name) else {
            continue;
        };
        let Some(def_size) = def.size.as_ref() else {
            // The spec keeps `def NAME size=WxH` mandatory because a sized
            // template is what `place` instantiates; a sizeless def cannot
            // produce a voxel volume. Surface the same warning lib code
            // uses for sizeless structs so the failure mode is consistent.
            diagnostics.push(diag_def_no_size(def));
            continue;
        };

        // The origin solver reads `placements` for prior-place lookups, so
        // the lookup has to happen before *this* placement is inserted.
        // Lookup misses only happen when the prior place was skipped at
        // lowering time (cascade from `W_DEF_NO_SIZE` /
        // `E_UNRESOLVED_PLACE_REF`); falling back to `(0, 0, 0)` would
        // silently stack the placement on top of `home1`, so we surface a
        // deferred warning and skip the row instead.
        let Some(origin) = resolve_place_origin(member, placements, &site.name) else {
            diagnostics.push(diag_deferred_member_reason(
                member,
                "the prior place referenced by `east_of=`/`north_of=` did not lower, so this placement's origin cannot be resolved",
            ));
            continue;
        };

        let Some(ba) = lower_body_to_block_array(
            BodyDescriptor {
                kind: BodyKind::Place,
                scope_label: place_id,
                size: def_size,
                members: &def.members,
                header_span: &member.span,
                source_scope: key,
            },
            Some(scope),
            materials,
            diagnostics,
        ) else {
            // The extent was refused; the diagnostic names the scope, and
            // recording a placement for a structure that does not exist
            // would leave the lockfile pointing at nothing.
            continue;
        };
        // `ba.source_scope` now owns the IR key — read it back so the two
        // map inserts share that one allocation as their canonical key
        // (one extra clone for `placements`, one move into `structures`).
        let dims = ba.dims;
        // First-write-wins, as above. Two `site` blocks of one name put
        // their `place id=` rows into one `site::NAME::` namespace, so
        // only a repeated `id=` collides — and the resolver has already
        // bound the first of those. `placements` and `structures` share
        // the key here, and the lockfile reads a placement's dims beside
        // the structure it names, so the two must agree on which body
        // won.
        placements
            .entry(ba.source_scope.clone())
            .or_insert(Placement {
                site: placement_site,
                place_id: placement_id,
                source_def: use_name.to_owned(),
                theme: theme_name.to_owned(),
                origin,
                dims,
            });
        structures.entry(ba.source_scope.clone()).or_insert(ba);
    }
}

/// What kind of body the lowering is processing. Lets diagnostic messages
/// distinguish a sizeless struct from a sizeless def without adding two
/// near-identical helpers, and lets a future fixtures pass switch on the
/// host when origin conventions differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyKind {
    Struct,
    Place,
}

/// Inputs shared by the struct and place lowering paths.
struct BodyDescriptor<'a> {
    kind: BodyKind,
    /// Display name for diagnostics (`cottage`, `home1`).
    scope_label: &'a str,
    size: &'a Size,
    members: &'a [Member],
    /// Span the `W_NO_THEME_BOUND` warning anchors at (struct/def header
    /// for a struct, `place` line for a place).
    header_span: &'a Span,
    /// IR key written into the resulting [`BlockArray::source_scope`].
    source_scope: String,
}

/// Lower one struct or place body into voxels.
///
/// `None` means the extent the body asks for is past
/// [`MAX_STRUCTURE_VOLUME`]; the diagnostic has already been pushed and the
/// caller drops the scope.
fn lower_body_to_block_array<'a>(
    body: BodyDescriptor<'a>,
    scope: Option<&'a ScopeResolution>,
    materials: Option<&'a dyn AbstractMaterialResolver>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<BlockArray> {
    let interior_w = body.size.w.get();
    let interior_h = body.size.h.get();

    let theme_missing = scope.is_none_or(|sc| sc.bound_theme.is_none());
    if theme_missing {
        diagnostics.push(diag_no_theme_bound_generic(
            body.kind,
            body.scope_label,
            body.header_span,
        ));
    }

    // Inflate the struct's footprint by the maximum `overhang=` across all
    // roof members so the roof's eaves and gable-end overhangs have voxel
    // room outside the wall ring. Floors, walls, doors, and windows are
    // authored against the *interior* size and shifted inward by this
    // amount in their respective fill helpers.
    let overhang = max_roof_overhang(body.members, diagnostics);
    // Level blocks contribute their own walls to the struct's tallest wall
    // voxel and their own roles to every phase. Flatten them once here so
    // the dim math (which needs the true wall-top including level walls)
    // and the phase buckets (which need the per-member y-offset) both work
    // off the same flattened view. `flatten_members` also emits the
    // `W_DEFERRED_MEMBER` diagnostics for malformed level blocks, so its
    // side effects need to happen exactly once per call.
    let flattened = flatten_members(body.members, diagnostics);
    let max_wall_top = max_wall_top(&flattened);
    let roof_extra = max_roof_extra_height(body.members, interior_w, interior_h, overhang);

    let dims = Dims {
        x: interior_w.saturating_add(overhang.saturating_mul(2)),
        y: 1u32.saturating_add(max_wall_top).saturating_add(roof_extra),
        z: interior_h.saturating_add(overhang.saturating_mul(2)),
    };
    // Ask before allocating. Each of `size=`, `height=`, `overhang=`, and
    // `level y=` is a valid `u32` in its own right, so nothing upstream can
    // see that their product is not: `size=100000x100000` alone asks the
    // allocator for 10^10 cells.
    if !dims.fits_volume_budget() {
        diagnostics.push(diag_structure_too_large(&body, dims));
        return None;
    }
    let mut palette = Palette::new_with_air();
    let mut voxels = vec![PaletteIndex::AIR; dims.volume()];

    let ctx = StructCtx {
        scope,
        materials,
        theme_missing,
        dims,
        overhang,
        interior_w,
        interior_h,
        wall_top: max_wall_top,
    };

    // Phase ordering: collect members per phase, then process the buckets
    // in massing → envelope → openings order. Within a phase source order
    // wins (the IndexMap is filled in source order via push). Each entry
    // carries the y-offset the flatten pass derived from any enclosing
    // `level y=N` block (0 for members that sit directly under the body).
    let mut massing: Vec<(u32, &Member)> = Vec::new();
    let mut envelope: Vec<(u32, &Member)> = Vec::new();
    let mut openings: Vec<(u32, &Member)> = Vec::new();
    for &(y_offset, member) in &flattened {
        // Actuator patches (`door[id=X] opened_by=sig.Y`) are metadata
        // overlays on an already-declared physical door — they carry
        // neither `side=` nor `at=` and must not enter the openings
        // phase, or `carve_door`'s `side_of` guard would false-positive
        // "missing side=". The recogniser handles the surface shape
        // here; the wired signal graph is threaded on by the future
        // redstone lowering pipeline (spec/redstone.md §14.2).
        if is_actuator_patch(member) {
            recognize_actuator_patch(member, &flattened, diagnostics);
            continue;
        }
        match member_phase(&member.role) {
            Some(Phase::Massing) => massing.push((y_offset, member)),
            Some(Phase::Envelope) => envelope.push((y_offset, member)),
            Some(Phase::Openings) => openings.push((y_offset, member)),
            None => match &member.role {
                // `circuit region=<label> void=<N>` reserves a routing
                // region for the future `logic_synth → logic_place →
                // logic_route` passes (spec/redstone.md §14.5 / §14.8).
                // Nothing lands in the block array from this member; the
                // recognizer only checks the surface shape so a valid
                // fixture stays quiet while a malformed one still
                // surfaces a targeted `W_DEFERRED_MEMBER`.
                MemberRole::Circuit => recognize_circuit_region(member, diagnostics),
                _ => diagnostics.push(diag_deferred_member(member)),
            },
        }
    }

    for (y_offset, member) in massing {
        lower_massing_member(
            member,
            y_offset,
            &ctx,
            &mut palette,
            &mut voxels,
            diagnostics,
        );
    }
    for (y_offset, member) in envelope {
        lower_envelope_member(
            member,
            y_offset,
            &ctx,
            &mut palette,
            &mut voxels,
            diagnostics,
        );
    }
    for (y_offset, member) in openings {
        lower_opening_member(
            member,
            y_offset,
            &ctx,
            &mut palette,
            &mut voxels,
            diagnostics,
        );
    }

    Some(BlockArray {
        dims,
        palette,
        voxels,
        block_entities: Vec::new(),
        entities: Vec::new(),
        source_scope: body.source_scope,
    })
}

/// The scope asked for more voxels than [`MAX_STRUCTURE_VOLUME`] allows.
///
/// Names the extent rather than only the limit: the numbers an author wrote
/// are `size=`, `height=`, and `overhang=`, and the product is what went out
/// of range, so showing the derived extent is what connects the two.
fn diag_structure_too_large(body: &BodyDescriptor<'_>, dims: Dims) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::StructureTooLarge,
        severity: Severity::Warning,
        span: body.header_span.clone(),
        primary: format!(
            "`{}` derives a {}x{}x{} voxel extent, past the \
             {MAX_STRUCTURE_VOLUME}-voxel maximum; block-array lowering skipped it",
            body.scope_label, dims.x, dims.y, dims.z,
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "the extent is derived from `size=` plus the tallest `height=` \
                      and the largest `overhang=`; reduce whichever of those is out of scale"
                .to_owned(),
        }],
        data: None,
    }
}

/// Solve the `at=origin` / `east_of=ID gap=N` / `north_of=ID gap=N` chain
/// for one `place` line.
///
/// Returns `None` when none of the three selectors is present in a usable
/// shape (the resolver already emitted `E_INVALID_PLACE_ORIGIN`); callers
/// fall back to `(0, 0, 0)` so the per-place [`BlockArray`] still lands.
/// `east_of` advances along `+x` past the prior placement's full inflated
/// `dims.x` (overhang already baked in); `north_of` retreats along `-z`
/// per the `spec/components-editing-sites.md` §9.3 front-is-`+z`
/// convention.
fn resolve_place_origin(
    member: &Member,
    placements: &IndexMap<String, Placement>,
    site_name: &str,
) -> Option<(i32, i32, i32)> {
    if let Some(value) = member.intent_state.get("at")
        && matches!(&value.value.kind, ValueKind::Ident(s) if s == "origin")
    {
        return Some((0, 0, 0));
    }
    let gap = member
        .intent_state
        .get("gap")
        .and_then(|v| match &v.value.kind {
            ValueKind::Int(n) => i32::try_from(*n).ok(),
            _ => None,
        })
        .unwrap_or(0);
    if let Some(target) = member
        .intent_state
        .get("east_of")
        .and_then(|v| v.value.as_label_str())
        && let Some(prev) = placements.get(&place_scope_key(site_name, target))
    {
        let next_x = prev
            .origin
            .0
            .saturating_add(i32::try_from(prev.dims.x).unwrap_or(i32::MAX))
            .saturating_add(gap);
        return Some((next_x, prev.origin.1, prev.origin.2));
    }
    if let Some(target) = member
        .intent_state
        .get("north_of")
        .and_then(|v| v.value.as_label_str())
        && let Some(prev) = placements.get(&place_scope_key(site_name, target))
    {
        let next_z = prev
            .origin
            .2
            .saturating_sub(i32::try_from(prev.dims.z).unwrap_or(i32::MAX))
            .saturating_sub(gap);
        return Some((prev.origin.0, prev.origin.1, next_z));
    }
    None
}

/// Bundle of per-struct context shared by every member-lowering helper.
///
/// Carried as a struct (rather than threaded as 7 positional args) so a new
/// per-struct field (e.g. theme name for selector-binding lookups) lands as
/// one field change instead of touching every helper signature.
struct StructCtx<'a> {
    scope: Option<&'a ScopeResolution>,
    materials: Option<&'a dyn AbstractMaterialResolver>,
    theme_missing: bool,
    dims: Dims,
    overhang: u32,
    interior_w: u32,
    interior_h: u32,
    /// Highest wall voxel coordinate (= max `height=` across walls members).
    /// `0` when no walls are present.
    wall_top: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Massing,
    Envelope,
    Openings,
}

fn member_phase(role: &MemberRole) -> Option<Phase> {
    match role {
        MemberRole::Floor | MemberRole::Walls => Some(Phase::Massing),
        MemberRole::Roof | MemberRole::Stair => Some(Phase::Envelope),
        MemberRole::Door | MemberRole::Window | MemberRole::PressurePlate => Some(Phase::Openings),
        // `Level` is consumed by `flatten_members` and never reaches
        // this function; the arm is exhaustive for the enum but omits
        // `Level` on purpose so a future call site that forgets to
        // flatten first fails the compile.
        MemberRole::Circuit | MemberRole::Place | MemberRole::Connect | MemberRole::Other(_) => {
            None
        }
        MemberRole::Level => unreachable!(
            "`Level` members must be flattened before phase-bucketing (see `flatten_members`)"
        ),
    }
}

/// Flatten level blocks so their children participate in phase-bucketing.
///
/// Returns each contributing member paired with the `y_offset` derived from
/// its enclosing `level y=N` (`0` for members that sit directly under the
/// struct/def/place body). `level` blocks themselves are consumed and never
/// reach the returned list; a `level` whose `y=` is missing or non-integer
/// earns a `W_DEFERRED_MEMBER` diagnostic and its entire body is dropped.
/// Nested `level` blocks are not yet supported — an inner `level` becomes a
/// per-child `W_DEFERRED_MEMBER` so the surrounding phase-bucket loop stays
/// simple. `logic` and `assert` items inside a level body are intentionally
/// not returned here — they live on the intent IR for the resolver and
/// redstone passes to consume, unaffected by block-array lowering.
fn flatten_members<'a>(
    members: &'a [Member],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<(u32, &'a Member)> {
    let mut out: Vec<(u32, &'a Member)> = Vec::new();
    for member in members {
        if matches!(member.role, MemberRole::Level) {
            let y_offset = match nonneg_int_or_defer(member, "y", diagnostics) {
                NonNegRead::Valid(v) => v,
                NonNegRead::Absent => {
                    diagnostics.push(diag_deferred_member_reason(
                        member,
                        "level requires `y=N` (non-negative integer) to place its children",
                    ));
                    continue;
                }
                NonNegRead::Deferred => continue,
            };
            for child in &member.children.members {
                if matches!(child.role, MemberRole::Level) {
                    // Nested `level` blocks are not yet supported. Emit
                    // one warning per direct grandchild-defer so an
                    // author who wrote deep nesting sees each dropped
                    // subtree instead of a single top-level defer that
                    // hides how many members were skipped.
                    diagnostics.push(diag_deferred_member_reason(
                        child,
                        "nested `level` blocks are not yet supported; this level and every member declared under it were dropped",
                    ));
                    continue;
                }
                out.push((y_offset, child));
            }
        } else {
            out.push((0, member));
        }
    }
    out
}

fn lower_massing_member(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &member.role {
        MemberRole::Floor => {
            if y_offset > 0 {
                // A `floor` inside a `level y=N` block would drop a second
                // slab in mid-air. The current lowering has no story for
                // per-level floors (the struct's ground-plane floor is the
                // only slab that goes down), so defer explicitly instead of
                // silently painting cells at an unexpected height.
                diagnostics.push(diag_deferred_member_reason(
                    member,
                    "level-scoped `floor` is not yet supported",
                ));
                return;
            }
            let Some(idx) = palette_index_for(
                member,
                ctx.scope,
                ctx.materials,
                palette,
                diagnostics,
                ctx.theme_missing,
            ) else {
                return;
            };
            fill_floor(ctx, idx, voxels);
        }
        MemberRole::Walls => {
            let Some(height) = wall_height(member, diagnostics) else {
                return;
            };
            let Some(idx) = palette_index_for(
                member,
                ctx.scope,
                ctx.materials,
                palette,
                diagnostics,
                ctx.theme_missing,
            ) else {
                return;
            };
            fill_walls(ctx, height, y_offset, idx, voxels);
        }
        _ => unreachable!("massing phase only contains floor/walls"),
    }
}

fn lower_envelope_member(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &member.role {
        MemberRole::Roof => {
            if y_offset > 0 {
                // A `roof` inside a `level` block would paint a second cap
                // below the struct's roof plane. The current lowering
                // assumes exactly one roof per struct, so a level-scoped
                // roof defers explicitly rather than corrupt the envelope.
                diagnostics.push(diag_deferred_member_reason(
                    member,
                    "level-scoped `roof` is not yet supported",
                ));
                return;
            }
            fill_roof(member, ctx, palette, voxels, diagnostics);
        }
        MemberRole::Stair => fill_stair(member, y_offset, ctx, palette, voxels, diagnostics),
        _ => unreachable!("envelope phase only contains roof/stair"),
    }
}

fn lower_opening_member(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &member.role {
        MemberRole::Door => carve_door(member, y_offset, ctx, voxels, diagnostics),
        MemberRole::Window => fill_window(member, y_offset, ctx, palette, voxels, diagnostics),
        MemberRole::PressurePlate => {
            fill_pressure_plate(member, y_offset, ctx, palette, voxels, diagnostics);
        }
        _ => unreachable!("openings phase only contains door/window/pressure_plate"),
    }
}

/// Resolve a member's `mat_slot=` binding into a concrete [`BlockState`]
/// without touching the palette.
///
/// Returns `None` (and emits at most one diagnostic) when:
/// - the scope had no theme bound (`theme_missing` short-circuits silently;
///   the `W_NO_THEME_BOUND` warning was already emitted once per struct),
/// - the member never carried a `mat_slot=`,
/// - the resolver already flagged the slot via `E_UNRESOLVED_SLOT` (the
///   binding has `slot_value == None`),
/// - the value lowered as an abstract token and no `materials` resolver was
///   offered (a `W_ABSTRACT_TOKEN_DEFERRED` warning is emitted),
/// - the value lowered as an abstract token the offered `materials` resolver
///   does not declare (an `E_UNKNOWN_ABSTRACT_TOKEN` error is emitted with
///   the nearest declared candidate, when one exists),
/// - the value was not a token at all (`E_UNKNOWN_SLOT_TARGET` already
///   fired during resolve, so no second diagnostic here).
///
/// Split out from [`palette_index_for`] so members that hard-code their
/// material (gable roof → `spruce_stairs`) can still resolve the user's
/// `mat_slot=` to check whether it agrees with the hard-coded id and emit
/// a warning when it does not — without polluting the palette with an
/// unreferenced entry.
fn resolve_member_state(
    member: &Member,
    scope: Option<&ScopeResolution>,
    materials: Option<&dyn AbstractMaterialResolver>,
    diagnostics: &mut Vec<Diagnostic>,
    theme_missing: bool,
) -> Option<BlockState> {
    if theme_missing {
        return None;
    }
    let scope = scope?;
    let binding = scope.members.get(&member.span.start)?;
    let slot_value: &ValueWithSpan = binding.slot_value.as_ref()?;
    match resolve_block_state(slot_value, materials) {
        Ok(state) => Some(state),
        Err(MaterialDeferred::Abstract(token)) => {
            diagnostics.push(diag_abstract_token(member, &token, slot_value));
            None
        }
        Err(MaterialDeferred::UnknownAbstract { token, suggestion }) => {
            diagnostics.push(diag_unknown_abstract_token(
                member,
                &token,
                suggestion.as_deref(),
                slot_value,
            ));
            None
        }
        Err(MaterialDeferred::AlreadyDiagnosed) => {
            // INVARIANT(upstream-diagnosed): `AlreadyDiagnosed` is returned
            // only when `slot_value.value.kind` is not `Token` (see
            // `material::resolve_block_state` /
            // `TokenKind::NotAToken`). For theme slot values the
            // `check_slot_targets` pass in
            // `resolve::resolver` (`resolver.rs` around the
            // `DiagnosticCode::UnknownSlotTarget` push) emits
            // `E_UNKNOWN_SLOT_TARGET` for exactly that shape during the
            // `resolve()` invocation that produced the `scope` we read
            // above; staying silent here avoids a duplicate diagnostic.
            // A local `debug_assert` would require threading the
            // resolver's `Resolution` into every caller of
            // `resolve_member_state` (palette helpers, opening carvers,
            // …). That blast radius is intentionally avoided here, so
            // the invariant is enforced by the resolver-pass unit
            // tests around `DiagnosticCode::UnknownSlotTarget` rather
            // than a local assert.
            None
        }
    }
}

/// Resolve a member's `mat_slot=` binding and intern the resulting state.
///
/// Thin shim over [`resolve_member_state`] for callers that always want to
/// store the material in the palette (floors, walls, windows).
fn palette_index_for(
    member: &Member,
    scope: Option<&ScopeResolution>,
    materials: Option<&dyn AbstractMaterialResolver>,
    palette: &mut Palette,
    diagnostics: &mut Vec<Diagnostic>,
    theme_missing: bool,
) -> Option<PaletteIndex> {
    resolve_member_state(member, scope, materials, diagnostics, theme_missing)
        .map(|state| palette.intern(state))
}

/// Highest wall voxel Y across every walls member the flatten pass surfaced.
///
/// The struct's roof plane must sit above the tallest wall column, and a
/// level-scoped `walls id=upper height=H` inside a `level y=N` block extends
/// the wall column up to `y = N + H`. Returning the maximum over the
/// `(y_offset, height)` pairs from the flattened member list keeps the
/// dim math correct when a struct mixes struct-scoped and level-scoped
/// walls. Members without a positive `height=` contribute `0`; the
/// `W_DEFERRED_MEMBER` for that member fires later in the massing phase
/// so a hand-built sizeless `walls` still surfaces its own diagnostic.
fn max_wall_top(flattened: &[(u32, &Member)]) -> u32 {
    flattened
        .iter()
        .filter(|(_, m)| matches!(m.role, MemberRole::Walls))
        .filter_map(|(y_offset, m)| height_value(m).map(|h| y_offset.saturating_add(h)))
        .max()
        .unwrap_or(0)
}

/// Largest `overhang=` across the scope's roof members.
///
/// This is the only place `overhang=` is read, so an out-of-range value has
/// to be diagnosed here or nowhere: treating it as absent silently shrank
/// the roof back to the wall line with nothing said. Every other `key=`
/// reaches the author through [`nonneg_int_or_defer`], and this one now
/// does too.
fn max_roof_overhang(members: &[Member], diagnostics: &mut Vec<Diagnostic>) -> u32 {
    members
        .iter()
        .filter(|m| matches!(m.role, MemberRole::Roof))
        .filter_map(|m| match nonneg_int_or_defer(m, "overhang", diagnostics) {
            NonNegRead::Valid(v) => Some(v),
            NonNegRead::Absent | NonNegRead::Deferred => None,
        })
        .max()
        .unwrap_or(0)
}

/// Maximum vertical contribution from any roof member with a
/// recognisable [`RoofKind`]. Roofs without a recognised kind (missing
/// `kind=` or a kind outside the supported set) contribute `0` here;
/// their `W_DEFERRED_MEMBER` warning fires later, during the envelope
/// phase, against the actual member span. Computing the dim from the
/// inflated roof bounding box (interior + 2 * overhang on each axis)
/// keeps the math consistent with each per-kind generator.
fn max_roof_extra_height(
    members: &[Member],
    interior_w: u32,
    interior_h: u32,
    overhang: u32,
) -> u32 {
    let roof_w = interior_w.saturating_add(overhang.saturating_mul(2));
    let roof_h = interior_h.saturating_add(overhang.saturating_mul(2));
    members
        .iter()
        .filter(|m| matches!(m.role, MemberRole::Roof))
        .filter_map(|m| roof_kind_of(m).map(|k| roof_extra_height(k, m, roof_w, roof_h)))
        .max()
        .unwrap_or(0)
}

fn roof_extra_height(kind: RoofKind, member: &Member, roof_w: u32, roof_h: u32) -> u32 {
    match kind {
        RoofKind::Gable => gable_extra_height(roof_w.min(roof_h)),
        RoofKind::Shed => {
            // Shed's slope axis depends on `slope_to=`. We do not have
            // diagnostics here (the dim pass runs before envelope-phase
            // diagnostics), so an unrecognised or missing `slope_to=`
            // contributes `0`; the same member will surface a
            // `W_DEFERRED_MEMBER` in `fill_roof_shed` and lower to no
            // voxels, keeping the dim math conservative. The axis
            // choice goes through `shed_slope_span` — the same helper
            // `shed_voxels` uses — so the dim and the generator cannot
            // disagree on which axis the slope runs along.
            match ident_value(member, "slope_to").and_then(WallSide::from_ident) {
                Some(slope_to) => shed_extra_height(shed_slope_span(roof_w, roof_h, slope_to)),
                None => 0,
            }
        }
        RoofKind::Hip => hip_extra_height(roof_w, roof_h),
        RoofKind::Flat => flat_extra_height(),
    }
}

fn roof_kind_of(member: &Member) -> Option<RoofKind> {
    let raw = member.intent_state.get("kind")?;
    let ValueKind::Ident(name) = &raw.value.kind else {
        return None;
    };
    RoofKind::from_ident(name)
}

fn wall_height(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<u32> {
    match height_value(member) {
        Some(h) if h >= 1 => Some(h),
        _ => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                "walls without a positive `height=` cannot voxelise",
            ));
            None
        }
    }
}

/// `None` covers "absent", "not a positive integer", and "past `u32`"
/// alike, which is what [`wall_height`] turns into one `W_DEFERRED_MEMBER`.
///
/// Saturating instead would put a wall top at `u32::MAX` because the author
/// asked for `2^33` — the outcome [`nonneg_int`] documents as the reason it
/// refuses rather than clamps.
fn height_value(member: &Member) -> Option<u32> {
    let raw = member.intent_state.get("height")?;
    match &raw.value.kind {
        ValueKind::Int(v) if *v > 0 => u32::try_from(*v).ok(),
        _ => None,
    }
}

/// Read `key=` as a non-negative `u32`.
///
/// Thin wrapper over [`Member::nonneg_u32`] so this file keeps its
/// local vocabulary; the rule itself lives on the member because
/// `check::nesting` decides whether a `level` has a usable offset and
/// has to agree with where the children are placed.
fn nonneg_int(member: &Member, key: &str) -> Option<u32> {
    member.nonneg_u32(key)
}

/// Result of reading a non-negative integer `key=` with defer semantics.
///
/// The block-array pass distinguishes three outcomes on a `key=`:
/// - `Valid(v)`: the key is present and parsed to a `u32`.
/// - `Absent`: the key was not written; the caller applies its own
///   default.
/// - `Deferred`: the key was present but did not parse to a
///   non-negative `u32`. A `W_DEFERRED_MEMBER` has already been pushed
///   and the caller must return.
///
/// Using a named tri-state keeps callers explicit about which case they
/// treat as a default vs which case aborts, and closes the
/// `y="top"`-silently-becomes-`0` gap that the plain [`nonneg_int`]
/// return type could not.
enum NonNegRead {
    Valid(u32),
    Absent,
    Deferred,
}

fn nonneg_int_or_defer(
    member: &Member,
    key: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> NonNegRead {
    if !member.intent_state.contains_key(key) {
        return NonNegRead::Absent;
    }
    if let Some(v) = nonneg_int(member, key) {
        NonNegRead::Valid(v)
    } else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!("`{key}=` must be a non-negative integer that fits in u32"),
        ));
        NonNegRead::Deferred
    }
}

fn ident_value<'a>(member: &'a Member, key: &str) -> Option<&'a str> {
    let raw = member.intent_state.get(key)?;
    match &raw.value.kind {
        ValueKind::Ident(name) => Some(name.as_str()),
        _ => None,
    }
}

fn bool_value(member: &Member, key: &str) -> Option<bool> {
    let raw = member.intent_state.get(key)?;
    match &raw.value.kind {
        ValueKind::Bool(b) => Some(*b),
        _ => None,
    }
}

fn size_value(member: &Member, key: &str) -> Option<(u32, u32)> {
    let raw = member.intent_state.get(key)?;
    match &raw.value.kind {
        ValueKind::Size { w, h } => Some((w.get(), h.get())),
        _ => None,
    }
}

fn fill_floor(ctx: &StructCtx<'_>, idx: PaletteIndex, voxels: &mut [PaletteIndex]) {
    let y = 0;
    for z_local in 0..ctx.interior_h {
        for x_local in 0..ctx.interior_w {
            let x = ctx.overhang + x_local;
            let z = ctx.overhang + z_local;
            if let Some(i) = ctx.dims.index(x, y, z) {
                voxels[i] = idx;
            }
        }
    }
}

fn fill_walls(
    ctx: &StructCtx<'_>,
    height: u32,
    y_offset: u32,
    idx: PaletteIndex,
    voxels: &mut [PaletteIndex],
) {
    // Walls fill y = y_offset+1 .. y_offset+height. Struct-scoped walls run
    // at y_offset=0 (the historical `1..=height` range). A `walls` inside a
    // `level y=N` block starts one voxel above the level's base plane, so
    // the range shifts up by N. The upper bound is capped at the volume's
    // Y extent so a stray out-of-range `height=` cannot panic — `dims.y`
    // already covers `max_wall_top + roof_extra + 1`, so under normal
    // lowering this never trims; the min is defensive against a hand-built
    // `BlockArray`.
    let start = 1u32.saturating_add(y_offset);
    let end = height
        .saturating_add(y_offset)
        .min(ctx.dims.y.saturating_sub(1));
    for y in start..=end {
        for z_local in 0..ctx.interior_h {
            for x_local in 0..ctx.interior_w {
                let on_edge = x_local == 0
                    || x_local + 1 == ctx.interior_w
                    || z_local == 0
                    || z_local + 1 == ctx.interior_h;
                if !on_edge {
                    continue;
                }
                let x = ctx.overhang + x_local;
                let z = ctx.overhang + z_local;
                if let Some(i) = ctx.dims.index(x, y, z) {
                    voxels[i] = idx;
                }
            }
        }
    }
}

fn fill_roof(
    member: &Member,
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(kind) = parse_roof_kind(member, diagnostics) else {
        return;
    };
    // Resolve the `mat_slot=` binding and use its id as the block id for
    // every voxel this roof paints. A missing `mat_slot=` falls back to
    // the kind's canonical hardcoded id (spruce_stairs / spruce_planks)
    // so a mat_slot-less roof keeps the pre-2027.1 behaviour. When the
    // author *did* write `mat_slot=` but resolution returned nothing,
    // defer — either the theme is missing (W_NO_THEME_BOUND already
    // fired against the struct) or the resolver emitted its own
    // diagnostic (E_UNRESOLVED_SLOT, E_UNKNOWN_ABSTRACT_TOKEN, …). The
    // extra `W_DEFERRED_MEMBER` here anchors the failure to *this*
    // roof so the author sees which member wound up painted with the
    // fallback species, in case they missed the resolver's own hit. A
    // resolved state with non-empty `properties` also defers: the
    // theme asked for a specific facing / half / shape and the
    // geometry generator has its own.
    let resolved = resolve_member_state(
        member,
        ctx.scope,
        ctx.materials,
        diagnostics,
        ctx.theme_missing,
    );
    if member.mat_slot.is_some() && resolved.is_none() {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "`{}` roof's `mat_slot=` did not resolve to a block id; the roof falls back to `{}`",
                kind.name(),
                kind.base_block_id(),
            ),
        ));
    }
    if let Some(state) = &resolved
        && !state.properties.is_empty()
    {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "`{}` roofs derive their stair `facing` / `half` / `shape` from the geometry; the `mat_slot=` binding to `{}[...]` also carried properties and was not applied verbatim",
                kind.name(),
                state.id,
            ),
        ));
    }
    let base_id = resolved
        .as_ref()
        .map_or(kind.base_block_id(), |s| s.id.as_str());

    match kind {
        RoofKind::Gable => fill_roof_gable(ctx, palette, voxels, base_id),
        RoofKind::Shed => fill_roof_shed(member, ctx, palette, voxels, diagnostics, base_id),
        RoofKind::Hip => fill_roof_hip(ctx, palette, voxels, base_id),
        RoofKind::Flat => fill_roof_flat(ctx, palette, voxels, base_id),
    }
}

fn fill_roof_gable(
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    base_id: &str,
) {
    let roof_w = ctx.dims.x;
    let roof_h = ctx.dims.z;
    let ridge_axis = gable_ridge_axis(roof_w, roof_h);
    // Intern each face's state once so a 99-voxel cottage roof costs four
    // `palette.intern` calls instead of one per voxel. The face → palette
    // index table is a small array because [`StairFace`] has four
    // variants; iteration order pins the palette layout for the lockfile
    // hash.
    let face_table = [
        StairFace::LowSlope,
        StairFace::HighSlope,
        StairFace::ApexLow,
        StairFace::ApexHigh,
    ];
    let mut face_indices = [PaletteIndex::AIR; 4];
    for (slot, face) in face_indices.iter_mut().zip(face_table.iter().copied()) {
        let mut state = gable_stair_state(ridge_axis, face);
        base_id.clone_into(&mut state.id);
        *slot = palette.intern(state);
    }
    for GableVoxel { pos, face } in gable_voxels(roof_w, roof_h, ctx.wall_top) {
        let idx = match face {
            StairFace::LowSlope => face_indices[0],
            StairFace::HighSlope => face_indices[1],
            StairFace::ApexLow => face_indices[2],
            StairFace::ApexHigh => face_indices[3],
        };
        if let Some(i) = ctx.dims.index(pos.0, pos.1, pos.2) {
            voxels[i] = idx;
        }
    }
}

fn fill_roof_shed(
    member: &Member,
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
    base_id: &str,
) {
    let Some(slope_to) = shed_slope_to(member, diagnostics) else {
        return;
    };
    let mut slope_state = shed_stair_state(slope_to, ShedFace::Slope);
    base_id.clone_into(&mut slope_state.id);
    let slope_idx = palette.intern(slope_state);
    let mut apex_state = shed_stair_state(slope_to, ShedFace::Apex);
    base_id.clone_into(&mut apex_state.id);
    let apex_idx = palette.intern(apex_state);
    for ShedVoxel { pos, face } in shed_voxels(ctx.dims.x, ctx.dims.z, ctx.wall_top, slope_to) {
        let idx = match face {
            ShedFace::Slope => slope_idx,
            ShedFace::Apex => apex_idx,
        };
        if let Some(i) = ctx.dims.index(pos.0, pos.1, pos.2) {
            voxels[i] = idx;
        }
    }
}

fn fill_roof_hip(
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    base_id: &str,
) {
    let roof_w = ctx.dims.x;
    let roof_h = ctx.dims.z;
    // Hip and gable share the same long-axis-wins-with-x-tiebreak ridge
    // rule (`spec/compilation.md` §4.5 falls through to §4.3). Reusing
    // `gable_ridge_axis` keeps the two paths from drifting if the
    // tiebreak rule ever changes.
    let ridge_axis = gable_ridge_axis(roof_w, roof_h);
    // Intern per voxel: `palette.intern` dedupes, so each face's state
    // lands at exactly one slot, in the order [`hip_voxels`] visits the
    // face for the first time. That order is fixed by the generator's
    // layer iteration, so the palette layout is deterministic without
    // a separate face → slot table. The match-on-face indirection a
    // pre-intern table requires is what made a slot mis-mapping
    // possible the moment `HipFace` grew or reordered; folding the
    // intern call into the voxel loop closes that gap.
    for HipVoxel { pos, face } in hip_voxels(roof_w, roof_h, ctx.wall_top) {
        let mut state = hip_stair_state(ridge_axis, face);
        base_id.clone_into(&mut state.id);
        let idx = palette.intern(state);
        if let Some(i) = ctx.dims.index(pos.0, pos.1, pos.2) {
            voxels[i] = idx;
        }
    }
}

fn fill_roof_flat(
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    base_id: &str,
) {
    let mut deck_state = flat_block_state();
    base_id.clone_into(&mut deck_state.id);
    let deck_idx = palette.intern(deck_state);
    for (x, y, z) in flat_voxels(ctx.dims.x, ctx.dims.z, ctx.wall_top) {
        if let Some(i) = ctx.dims.index(x, y, z) {
            voxels[i] = deck_idx;
        }
    }
}

/// Resolve a `roof` member's `kind=` to a [`RoofKind`].
///
/// Pushes a `W_DEFERRED_MEMBER` warning and returns `None` when the
/// `kind=` is missing, typed wrong, or names a kind outside the supported
/// set. Keeping the dispatch table in [`RoofKind::from_ident`] and the
/// diagnostic phrasing here lets each side stay self-contained.
fn parse_roof_kind(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<RoofKind> {
    let Some(raw) = ident_value(member, "kind") else {
        let reason = if member.intent_state.contains_key("kind") {
            "roof `kind=` must be one of gable, shed, hip, flat"
        } else {
            "missing `kind=` (expected one of gable, shed, hip, flat)"
        };
        diagnostics.push(diag_deferred_member_reason(member, reason));
        return None;
    };
    if let Some(k) = RoofKind::from_ident(raw) {
        return Some(k);
    }
    diagnostics.push(diag_deferred_member_reason(
        member,
        &format!("unknown roof `kind={raw}` (expected one of gable, shed, hip, flat)"),
    ));
    None
}

/// Resolve a shed roof's `slope_to=` argument.
///
/// Required for `kind=shed` because the slope direction has no sensible
/// default — picking one silently would let a typo emit a roof that
/// peaks on the wrong wall. Missing or mis-typed `slope_to=` therefore
/// surfaces a `W_DEFERRED_MEMBER` warning.
fn shed_slope_to(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<WallSide> {
    let Some(raw) = ident_value(member, "slope_to") else {
        let reason = if member.intent_state.contains_key("slope_to") {
            "shed `slope_to=` must be one of front, back, left, right"
        } else {
            "shed roof requires `slope_to=` (one of front, back, left, right)"
        };
        diagnostics.push(diag_deferred_member_reason(member, reason));
        return None;
    };
    if let Some(side) = WallSide::from_ident(raw) {
        return Some(side);
    }
    diagnostics.push(diag_deferred_member_reason(
        member,
        &format!("unknown shed `slope_to={raw}` (expected one of front, back, left, right)"),
    ));
    None
}

fn carve_door(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(side) = side_of(member, diagnostics) else {
        return;
    };
    // A door needs at least one wall row to carve into. Without a positive
    // wall height there is nothing above the floor to open up; the
    // envelope phase has already written roof voxels at y=1, and carving
    // them would punch a gap into the roof.
    if ctx.wall_top < 1 {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "door requires a `walls` member with positive `height=` to carve into",
        ));
        return;
    }
    let len = wall_length(side, ctx.interior_w, ctx.interior_h);
    // Three named anchors are accepted: `center` (`len / 2`, round-down
    // on even widths — documented in spec/syntax.md §5.4), `left` (`0`,
    // the wall-local axis origin), and `right` (`len - 1`, the far
    // corner). The same vocabulary is recognised by
    // `super::walkway::door_anchor_offset` for port resolution, so the
    // openings cut and any walkway that connects to this door land at
    // the same column. Numeric offsets are reserved for a future
    // extension. `len.saturating_sub(1)` returns 0 for the degenerate
    // `len == 0` case; `wall_local_to_grid` then rejects the bounds and
    // the door defers cleanly, so no out-of-range carve sneaks through.
    let at = match ident_value(member, "at") {
        Some("center") => len / 2,
        Some("left") => 0,
        Some("right") => len.saturating_sub(1),
        Some(other) => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "door `at={other}` is not yet supported (use `at=center | left | right`)",
                ),
            ));
            return;
        }
        None => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                "door without `at=` is not yet supported (use `at=center | left | right`)",
            ));
            return;
        }
    };
    // Doors carve a 1-wide opening starting at v_local=1 (the row just
    // above the floor of whichever level this door belongs to), capped
    // at the wall column above this door so a short-wall door cannot
    // overwrite roof voxels written in the envelope phase. The cap
    // subtracts the level's `y_offset` from the struct's `wall_top` so a
    // level-scoped door never punches past its own wall column — using
    // `wall_top` directly (which now aggregates every level's walls)
    // would let a `level y=8 door` carve at world y=9, 10 when the wall
    // above only reaches y=9. Deferring instead of clamping to 0 when
    // the level sits at or above `wall_top` keeps the failure loud: the
    // author almost certainly wrote the door against a missing wall.
    // The door block itself (`oak_door`, hinge / half / facing / open)
    // is not yet placed; that landed deferred along with per-theme door
    // materials.
    let effective_top = ctx.wall_top.saturating_sub(y_offset);
    if effective_top < 1 {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "door needs at least one wall voxel above its level to carve into",
        ));
        return;
    }
    let door_height = effective_top.min(2);
    for v_local in 1..=door_height {
        let v = v_local.saturating_add(y_offset);
        let Some((x, y, z)) = wall_local_to_grid(
            side,
            at,
            v,
            ctx.overhang,
            ctx.interior_w,
            ctx.interior_h,
            ctx.dims,
        ) else {
            continue;
        };
        if let Some(i) = ctx.dims.index(x, y, z) {
            voxels[i] = PaletteIndex::AIR;
        }
    }
}

/// Voxelise a `stair` member as a horizontal band of stair blocks along one
/// wall — the eave pattern the `themed-tower` example uses to trim the
/// transition between floors.
///
/// Only the subset the example exercises is implemented: `kind=stairs`, a
/// `side=` naming one of the four cardinal walls, an optional
/// `half=top|bottom` (defaults to `top` so a plain `stair` reads as an
/// inverted eave), an optional `facing=out|in` (defaults to `out`) that
/// rotates the stair so its riser points away from the interior, and an
/// optional `shape=straight|outer_left|outer_right` (defaults to
/// `straight`). Any other `kind=` / `facing=` / `shape=` fires
/// `W_DEFERRED_MEMBER`. The row lands at `y = y_offset + (local y | 0)`
/// in the overhang column one voxel outside the wall (so it does not
/// overwrite the wall itself). Overhang has to be at least 1 for the
/// eave to sit outside the wall; without one the stair collapses onto
/// the wall row and a `W_DEFERRED_MEMBER` fires instead.
#[allow(clippy::too_many_lines)] // one linear defer-and-paint chain reads better than 6 tiny helpers
fn fill_stair(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(raw_kind) = ident_value(member, "kind") else {
        let reason = if member.intent_state.contains_key("kind") {
            "stair `kind=` must be `stairs`"
        } else {
            "stair without `kind=` is not yet supported (currently only `kind=stairs`)"
        };
        diagnostics.push(diag_deferred_member_reason(member, reason));
        return;
    };
    if raw_kind != "stairs" {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!("stair `kind={raw_kind}` is not yet supported (currently only `kind=stairs`)"),
        ));
        return;
    }
    let Some(side) = side_of(member, diagnostics) else {
        return;
    };
    let half = match ident_value(member, "half") {
        Some("top") | None => "top",
        Some("bottom") => "bottom",
        Some(other) => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!("stair `half={other}` is not yet supported (use `top` or `bottom`)"),
            ));
            return;
        }
    };
    let facing = match ident_value(member, "facing") {
        Some("out") | None => shed_high_side(side),
        Some("in") => inward_cardinal(side),
        Some(other) => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!("stair `facing={other}` is not yet supported (use `out` or `in`)"),
            ));
            return;
        }
    };
    let shape = match ident_value(member, "shape") {
        Some("straight") | None => StairShape::Straight,
        Some("outer_left") => StairShape::OuterLeft,
        Some("outer_right") => StairShape::OuterRight,
        Some(other) => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "stair `shape={other}` is not yet supported (use `straight`, `outer_left`, or `outer_right`)",
                ),
            ));
            return;
        }
    };
    if ctx.overhang == 0 {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "eave `stair` requires a roof `overhang=` of at least 1 so the band can sit outside the wall",
        ));
        return;
    }
    let y_local = match nonneg_int_or_defer(member, "y", diagnostics) {
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 0,
        NonNegRead::Deferred => return,
    };
    let y_world = y_local.saturating_add(y_offset);
    if y_world >= ctx.dims.y {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "stair y={y_world} does not fit in the struct (dims.y={})",
                ctx.dims.y,
            ),
        ));
        return;
    }
    // Resolve the `mat_slot=` binding to a base block id. A missing
    // `mat_slot=` falls back to the vanilla roof-stair id so a
    // decorative stair without a theme still lowers. When `mat_slot=`
    // was written but resolution returned nothing, defer for the same
    // reason `fill_roof` does — silent fallback to the vanilla id
    // hides that the theme did not take effect. A binding whose
    // resolved state carries `properties` fires the same defer as
    // roofs — the shape/facing/half here are geometry-derived, not
    // theme-derived.
    let resolved = resolve_member_state(
        member,
        ctx.scope,
        ctx.materials,
        diagnostics,
        ctx.theme_missing,
    );
    if member.mat_slot.is_some() && resolved.is_none() {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "eave `stair`'s `mat_slot=` did not resolve to a block id; the band falls back to `{STAIR_BASE_ID}`",
            ),
        ));
    }
    if let Some(state) = &resolved
        && !state.properties.is_empty()
    {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "eave `stair` derives its `facing` / `half` / `shape` from the member arguments; the `mat_slot=` binding to `{}[...]` also carried properties and was not applied verbatim",
                state.id,
            ),
        ));
    }
    let base_id = resolved.as_ref().map_or(STAIR_BASE_ID, |s| s.id.as_str());
    let idx = palette.intern(stair_state(base_id, facing, half, shape));
    let length = wall_length(side, ctx.interior_w, ctx.interior_h);
    for u in 0..length {
        let Some((wx, _wy, wz)) = wall_local_to_grid(
            side,
            u,
            y_world,
            ctx.overhang,
            ctx.interior_w,
            ctx.interior_h,
            ctx.dims,
        ) else {
            continue;
        };
        let (x, z) = shift_outward(side, wx, wz);
        if let Some(i) = ctx.dims.index(x, y_world, z) {
            voxels[i] = idx;
        }
    }
}

/// Opposite of the wall's outward normal — used for `facing=in`.
///
/// The `facing=out` case reuses [`shed_high_side`] because a shed roof's
/// high edge points in the same cardinal as a wall's outward normal
/// (both are "the direction the wall or slope faces the sky").
/// Duplicating the mapping in a second helper would let the two drift.
fn inward_cardinal(side: WallSide) -> Cardinal {
    match side {
        WallSide::Front => Cardinal::North,
        WallSide::Back => Cardinal::South,
        WallSide::Left => Cardinal::East,
        WallSide::Right => Cardinal::West,
    }
}

/// Shift a wall voxel's `(x, z)` by one voxel toward the wall's outward
/// normal so an eave lands in the overhang row instead of overwriting the
/// wall itself.
fn shift_outward(side: WallSide, x: u32, z: u32) -> (u32, u32) {
    match side {
        WallSide::Front => (x, z.saturating_add(1)),
        WallSide::Back => (x, z.saturating_sub(1)),
        WallSide::Left => (x.saturating_sub(1), z),
        WallSide::Right => (x.saturating_add(1), z),
    }
}

/// Shift a wall voxel's `(x, z)` by one voxel toward the interior so a
/// fixture placed with `at=inside.<side>` sits on the interior floor row
/// next to the wall rather than overwriting the wall itself.
fn shift_inward(side: WallSide, x: u32, z: u32) -> (u32, u32) {
    match side {
        WallSide::Front => (x, z.saturating_sub(1)),
        WallSide::Back => (x, z.saturating_add(1)),
        WallSide::Left => (x.saturating_add(1), z),
        WallSide::Right => (x.saturating_sub(1), z),
    }
}

/// Which side of the wall a `pressure_plate at=…` anchor sits on.
///
/// The DSL spells this as a two-segment `DotRef`:
/// - `at=<side>.outside` — plate sits on the exterior overhang column
///   adjacent to `<side>`. When the struct has no overhang the plate
///   falls back to the wall's own foundation cell (the floor voxel
///   directly beneath the wall column) so authors can still write
///   `at=front.outside` on a plain flat-roof gatehouse.
/// - `at=inside.<side>` — plate sits one voxel toward the interior from
///   the wall's own column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlateAnchor {
    Outside(WallSide),
    Inside(WallSide),
}

impl PlateAnchor {
    fn side(self) -> WallSide {
        match self {
            Self::Outside(s) | Self::Inside(s) => s,
        }
    }
}

/// Parse a `pressure_plate at=…` value into a [`PlateAnchor`].
///
/// Returns `Err(reason)` when the `at=` argument is not a two-segment
/// dotted reference of the shape the DSL accepts. The caller turns the
/// reason into a `W_DEFERRED_MEMBER` diagnostic anchored on the member.
fn plate_anchor_of(member: &Member) -> Result<PlateAnchor, String> {
    let raw = member
        .intent_state
        .get("at")
        .ok_or_else(|| "pressure_plate without `at=` is not supported (use `at=<side>.outside` or `at=inside.<side>`)".to_owned())?;
    let ValueKind::DotRef(dotref) = &raw.value.kind else {
        return Err(
            "pressure_plate `at=` must be `<side>.outside` or `inside.<side>` (two-segment dotted reference)"
                .to_owned(),
        );
    };
    let segments = dotref.segments();
    if segments.len() != 2 {
        return Err(format!(
            "pressure_plate `at={dotref}` must have exactly two segments (`<side>.outside` or `inside.<side>`)",
        ));
    }
    let (head, tail) = (segments[0].as_str(), segments[1].as_str());
    // `<side>.outside` — the head names the wall side, the tail is the
    // literal `outside`. Everything else fails through to the paired
    // `inside.<side>` shape below.
    if tail == "outside" {
        return WallSide::from_ident(head)
            .map(PlateAnchor::Outside)
            .ok_or_else(|| {
                format!(
                    "pressure_plate `at={head}.outside`: `{head}` is not one of front, back, left, right",
                )
            });
    }
    if head == "inside" {
        return WallSide::from_ident(tail)
            .map(PlateAnchor::Inside)
            .ok_or_else(|| {
                format!(
                    "pressure_plate `at=inside.{tail}`: `{tail}` is not one of front, back, left, right",
                )
            });
    }
    Err(format!(
        "pressure_plate `at={dotref}` is not a recognised anchor (use `<side>.outside` or `inside.<side>`)",
    ))
}

/// Paint a `pressure_plate` fixture onto the block array as a single
/// vanilla plate voxel.
///
/// Only the compound-anchor subset the intent IR currently exposes is
/// honoured:
/// - `at=<side>.outside` / `at=inside.<side>` compound anchors,
/// - `offset=N` and `y=N` non-negative integer offsets along the wall's
///   axis and the vertical axis (both default to 0 when absent),
/// - an optional `mat_slot=` that resolves to a bare block id (no
///   `[property=…]` state literal). Anything else emits a
///   `W_DEFERRED_MEMBER` warning so the fixture is loud about what it
///   dropped.
///
/// The `-> sig.<name>` signal binding on `member.binding` is parsed but
/// not consumed here — sensor/actuator wiring belongs to the redstone
/// lowering pass that voxelises `logic` / `assert` / `circuit` items.
/// The physical block is placed regardless, mirroring `carve_door`'s
/// handling of `mat_slot=`.
fn fill_pressure_plate(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let anchor = match plate_anchor_of(member) {
        Ok(a) => a,
        Err(reason) => {
            diagnostics.push(diag_deferred_member_reason(member, &reason));
            return;
        }
    };
    let Some((x, y_world, z)) = plate_voxel_position(member, y_offset, anchor, ctx, diagnostics)
    else {
        return;
    };
    let Some(base_id) = resolve_plate_base_id(member, ctx, diagnostics) else {
        return;
    };
    let idx = palette.intern(BlockState::bare(base_id));
    if let Some(i) = ctx.dims.index(x, y_world, z) {
        voxels[i] = idx;
    } else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "pressure_plate resolved to a voxel outside the struct's block array",
        ));
    }
}

/// Resolve a `pressure_plate` anchor + `offset=` + `y=` into the world
/// voxel `(x, y, z)` the plate should paint onto, or `None` (with a
/// diagnostic already pushed) when any of the inputs is missing / out
/// of range / lands outside the block array.
///
/// `<side>.outside` shifts one voxel toward the exterior. When the
/// shift lands outside the struct's dims *and* `y_world == 0`, it falls
/// back to the wall's own foundation cell (the floor voxel directly
/// under the wall column) so authors can still write `at=front.outside`
/// on a plain flat-roof gatehouse with no overhang. Above the floor row
/// the same fallback would overwrite the wall block that the massing
/// phase painted, so `y_world >= 1` without a usable exterior cell
/// defers instead.
///
/// `inside.<side>` always shifts one voxel inward and defers when the
/// shift saturates onto the wall itself (a 1-voxel-thin struct has no
/// interior cell adjacent to any wall).
fn plate_voxel_position(
    member: &Member,
    y_offset: u32,
    anchor: PlateAnchor,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(u32, u32, u32)> {
    let side = anchor.side();
    let offset = match nonneg_int_or_defer(member, "offset", diagnostics) {
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 0,
        NonNegRead::Deferred => return None,
    };
    let y_local = match nonneg_int_or_defer(member, "y", diagnostics) {
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 0,
        NonNegRead::Deferred => return None,
    };
    let y_world = y_local.saturating_add(y_offset);
    if y_world >= ctx.dims.y {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "pressure_plate y={y_world} does not fit in the struct (dims.y={})",
                ctx.dims.y,
            ),
        ));
        return None;
    }
    let length = wall_length(side, ctx.interior_w, ctx.interior_h);
    if offset >= length {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "pressure_plate `offset={offset}` runs past the {} wall (length {length})",
                side_name(side),
            ),
        ));
        return None;
    }
    let Some((wx, _wy, wz)) = wall_local_to_grid(
        side,
        offset,
        y_world,
        ctx.overhang,
        ctx.interior_w,
        ctx.interior_h,
        ctx.dims,
    ) else {
        // Preceding bounds checks (`y_world < dims.y`, `offset < length`)
        // already cover every rejection `wall_local_to_grid` performs
        // today. Turning the `None` into a defer keeps the guard honest
        // if that helper grows a new failure mode: a silent skip would
        // let plates disappear without a diagnostic.
        diagnostics.push(diag_deferred_member_reason(
            member,
            "pressure_plate anchor did not map onto the wall grid (internal invariant broken)",
        ));
        return None;
    };
    match anchor {
        PlateAnchor::Outside(_) => {
            let (sx, sz) = shift_outward(side, wx, wz);
            // The shift succeeds only when it produces a genuinely new
            // voxel that lives in dims. A saturating shift that lands
            // back on the wall column (Left/Back walls at coordinate 0
            // when overhang=0) is not a real exterior cell, so treat it
            // as "no exterior available" just like an out-of-dims shift.
            let shift_reached_exterior =
                (sx, sz) != (wx, wz) && ctx.dims.index(sx, y_world, sz).is_some();
            if shift_reached_exterior {
                Some((sx, y_world, sz))
            } else if y_world == 0 {
                // Foundation fallback: the wall column's y=0 cell is
                // still floor material (walls start at y=1), so
                // replacing it with a plate is honest to the anchor
                // name.
                Some((wx, y_world, wz))
            } else {
                diagnostics.push(diag_deferred_member_reason(
                    member,
                    &format!(
                        "pressure_plate `at={}.outside` at y={y_world} has no exterior voxel to sit on (the struct has no overhang; the foundation fallback only applies at y=0 so a higher plate would overwrite the wall)",
                        side_name(side),
                    ),
                ));
                None
            }
        }
        PlateAnchor::Inside(_) => {
            let (sx, sz) = shift_inward(side, wx, wz);
            if (sx, sz) == (wx, wz) {
                diagnostics.push(diag_deferred_member_reason(
                    member,
                    &format!(
                        "pressure_plate `at=inside.{}`: no interior voxel to place the fixture on",
                        side_name(side),
                    ),
                ));
                return None;
            }
            Some((sx, y_world, sz))
        }
    }
}

/// Resolve a `pressure_plate` `mat_slot=` binding into the concrete
/// block id the palette entry should carry, defaulting to
/// [`PRESSURE_PLATE_BASE_ID`] when no binding is present or the resolver
/// returned no state.
///
/// `resolve_member_state` already emits `W_ABSTRACT_TOKEN_DEFERRED` /
/// `E_UNKNOWN_ABSTRACT_TOKEN` for abstract-token failures and the
/// struct-level `W_NO_THEME_BOUND` for a missing theme, so a `None`
/// return here is already diagnosed upstream — echoing the failure
/// with another `W_DEFERRED_MEMBER` would just double up on the same
/// root cause. Falling back to `PRESSURE_PLATE_BASE_ID` keeps the
/// fixture visible in-game so authors can still read the artefact.
///
/// A resolved state with non-empty `properties` still defers *and*
/// skips the paint: the block-array IR has no handling for bracketed
/// state literals on plates, so silently reducing a `plate[...]`
/// binding to a plain plate would drop the author's intent. This is
/// stricter than `fill_stair`'s current behaviour (which defers but
/// keeps painting) — `pressure_plate` has no geometry-derived state
/// axis of its own, so the plain-plate fallback carries less signal
/// than the stair band's does.
fn resolve_plate_base_id(
    member: &Member,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    let resolved = resolve_member_state(
        member,
        ctx.scope,
        ctx.materials,
        diagnostics,
        ctx.theme_missing,
    );
    if let Some(state) = &resolved
        && !state.properties.is_empty()
    {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "pressure_plate does not honour bracketed state literals; the `mat_slot=` binding to `{}[...]` was not applied",
                state.id,
            ),
        ));
        return None;
    }
    Some(resolved.map_or_else(|| PRESSURE_PLATE_BASE_ID.to_owned(), |s| s.id))
}

/// Recognise a `circuit region=<label> void=<N>` fixture without emitting
/// voxels.
///
/// `circuit` reserves a routing region for the future
/// `logic_synth → logic_place → logic_route` passes (spec/redstone.md
/// §14.5 / §14.8). Nothing lands in the block array at this stage — the
/// physical dust / repeater / cell tiles are decided by the logic layer,
/// which is not part of block-array lowering yet. Recognising the shape
/// here (rather than defaulting to `W_DEFERRED_MEMBER`) keeps
/// `redstone-door.crn` from firing a per-source-line warning while the
/// downstream passes are still under construction, mirroring how
/// `logic` / `assert` items never reach this function at all.
///
/// The recognised region name is intentionally NOT threaded onto the
/// [`BlockArray`] today: the receiver is the future logic pipeline,
/// which walks the intent IR directly and does not consume block-array
/// side-channels. When that pipeline lands the hand-off will be a fresh
/// intent-IR walk rather than an extension of this recogniser.
///
/// The surface contract accepted today:
/// - `region=<label>` — the region name a later logic pass will look up
///   (`floor`, `basement`, …). Accepts any `Ident` or `Str` value; other
///   value kinds (integers, booleans, dotted refs, …) defer with a
///   kind-mismatch primary that names the offending kind. The
///   block-array pass does not yet validate that the label matches an
///   existing member kind on the struct — that check belongs to the
///   routing pass, which owns the catalogue of routable regions.
/// - `void=<N>` — a `u32` service-layer height greater than zero. A
///   present-but-invalid value defers via `nonneg_int_or_defer` (which
///   also catches values that overflow `u32`), and `void=0` explicitly
///   defers because reserving zero blocks of routing headroom is almost
///   always a typo (an author who wants no reserved layer just drops
///   the `circuit` line).
///
/// Malformed shapes fall back to `diag_deferred_member_reason` so the
/// author still sees a targeted warning naming the missing / invalid
/// key rather than the generic "not yet handled" message.
fn recognize_circuit_region(member: &Member, diagnostics: &mut Vec<Diagnostic>) {
    let Some(raw_region) = member.intent_state.get("region") else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "circuit requires `region=<label>` (e.g. `region=floor`, `region=basement`)",
        ));
        return;
    };
    let Some(region) = raw_region.value.as_label_str() else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "circuit `region=` must be an identifier or string label, got {}",
                raw_region.value.kind_name(),
            ),
        ));
        return;
    };
    if region.is_empty() {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "circuit `region=` must be a non-empty label",
        ));
        return;
    }
    // `NonNegRead::Deferred` already pushed its own `void=` primary via
    // `nonneg_int_or_defer` (covering both non-integer values and
    // integers that overflow `u32`), so it shares the "no extra
    // diagnostic" arm with the valid-positive case.
    match nonneg_int_or_defer(member, "void", diagnostics) {
        NonNegRead::Valid(0) => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                "circuit `void=0` reserves no service layer; use a `u32` value >= 1",
            ));
        }
        NonNegRead::Absent => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                "circuit requires `void=<N>` (a `u32` service-layer height >= 1)",
            ));
        }
        NonNegRead::Valid(_) | NonNegRead::Deferred => {}
    }
}

/// The only selector attribute an actuator patch recognises today.
const ACTUATOR_PATCH_SELECTOR_KEYS: &[&str] = &["id"];

/// The only intent-state key an actuator patch recognises today. Extending
/// this list to `lit_by` / `powered_by` / `fired_by` requires landing the
/// matching keyword in the role table first (spec/redstone.md §14.2).
const ACTUATOR_PATCH_INTENT_KEYS: &[&str] = &["opened_by"];

/// A door member is an **actuator patch** when its surface line uses the
/// selector form (`door[…] …`). The bracketed selector references an
/// already-declared physical door by `id=`; its role in block-array
/// lowering is pure metadata (an `opened_by=` signal binding for the
/// future redstone lowering pipeline, `spec/redstone.md` §14.2). Routing
/// patch lines through `carve_door` would false-positive `side_of`'s
/// "missing `side=`" guard, so `lower_body_to_block_array` peels them off
/// before the phase-bucketing match. Whether the patch also carries
/// stray `side=` / `at=` keys is checked inside the recogniser, not
/// here, so the classifier stays a one-liner and the diagnostic path
/// owns key-allowlist enforcement.
fn is_actuator_patch(member: &Member) -> bool {
    matches!(member.role, MemberRole::Door) && member.selector.is_some()
}

/// Recognise a `door[id=X] opened_by=sig.Y` actuator patch without
/// emitting voxels.
///
/// The block-array pass owns the *surface shape* of the patch — the
/// selector allowlist, the `opened_by=` presence, and the `sig.<name>`
/// signal-reference well-formedness — so a malformed patch fails loud
/// with a targeted `W_DEFERRED_MEMBER` while a well-formed one stays
/// quiet. Threading the signal binding onto the future logic pipeline
/// happens in the redstone lowering pass that consumes the intent IR
/// directly (mirroring how `recognize_circuit_region` leaves its
/// recognised region name off the block array).
///
/// The surface contract accepted today:
/// - The `[selector]` must contain an `id=<label>` — an `Ident` or
///   `Str` value. Other value kinds defer with a kind-mismatch primary.
/// - The selector's only recognised key is `id=` (see
///   [`ACTUATOR_PATCH_SELECTOR_KEYS`]). Any other selector attribute
///   defers with a primary that names the unknown key so a stray
///   `class=main` or `foo=bar` does not slip through silently.
/// - The label must match exactly one physical `door` member in the
///   same `flatten_members` view (so a door authored under `level y=N`
///   is still selectable). Absence defers with a primary that lists
///   the ids that ARE declared; ambiguity (the same `id=` declared in
///   two scopes the flattener merges) defers separately with an
///   "ambiguous" primary so the author is never silently rebinding the
///   first hit.
/// - `opened_by=` must be present. `spec/redstone.md` §14.2 also
///   defines `lit_by=` on lamps, `powered_by=` on pistons, and
///   `fired_by=` on dispensers, but those keywords are not yet in the
///   role table — door + `opened_by` is the only shape
///   `redstone-door.crn` exercises today. Any other intent-state key
///   defers with a primary that names the offending key so a future
///   `powered_by=` implementation cannot silently change the meaning
///   of existing source (see [`ACTUATOR_PATCH_INTENT_KEYS`]).
/// - The `opened_by=` value must be a two-segment `sig.<name>`
///   `DotRef`. Non-`DotRef` values defer with a "got <kind>" primary;
///   a `DotRef` whose head is not `sig` or whose segment count is not
///   2 defers with the offending path rendered verbatim.
///
/// The signal *name* on the RHS of `sig.` is intentionally not
/// validated against a namespace here — the signal graph does not
/// exist at block-array lowering time, so a `sig.does_not_exist`
/// binding surfaces later in the redstone lowering pass when the
/// graph is walked. The surface recogniser only owns the syntactic
/// shape.
///
/// `siblings` is the same flattened `(y_offset, &Member)` list the
/// phase-bucketing loop iterates over. Passed as a slice so the
/// recogniser can look up physical doors without a second walk of the
/// intent IR.
#[allow(clippy::too_many_lines)] // one linear surface-guard chain reads better than 8 tiny helpers
fn recognize_actuator_patch(
    member: &Member,
    siblings: &[(u32, &Member)],
    diagnostics: &mut Vec<Diagnostic>,
) {
    // `is_actuator_patch` guarantees `selector.is_some()` at the call
    // site. Pinning the invariant with a `debug_assert!` fails loud in
    // tests if a future refactor of the classifier drops the selector
    // check without also revisiting this recogniser; the `let-else`
    // below then keeps release builds usable rather than panicking.
    debug_assert!(
        member.selector.is_some(),
        "recognize_actuator_patch called on a member without a selector; \
         is_actuator_patch invariant broken",
    );
    let Some(selector) = member.selector.as_ref() else {
        return;
    };
    let unknown_selector_keys: Vec<&str> = selector
        .keys()
        .map(String::as_str)
        .filter(|k| !ACTUATOR_PATCH_SELECTOR_KEYS.contains(k))
        .collect();
    if !unknown_selector_keys.is_empty() {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "door actuator patch `[selector]` accepts only `id=<label>`; unknown attribute(s): {}",
                unknown_selector_keys.join(", "),
            ),
        ));
        return;
    }
    let Some(id_value) = selector.get("id") else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "door actuator patch requires an `[id=<label>]` selector naming the physical door to bind against",
        ));
        return;
    };
    let Some(id_label) = id_value.value.as_label_str() else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "door actuator patch `[id=]` selector must be an identifier or string label, got {}",
                id_value.value.kind_name(),
            ),
        ));
        return;
    };
    // Walk the flattened view to gather every physical door's id along
    // with an occurrence count. A physical door is `MemberRole::Door`
    // with no selector of its own — a selector-bearing door would be
    // another patch, not a target. Source order is preserved for the
    // "known door ids" listing so the rendering is stable across runs.
    // The occurrence count catches the ambiguous shape a top-level
    // `door id=X` plus a `level y=N door id=X` produces after
    // flattening — `duplicate` runs per-scope and does not flag it, so
    // a silent "first hit wins" would let the patch bind to whichever
    // door happened to sort first.
    let mut physical_door_ids: Vec<(&str, u32)> = Vec::new();
    for (_, m) in siblings {
        if !matches!(m.role, MemberRole::Door) || m.selector.is_some() {
            continue;
        }
        let Some(door_id) = m.id.as_deref() else {
            continue;
        };
        if let Some((_, count)) = physical_door_ids.iter_mut().find(|(id, _)| *id == door_id) {
            *count = count.saturating_add(1);
        } else {
            physical_door_ids.push((door_id, 1));
        }
    }
    let selected = physical_door_ids
        .iter()
        .find(|(id, _)| *id == id_label)
        .copied();
    match selected {
        Some((_, count)) if count >= 2 => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "door actuator patch selects `id={id_label}` but the same id is declared on {count} physical doors in this scope; disambiguate the target before binding an actuator signal",
                ),
            ));
            return;
        }
        Some(_) => {}
        None => {
            let known_list = if physical_door_ids.is_empty() {
                "no physical door members are declared in this scope".to_owned()
            } else {
                let ids: Vec<&str> = physical_door_ids.iter().map(|(id, _)| *id).collect();
                format!("known door ids: {}", ids.join(", "))
            };
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "door actuator patch selects `id={id_label}` but no physical door with that id exists ({known_list})",
                ),
            ));
            return;
        }
    }
    let unknown_intent_keys: Vec<&str> = member
        .intent_state
        .keys()
        .map(String::as_str)
        .filter(|k| !ACTUATOR_PATCH_INTENT_KEYS.contains(k))
        .collect();
    if !unknown_intent_keys.is_empty() {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "door actuator patch accepts only `opened_by=sig.<name>` today; unknown attribute(s): {} (spec/redstone.md §14.2 reserves `lit_by=` / `powered_by=` / `fired_by=` for future keywords)",
                unknown_intent_keys.join(", "),
            ),
        ));
        return;
    }
    let Some(opened_by) = member.intent_state.get("opened_by") else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "door actuator patch requires an `opened_by=sig.<name>` binding (only `opened_by=` is recognised on doors today)",
        ));
        return;
    };
    match &opened_by.value.kind {
        ValueKind::DotRef(dotref) if dotref.head() == "sig" && dotref.tail().len() == 1 => {}
        ValueKind::DotRef(dotref) if dotref.head() == "sig" => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "door actuator patch `opened_by=` must be a two-segment signal reference `sig.<name>`, got `{dotref}`",
                ),
            ));
        }
        ValueKind::DotRef(dotref) => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "door actuator patch `opened_by=` must be a signal reference `sig.<name>` (head must be `sig`), got `{dotref}`",
                ),
            ));
        }
        _ => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "door actuator patch `opened_by=` must be a signal reference `sig.<name>`, got {}",
                    opened_by.value.kind_name(),
                ),
            ));
        }
    }
}

#[allow(clippy::too_many_lines)] // one linear parse-and-paint chain reads better than 6 tiny helpers
fn fill_window(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut Palette,
    voxels: &mut [PaletteIndex],
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(side) = side_of(member, diagnostics) else {
        return;
    };
    // `offset=` defaults to 0 (the wall-local axis origin) when absent, so a
    // decorative repeat=N series can be authored as `window ... repeat=N
    // step=M size=WxH` without a redundant `offset=0`. A key that is
    // present but not a non-negative integer still defers — validation is
    // stricter than "missing" and matches how `repeat=` and `step=` treat
    // the same shape below.
    let offset = match nonneg_int_or_defer(member, "offset", diagnostics) {
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 0,
        NonNegRead::Deferred => return,
    };
    let Some(y_start_local) = nonneg_int(member, "y") else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "window without `y=` is not yet supported",
        ));
        return;
    };
    let y_start = y_start_local.saturating_add(y_offset);
    let Some((sw, sh)) = size_value(member, "size") else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "window without `size=WxH` is not yet supported",
        ));
        return;
    };
    let sym = bool_value(member, "sym").unwrap_or(false);
    // `repeat=` stamps the same rectangle multiple times along the wall,
    // separated by `step=` voxels. Both keys are optional: an absent
    // `repeat` collapses to a single instance (the pre-repeat
    // behaviour). Present-but-invalid keys defer via
    // `nonneg_int_or_defer` so a typo like `repeat=abc` earns a
    // diagnostic instead of silently rounding to 1. `repeat=0` also
    // defers because "stamp zero times" is almost always a bug (an
    // author who wants no window just deletes the line). `step=0
    // repeat>=2` would stamp on top of itself, which is caught below.
    // The `shape=` key (used by `class=arrow_slit` as `shape=slit`) is
    // read only for source-level acceptance — the block-array pass
    // doesn't alter the palette based on it yet, so it neither defers
    // nor changes the voxel output. The slit look-and-feel is a
    // follow-up.
    let repeat = match nonneg_int_or_defer(member, "repeat", diagnostics) {
        NonNegRead::Valid(0) => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                "window `repeat=0` would stamp no instances; drop the window instead",
            ));
            return;
        }
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 1,
        NonNegRead::Deferred => return,
    };
    let step = match nonneg_int_or_defer(member, "step", diagnostics) {
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 0,
        NonNegRead::Deferred => return,
    };
    if repeat > 1 && sym {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "window with both `repeat=` and `sym=true` is not yet supported",
        ));
        return;
    }
    if repeat > 1 && step == 0 {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "window `repeat=` requires a positive `step=` so instances do not overlap",
        ));
        return;
    }
    // A window without a `mat_slot=` is an *opening* rather than a fill:
    // the rectangle is carved to air, giving the `class=arrow_slit`
    // pattern themed-tower uses a way to punch narrow slits through a
    // stone wall without also picking a decorative species. Windows with
    // an explicit `mat_slot=` continue to resolve through the palette;
    // resolution failure still short-circuits so the resolver's own
    // diagnostic isn't shadowed here.
    let idx = if member.mat_slot.is_some() {
        let Some(idx) = palette_index_for(
            member,
            ctx.scope,
            ctx.materials,
            palette,
            diagnostics,
            ctx.theme_missing,
        ) else {
            return;
        };
        idx
    } else {
        PaletteIndex::AIR
    };

    let len = wall_length(side, ctx.interior_w, ctx.interior_h);
    let span_end = offset
        .saturating_add(step.saturating_mul(repeat.saturating_sub(1)))
        .saturating_add(sw);
    if span_end > len {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "window extends beyond the `{}` wall (offset={offset} size={sw}x{sh} repeat={repeat} step={step}, wall length={len})",
                side_name(side),
            ),
        ));
        return;
    }
    // Windows have to fit *inside the wall column*, not just inside the
    // struct's inflated volume. Gating on `dims.y` alone would let a
    // mat_slot-less `class=arrow_slit` window carve air past the wall
    // top and punch a hole through roof voxels the envelope phase
    // wrote at `y = wall_top + 1` and above. Cap at `wall_top` (the
    // highest wall voxel) — inclusive, because the top wall row is a
    // legal window cell.
    let wall_ceiling = ctx.wall_top;
    if y_start.saturating_add(sh) > wall_ceiling.saturating_add(1) {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "window extends above the wall column (y={y_start} size={sw}x{sh}, wall_top={wall_ceiling})",
            ),
        ));
        return;
    }
    let base_rect = WindowRect {
        side,
        offset,
        y_start,
        width: sw,
        height: sh,
        palette_index: idx,
    };
    for i in 0..repeat {
        let stamped_offset = offset.saturating_add(step.saturating_mul(i));
        paint_window_rect(
            ctx,
            WindowRect {
                offset: stamped_offset,
                ..base_rect
            },
            voxels,
        );
    }
    if sym {
        let mirror_offset = len.saturating_sub(offset).saturating_sub(sw);
        if mirror_offset == offset {
            // The mirror sits exactly on top of the primary; emitting it
            // again would be a no-op so we silently coalesce.
            return;
        }
        // Reject overlapping mirrors: a `sym=true` window asks for a
        // *pair*, not one wide span. If the two rectangles intersect the
        // user almost certainly wrote a window that is more than half as
        // wide as the wall — diagnose and skip the mirror so the primary
        // is still emitted cleanly.
        let primary_end = offset.saturating_add(sw);
        let mirror_end = mirror_offset.saturating_add(sw);
        let overlap = offset < mirror_end && mirror_offset < primary_end;
        if overlap {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "`sym=true` window at offset={offset} size={sw}x{sh} on the `{}` wall would overlap its mirror (wall length={len}); the mirror was skipped",
                    side_name(side),
                ),
            ));
            return;
        }
        paint_window_rect(
            ctx,
            WindowRect {
                offset: mirror_offset,
                ..base_rect
            },
            voxels,
        );
    }
}

#[derive(Debug, Clone, Copy)]
struct WindowRect {
    side: WallSide,
    offset: u32,
    y_start: u32,
    width: u32,
    height: u32,
    palette_index: PaletteIndex,
}

fn paint_window_rect(ctx: &StructCtx<'_>, rect: WindowRect, voxels: &mut [PaletteIndex]) {
    for du in 0..rect.width {
        for dv in 0..rect.height {
            let Some((x, y, z)) = wall_local_to_grid(
                rect.side,
                rect.offset + du,
                rect.y_start + dv,
                ctx.overhang,
                ctx.interior_w,
                ctx.interior_h,
                ctx.dims,
            ) else {
                continue;
            };
            if let Some(i) = ctx.dims.index(x, y, z) {
                voxels[i] = rect.palette_index;
            }
        }
    }
}

fn side_of(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<WallSide> {
    let Some(raw) = ident_value(member, "side") else {
        // Distinguish "missing entirely" (no `side=` key) from "wrong
        // type" (`side=` present but its value is not an identifier). A
        // silent return on the missing case would let a `door at=center`
        // line lower to nothing without telling the author, which breaks
        // the module-level promise that every dropped member surfaces a
        // diagnostic.
        let reason = if member.intent_state.contains_key("side") {
            "`side=` must be one of front, back, left, right"
        } else {
            "missing `side=` (expected one of front, back, left, right)"
        };
        diagnostics.push(diag_deferred_member_reason(member, reason));
        return None;
    };
    if let Some(side) = WallSide::from_ident(raw) {
        return Some(side);
    }
    diagnostics.push(diag_deferred_member_reason(
        member,
        &format!("unknown `side={raw}` (expected one of front, back, left, right)"),
    ));
    None
}

fn side_name(side: WallSide) -> &'static str {
    match side {
        WallSide::Front => "front",
        WallSide::Back => "back",
        WallSide::Left => "left",
        WallSide::Right => "right",
    }
}

fn diag_struct_no_size(s: &StructIr) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::StructNoSize,
        severity: Severity::Warning,
        span: s.span.clone(),
        primary: format!(
            "struct `{}` has no `size=WxH`; block-array lowering skipped it",
            s.name,
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "add a `size=WxH` header to give the struct a voxel footprint".to_owned(),
        }],
        data: None,
    }
}

fn diag_no_theme_bound_generic(kind: BodyKind, label: &str, header_span: &Span) -> Diagnostic {
    let host = match kind {
        BodyKind::Struct => "struct",
        BodyKind::Place => "place",
    };
    Diagnostic {
        code: DiagnosticCode::NoThemeBound,
        severity: Severity::Warning,
        span: header_span.clone(),
        primary: format!(
            "{host} `{label}` has no theme bound; every `mat_slot=` will lower to air",
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "declare exactly one `theme NAME:` in the module, or set `theme=` on the \
                      `place` for multi-theme files"
                .to_owned(),
        }],
        data: None,
    }
}

fn diag_def_no_size(def: &DefIr) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::DefNoSize,
        severity: Severity::Warning,
        span: def.span.clone(),
        primary: format!(
            "def `{}` has no `size=WxH`; placements that `use={}` cannot derive a voxel footprint",
            def.name, def.name,
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "add a `size=WxH` header to give the def a voxel footprint".to_owned(),
        }],
        data: None,
    }
}

fn diag_deferred_member(member: &Member) -> Diagnostic {
    let role = MemberRole::keyword(&member.role);
    diag_deferred_member_reason(
        member,
        &format!("`{role}` is not yet handled by block-array lowering"),
    )
}

fn diag_deferred_member_reason(member: &Member, reason: &str) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::DeferredMember,
        severity: Severity::Warning,
        span: member.span.clone(),
        primary: reason.to_owned(),
        notes: vec![DiagnosticNote {
            span: None,
            message: "block-array lowering currently voxelises floor, walls, door, window, \
                      roof (kind=gable|shed|hip|flat), stair (kind=stairs), pressure_plate \
                      (at=<side>.outside|inside.<side>), and level y=N grouping, and \
                      recognises circuit region=<label> void=<N> (u32, N>=1) plus \
                      door[id=<name>] opened_by=sig.<name> actuator patches; other roles \
                      will be added as their lowering rules are spec'd"
                .to_owned(),
        }],
        data: None,
    }
}

fn diag_abstract_token(member: &Member, token: &str, slot: &ValueWithSpan) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::AbstractTokenDeferred,
        severity: Severity::Warning,
        span: member_or_slot_span(member, slot),
        primary: format!(
            "abstract token `@{token}` cannot be lowered without the registry pack; the cell falls back to air",
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message:
                "use a canonical block token (e.g. `@oak_planks`) until the registry pack ships"
                    .to_owned(),
        }],
        data: None,
    }
}

fn diag_unknown_abstract_token(
    member: &Member,
    token: &str,
    suggestion: Option<&str>,
    slot: &ValueWithSpan,
) -> Diagnostic {
    let primary = format!(
        "abstract token `@{token}` is not declared by the registry pack's materials catalog",
    );
    let mut notes = Vec::with_capacity(2);
    if let Some(s) = suggestion {
        notes.push(DiagnosticNote {
            span: None,
            message: format!("did you mean `@{s}`?"),
        });
    }
    notes.push(DiagnosticNote {
        span: None,
        message: "abstract material tokens must be declared in the pack's `materials` catalog \
                  (see `spec/materials-themes.md` §7.2)"
            .to_owned(),
    });
    Diagnostic {
        code: DiagnosticCode::UnknownAbstractToken,
        severity: Severity::Error,
        span: member_or_slot_span(member, slot),
        primary,
        notes,
        data: None,
    }
}

/// Prefer the slot-binding span (which points at the `@token`) over the
/// member line so the warning underlines the exact value that could not be
/// lowered.
fn member_or_slot_span(member: &Member, slot: &ValueWithSpan) -> Span {
    if slot.span.start == 0 && slot.span.end == 0 {
        member.span.clone()
    } else {
        slot.span.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::block_array::BlockState;
    use crate::{lower, parse, resolve};

    fn lowered(source: &str) -> BlockArrayIr {
        let module = parse(source).expect("parse");
        let ir = lower(&module);
        let resolution = resolve(&ir, None);
        lower_to_block_array(&ir, &resolution, None)
    }

    fn lowered_with_resolver(
        source: &str,
        resolver: &dyn AbstractMaterialResolver,
    ) -> BlockArrayIr {
        let module = parse(source).expect("parse");
        let ir = lower(&module);
        let resolution = resolve(&ir, None);
        lower_to_block_array(&ir, &resolution, Some(resolver))
    }

    struct FakeResolver {
        entries: Vec<(&'static str, &'static str)>,
    }

    impl AbstractMaterialResolver for FakeResolver {
        fn lookup(&self, token: &str) -> Option<BlockState> {
            self.entries
                .iter()
                .find(|(t, _)| *t == token)
                .map(|(_, id)| BlockState::bare(format!("minecraft:{id}")))
        }

        fn known_tokens(&self) -> Vec<String> {
            self.entries.iter().map(|(t, _)| (*t).to_owned()).collect()
        }
    }

    fn block_id(ba: &BlockArray, x: u32, y: u32, z: u32) -> &str {
        let i = ba.dims.index(x, y, z).expect("in-range coordinate");
        let pi = ba.voxels[i];
        ba.palette.entries[usize::from(pi.0)].id.as_str()
    }

    fn deferred_count(out: &BlockArrayIr) -> usize {
        out.diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .count()
    }

    #[test]
    fn floor_only_fills_y_zero_plane() {
        let src = "theme t:\n  slot f -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").expect("structure lowered");
        assert_eq!(ba.dims, Dims { x: 3, y: 1, z: 3 });
        for z in 0..3 {
            for x in 0..3 {
                assert_eq!(block_id(ba, x, 0, z), "minecraft:cobblestone");
            }
        }
        assert!(
            out.diagnostics.is_empty(),
            "no diagnostics expected, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn walls_only_fills_outline_above_floor() {
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=3x3\n  walls mat_slot=w height=2\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims, Dims { x: 3, y: 3, z: 3 });
        // y=0 stays air everywhere — there is no floor in this struct.
        for z in 0..3 {
            for x in 0..3 {
                assert_eq!(block_id(ba, x, 0, z), BlockState::AIR_ID);
            }
        }
        // y=1 and y=2 carry the outline; the centre cell stays air.
        for y in 1..=2 {
            assert_eq!(block_id(ba, 1, y, 1), BlockState::AIR_ID, "centre at y={y}");
            for z in 0..3 {
                for x in 0..3 {
                    let on_edge = x == 0 || x == 2 || z == 0 || z == 2;
                    let expected = if on_edge {
                        "minecraft:cobblestone"
                    } else {
                        BlockState::AIR_ID
                    };
                    assert_eq!(block_id(ba, x, y, z), expected, "({x},{y},{z})");
                }
            }
        }
    }

    #[test]
    fn floor_and_walls_combine() {
        let src = "theme t:\n  slot f -> @oak_planks\n  slot w -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n  walls mat_slot=w height=2\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims, Dims { x: 3, y: 3, z: 3 });
        // Floor plane.
        for z in 0..3 {
            for x in 0..3 {
                assert_eq!(block_id(ba, x, 0, z), "minecraft:oak_planks");
            }
        }
        // Walls outline at y=1.
        assert_eq!(block_id(ba, 0, 1, 0), "minecraft:cobblestone");
        assert_eq!(block_id(ba, 1, 1, 1), BlockState::AIR_ID);
    }

    #[test]
    fn unknown_role_warns_and_skips() {
        // `stair` is in the keyword table but no phase claims it yet, so
        // it must surface as DeferredMember without touching voxels.
        let src = "theme t:\n  slot f -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n  stair side=front\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(deferred_count(&out), 1);
        for z in 0..3 {
            for x in 0..3 {
                assert_eq!(block_id(ba, x, 0, z), "minecraft:cobblestone");
            }
        }
    }

    #[test]
    fn circuit_region_recognised_without_deferred_warning() {
        // `circuit region=<label> void=<N>` is a routing marker for the
        // future logic passes; block-array lowering must accept it
        // silently without a `W_DEFERRED_MEMBER`.
        let src = "theme t:\n  slot f -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n  circuit region=floor void=2\n";
        let out = lowered(src);
        assert_eq!(
            deferred_count(&out),
            0,
            "circuit should not emit W_DEFERRED_MEMBER, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn circuit_without_region_defers() {
        let src = "theme t:\n  slot f -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n  circuit void=2\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("region="),
            "expected the primary to mention region=, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn circuit_without_void_defers() {
        let src = "theme t:\n  slot f -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n  circuit region=floor\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("void="),
            "expected the primary to mention void=, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn circuit_with_zero_void_defers() {
        let src = "theme t:\n  slot f -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n  circuit region=floor void=0\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("void=0"),
            "expected the primary to mention void=0, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn circuit_with_nonu32_void_defers() {
        // `void=` values that overflow `u32` land in
        // `NonNegRead::Deferred`; `nonneg_int_or_defer` owns the primary
        // so `recognize_circuit_region` must not also push its own —
        // exactly one diagnostic naming `void=` should fire.
        let src = "theme t:\n  slot f -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n  circuit region=floor void=99999999999\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("void="),
            "expected the primary to mention void=, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn circuit_with_empty_region_defers() {
        // Empty `region=""` is reachable through `ValueKind::Str("")`
        // and must earn its own primary distinct from "region= absent".
        let src = "theme t:\n  slot f -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n  circuit region=\"\" void=2\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("non-empty"),
            "expected the primary to say `region=` must be non-empty, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn circuit_with_non_label_region_defers() {
        // `region=42` is well-formed but the wrong kind — the recogniser
        // must distinguish "kind mismatch" from "missing key" so an
        // author sees a targeted primary that names the offending kind.
        let src = "theme t:\n  slot f -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=f\n  circuit region=42 void=2\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("identifier or string label"),
            "expected the primary to explain the region= label requirement, got {}",
            deferred[0].primary,
        );
        assert!(
            deferred[0].primary.contains("integer"),
            "expected the primary to name the offending kind, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn actuator_patch_opened_by_recognised_without_deferred_warning() {
        // A `door[id=front] opened_by=sig.open` patch line references an
        // already-declared physical door and binds its `opened_by=` signal.
        // Block-array lowering routes the patch out of the openings phase
        // so `carve_door` never sees it — no "missing side=" defer, no
        // duplicate carve.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  \
                   walls mat_slot=w height=3\n  \
                   door id=front side=front at=center\n  \
                   door[id=front] opened_by=sig.open\n";
        let out = lowered(src);
        assert_eq!(
            deferred_count(&out),
            0,
            "actuator patch should not emit W_DEFERRED_MEMBER, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn actuator_patch_unknown_selector_key_defers() {
        // A door patch line whose `[selector]` carries anything beyond
        // the whitelisted `id=` key would silently drop the extra
        // attribute — including a future actuator selector before its
        // support lands. The recogniser rejects the shape with a
        // primary that both names the offending key and reminds the
        // author which selector attribute IS accepted.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  \
                   walls mat_slot=w height=3\n  \
                   door id=front side=front at=center\n  \
                   door[id=front class=main] opened_by=sig.open\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("class"),
            "expected the primary to name the unknown selector key `class`, got {}",
            deferred[0].primary,
        );
        assert!(
            deferred[0].primary.contains("id="),
            "expected the primary to remind the author about the `id=<label>` selector shape, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn actuator_patch_empty_selector_defers() {
        // `door[]` — the parser accepts an empty selector, but there is
        // no id to bind against. This exercises the "id missing"
        // branch on its own (without also tripping the unknown-key
        // branch that `actuator_patch_unknown_selector_key_defers`
        // covers) so a regression that collapses the two arms into
        // one still fails on this test.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  \
                   walls mat_slot=w height=3\n  \
                   door id=front side=front at=center\n  \
                   door[] opened_by=sig.open\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("id="),
            "expected the primary to name the missing `id=` selector key, got {}",
            deferred[0].primary,
        );
        assert!(
            !deferred[0].primary.contains("unknown attribute"),
            "empty selector should NOT route through the unknown-key branch, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn actuator_patch_selector_id_non_label_defers() {
        // `door[id=3] opened_by=sig.open` — the selector is present but
        // its value is not a label. Mirrors circuit's kind-mismatch arm.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  \
                   walls mat_slot=w height=3\n  \
                   door id=front side=front at=center\n  \
                   door[id=3] opened_by=sig.open\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("integer"),
            "expected the primary to name the offending kind, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn actuator_patch_unknown_id_defers_with_known_ids_note() {
        // The selector `id=back` names no physical door in the same
        // struct. The recogniser must fail loud with a primary that both
        // names the unknown id and lists the ids that ARE declared, so
        // the author can spot the near-miss without scrolling back.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  \
                   walls mat_slot=w height=3\n  \
                   door id=front side=front at=center\n  \
                   door[id=back] opened_by=sig.open\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("back"),
            "expected the primary to name the unknown id `back`, got {}",
            deferred[0].primary,
        );
        assert!(
            deferred[0].primary.contains("front"),
            "expected the primary to list `front` as a known door id, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn actuator_patch_without_actuator_key_defers() {
        // A `door[id=front]` line with no `opened_by=` — currently the
        // only recognised actuator key on doors — carries no metadata to
        // record. Silent acceptance would drop the author's intent, so
        // the recogniser defers with a primary that names `opened_by`.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  \
                   walls mat_slot=w height=3\n  \
                   door id=front side=front at=center\n  \
                   door[id=front]\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("opened_by"),
            "expected the primary to name the missing `opened_by=` key, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn actuator_patch_opened_by_non_signal_defers() {
        // `opened_by=3` — the value is not a signal reference. The
        // recogniser must name the offending kind and the required
        // `sig.<name>` shape.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  \
                   walls mat_slot=w height=3\n  \
                   door id=front side=front at=center\n  \
                   door[id=front] opened_by=3\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("sig."),
            "expected the primary to name the required `sig.<name>` shape, got {}",
            deferred[0].primary,
        );
        assert!(
            deferred[0].primary.contains("integer"),
            "expected the primary to name the offending kind, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn actuator_patch_opened_by_non_sig_dotref_defers() {
        // `opened_by=foo.bar` parses as a two-segment DotRef but the
        // head is not `sig`, so it cannot be a signal reference under
        // spec/redstone.md §14.2's namespace.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  \
                   walls mat_slot=w height=3\n  \
                   door id=front side=front at=center\n  \
                   door[id=front] opened_by=foo.bar\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("sig."),
            "expected the primary to name the required `sig.<name>` shape, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn actuator_patch_selects_door_declared_inside_level() {
        // A physical door authored under `level y=0` is still selectable
        // by the top-level actuator patch — the recogniser walks the
        // flattened member list so nesting does not hide the id. A
        // `y=0` level exercises the flattener's grouping without also
        // tripping the wall-top interaction that a higher y would
        // introduce; `flatten_members` treats every level the same
        // regardless of `y=`, so the visibility guarantee generalises.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  \
                   walls mat_slot=w height=3\n  \
                   level y=0\n    \
                     door id=inner side=front at=center\n  \
                   door[id=inner] opened_by=sig.tick\n";
        let out = lowered(src);
        assert_eq!(
            deferred_count(&out),
            0,
            "actuator patch should resolve a level-nested door id, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn actuator_patch_unknown_intent_key_defers() {
        // `door[id=front] opened_by=sig.x powered_by=sig.y` — the
        // recogniser accepts only `opened_by=` today. Silently allowing
        // a `powered_by=` on doors would let a future PR that lands
        // `powered_by=` on doors silently change the meaning of source
        // that shipped meanwhile. Reject the shape now with a primary
        // that names the offending key(s) and points at
        // spec/redstone.md §14.2.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  \
                   walls mat_slot=w height=3\n  \
                   door id=front side=front at=center\n  \
                   door[id=front] opened_by=sig.open powered_by=sig.on\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("powered_by"),
            "expected the primary to name the unknown attribute `powered_by`, got {}",
            deferred[0].primary,
        );
        assert!(
            deferred[0].primary.contains("opened_by"),
            "expected the primary to remind the author which key IS recognised, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn actuator_patch_ambiguous_id_defers() {
        // A `door id=front` at the top level plus another `door id=front`
        // nested under a `level y=0` produces two physical doors with
        // the same id after `flatten_members` runs. The `duplicate`
        // check pass scopes id-uniqueness per body, so it does NOT flag
        // this shape. Silently binding the patch to whichever door
        // sorted first would drop the author's intent — the recogniser
        // must defer with an explicit "ambiguous" primary.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  \
                   walls mat_slot=w height=3\n  \
                   door id=front side=front at=center\n  \
                   level y=0\n    \
                     door id=front side=back at=center\n  \
                   door[id=front] opened_by=sig.open\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("ambiguous")
                || deferred[0].primary.contains("2 physical doors"),
            "expected the primary to flag the ambiguity, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn actuator_patch_no_physical_doors_defers() {
        // An actuator patch in a scope with no physical doors at all
        // exercises the empty-`known_list` branch of the unknown-id
        // recogniser. The primary must call that scope shape out
        // explicitly ("no physical door members are declared") rather
        // than render an empty "known door ids: " suffix that reads
        // like a truncation.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  \
                   walls mat_slot=w height=3\n  \
                   door[id=front] opened_by=sig.open\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("no physical door"),
            "expected the primary to name the empty-scope shape, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn actuator_patch_three_segment_sig_defers() {
        // `opened_by=sig.a.b` — the head is `sig` but the tail has more
        // than one segment, so this is not a signal reference under
        // spec/redstone.md §14.2. Silently accepting a
        // longer-than-expected DotRef would let a future guard that
        // degrades to `head() == "sig"` (dropping the segment-count
        // check) slip through unnoticed; pin the segment-count arm
        // explicitly.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  \
                   walls mat_slot=w height=3\n  \
                   door id=front side=front at=center\n  \
                   door[id=front] opened_by=sig.open.extra\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0].primary.contains("two-segment"),
            "expected the primary to name the two-segment requirement, got {}",
            deferred[0].primary,
        );
        assert!(
            deferred[0].primary.contains("sig.open.extra"),
            "expected the primary to render the offending path verbatim, got {}",
            deferred[0].primary,
        );
    }

    #[test]
    fn actuator_patch_does_not_repaint_the_door_voxels() {
        // The physical `door id=front side=front at=center` on the front
        // wall carves a 2-cell opening at (2, 1..=2, 4). The actuator
        // patch that follows must NOT touch those voxels — no re-carve,
        // no palette entry added. Assert both cells stay air, the wall
        // material lives at every other front-wall cell, and the palette
        // still holds exactly {air, wall}.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  \
                   walls mat_slot=w height=3\n  \
                   door id=front side=front at=center\n  \
                   door[id=front] opened_by=sig.open\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(block_id(ba, 2, 1, 4), BlockState::AIR_ID);
        assert_eq!(block_id(ba, 2, 2, 4), BlockState::AIR_ID);
        assert_eq!(block_id(ba, 0, 1, 4), "minecraft:cobblestone");
        let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec![BlockState::AIR_ID, "minecraft:cobblestone"]);
    }

    #[test]
    fn missing_theme_warns_and_air_fills() {
        let src = "struct s size=3x3\n  floor mat_slot=f\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::NoThemeBound),
        );
        for z in 0..3 {
            for x in 0..3 {
                assert_eq!(block_id(ba, x, 0, z), BlockState::AIR_ID);
            }
        }
    }

    #[test]
    fn already_diagnosed_slot_does_not_re_warn() {
        // The resolver emits E_UNRESOLVED_SLOT for `mat_slot=missing`. We
        // must NOT also emit `W_DEFERRED_MEMBER` or
        // `W_ABSTRACT_TOKEN_DEFERRED` for the same span — double diagnosis
        // would teach a user there are two unrelated problems when there
        // is one.
        let src =
            "theme t:\n  slot f -> @cobblestone\n\nstruct s size=3x3\n  floor mat_slot=missing\n";
        let out = lowered(src);
        assert!(
            !out.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::DeferredMember
                    || d.code == DiagnosticCode::AbstractTokenDeferred),
            "no follow-on diagnostics expected, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn abstract_token_lifts_through_supplied_resolver() {
        // When `lower_to_block_array` is given a resolver that knows the
        // bound abstract token, the cell must lower to the catalog's
        // canonical id instead of staying air with W_ABSTRACT_TOKEN_DEFERRED.
        let resolver = FakeResolver {
            entries: vec![
                ("floor.wood.broadleaf", "oak_planks"),
                ("wall.stone.cobble", "cobblestone"),
            ],
        };
        let src = "theme t:\n  \
                   slot f -> @floor.wood.broadleaf\n  \
                   slot w -> @wall.stone.cobble\n\n\
                   struct s size=3x3\n  \
                   floor mat_slot=f\n  \
                   walls mat_slot=w height=2\n";
        let out = lowered_with_resolver(src, &resolver);
        assert!(
            out.diagnostics
                .iter()
                .all(|d| d.code != DiagnosticCode::AbstractTokenDeferred),
            "no abstract-token deferral expected when the resolver covers every token, got {:?}",
            out.diagnostics,
        );
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(block_id(ba, 1, 0, 1), "minecraft:oak_planks");
        assert_eq!(block_id(ba, 0, 1, 0), "minecraft:cobblestone");
    }

    #[test]
    fn unknown_abstract_token_emits_e_unknown_abstract_token() {
        // When the resolver does not declare the bound token, lowering must
        // surface E_UNKNOWN_ABSTRACT_TOKEN with the nearest-declared
        // candidate as a note. Cell falls back to air.
        let resolver = FakeResolver {
            entries: vec![("floor.wood.broadleaf", "oak_planks")],
        };
        let src = "theme t:\n  \
                   slot f -> @floor.wood.broadlef\n\n\
                   struct s size=3x3\n  \
                   floor mat_slot=f\n";
        let out = lowered_with_resolver(src, &resolver);
        let diag = out
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::UnknownAbstractToken)
            .expect("expected E_UNKNOWN_ABSTRACT_TOKEN, got {:?}");
        assert_eq!(diag.severity, Severity::Error);
        assert!(
            diag.notes
                .iter()
                .any(|n| n.message.contains("floor.wood.broadleaf")),
            "expected suggestion note in {:?}",
            diag.notes,
        );
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(block_id(ba, 1, 0, 1), BlockState::AIR_ID);
    }

    #[test]
    fn struct_without_size_is_skipped_with_warning() {
        let src = "theme t:\n  slot f -> @cobblestone\n\nstruct s\n  floor mat_slot=f\n";
        let out = lowered(src);
        assert!(!out.structures.contains_key("struct::s"));
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::StructNoSize),
        );
    }

    #[test]
    fn state_literal_round_trips_through_palette() {
        // Bracketed tokens are not yet emitted by the surface parser, so
        // this exercises the palette/material path directly to lock the
        // canonical-id and property-bag contract before the state-literal
        // grammar lands.
        let mut palette = Palette::new_with_air();
        let token = ValueWithSpan::from_value(crate::ast::Value::new(
            ValueKind::Token("oak_log[axis=x]".to_owned()),
            0..16,
        ));
        let bs = resolve_block_state(&token, None).unwrap();
        let idx = palette.intern(bs);
        assert_eq!(palette.entries[usize::from(idx.0)].id, "minecraft:oak_log");
        assert_eq!(
            palette.entries[usize::from(idx.0)]
                .properties
                .get("axis")
                .map(String::as_str),
            Some("x"),
        );
    }

    // --- door / window / roof voxelisation ----------------------------------

    #[test]
    fn phase_order_independent_of_source_order() {
        // door is written BEFORE walls in source; phase ordering must still
        // run massing first, then openings, so the door's AIR carve survives
        // through the wall fill.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  door side=front at=center\n  walls mat_slot=w height=3\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        // Front wall is z = dims.z - 1 = 4. Center x = (5-1)/2 = 2. Door y=1,2.
        assert_eq!(block_id(ba, 2, 1, 4), BlockState::AIR_ID);
        assert_eq!(block_id(ba, 2, 2, 4), BlockState::AIR_ID);
        // Wall corners survived.
        assert_eq!(block_id(ba, 0, 1, 0), "minecraft:cobblestone");
    }

    #[test]
    fn roof_increases_dims_y_by_ceil_half_span() {
        // size=9x7, walls height=4, kind=gable, overhang=0.
        // roof bbox short axis = min(9, 7) = 7 → ridge_extra = ceil(7/2) = 4.
        // dims.y = 1 + 4 + 4 = 9.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=9x7\n  walls mat_slot=w height=4\n  roof kind=gable mat_slot=r\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims, Dims { x: 9, y: 9, z: 7 });
    }

    #[test]
    fn roof_overhang_extends_xz_dims_and_shifts_walls() {
        // overhang=1 → dims.x = 9+2 = 11, dims.z = 7+2 = 9.
        // Floor is the 9x7 interior placed at x∈[1, 9], z∈[1, 7].
        let src = "theme t:\n  slot f -> @oak_planks\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=9x7\n  floor mat_slot=f\n  walls mat_slot=w height=4\n  roof kind=gable mat_slot=r overhang=1\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims.x, 11);
        assert_eq!(ba.dims.z, 9);
        // Floor inside the interior, air at the overhang ring.
        assert_eq!(block_id(ba, 1, 0, 1), "minecraft:oak_planks");
        assert_eq!(block_id(ba, 9, 0, 7), "minecraft:oak_planks");
        assert_eq!(block_id(ba, 0, 0, 0), BlockState::AIR_ID);
        assert_eq!(block_id(ba, 10, 0, 8), BlockState::AIR_ID);
        // Wall corner shifted to (1, 1, 1) rather than (0, 1, 0).
        assert_eq!(block_id(ba, 1, 1, 1), "minecraft:cobblestone");
        assert_eq!(block_id(ba, 0, 1, 0), BlockState::AIR_ID);
    }

    #[test]
    fn gable_roof_places_stairs_with_facing() {
        let src = "theme t:\n  slot r -> @spruce_stairs\n\nstruct s size=9x7\n  walls mat_slot=r height=4\n  roof kind=gable mat_slot=r overhang=1\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        // Layer 0 of the roof sits at y=5. Ridge along x (long axis with
        // overhang dims.x=11, dims.z=9 → span=9 along z).
        let north_eave = block_state_at(ba, 0, 5, 0);
        assert_eq!(north_eave.id, "minecraft:spruce_stairs");
        assert_eq!(north_eave.properties.get("facing").unwrap(), "south");
        assert_eq!(north_eave.properties.get("half").unwrap(), "bottom");
        let south_eave = block_state_at(ba, 0, 5, 8);
        assert_eq!(south_eave.properties.get("facing").unwrap(), "north");
        // Apex: gable_extra_height(9) = 5 → y = 4 + 5 = 9, z = 4 (centre).
        let apex = block_state_at(ba, 0, 9, 4);
        assert_eq!(apex.properties.get("half").unwrap(), "top");
        assert_eq!(apex.properties.get("facing").unwrap(), "south");
    }

    #[test]
    fn door_carves_opening_through_front_wall() {
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=9x7\n  walls mat_slot=w height=4\n  door side=front at=center\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        // Front wall at z=6 (no overhang). Center x = (9-1)/2 = 4. y=1,2.
        assert_eq!(block_id(ba, 4, 1, 6), BlockState::AIR_ID);
        assert_eq!(block_id(ba, 4, 2, 6), BlockState::AIR_ID);
        // Surrounding wall cells still cobblestone.
        assert_eq!(block_id(ba, 3, 1, 6), "minecraft:cobblestone");
        assert_eq!(block_id(ba, 4, 3, 6), "minecraft:cobblestone");
    }

    #[test]
    fn door_at_left_carves_first_column_of_front_wall() {
        // `at=left` pins the carve column to the wall-local origin
        // (u = 0). Front wall sits at z=6 (no overhang); the door
        // should AIR x=0 at y=1,2 while the rest of the row remains
        // wall material.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=9x7\n  walls mat_slot=w height=4\n  door side=front at=left\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(block_id(ba, 0, 1, 6), BlockState::AIR_ID);
        assert_eq!(block_id(ba, 0, 2, 6), BlockState::AIR_ID);
        // Centre column stays solid (the previous default position).
        assert_eq!(block_id(ba, 4, 1, 6), "minecraft:cobblestone");
        assert_eq!(deferred_count(&out), 0);
    }

    #[test]
    fn door_at_right_carves_last_column_of_front_wall() {
        // `at=right` pins the carve column to `wall_length - 1`.
        // Front wall length = 9 → x = 8. Same vertical band as the
        // centred door.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=9x7\n  walls mat_slot=w height=4\n  door side=front at=right\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(block_id(ba, 8, 1, 6), BlockState::AIR_ID);
        assert_eq!(block_id(ba, 8, 2, 6), BlockState::AIR_ID);
        // Centre column stays solid.
        assert_eq!(block_id(ba, 4, 1, 6), "minecraft:cobblestone");
        assert_eq!(deferred_count(&out), 0);
    }

    #[test]
    fn door_at_unknown_value_defers_with_named_anchors_in_note() {
        // `at=middle` is not one of `center | left | right`. Lowering
        // must defer (no AIR carved) and the defer message must list
        // the accepted anchors so the user can self-correct.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=9x7\n  walls mat_slot=w height=4\n  door side=front at=middle\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(deferred_count(&out), 1);
        // Front wall row remains intact.
        for x in 0..9 {
            assert_eq!(block_id(ba, x, 1, 6), "minecraft:cobblestone");
        }
        let primary = &out.diagnostics[0].primary;
        assert!(
            primary.contains("at=center | left | right"),
            "expected anchor list in defer message, got {primary}",
        );
    }

    #[test]
    fn window_places_glass_with_symmetry() {
        let src = "theme t:\n  slot w -> @cobblestone\n  slot g -> @glass_pane\n\nstruct s size=9x7\n  walls mat_slot=w height=4\n  window side=front offset=2 y=2 size=2x2 sym=true mat_slot=g\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        // Front wall at z=6. Primary rectangle: x∈[2,4), y∈[2,4).
        for dx in 0..2 {
            for dy in 0..2 {
                assert_eq!(
                    block_id(ba, 2 + dx, 2 + dy, 6),
                    "minecraft:glass_pane",
                    "primary ({},{})",
                    2 + dx,
                    2 + dy,
                );
            }
        }
        // Mirror: wall length = 9, mirror_offset = 9 - 2 - 2 = 5 → x∈[5,7).
        for dx in 0..2 {
            for dy in 0..2 {
                assert_eq!(
                    block_id(ba, 5 + dx, 2 + dy, 6),
                    "minecraft:glass_pane",
                    "mirror ({},{})",
                    5 + dx,
                    2 + dy,
                );
            }
        }
    }

    #[test]
    fn window_out_of_bounds_warns_and_skips() {
        let src = "theme t:\n  slot w -> @cobblestone\n  slot g -> @glass_pane\n\nstruct s size=5x5\n  walls mat_slot=w height=4\n  window side=front offset=3 y=2 size=3x2 mat_slot=g\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        let deferred = deferred_count(&out);
        assert_eq!(deferred, 1);
        // Front wall at z=4 should retain cobblestone (no glass painted).
        for x in 0..5 {
            assert_eq!(block_id(ba, x, 2, 4), "minecraft:cobblestone");
        }
    }

    #[test]
    fn unknown_roof_kind_warns_and_skips() {
        // `pyramid` sits outside the supported gable|shed|hip|flat set,
        // so lowering must surface a deferred-member warning and emit no
        // roof voxels (dims.y stays at `1 + wall_height` because the
        // unknown kind contributes 0 to `max_roof_extra_height`).
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=4\n  roof kind=pyramid mat_slot=r\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(deferred_count(&out), 1);
        assert_eq!(ba.dims.y, 5);
    }

    #[test]
    fn shed_roof_voxelises_with_slope_to_front() {
        // size=5x5, walls height=3, shed slope_to=front, no overhang.
        // slope_span = roof_h = 5 → extra_height = 5 → dims.y = 1 + 3 + 5 = 9.
        // Slope axis is z (Front=+z). Layer 0 (y=4) sits at z=0; apex
        // (y=8) sits at z=4. Stairs facing south.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=shed slope_to=front mat_slot=r\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims.y, 9);
        let layer0 = block_state_at(ba, 0, 4, 0);
        assert_eq!(layer0.id, "minecraft:spruce_stairs");
        assert_eq!(layer0.properties.get("facing").unwrap(), "south");
        assert_eq!(layer0.properties.get("half").unwrap(), "bottom");
        let apex = block_state_at(ba, 0, 8, 4);
        assert_eq!(apex.properties.get("half").unwrap(), "top");
    }

    #[test]
    fn shed_roof_without_slope_to_emits_deferred_warning() {
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=shed mat_slot=r\n";
        let out = lowered(src);
        assert_eq!(deferred_count(&out), 1);
        let primary = &out.diagnostics[0].primary;
        assert!(
            primary.contains("slope_to"),
            "expected slope_to mention, got {primary}",
        );
    }

    #[test]
    fn hip_roof_voxelises_square_footprint() {
        // size=5x5, walls height=3. hip_extra_height = ceil(5/2) = 3.
        // dims.y = 1 + 3 + 3 = 7 (so highest valid y is 6). Apex sits at
        // y = wall_top + extra_height = 6, single cell at (2, 6, 2).
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=hip mat_slot=r\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims.y, 7);
        let apex = block_state_at(ba, 2, 6, 2);
        assert_eq!(apex.id, "minecraft:spruce_stairs");
        assert_eq!(apex.properties.get("half").unwrap(), "top");
        // North-west corner of layer 0 uses `shape=outer_left`.
        let nw_corner = block_state_at(ba, 0, 4, 0);
        assert_eq!(nw_corner.properties.get("shape").unwrap(), "outer_left");
        assert_eq!(nw_corner.properties.get("facing").unwrap(), "south");
    }

    #[test]
    fn flat_roof_voxelises_single_layer_of_planks() {
        // size=5x5, walls height=3, flat → extra_height=1, dims.y = 5.
        // Every cell at y=4 is spruce_planks.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_planks\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims.y, 5);
        for z in 0..5 {
            for x in 0..5 {
                assert_eq!(block_id(ba, x, 4, z), "minecraft:spruce_planks");
            }
        }
        assert_eq!(deferred_count(&out), 0);
    }

    #[test]
    fn flat_roof_honours_bound_mat_slot_id() {
        // A theme binding to something other than the flat kind's canonical
        // spruce_planks id now lands in the palette verbatim (per-theme
        // roof species landed alongside level lowering). No warning; the
        // deck voxel at the roof plane uses the resolved id.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r\n";
        let out = lowered(src);
        assert_eq!(
            deferred_count(&out),
            0,
            "unexpected diagnostics: {:?}",
            out.diagnostics
        );
        let ba = out.structures.get("struct::s").unwrap();
        // Flat deck sits at wall_top + 1 = 4. Interior x∈[0, 4], z∈[0, 4].
        assert_eq!(block_id(ba, 2, 4, 2), "minecraft:spruce_stairs");
    }

    fn block_state_at(ba: &BlockArray, x: u32, y: u32, z: u32) -> &BlockState {
        let i = ba.dims.index(x, y, z).expect("in-range coord");
        let pi = ba.voxels[i];
        &ba.palette.entries[usize::from(pi.0)]
    }

    // --- regression coverage for review feedback ----------------------------

    #[test]
    fn door_without_side_emits_deferred_warning() {
        // A `door at=center` line with no `side=` used to drop silently
        // because `side_of` short-circuited on the missing key. Every
        // dropped member must surface a diagnostic.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  door at=center\n";
        let out = lowered(src);
        assert_eq!(deferred_count(&out), 1);
        let primary = &out.diagnostics[0].primary;
        assert!(
            primary.contains("missing `side="),
            "expected missing-side reason, got {primary}",
        );
    }

    #[test]
    fn window_with_non_ident_side_emits_deferred_warning() {
        // `side=` present but typed wrong (here as an integer literal).
        // The `wrong type` branch in `side_of` must fire so the user
        // hears about it.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot g -> @glass_pane\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  window side=3 offset=1 y=1 size=1x1 mat_slot=g\n";
        let out = lowered(src);
        let deferred = deferred_count(&out);
        assert!(deferred >= 1, "expected a side= diagnostic, got {deferred}");
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("`side=`")),
            "expected a `side=` mention in diagnostics: {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn sym_window_overlap_skips_mirror_with_warning() {
        // wall length=6, offset=2, size=3 → mirror_offset = 6-2-3 = 1.
        // [2..5) and [1..4) overlap — the mirror would fuse with the
        // primary into one wide span. We diagnose and keep only the
        // primary so the user notices.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot g -> @glass_pane\n\nstruct s size=6x5\n  walls mat_slot=w height=4\n  window side=front offset=2 y=2 size=3x1 sym=true mat_slot=g\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("overlap")),
            "expected overlap diagnostic, got {:?}",
            out.diagnostics,
        );
        // Primary rectangle [x=2..5, y=2] painted.
        for x in 2..5 {
            assert_eq!(block_id(ba, x, 2, 4), "minecraft:glass_pane");
        }
        // Mirror cells outside the primary stay cobblestone (x=1).
        assert_eq!(block_id(ba, 1, 2, 4), "minecraft:cobblestone");
    }

    #[test]
    fn door_capped_at_wall_top_does_not_punch_through_roof() {
        // walls height=1 → wall_top=1. Door y=1..=2 would carve a hole at
        // y=2 which the roof's south-eave layer occupies. Capping at
        // wall_top keeps the roof intact.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=1\n  roof kind=gable mat_slot=r\n  door side=front at=center\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        // Door carves only y=1 of the front wall.
        assert_eq!(block_id(ba, 2, 1, 4), BlockState::AIR_ID);
        // y=2 on the front-eave row of the roof must still be stairs.
        // span = min(5,5) = 5, ridge axis = x, low slope at z=0 layer 0,
        // high slope at z=4 layer 0, y = wall_top+1 = 2.
        let south_eave = block_state_at(ba, 2, 2, 4);
        assert_eq!(south_eave.id, "minecraft:spruce_stairs");
    }

    #[test]
    fn door_without_walls_emits_deferred_warning() {
        // No walls member → wall_top=0. The door cannot carve anything
        // and must complain instead of doing nothing silently.
        let src = "theme t:\n  slot f -> @oak_planks\n\nstruct s size=5x5\n  floor mat_slot=f\n  door side=front at=center\n";
        let out = lowered(src);
        assert!(
            out.diagnostics.iter().any(|d| d.primary.contains("walls")),
            "expected walls-required diagnostic, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn at_center_picks_right_of_centre_on_even_width_walls() {
        // size=8x5 → wall length 8. `at=center` should pick column 4 (the
        // right half-block of the geometric centre), not column 3, so the
        // door is consistent with round-half-up semantics.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=8x5\n  walls mat_slot=w height=3\n  door side=front at=center\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        // Front wall z=4. y=1 air at x=4, cobblestone at x=3.
        assert_eq!(block_id(ba, 4, 1, 4), BlockState::AIR_ID);
        assert_eq!(block_id(ba, 3, 1, 4), "minecraft:cobblestone");
    }

    #[test]
    fn gable_honours_bound_mat_slot_id() {
        // A theme that binds `slot roof -> @oak_stairs` now lands
        // oak_stairs on every gable voxel instead of silently getting the
        // hardcoded spruce_stairs. No deferred warning fires because the
        // resolved id is used verbatim.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @oak_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=gable mat_slot=r\n";
        let out = lowered(src);
        assert_eq!(
            deferred_count(&out),
            0,
            "unexpected diagnostics: {:?}",
            out.diagnostics
        );
        let ba = out.structures.get("struct::s").unwrap();
        assert!(
            ba.palette
                .entries
                .iter()
                .any(|s| s.id == "minecraft:oak_stairs"),
            "expected oak_stairs in palette, got {:?}",
            ba.palette
                .entries
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
        );
    }

    #[test]
    fn gable_with_matching_mat_slot_stays_silent() {
        // The cottage case: theme binds the slot to spruce_stairs, the
        // generator emits spruce_stairs — no warning.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=gable mat_slot=r\n";
        let out = lowered(src);
        assert_eq!(
            deferred_count(&out),
            0,
            "expected silence on matching mat_slot, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn even_span_gable_apex_uses_half_top() {
        // size=8x4 → roof span (short axis) = 4 (even). The apex layer
        // must cap with two half=top rows or the ridge has an open V.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=8x4\n  walls mat_slot=w height=4\n  roof kind=gable mat_slot=r\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        // gable_extra_height(4) = 2 layers. Apex layer at y = 4+2 = 6.
        let apex_low = block_state_at(ba, 0, 6, 1);
        let apex_high = block_state_at(ba, 0, 6, 2);
        assert_eq!(apex_low.properties.get("half").unwrap(), "top");
        assert_eq!(apex_high.properties.get("half").unwrap(), "top");
    }

    // ---- site lowering: per-place IR emission and the coord solver ----

    #[test]
    fn place_lowers_def_with_referenced_theme() {
        // Cross-scope theme resolution proof: the def `cottage` has no
        // theme of its own, but `place ... theme=t` makes `t`'s slot
        // bindings flow into the place's lowering. The result lands
        // under `site::s::home1`, not `struct::cottage`.
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def cottage size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "\n",
            "site s:\n",
            "  place id=home1 use=cottage theme=t at=origin\n",
        );
        let out = lowered(src);
        let ba = out
            .structures
            .get("site::s::home1")
            .expect("place lowered under site::s::home1 key");
        assert_eq!(
            ba.dims,
            Dims { x: 3, y: 3, z: 3 },
            "place inherits the def's interior size, no overhang",
        );
        // Wall voxel at the corner should be cobblestone (theme slot
        // resolved across scopes).
        assert_eq!(block_id(ba, 0, 1, 0), "minecraft:cobblestone");

        let placement = out
            .placements
            .get("site::s::home1")
            .expect("placement record present");
        assert_eq!(placement.site, "s");
        assert_eq!(placement.place_id, "home1");
        assert_eq!(placement.source_def, "cottage");
        assert_eq!(placement.theme, "t");
        assert_eq!(placement.origin, (0, 0, 0));
        assert_eq!(placement.dims, ba.dims);
    }

    #[test]
    fn east_of_offset_sums_prior_dims_and_gap() {
        // east_of advances along +x past the prior placement's full inflated
        // dims.x (no overhang here, so just the interior 3) plus gap=2.
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def cottage size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "\n",
            "site s:\n",
            "  place id=a use=cottage theme=t at=origin\n",
            "  place id=b use=cottage theme=t east_of=a gap=2\n",
        );
        let out = lowered(src);
        let b = out
            .placements
            .get("site::s::b")
            .expect("placement b present");
        assert_eq!(
            b.origin,
            (5, 0, 0),
            "x = prev.x(0) + prev.dims.x(3) + gap(2)"
        );
        assert_eq!(b.origin.2, 0, "east_of does not move along z");
    }

    #[test]
    fn north_of_subtracts_dims_and_gap_on_z_axis() {
        // north_of retreats along -z by the prior placement's full inflated
        // dims.z plus gap. Front-is-+z (`spec/components-editing-sites.md`
        // §5.4 / §9.3) means north sits at the negative-z half-space.
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def cottage size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "\n",
            "site s:\n",
            "  place id=a use=cottage theme=t at=origin\n",
            "  place id=b use=cottage theme=t north_of=a gap=4\n",
        );
        let out = lowered(src);
        let b = out
            .placements
            .get("site::s::b")
            .expect("placement b present");
        assert_eq!(
            b.origin,
            (0, 0, -7),
            "z = prev.z(0) - prev.dims.z(3) - gap(4)"
        );
    }

    fn village_pair_source(extra_connects: &str) -> String {
        // Tiny two-place village shared by the connect-dedup tests so
        // each test only spells out the extra rows under exercise.
        format!(
            concat!(
                "theme t:\n",
                "  slot wall -> @cobblestone\n",
                "\n",
                "def cottage size=3x3:\n",
                "  walls mat_slot=wall height=2\n",
                "  door id=entry side=front at=center\n",
                "\n",
                "site s:\n",
                "  place id=a use=cottage theme=t at=origin\n",
                "  place id=b use=cottage theme=t east_of=a gap=4\n",
                "{}",
            ),
            extra_connects,
        )
    }

    #[test]
    fn duplicate_connect_emits_w_duplicate_walkway_and_lays_one_strip() {
        // The same `(a.entry, b.entry)` written twice in source order
        // must land exactly one walkway and exactly one
        // W_DUPLICATE_WALKWAY warning on the second row.
        let src = village_pair_source(
            "  connect a.entry to b.entry path=@gravel\n  connect a.entry to b.entry path=@gravel\n",
        );
        let out = lowered(&src);
        let dup_count = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DuplicateWalkway)
            .count();
        assert_eq!(dup_count, 1, "expected exactly one W_DUPLICATE_WALKWAY");
        assert_eq!(out.walkways.len(), 1, "second row must not lay a strip");
    }

    #[test]
    fn reverse_connect_dedupes_against_first_row() {
        // `a.entry → b.entry` and `b.entry → a.entry` are the same
        // walkway. The endpoint sort in `lower_connects` must collapse
        // the pair so the second row earns a duplicate warning and the
        // strip is laid once.
        let src = village_pair_source(
            "  connect a.entry to b.entry path=@gravel\n  connect b.entry to a.entry path=@gravel\n",
        );
        let out = lowered(&src);
        let dup_count = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DuplicateWalkway)
            .count();
        assert_eq!(
            dup_count, 1,
            "expected exactly one W_DUPLICATE_WALKWAY on the reversed row",
        );
        assert_eq!(
            out.walkways.len(),
            1,
            "reversed row must not lay a second strip"
        );
    }

    fn walkway_with_blocked_l_path_source() -> &'static str {
        // Two `home` placements wired back-to-back so the straight L-path
        // between their ports threads through `b`'s floor. `a` exposes a
        // `side=back` door (port sits at world z = -1), `b` exposes
        // `side=front` (port sits at world z = 3). With `east_of=a gap=2`
        // `b` lands at origin (5, 0, 0) with interior 3x3, so the L-path
        // `(1, -1) -> (6, -1) -> (6, 3)` would pass through (6, 0),
        // (6, 1), (6, 2) — the middle column of `b`'s 3×3 floor.
        // A detour through the open x∈[3,4] gap between the two floors
        // exists, so the router must find it.
        concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def back_home size=3x3:\n",
            "  floor mat_slot=wall\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=back side=back at=center\n",
            "\n",
            "def front_home size=3x3:\n",
            "  floor mat_slot=wall\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=front side=front at=center\n",
            "\n",
            "site s:\n",
            "  place id=a use=back_home theme=t at=origin\n",
            "  place id=b use=front_home theme=t east_of=a gap=2\n",
            "  connect a.back to b.front path=@gravel\n",
        )
    }

    #[test]
    fn walkway_routes_around_obstructed_l_path_without_warning() {
        // When the straight L collides with a placement floor but an
        // unobstructed detour exists, the router must lay the detour and
        // the row must not earn a `W_WALKWAY_BLOCKED` — the strip stays
        // unbroken instead of shipping with a hole through the building.
        let out = lowered(walkway_with_blocked_l_path_source());
        assert!(
            out.diagnostics
                .iter()
                .all(|d| d.code != DiagnosticCode::WalkwayBlocked),
            "detourable row must not warn, got {:?}",
            out.diagnostics,
        );
        let walkway = out
            .walkways
            .get("walkway::s::a.back__b.front")
            .expect("walkway IR present");
        // Both minimal detours (via the x=3 or x=4 column) share the
        // L-path's bounding box, so origin/footprint are pinned exactly
        // even though the tie-break picks one column.
        assert_eq!(walkway.origin, (1, 0, -1));
        assert_eq!(walkway.footprint, Footprint { x: 6, z: 5 });
        let ba = out
            .structures
            .get("walkway::s::a.back__b.front")
            .expect("walkway block array present");
        // The detour is a shortest route: Manhattan distance 9 → 10
        // cells, all gravel, none skipped.
        let gravel_count = (0..ba.dims.volume())
            .filter(|&i| ba.palette.entries[usize::from(ba.voxels[i].0)].id == "minecraft:gravel")
            .count();
        assert_eq!(gravel_count, 10, "shortest detour lays 10 gravel cells");
        // Endpoints are gravel; the cells the L would have crossed on
        // `b`'s floor edge (world (6, 0..=2) → local (5, 1..=3)) stay
        // air because the route went around them.
        assert_eq!(block_id(ba, 0, 0, 0), "minecraft:gravel");
        assert_eq!(block_id(ba, 5, 0, 4), "minecraft:gravel");
        for dz in 1..=3 {
            assert_eq!(
                block_id(ba, 5, 0, dz),
                BlockState::AIR_ID,
                "floor cell at local (5, 0, {dz}) must stay untouched",
            );
        }
    }

    #[test]
    fn walkway_detour_is_deterministic_across_lowerings() {
        // The router breaks ties by fixed expansion order, never by hash
        // iteration order — two lowerings of the same source must produce
        // the identical voxel grid (the lockfile pins walkway placement,
        // so a wobbling tie-break would break reproducible builds).
        let first = lowered(walkway_with_blocked_l_path_source());
        let second = lowered(walkway_with_blocked_l_path_source());
        assert_eq!(
            first.structures.get("walkway::s::a.back__b.front"),
            second.structures.get("walkway::s::a.back__b.front"),
        );
    }

    fn walkway_with_unroutable_port_source() -> &'static str {
        // Same back-to-back pair as
        // `walkway_with_blocked_l_path_source`, plus a third placement
        // `c` stacked directly north of `a` (`gap=0` → origin
        // (0, 0, -3)) whose floor covers `a`'s back port cell (1, -1).
        // A route cannot leave a buried port, so the row must fall back
        // to the L-path skip-and-warn lay. The L crosses `c`'s floor at
        // (1, -1) and (2, -1) and `b`'s floor at (6, 0..=2) — 5 skipped
        // cells.
        concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def back_home size=3x3:\n",
            "  floor mat_slot=wall\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=back side=back at=center\n",
            "\n",
            "def front_home size=3x3:\n",
            "  floor mat_slot=wall\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=front side=front at=center\n",
            "\n",
            "site s:\n",
            "  place id=a use=back_home theme=t at=origin\n",
            "  place id=b use=front_home theme=t east_of=a gap=2\n",
            "  place id=c use=front_home theme=t north_of=a gap=0\n",
            "  connect a.back to b.front path=@gravel\n",
        )
    }

    #[test]
    fn walkway_blocked_cells_skip_with_w_walkway_blocked_count() {
        // When no unobstructed route exists (here: `a`'s port cell is
        // buried under `c`'s floor), `lower_connects` must emit exactly
        // one `W_WALKWAY_BLOCKED` per connect row and the warning's
        // primary must name how many cells were skipped. The collision
        // count is load-bearing: a regression that swallows obstructions
        // silently would leave the lockfile claiming voxels the on-disk
        // NBT does not actually carry.
        let out = lowered(walkway_with_unroutable_port_source());
        let blocked: Vec<_> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::WalkwayBlocked)
            .collect();
        assert_eq!(
            blocked.len(),
            1,
            "expected one W_WALKWAY_BLOCKED, got {:?}",
            out.diagnostics,
        );
        assert_eq!(
            blocked[0].data,
            Some(DiagnosticData::WalkwayBlocked { skipped: 5 }),
            "expected the structured payload to report five skipped cells, got data={:?} primary={}",
            blocked[0].data,
            blocked[0].primary,
        );
        // The note must name the actual cause — `a.back` is buried
        // under `c`'s floor — not fall back to the generic
        // widen-the-gap suggestion, which cannot fix a buried port.
        let note = &blocked[0].notes[0].message;
        assert!(
            note.contains("port `a.back` is buried"),
            "note must point at the buried port, got {note}",
        );
        // AC4 from issue #40: the `primary` string is part of the gcc-style
        // text-format contract that humans (and existing pre-payload test
        // harnesses) read; the structured `data` is meant to *augment* it,
        // not replace it. Asserting both keeps a regression that drops
        // `{skipped}` from the format string — or changes the
        // `port_label` shape — from sliding past CI.
        assert!(
            blocked[0].primary.contains("skipped 5 cells"),
            "primary text contract must still name the skip count, got {}",
            blocked[0].primary,
        );
        // The render → JSON path is the contract surface for downstream
        // tooling (LSP quick-fix, CI annotator). Locking the serialised
        // shape here keeps the `cairn check --format json` payload stable
        // even though the `check` CLI does not itself drive walkway
        // lowering — any future caller that wires the JSON formatter
        // around `lower_to_block_array` inherits the same contract.
        let source = walkway_with_unroutable_port_source();
        let lines = crate::check::LineStarts::new(source);
        let rendered = blocked[0].render(source, &lines);
        let value = serde_json::to_value(&rendered).expect("rendered serialises");
        assert_eq!(
            value["data"],
            serde_json::json!({"kind": "walkway_blocked", "skipped": 5}),
            "JSON `data` payload must match the structured contract, got {value}",
        );
        // AC3 negative-case ride-along: every *other* diagnostic emitted by
        // this fixture must leave `data` unset. A regression where some
        // code starts attaching a payload it should not — say, copying
        // the WalkwayBlocked shape into an unrelated cascade — would
        // otherwise slip past CI because the positive assertion only
        // looks at the WalkwayBlocked entry.
        for d in &out.diagnostics {
            if d.code != DiagnosticCode::WalkwayBlocked {
                assert!(
                    d.data.is_none(),
                    "code {:?} unexpectedly carries a payload: {:?}",
                    d.code,
                    d.data,
                );
            }
        }
        // Walkway IR still emitted — the row survives, only the colliding
        // cells stay air. Bounding box covers x∈[1,6], z∈[-1,3].
        let walkway = out
            .walkways
            .get("walkway::s::a.back__b.front")
            .expect("walkway IR present despite collisions");
        assert_eq!(walkway.origin, (1, 0, -1));
        assert_eq!(walkway.footprint, Footprint { x: 6, z: 5 });
        let ba = out
            .structures
            .get("walkway::s::a.back__b.front")
            .expect("walkway block array present despite collisions");
        // Path corner at (6, -1) is still gravel.
        assert_eq!(block_id(ba, 5, 0, 0), "minecraft:gravel");
        // The buried port cell and its neighbour under `c`'s floor
        // (world (1, -1), (2, -1) → local (0, 0), (1, 0)) stay air.
        for dx in 0..=1 {
            assert_eq!(
                block_id(ba, dx, 0, 0),
                BlockState::AIR_ID,
                "blocked cell at local ({dx}, 0, 0) should stay air",
            );
        }
        // Cells colliding with `b`'s floor (world (6,0), (6,1), (6,2))
        // map to local (5, 1), (5, 2), (5, 3) — they must stay air.
        for dz in 1..=3 {
            assert_eq!(
                block_id(ba, 5, 0, dz),
                BlockState::AIR_ID,
                "blocked cell at local (5, 0, {dz}) should stay air",
            );
        }
        // Endpoint at b's port (world (6, 3) → local (5, 4)) remains
        // gravel — the port itself sits outside `b`'s floor.
        assert_eq!(block_id(ba, 5, 0, 4), "minecraft:gravel");
    }

    fn walkway_pair_source(path_token: &str) -> String {
        // The walkway-side abstract-token tests reuse the same two-place
        // fixture but vary the `path=` value. Floors are omitted so the
        // path never collides.
        format!(
            concat!(
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
                "  connect a.entry to b.entry path={token}\n",
            ),
            token = path_token,
        )
    }

    #[test]
    fn walkway_abstract_path_lifts_through_registry_pack() {
        // `path=@walkway.gravel` is an abstract token; with a pack that
        // declares it, lowering must emit a `gravel` walkway with no
        // deferred / unknown-token diagnostic on the row.
        let resolver = FakeResolver {
            entries: vec![("walkway.gravel", "gravel")],
        };
        let src = walkway_pair_source("@walkway.gravel");
        let out = lowered_with_resolver(&src, &resolver);
        assert!(
            out.diagnostics
                .iter()
                .all(|d| d.code != DiagnosticCode::AbstractTokenDeferred
                    && d.code != DiagnosticCode::UnknownAbstractToken),
            "no abstract-token diagnostics expected, got {:?}",
            out.diagnostics,
        );
        let walkway = out
            .walkways
            .get("walkway::s::a.entry__b.entry")
            .expect("walkway lowered");
        assert_eq!(walkway.path_material, "minecraft:gravel");
        // Pin origin/dims so a coordinate swap in the abstract-token
        // lift path fails loud independently of the concrete-token
        // village test. a's front port is at (1, 0, 3); b east_of=a
        // gap=2 with cottage dims 3×3 → origin (5, 0, 0), front port
        // (6, 0, 3). The L-path collapses to a pure x-axis run.
        assert_eq!(walkway.origin, (1, 0, 3));
        assert_eq!(walkway.footprint, Footprint { x: 6, z: 1 });
    }

    #[test]
    fn walkway_abstract_path_without_pack_emits_w_abstract_token_deferred() {
        // No resolver supplied → the connect row earns
        // `W_ABSTRACT_TOKEN_DEFERRED` and is dropped from the walkway map
        // so the lockfile does not pin a strip that has no material.
        let src = walkway_pair_source("@walkway.gravel");
        let out = lowered(&src);
        let diag = out
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::AbstractTokenDeferred)
            .unwrap_or_else(|| {
                panic!(
                    "expected W_ABSTRACT_TOKEN_DEFERRED on the connect row, got {:?}",
                    out.diagnostics,
                )
            });
        assert_eq!(diag.severity, Severity::Warning);
        assert!(
            diag.primary.contains("@walkway.gravel"),
            "expected the abstract token to be named, got {}",
            diag.primary,
        );
        assert!(
            !out.walkways.contains_key("walkway::s::a.entry__b.entry"),
            "no walkway should land without a material",
        );
    }

    #[test]
    fn walkway_unknown_abstract_path_emits_e_unknown_abstract_token() {
        // Pack supplied but does not declare `@walkway.grvl`; spec §7.2
        // requires fail-loud here — the typo must surface as
        // `E_UNKNOWN_ABSTRACT_TOKEN` with the nearest declared token as a
        // suggestion note.
        let resolver = FakeResolver {
            entries: vec![("walkway.gravel", "gravel")],
        };
        let src = walkway_pair_source("@walkway.grvl");
        let out = lowered_with_resolver(&src, &resolver);
        let diag = out
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::UnknownAbstractToken)
            .unwrap_or_else(|| {
                panic!(
                    "expected E_UNKNOWN_ABSTRACT_TOKEN on the connect row, got {:?}",
                    out.diagnostics,
                )
            });
        assert_eq!(diag.severity, Severity::Error);
        assert!(
            diag.notes
                .iter()
                .any(|n| n.message.contains("walkway.gravel")),
            "expected nearest-match note pointing at walkway.gravel, got {:?}",
            diag.notes,
        );
        assert!(
            !out.walkways.contains_key("walkway::s::a.entry__b.entry"),
            "no walkway should land for an unknown abstract token",
        );
    }

    fn endpoint_cascade_source(a_def: &str, b_def: &str, defs: &str, connect_line: &str) -> String {
        // Endpoint-cascade fixture: caller supplies the two def names a
        // and b reference, the def declarations themselves, and the
        // connect row. Lets each side combination drop in without
        // re-spelling the boilerplate.
        format!(
            concat!(
                "theme t:\n",
                "  slot wall -> @cobblestone\n",
                "\n",
                "{defs}",
                "\n",
                "site s:\n",
                "  place id=a use={a_def} theme=t at=origin\n",
                "  place id=b use={b_def} theme=t at=origin\n",
                "  {connect_line}\n",
            ),
            defs = defs,
            a_def = a_def,
            b_def = b_def,
            connect_line = connect_line,
        )
    }

    fn sized_then_sizeless_defs() -> &'static str {
        concat!(
            "def sized size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "\n",
            "def sizeless:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "\n",
        )
    }

    fn cascade_warning(out: &BlockArrayIr) -> &Diagnostic {
        out.diagnostics
            .iter()
            .find(|d| {
                d.code == DiagnosticCode::DeferredMember
                    && d.primary.contains("walkway")
                    && d.primary.contains("did not lower")
            })
            .unwrap_or_else(|| {
                panic!(
                    "expected a cascade W_DEFERRED_MEMBER, got {:?}",
                    out.diagnostics,
                )
            })
    }

    #[test]
    fn walkway_endpoint_skipped_to_side_cascades_w_deferred_member() {
        // `b` is sizeless → `to` placement missing. The cascade warning
        // must name `b.entry` only.
        let src = endpoint_cascade_source(
            "sized",
            "sizeless",
            sized_then_sizeless_defs(),
            "connect a.entry to b.entry path=@gravel",
        );
        let out = lowered(&src);
        let cascade = cascade_warning(&out);
        assert!(
            cascade.primary.contains("`b.entry` placement")
                && !cascade.primary.contains("`a.entry`"),
            "expected the cascade to single out `b.entry`, got {}",
            cascade.primary,
        );
        assert!(
            !out.walkways.contains_key("walkway::s::a.entry__b.entry"),
            "walkway must not lay against a placement that did not lower",
        );
    }

    #[test]
    fn walkway_endpoint_skipped_from_side_cascades_w_deferred_member() {
        // Swap the roles: `a` references the sizeless def, so the
        // `from` half is the missing one. The cascade must mention
        // `a.entry` and stay silent about `b.entry`.
        let src = endpoint_cascade_source(
            "sizeless",
            "sized",
            sized_then_sizeless_defs(),
            "connect a.entry to b.entry path=@gravel",
        );
        let out = lowered(&src);
        let cascade = cascade_warning(&out);
        assert!(
            cascade.primary.contains("`a.entry` placement")
                && !cascade.primary.contains("`b.entry`"),
            "expected the cascade to single out `a.entry`, got {}",
            cascade.primary,
        );
        assert!(
            !out.walkways.contains_key("walkway::s::a.entry__b.entry"),
            "walkway must not lay against a placement that did not lower",
        );
    }

    #[test]
    fn walkway_endpoint_skipped_both_sides_cascades_w_deferred_member() {
        // Both placements reference sizeless defs → the cascade arm for
        // `(true, true)` triggers and the message must list both sides.
        let defs = concat!(
            "def sizeless_a:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "\n",
            "def sizeless_b:\n",
            "  walls mat_slot=wall height=2\n",
            "  door id=entry side=front at=center\n",
            "\n",
        );
        let src = endpoint_cascade_source(
            "sizeless_a",
            "sizeless_b",
            defs,
            "connect a.entry to b.entry path=@gravel",
        );
        let out = lowered(&src);
        let cascade = cascade_warning(&out);
        assert!(
            cascade.primary.contains("`a.entry`") && cascade.primary.contains("`b.entry`"),
            "expected the cascade to name both endpoints, got {}",
            cascade.primary,
        );
        assert!(
            !out.walkways.contains_key("walkway::s::a.entry__b.entry"),
            "walkway must not lay against a pair of skipped placements",
        );
    }

    #[test]
    fn east_of_skipped_prior_does_not_silently_stack_at_origin() {
        // Prior place `a` references a sizeless def, so it earns
        // `W_DEF_NO_SIZE` and never lands in `placements`. The
        // `east_of=a` lookup on `b` would silently fall back to `(0, 0,
        // 0)` under the old code path, stacking both buildings on top
        // of each other. The new path emits W_DEFERRED_MEMBER and skips
        // the placement instead.
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def sized size=3x3:\n",
            "  walls mat_slot=wall height=2\n",
            "\n",
            "def sizeless:\n",
            "  walls mat_slot=wall height=2\n",
            "\n",
            "site s:\n",
            "  place id=a use=sizeless theme=t at=origin\n",
            "  place id=b use=sized theme=t east_of=a gap=2\n",
        );
        let out = lowered(src);
        assert!(
            !out.placements.contains_key("site::s::b"),
            "placement b must be skipped, not silently stacked at origin",
        );
        let cascade = out.diagnostics.iter().any(|d| {
            d.code == DiagnosticCode::DeferredMember
                && d.primary.contains("east_of")
                && d.primary.contains("origin cannot be resolved")
        });
        assert!(
            cascade,
            "expected a cascade W_DEFERRED_MEMBER mentioning the unresolvable origin, got {:?}",
            out.diagnostics,
        );
    }

    // --- level flattening / y_offset coverage (CG-1, IG-1) ------------------

    #[test]
    fn level_without_y_defers_and_drops_its_children() {
        // `level` without `y=` cannot place its children, so the whole
        // subtree drops with a single defer at the level's span.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=floor2\n    walls id=upper mat_slot=w height=2\n";
        let out = lowered(src);
        let defers: Vec<&str> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .map(|d| d.primary.as_str())
            .collect();
        assert_eq!(
            defers.len(),
            1,
            "level with missing y= should defer once, got {defers:?}",
        );
        assert!(
            defers[0].contains("level requires `y="),
            "defer reason should mention required y=; got {}",
            defers[0],
        );
    }

    #[test]
    fn level_with_non_integer_y_defers_with_generic_reason() {
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=floor2 y=top\n    walls id=upper mat_slot=w height=2\n";
        let out = lowered(src);
        let defers: Vec<&str> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .map(|d| d.primary.as_str())
            .collect();
        assert!(
            defers.iter().any(|d| d.contains("`y=`")),
            "expected a defer mentioning `y=` on a non-integer value, got {defers:?}",
        );
    }

    #[test]
    fn nested_level_defers_per_inner_level_child() {
        // Two inner `level` children → two defers, so the count reflects
        // how many subtrees were skipped.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=outer y=0\n    level id=inner1 y=1\n      walls id=a mat_slot=w height=1\n    level id=inner2 y=2\n      walls id=b mat_slot=w height=1\n";
        let out = lowered(src);
        let nested: Vec<&str> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember && d.primary.contains("nested"))
            .map(|d| d.primary.as_str())
            .collect();
        assert_eq!(
            nested.len(),
            2,
            "one defer per inner level expected, got {nested:?}",
        );
    }

    #[test]
    fn max_wall_top_aggregates_across_level_walls() {
        // struct walls height=5 + level y=5 walls height=4 → tower top
        // at y=9. `dims.y = 1 + 9 + gable_extra` with roof_w=5, roof_h=5
        // (no overhang) so ridge span=5 and gable_extra = ceil(5/2) = 3.
        // dims.y = 1 + 9 + 3 = 13.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=5\n  roof kind=gable mat_slot=r\n  level id=floor2 y=5\n    walls id=upper mat_slot=w height=4\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").expect("structure lowered");
        assert_eq!(ba.dims.y, 13);
        assert_eq!(
            deferred_count(&out),
            0,
            "unexpected defers: {:?}",
            out.diagnostics
        );
        // Second-floor SW-most (low-x, low-z) corner at y=6..=9 is
        // cobblestone from the level walls, not air.
        for y in 6..=9 {
            assert_eq!(block_id(ba, 0, y, 0), "minecraft:cobblestone");
        }
    }

    // --- fill_stair unhappy path coverage (CG-2, CG-3) ----------------------

    #[test]
    fn stair_without_kind_defers() {
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=gable mat_slot=r overhang=1\n  stair id=e mat_slot=r side=front half=top facing=out\n";
        let out = lowered(src);
        let defers: Vec<&str> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .map(|d| d.primary.as_str())
            .collect();
        assert!(
            defers.iter().any(|d| d.contains("stair without `kind=`")),
            "expected `stair without kind=` defer, got {defers:?}",
        );
    }

    #[test]
    fn stair_with_unknown_kind_defers() {
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=gable mat_slot=r overhang=1\n  stair id=e kind=spiral mat_slot=r side=front half=top\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("stair `kind=spiral`")),
            "expected defer for kind=spiral, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn stair_with_unknown_half_defers() {
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=gable mat_slot=r overhang=1\n  stair id=e kind=stairs mat_slot=r side=front half=middle\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("stair `half=middle`")),
            "expected defer for half=middle, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn stair_with_unknown_facing_defers() {
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=gable mat_slot=r overhang=1\n  stair id=e kind=stairs mat_slot=r side=front half=top facing=north\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("stair `facing=north`")),
            "expected defer for facing=north, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn stair_with_unknown_shape_defers() {
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=gable mat_slot=r overhang=1\n  stair id=e kind=stairs mat_slot=r side=front half=top facing=out shape=inner_left\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("stair `shape=inner_left`")),
            "expected defer for shape=inner_left, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn stair_without_overhang_defers() {
        // A roof with `overhang=0` (or missing overhang) leaves no room
        // for the eave to sit outside the wall; the defer explains why.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r\n  stair id=e kind=stairs mat_slot=r side=front half=top facing=out\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("overhang=")),
            "expected defer for overhang=0, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn stair_with_y_out_of_bounds_defers() {
        // struct walls height=3, roof kind=flat → dims.y = 1 + 3 + 1 = 5.
        // A stair at y=99 (well past dims.y) must defer instead of
        // silently painting into thin air.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r overhang=1\n  stair id=e kind=stairs mat_slot=r side=front half=top facing=out y=99\n";
        let out = lowered(src);
        assert!(
            out.diagnostics.iter().any(|d| d.primary.contains("y=99")),
            "expected defer mentioning y=99, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn stair_facing_in_paints_the_inward_cardinal() {
        // side=front + facing=in → the stair riser points -z (north).
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=gable mat_slot=r overhang=1\n  stair id=e kind=stairs mat_slot=r side=front half=top facing=in\n";
        let out = lowered(src);
        assert_eq!(
            deferred_count(&out),
            0,
            "unexpected defers: {:?}",
            out.diagnostics
        );
        let ba = out.structures.get("struct::s").unwrap();
        // Overhang row outside front wall is z = overhang + interior_h = 6.
        // Local y=0 → world y=0. Interior x∈[1, 5]. Grab column x=3.
        let state = &ba.palette.entries[usize::from(ba.voxels[ba.dims.index(3, 0, 6).unwrap()].0)];
        assert_eq!(
            state.properties.get("facing").map(String::as_str),
            Some("north")
        );
    }

    // --- fill_window unhappy path coverage (CG-4) ---------------------------

    #[test]
    fn window_repeat_zero_defers() {
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=6x5\n  walls mat_slot=w height=3\n  window side=front y=1 size=1x1 repeat=0\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("repeat=0")),
            "expected defer for repeat=0, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn window_repeat_without_step_defers() {
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=6x5\n  walls mat_slot=w height=3\n  window side=front y=1 size=1x1 repeat=3\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("requires a positive `step=`")),
            "expected defer for repeat without step, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn window_repeat_with_sym_defers() {
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=6x5\n  walls mat_slot=w height=3\n  window side=front y=1 size=1x1 repeat=2 step=2 sym=true\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("`repeat=` and `sym=true`")),
            "expected defer for repeat+sym, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn window_span_beyond_wall_defers() {
        // wall length 6, size=2, repeat=4 step=2 → span_end = 0 + 3*2 + 2 = 8 > 6.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=6x5\n  walls mat_slot=w height=3\n  window side=front y=1 size=2x1 repeat=4 step=2\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("extends beyond")),
            "expected span-overrun defer, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn window_repeat_step_leaves_columns_between_stamps_alone() {
        // repeat=2 step=3 size=1x1 offset=0 on a 6-wide front wall:
        // stamps at u=0 and u=3. Wall x mapping: overhang=0, interior_w=6,
        // z=interior_h - 1 = 4. Column at u=1 must remain cobblestone.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=6x5\n  walls mat_slot=w height=3\n  window side=front y=1 size=1x1 repeat=2 step=3\n";
        let out = lowered(src);
        assert_eq!(
            deferred_count(&out),
            0,
            "unexpected defers: {:?}",
            out.diagnostics
        );
        let ba = out.structures.get("struct::s").unwrap();
        // Air carve (no mat_slot=) → voxel is air (palette index 0).
        assert_eq!(block_id(ba, 0, 1, 4), BlockState::AIR_ID);
        assert_eq!(block_id(ba, 3, 1, 4), BlockState::AIR_ID);
        // Between stamps at u=1 and u=2 the wall stays.
        assert_eq!(block_id(ba, 1, 1, 4), "minecraft:cobblestone");
        assert_eq!(block_id(ba, 2, 1, 4), "minecraft:cobblestone");
    }

    #[test]
    fn window_without_mat_slot_carves_air_regression() {
        // A single-stamp mat_slot-less window should carve air (not
        // silently drop). Independent of the arrow-slit repeat pattern.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  window side=front y=1 offset=2 size=1x1\n";
        let out = lowered(src);
        assert_eq!(
            deferred_count(&out),
            0,
            "unexpected defers: {:?}",
            out.diagnostics
        );
        let ba = out.structures.get("struct::s").unwrap();
        // The cell that used to be a wall voxel is now air.
        assert_eq!(block_id(ba, 2, 1, 4), BlockState::AIR_ID);
    }

    #[test]
    fn window_carve_cannot_exceed_wall_top() {
        // walls height=3 → wall_top=3, roof kind=flat → dims.y=5. A mat_slot-less
        // window at y=3 size=1x2 would reach y=4 (roof deck) without a defer
        // if the check only gated on dims.y. It must defer.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r\n  window side=front y=3 size=1x2\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("extends above the wall column")),
            "expected wall_top-gate defer, got {:?}",
            out.diagnostics,
        );
    }

    // --- carve_door level cap regression (C2) -------------------------------

    #[test]
    fn door_defers_when_level_sits_at_or_above_wall_top() {
        // struct walls height=3 (top=3) with a door inside `level y=3`:
        // the door would try to carve at world y=4, 5 which are outside
        // any wall column. The cap `wall_top - y_offset < 1` fires the
        // "no wall above this level" defer instead of silently painting.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=roofline y=3\n    door id=hole side=front at=center\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("at least one wall voxel above")),
            "expected level-cap defer, got {:?}",
            out.diagnostics,
        );
    }

    // --- pressure_plate lowering --------------------------------------------

    #[test]
    fn pressure_plate_outside_with_overhang_paints_in_the_overhang_column() {
        // With `overhang=1` on the roof the struct inflates by one voxel
        // on every horizontal axis (dims.x = 3+2 = 5, dims.z = 5). The
        // front wall sits at z=overhang+ih-1=3, and the exterior
        // overhang column is at z=4. `at=front.outside offset=0 y=0`
        // must land in that exterior column, not fall back to the wall
        // row.
        let src = "theme t:\n  slot p -> @oak_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=p height=2\n  roof kind=flat mat_slot=p overhang=1\n  pressure_plate at=front.outside offset=0 y=0\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims.x, 5);
        assert_eq!(ba.dims.z, 5);
        assert_eq!(block_id(ba, 1, 0, 4), "minecraft:oak_pressure_plate");
        // The wall's own foundation cell at z=3 must NOT hold a plate.
        assert_ne!(block_id(ba, 1, 0, 3), "minecraft:oak_pressure_plate");
    }

    #[test]
    fn pressure_plate_outside_at_y_zero_without_overhang_falls_back_to_foundation() {
        // No roof → dims stay at the authored 3x3 footprint (overhang=0).
        // `at=front.outside offset=0 y=0` cannot reach an exterior cell,
        // so the foundation fallback paints on the wall's own y=0 column
        // (still floor material at that row).
        let src = "theme t:\n  slot p -> @oak_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=p height=2\n  pressure_plate at=front.outside offset=0 y=0\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(ba.dims.x, 3);
        assert_eq!(ba.dims.z, 3);
        assert_eq!(block_id(ba, 0, 0, 2), "minecraft:oak_pressure_plate");
        assert_eq!(deferred_count(&out), 0);
    }

    #[test]
    fn pressure_plate_outside_above_ground_without_overhang_defers() {
        // A plate at `y=1` with no overhang would clobber the wall block
        // the massing phase painted directly above the foundation. The
        // fallback is restricted to y=0 for that reason and higher plates
        // must defer.
        let src = "theme t:\n  slot p -> @oak_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=p height=2\n  pressure_plate at=front.outside offset=0 y=1\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("has no exterior voxel")),
            "expected an exterior-voxel defer at y=1, got {:?}",
            out.diagnostics,
        );
    }

    #[test]
    fn pressure_plate_outside_saturating_shift_defers_above_ground() {
        // Left wall at overhang=0 sits at x=0. `shift_outward` saturates
        // back to x=0, which is a valid dims cell but *not* an exterior
        // voxel. At y=1 the saturating shift must NOT silently overwrite
        // the wall block — the anchor defers instead.
        let src = "theme t:\n  slot p -> @oak_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=p height=2\n  pressure_plate at=left.outside offset=0 y=1\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("has no exterior voxel")),
            "expected saturating-shift defer, got {:?}",
            out.diagnostics,
        );
        // And the wall block above the foundation must survive intact.
        assert_eq!(block_id(ba, 0, 1, 1), "minecraft:oak_pressure_plate");
    }

    #[test]
    fn pressure_plate_inside_paints_one_voxel_toward_the_interior() {
        let src = "theme t:\n  slot p -> @oak_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=p height=2\n  pressure_plate at=inside.front offset=0 y=0\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(block_id(ba, 0, 0, 1), "minecraft:oak_pressure_plate");
        assert_eq!(deferred_count(&out), 0);
    }

    #[test]
    fn pressure_plate_mat_slot_resolves_into_the_palette() {
        // A `mat_slot=` bound to a canonical id must land in the palette
        // verbatim — the default `oak_pressure_plate` fallback only
        // fires when no binding resolves.
        let src = "theme t:\n  slot fixture -> @spruce_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=fixture height=2\n  pressure_plate mat_slot=fixture at=inside.front offset=0 y=0\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
        assert!(
            ids.contains(&"minecraft:spruce_pressure_plate"),
            "palette should carry the resolved id, got {ids:?}",
        );
        assert_eq!(block_id(ba, 0, 0, 1), "minecraft:spruce_pressure_plate");
    }

    #[test]
    fn pressure_plate_rejects_missing_and_malformed_anchors() {
        for (source, needle) in [
            (
                "theme t:\n  slot p -> @oak_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=p height=2\n  pressure_plate offset=0 y=0\n",
                "without `at=`",
            ),
            (
                "theme t:\n  slot p -> @oak_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=p height=2\n  pressure_plate at=center offset=0 y=0\n",
                "must be `<side>.outside`",
            ),
            (
                "theme t:\n  slot p -> @oak_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=p height=2\n  pressure_plate at=up.outside offset=0 y=0\n",
                "is not one of front, back, left, right",
            ),
        ] {
            let out = lowered(source);
            assert!(
                out.diagnostics.iter().any(|d| d.primary.contains(needle)),
                "expected `{needle}` in diagnostics for source={source:?}, got {:?}",
                out.diagnostics,
            );
        }
    }

    #[test]
    fn pressure_plate_out_of_range_offset_or_y_defers() {
        // offset=99 past a 3-length front wall.
        let out = lowered(
            "theme t:\n  slot p -> @oak_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=p height=2\n  pressure_plate at=inside.front offset=99 y=0\n",
        );
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("runs past the front wall")),
            "expected offset-out-of-range defer, got {:?}",
            out.diagnostics,
        );
        // y=99 past the struct's dims.y.
        let out = lowered(
            "theme t:\n  slot p -> @oak_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=p height=2\n  pressure_plate at=inside.front offset=0 y=99\n",
        );
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("does not fit in the struct")),
            "expected y-out-of-range defer, got {:?}",
            out.diagnostics,
        );
    }

    // --- nonneg_int overflow guard (I5) -------------------------------------

    #[test]
    fn nonneg_int_rejects_values_that_do_not_fit_in_u32() {
        // 5_000_000_000 exceeds u32::MAX (~4.29 * 10^9). The overflow
        // used to clamp to u32::MAX silently; it now defers via
        // `nonneg_int_or_defer` at the level's `y=`.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=huge y=5000000000\n    walls id=upper mat_slot=w height=1\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("fits in u32")),
            "expected overflow defer, got {:?}",
            out.diagnostics,
        );
    }
}
