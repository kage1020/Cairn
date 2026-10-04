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
//! `spec/compilation` "Phase evaluation" evaluates members in a fixed phase
//! order independent of source order:
//!
//! ```text
//! massing  (floor, walls)
//!   → envelope (roof, stair)
//!   → openings (door, window)
//!   → fixtures (pressure_plate)
//! ```
//!
//! The current pass implements those four. That section continues with the
//! three redstone phases, which `cairn-lang-redstone` owns, and closes with
//! `raw` — not a keyword the surface accepts yet, so a `raw` line is
//! `E_UNKNOWN_KEYWORD` from the allowlist pass rather than a phase this
//! one is missing.
//!
//! Members are bucketed by role and processed phase-by-phase; within a
//! phase source order wins (the last-wins rule for local overrides), and
//! two members of one phase contesting a voxel earn a
//! `W_PHASE_CONFLICT` rather than settling it silently. Roles outside the
//! four implemented phases emit `W_DEFERRED_MEMBER` and skip. `level y=N` blocks are flattened into
//! their children before the volume is sized so a nested `walls` / `door`
//! / `window` / `stair` / `pressure_plate` reaches both the dim math and
//! its phase with the level's `y=` applied as an authored offset. That one
//! flattened list is the pass's paint set, and what it drops costs nothing
//! (see [`flatten_members`]).
//!
//! Sizing reads that list twice over, in two steps. `walls` and `roof`
//! shape the volume — the walls set its height, the roof its overhang and
//! the headroom above the wall top — while `floor`, `door`, `window`,
//! `stair`, and `pressure_plate` are authored against the volume those two
//! produce and are checked against it when their phase paints them. So a
//! member being in the list does not mean the dims read it; it means the
//! dims and the paint pass are looking at the same members.
//!
//! A `def` is not voxelised on its own: it concretises only through a
//! `site`'s `place ... use=def_name`, and this pass walks that body once
//! per placement, against the theme that placement bound. So a `def`'s
//! members are reached as many times as they are placed, which is why
//! `lower_to_block_array` ends by dropping findings that repeat.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use crate::ast::{SIGNAL_HEAD, ValueKind};
use crate::check::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::error::Span;
use crate::ids::{PlaceId, PortId, SiteName, WalkwayEndpoint, WalkwayScopeKey};
use crate::intent::{
    DefIr, IntentModule, Member, MemberRole, PatchTargetError, SiteIr, Size, StructIr,
    ValueWithSpan, actuator_patch_target,
};
use crate::resolve::{Resolution, ScopeResolution, place_scope_key};
use crate::suggest::{candidate_list, did_you_mean_note};

use super::{Footprint, MAX_STRUCTURE_VOLUME, Placement, Walkway};

use super::material::{
    IdOrigin, MaterialDeferred, TargetRegistry, UnknownId, resolve_block_state, validated_id,
};
use super::openings::{WallSide, wall_length, wall_local_to_grid};
use super::roof::{
    Cardinal, FLAT_BASE_ID, GableVoxel, HipVoxel, RoofKind, STAIR_BASE_ID, ShedFace, ShedVoxel,
    StairFace, StairShape, flat_block_state, flat_extra_height, flat_voxels, gable_extra_height,
    gable_ridge_axis, gable_stair_state, gable_voxels, hip_extra_height, hip_stair_state,
    hip_voxels, is_stair, shed_extra_height, shed_high_side, shed_slope_span, shed_stair_state,
    shed_voxels, stair_state,
};
use super::walkway::{
    BlockedIndex, ROUTE_AREA_CAP, RoutePathError, SearchRect, WalkwayLayout, WindowArgs,
    build_walkway_array, door_anchor_offset, door_at_deferral, l_path, l_path_area,
    port_world_position, read_window_args, route_path,
};
use super::wall_column::WallColumn;
use super::{
    BlockArray, BlockArrayIr, BlockState, Dims, PALETTE_CAPACITY, Palette, PaletteFull,
    PaletteIndex,
};

/// Catalog token a `pressure_plate` member's default material comes from.
///
/// The id itself is edition-specific — Java spells it
/// `oak_pressure_plate`, Bedrock `wooden_pressure_plate` — and lowering
/// has no edition of its own to decide with. Reading it out of the pack
/// keeps the knowledge in the one place that is already per-edition, at
/// the cost of the token also being writable by an author as
/// `@pressure_plate.default`. That is accepted: the alternative is a
/// second pack component whose only member is this one row.
const PRESSURE_PLATE_TOKEN: &str = "pressure_plate.default";

/// Pressure plate id used when the pack cannot supply one — no registry
/// at all, or a registry with no [`PRESSURE_PLATE_TOKEN`] row.
///
/// Neither case carries an edition, and `spec/versioning-editions` "Backend =
/// data tables" makes Java the base, so the Java spelling is the honest
/// default. It is still checked against the pinned target before it reaches a
/// palette.
/// Species-specific plates (spruce, dark oak, ...) still come from a
/// `mat_slot=` binding, which is honoured verbatim — mirroring
/// [`FLAT_BASE_ID`]'s contract, not [`STAIR_BASE_ID`]'s. A plate attaches
/// no blockstates, so it has nothing to require of the block it names.
const PRESSURE_PLATE_BASE_ID: &str = "minecraft:oak_pressure_plate";

/// Rows a doorway opens where the course it is cut into has room for
/// them: the head height of a Minecraft door.
///
/// A ceiling rather than a size. The rows are counted from the row the
/// door opens at, that row included, so a course whose top *is* that row
/// yields an opening one row tall — the row above a short course is the
/// roof or the gap over the wall, not masonry to cut. See [`carve_door`].
const DOOR_HEIGHT: u32 = 2;

/// Block ids lowering can put in a palette that no pack can redirect.
///
/// These are compiled into this crate, so nothing checks them against the
/// target the way `E_UNKNOWN_ID` checks an authored id — and no pack can
/// respell them either, because none of them reads a catalog token. They
/// must therefore be valid on every supported target of every edition, and
/// a pack-side test holds them to exactly that.
///
/// The list is not self-verifying: a new hardcoded id that nobody adds here
/// is caught by
/// `cairn-lang-formats/tests/pack_ids_exist.rs::every_id_the_examples_intern_exists_in_its_target`,
/// which lowers real sources and reads the palette rather than trusting a
/// list. This one stays because it covers the three ids no example is
/// obliged to reach.
///
/// `PRESSURE_PLATE_BASE_ID` is deliberately absent: it *is* redirectable
/// (through `PRESSURE_PLATE_TOKEN`), so its per-edition correctness is a
/// question about the packs, and the pack-side tests ask it there.
pub const BUILTIN_BLOCK_IDS: &[&str] = &[BlockState::AIR_ID, STAIR_BASE_ID, FLAT_BASE_ID];

/// Lower every `struct` in `intent` into a [`BlockArray`].
///
/// Pairs each struct with its [`ScopeResolution`] from `resolution` so the
/// material lookups go through the same theme bindings `cairn check` and
/// `cairn info` already used. Members are processed in phase order
/// (massing → envelope → openings → fixtures), so a `door` written before
/// `walls` in the source still cuts an opening through the resulting wall.
/// Roles outside the four implemented phases are reported via
/// `W_DEFERRED_MEMBER` and skipped.
///
/// `registry` is the registry-pack-backed view of the compile's target.
/// `Some` turns `@floor.wood.broadleaf`-style tokens into concrete ids,
/// fails loud on misses, and — when the run pinned a `--target` — refuses
/// any id that target does not declare (`E_UNKNOWN_ID`). `None` keeps the
/// behaviour from before the pack carried a materials catalog: every
/// abstract token degrades to a `W_ABSTRACT_TOKEN_DEFERRED` warning, so
/// library callers without a pack still get a partial build.
#[must_use]
pub fn lower_to_block_array(
    intent: &IntentModule,
    resolution: &Resolution,
    registry: Option<&dyn TargetRegistry>,
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
        if let Some(ba) = lower_struct(s, scope, registry, &mut diagnostics) {
            // First-write-wins on a duplicate name, matching
            // `resolve`'s `FIRST_BINDING_WINS`. `resolution.scopes` has
            // already bound the first body; taking the last here would
            // paint the second body's voxels with the first body's
            // resolved materials.
            structures.entry(key).or_insert(ba);
        }
    }

    let mut placed: IndexMap<String, PlacedBody> = IndexMap::new();
    let mut walkways: IndexMap<WalkwayScopeKey, Walkway> = IndexMap::new();
    for site in &intent.sites {
        lower_site(
            site,
            &intent.defs,
            resolution,
            registry,
            &mut structures,
            &mut placed,
            &mut diagnostics,
        );
    }
    // Walkways are laid after every site has emitted its per-place
    // BlockArrays so the collision set already covers every floor tile
    // the strip might cross. Connects survive site boundaries — the
    // resolver tags each `ValidatedConnect` with the `site` name so we
    // can pair it back to the right `placements` lookup here.
    let floor = collect_floor_cells(&structures, &placed);
    lower_connects(
        &ConnectInputs {
            resolution,
            defs: &intent.defs,
            registry,
            placed: &placed,
            floor: &floor,
        },
        &mut structures,
        &mut walkways,
        &mut diagnostics,
    );

    // Report in source order, the way `check::Sink::into_sorted` does. This
    // pass emits in pass order — flatten before dims before phases, scope
    // after scope — so which finding comes first has never tracked which
    // line comes first, and a reader working down a file has to jump
    // around. The sort is stable, so two findings on one span keep the
    // order the passes raised them in.
    diagnostics.sort_by_key(|d| (d.span.start, d.span.end));
    // A lowering diagnostic's identity is the diagnostic. Two that agree on
    // code, span, message, notes and data are one finding reported twice,
    // and the second copy is an artifact of how this pass walks rather than
    // anything the author can act on: a `def` body is voxelised once per
    // `place` that instantiates it, so a finding about the def — or about
    // the theme `slot` line the def reads — comes back once per placement,
    // byte-for-byte the same. Three placements used to mean three copies of
    // `E_INCOMPATIBLE_MATERIAL`, each anchored on the same theme line and
    // each ending in a note saying every member reading that slot has it too.
    //
    // The rule holds only while every finding that is not a repeat says so
    // in its own text, which is a live obligation on the messages here and
    // not a property of the walk: `geometry_material_id`'s deferral names
    // its theme for exactly this reason. `tests/def_member_lowering_diagnostics.rs`
    // holds the cases that must stay apart, and `resolve::resolver`'s module
    // doc says why that stage keeps ledgers instead.
    //
    // `retain` rather than a rebuild, so what is left keeps its order.
    let mut said: HashSet<Diagnostic> = HashSet::new();
    diagnostics.retain(|d| said.insert(d.clone()));

    BlockArrayIr {
        structures,
        // The wall columns stop here: they exist so the `connect` pass
        // can ask a placement's masonry the question the openings phase
        // asked it, and that pass has run.
        placements: placed
            .into_iter()
            .map(|(key, body)| (key, body.placement))
            .collect(),
        walkways,
        diagnostics,
    }
}

/// One `place` row that lowered, and the masonry its body lowered with.
///
/// The column travels beside the placement rather than in a map of its
/// own so the two cannot disagree about which bodies exist: a key in one
/// and not the other would be a `connect` port judged against a body
/// that is in no artifact, and two `place` rows sharing an `id=` is the
/// shape that produces it — the resolver refuses the duplicate, the
/// second body still lowers, and only the first is kept.
///
/// Not a field on [`Placement`]: that record is hashed into
/// `resolved_ir_hash`, and the rows a wall occupies must not move it.
/// The lockfile's own `LockPlacement` is a named projection that would
/// not carry them anyway, and [`WallColumn`] derives no `Serialize`, so
/// the question would not compile before it could be decided.
struct PlacedBody {
    placement: Placement,
    /// The rows this body's `walls` painted — the value the openings
    /// phase cut against. A `connect` row anchoring a port here reads
    /// it, so the port and the cut ask the wall one question.
    walls: WallColumn,
    /// The doors and windows the openings phase cut, by member span. A
    /// port on any other opening is refused: whatever stopped the cut, the
    /// strip would end against the wall.
    cut: HashSet<Span>,
}

/// The walk plane the placements occupy: every cell a walkway may not
/// overwrite, who lays each one, and how far each placement's share of it
/// reaches.
struct FloorPlan {
    /// World-space `(x, y, z)` of every non-air voxel on the y=0 plane of
    /// every placement. The walkway voxeliser uses this set to skip cells
    /// that would overwrite an existing floor tile — a strip ducking under
    /// a corner of a building still completes, but the colliding cell
    /// stays air and the row earns a `W_WALKWAY_BLOCKED` warning.
    cells: HashSet<(i32, i32, i32)>,
    /// For each cell in [`Self::cells`], what lays it, in placement
    /// order. Recorded as the cell is
    /// inserted, so a note naming what covers a port reads the same
    /// derivation the router was blocked by.
    owners: HashMap<(i32, i32, i32), Vec<FloorOwner>>,
    /// Per placement that put at least one cell in [`Self::cells`], under
    /// its `site::SITE::PLACE_ID` key, the box those cells span. The
    /// router's search rectangle is the union of these boxes on its plane
    /// together with the two port cells, inflated by one cell on every
    /// side (see `walkway::search_rect`), so a note about that rectangle
    /// can name the placements that stretched it.
    extents: IndexMap<String, FloorExtent>,
}

/// One placement's voxel on a cell of [`FloorPlan::cells`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FloorOwner {
    /// The placement's index in `placed`.
    placement: usize,
    /// The voxel, as an index into that placement's palette.
    voxel: PaletteIndex,
}

/// The world-space box one placement's cells in [`FloorPlan::cells`] span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FloorExtent {
    /// The walk plane the cells lie on.
    y: i32,
    min_x: i32,
    max_x: i32,
    min_z: i32,
    max_z: i32,
}

/// Collect the [`FloorPlan`] of every lowered placement.
fn collect_floor_cells(
    structures: &IndexMap<String, BlockArray>,
    placed: &IndexMap<String, PlacedBody>,
) -> FloorPlan {
    let mut cells: HashSet<(i32, i32, i32)> = HashSet::new();
    let mut owners: HashMap<(i32, i32, i32), Vec<FloorOwner>> = HashMap::new();
    let mut extents: IndexMap<String, FloorExtent> = IndexMap::new();
    for (index, (key, PlacedBody { placement, .. })) in placed.iter().enumerate() {
        let Some(ba) = structures.get(key) else {
            // INVARIANT: `lower_site` inserts each key into `placed` and
            // into `structures` together, first write winning in both, and
            // nothing removes one before this runs.
            debug_assert!(false, "placement `{key}` has no structure");
            continue;
        };
        // Only the y=0 plane matters: walkways sit at the ports' shared
        // Y (=0 for every example). 3D path search (staircases, multi-level
        // walkways) is intentionally out of scope so the port surface lands
        // in one piece.
        for z in 0..ba.dims.z {
            for x in 0..ba.dims.x {
                let Some(i) = ba.dims.index(x, 0, z) else {
                    // INVARIANT: the loop keeps `x` and `z` inside `dims`,
                    // and a lowered body is at least one cell tall, so its
                    // row 0 exists.
                    debug_assert!(
                        false,
                        "cell ({x}, 0, {z}) of `{key}` is outside {:?}",
                        ba.dims
                    );
                    continue;
                };
                let voxel = ba.voxels[i];
                if voxel == PaletteIndex::AIR {
                    continue;
                }
                let world = |origin: i32, local: u32| {
                    i32::try_from(local)
                        .ok()
                        .and_then(|local| origin.checked_add(local))
                };
                // INVARIANT: `PlaceAnchor::origin` refuses a row whose body
                // reaches past `i32`, so every cell of a placed body has a
                // world coordinate. Loud in debug builds; a release build
                // skips the cell. Saturating it instead would fold every
                // column past the range onto the edge cell, which would then
                // read as laid by whichever of them was not air.
                let (Some(wx), Some(wz)) =
                    (world(placement.origin.0, x), world(placement.origin.2, z))
                else {
                    debug_assert!(
                        false,
                        "cell ({x}, 0, {z}) of `{key}` at {:?} has no world coordinate; \
                         `PlaceAnchor::origin` should have refused the row",
                        placement.origin,
                    );
                    continue;
                };
                let cell = (wx, placement.origin.1, wz);
                cells.insert(cell);
                owners.entry(cell).or_default().push(FloorOwner {
                    placement: index,
                    voxel,
                });
                extents
                    .entry(key.clone())
                    .and_modify(|e| {
                        e.min_x = e.min_x.min(wx);
                        e.max_x = e.max_x.max(wx);
                        e.min_z = e.min_z.min(wz);
                        e.max_z = e.max_z.max(wz);
                    })
                    .or_insert(FloorExtent {
                        y: placement.origin.1,
                        min_x: wx,
                        max_x: wx,
                        min_z: wz,
                        max_z: wz,
                    });
            }
        }
    }
    FloorPlan {
        cells,
        owners,
        extents,
    }
}

/// What laying the walkways reads: the whole finished site at once —
/// every placement, the masonry each of them lowered with, and the floor
/// plan they occupy.
struct ConnectInputs<'a> {
    resolution: &'a Resolution,
    defs: &'a [DefIr],
    registry: Option<&'a dyn TargetRegistry>,
    /// Every `place` row that lowered, under its `site::SITE::PLACE_ID`
    /// key.
    placed: &'a IndexMap<String, PlacedBody>,
    /// World cells on the walk plane already occupied by a placement
    /// floor, and the extent each placement's floor covers.
    floor: &'a FloorPlan,
}

/// Lower every resolved `connect` row into a walkway `BlockArray` and
/// a matching [`Walkway`] metadata record.
///
/// Skips a row whose port [`port_world_position`] refuses — a role other
/// than [`MemberRole::Door`] or [`MemberRole::Window`], an argument the
/// opening cannot use, masonry the opening does not reach, an opening the
/// openings phase did not cut, a coordinate past `i32` — with a
/// `W_DEFERRED_MEMBER` carrying one note per refused endpoint, each naming
/// that endpoint's [`super::walkway::PortRejection`]. Emits a `W_DUPLICATE_WALKWAY` when
/// the same `(from, to)` pair has already been laid in the same site.
///
/// Every finding raised here anchors on `ValidatedConnect::span`, which
/// the resolver sets to one `connect` member's span, and this loop runs
/// once per row — so two rows never share a span, and the dedup at the end
/// of [`lower_to_block_array`] cannot reach them. That holds because
/// `connect` is one row per pair. A block form, where several pairs sat
/// under one header, would give them a shared span and two identical
/// `W_WALKWAY_BLOCKED` warnings would collapse into one, `skipped` count
/// and all.
#[allow(clippy::too_many_lines)] // one linear resolve-route-and-lay chain per row
fn lower_connects(
    inputs: &ConnectInputs<'_>,
    structures: &mut IndexMap<String, BlockArray>,
    walkways: &mut IndexMap<WalkwayScopeKey, Walkway>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let &ConnectInputs {
        resolution,
        defs,
        registry,
        placed,
        floor,
    } = inputs;
    let blocked = &floor.cells;
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
        let from_body = placed.get(&from_key);
        let to_body = placed.get(&to_key);
        let (Some(from_body), Some(to_body)) = (from_body, to_body) else {
            // At least one placement was rejected upstream (sizeless def,
            // unresolved theme, broken origin chain). The connect itself
            // resolved, so without a follow-up warning the walkway would
            // vanish silently — emit a cascade `W_DEFERRED_MEMBER` that
            // names the offending side so the user can see *why* the
            // strip was not laid.
            diagnostics.push(diag_walkway_endpoint_skipped(
                connect,
                from_body.is_none(),
                to_body.is_none(),
            ));
            continue;
        };
        let from_def = defs
            .iter()
            .find(|d| d.name == from_body.placement.source_def);
        let to_def = defs.iter().find(|d| d.name == to_body.placement.source_def);
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
        // One note per refused endpoint, naming the reason
        // `port_world_position` gave for it rather than every contract a
        // port has. Where the reason is on a member, the note points at
        // that member's line — which, for a side, argument or masonry
        // fault, carries the member's own deferral too, since the opening
        // was not cut either. Each refusing arm is spelled out and builds
        // its notes from the `Err`s it matched, so a defer with a primary
        // and no note under it is not a shape this can produce.
        let refused = match (from_pos, to_pos) {
            (Ok(from_pos), Ok(to_pos)) => Ok((from_pos, to_pos)),
            (Err(from_err), Err(to_err)) => Err((
                vec![
                    from_err.note(&connect.from.to_string()),
                    to_err.note(&connect.to.to_string()),
                ],
                "ports",
                (true, true),
            )),
            (Err(from_err), Ok(_)) => Err((
                vec![from_err.note(&connect.from.to_string())],
                "port",
                (true, false),
            )),
            (Ok(_), Err(to_err)) => Err((
                vec![to_err.note(&connect.to.to_string())],
                "port",
                (false, true),
            )),
        };
        let (from_pos, to_pos) = match refused {
            Ok(positions) => positions,
            Err((notes, noun, (from_refused, to_refused))) => {
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
                continue;
            }
        };

        // Duplicate guard: pin on (site, from_place, from_port,
        // to_place, to_port). `walkway_pair` sorts the two ends so
        // `a.entry → b.entry` and `b.entry → a.entry` count as the
        // same walkway — laying the strip both ways would be a silent
        // double-write. The pair is recorded only once its strip is
        // laid, at the bottom of this loop: an earlier row with the same
        // pair that the checks below refused laid nothing, so this row
        // is not a duplicate of it.
        let dedup_key = connect.walkway_pair();
        if seen_pairs.contains(&dedup_key) {
            diagnostics.push(Diagnostic {
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
            });
            continue;
        }

        let material = match resolve_block_state(&connect.path, registry) {
            Ok(state) => {
                diagnostics.extend(diag_state_literal_unchecked(&connect.path, &state));
                state
            }
            Err(MaterialDeferred::Abstract(token)) => {
                diagnostics.push(diag_abstract_token(
                    connect.path.span.clone(),
                    &token,
                    TokenSite::WalkwayPath,
                ));
                continue;
            }
            Err(MaterialDeferred::UnknownAbstract { token, suggestion }) => {
                diagnostics.push(diag_unknown_abstract_token(
                    connect.path.span.clone(),
                    &token,
                    suggestion.as_deref(),
                    TokenSite::WalkwayPath,
                ));
                continue;
            }
            Err(MaterialDeferred::UnknownId(unknown)) => {
                diagnostics.push(diag_unknown_id(connect.path.span.clone(), &unknown));
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
        //
        // Measure the L before building it. `route_path` refuses on the
        // same quantity, but it runs second and only when the straight L
        // is obstructed, so an unobstructed pair would otherwise size a
        // voxel buffer from the bounding box directly.
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
                span: connect.span.clone(),
                primary: format!(
                    "walkway `{from} ↔ {to}` skipped {skipped} cells that overlapped an existing structure",
                    from = connect.from,
                    to = connect.to,
                ),
                notes: vec![DiagnosticNote {
                    span: None,
                    message: walkway_blocked_note(
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
                    ),
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
        // `from_parts` refuses every id that could alias another row's key
        // (`W_INVALID_WALKWAY_IDENT` above), so a replaced entry here means
        // that rule has a hole: the earlier row's walkway would vanish from
        // the build. The `insert` is bound first because a `debug_assert!`
        // around it would drop the insert in release builds, and a release
        // build still reports the loss, as `W_WALKWAY_BLOCKED` does above.
        let replaced = structures.insert(scope_key.as_str().to_owned(), array);
        debug_assert!(
            replaced.is_none(),
            "walkway `{scope_key}` replaced an existing structure; two `connect` rows \
             encoded to one scope key",
        );
        if replaced.is_some() {
            diagnostics.push(Diagnostic {
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
            });
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

/// The two ports of a `connect` row a `W_WALKWAY_BLOCKED` note talks
/// about: each one's world cell and the `site::SITE::PLACE_ID` key of the
/// placement it belongs to.
struct BlockedEndpoints<'a> {
    from: ((i32, i32, i32), &'a str),
    to: ((i32, i32, i32), &'a str),
}

/// The finished floor plan a `W_WALKWAY_BLOCKED` note reads to name what
/// is in the way.
struct SiteFloor<'a> {
    structures: &'a IndexMap<String, BlockArray>,
    placed: &'a IndexMap<String, PlacedBody>,
    floor: &'a FloorPlan,
}

/// Note text for a `W_WALKWAY_BLOCKED` warning on a row whose obstructed
/// straight L sent it to the router, matched to why the router could not
/// detour. The remedies differ per cause — widening the gap fixes an
/// enclosed target but does nothing for a buried port or a site past the
/// area cap — so a single catch-all suggestion would misdirect the author
/// on three of the four arms.
fn walkway_blocked_note(
    connect: &crate::resolve::ValidatedConnect,
    route_failure: Option<RoutePathError>,
    ends: &BlockedEndpoints<'_>,
    site: &SiteFloor<'_>,
) -> String {
    match route_failure {
        Some(RoutePathError::EndpointBlocked {
            from_blocked,
            to_blocked,
        }) => {
            let mut clauses = Vec::new();
            if from_blocked {
                clauses.push(buried_port_clause(&connect.from, ends.from, connect, site));
            }
            if to_blocked {
                clauses.push(buried_port_clause(&connect.to, ends.to, connect, site));
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

/// The note for a router whose [`SearchRect`] passed the area cap.
///
/// The rectangle spans both ports and every placement floor on the walk
/// plane, so either can be what passed the cap, and the remedies differ.
/// When the two ports' own box, with the router's margin, is already past
/// the cap, moving any placement does nothing: the note says to bring the
/// two structures closer. Otherwise the floors stretched it, and the note
/// names the placements whose floor reaches an edge beyond both ports.
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
        // The rectangle is the ports' box unless a floor stretches it, and
        // the ports' box is inside the cap, so some floor must.
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
        list = and_list(&labels),
    )
}

/// Cells in the box around `from` and `to` inflated by one cell on every
/// side — the router's rectangle with no floor in it.
fn margin_box_area(from: (i32, i32, i32), to: (i32, i32, i32)) -> u64 {
    let span = |a: i32, b: i32| u64::from(a.abs_diff(b)) + 3;
    span(from.0, to.0).saturating_mul(span(from.2, to.2))
}

/// The keys of the placements whose floor sets an edge of `rect` beyond
/// both ports, in placement order: on some side, the floor reaches the
/// cell just inside the router's one-cell margin, past the outer port on
/// that side.
fn edge_pushers(
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

/// A placement as a note names it: its `place id=` in backticks, plus
/// the site when that differs from the `connect` row's own.
fn placement_label(placement: &Placement, connect: &crate::resolve::ValidatedConnect) -> String {
    if placement.site == connect.site {
        format!("`{}`", placement.place_id)
    } else {
        format!("`{}` in site `{}`", placement.place_id, placement.site)
    }
}

/// One clause of a buried-endpoint note: what covers the port cell, and
/// the remedy for each. Another placement's floor is fixed by moving the
/// opening or the placements apart; a block the port's own placement
/// lays there (a pressure plate in front of its door) is fixed by moving
/// that block or the opening, and pulling placements apart does nothing.
/// When both cover the cell, both are named, since fixing either alone
/// leaves the port buried.
fn buried_port_clause(
    port: &crate::resolve::PortRef,
    (cell, own_key): ((i32, i32, i32), &str),
    connect: &crate::resolve::ValidatedConnect,
    site: &SiteFloor<'_>,
) -> String {
    let mut own_block: Option<Option<&str>> = None;
    let mut own_label = None;
    let mut others: Vec<String> = Vec::new();
    for owner in site.floor.owners.get(&cell).into_iter().flatten() {
        let Some((key, body)) = site.placed.get_index(owner.placement) else {
            // INVARIANT: `collect_floor_cells` records indices into this
            // same `placed`, which nothing mutates after.
            debug_assert!(false, "floor owner {owner:?} indexes past `placed`");
            continue;
        };
        if key == own_key {
            let block = site
                .structures
                .get(key)
                .and_then(|ba| ba.palette.entries.get(usize::from(owner.voxel.0)));
            if block.is_none() {
                // INVARIANT: the voxel was read from this placement's own
                // array, whose palette `ScopePalette` built. A miss is a
                // broken array; the clause still names the placement, just
                // not the block.
                debug_assert!(
                    false,
                    "voxel {owner:?} of `{key}` names no entry of its palette",
                );
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
        format!("the floor of {noun} {}", and_list(&others))
    });
    match (own, others) {
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
            // INVARIANT: the router reported this cell blocked, and every
            // cell of `FloorPlan::cells` was recorded with its owners.
            debug_assert!(
                false,
                "port `{port}` at {cell:?} is blocked but has no owner"
            );
            format!(
                "port `{port}` is buried inside another placement's floor; move that \
                 door/window to an unobstructed wall or pull the placements apart"
            )
        }
    }
}

/// `a`, `a and b`, `a, b, and c`: the items joined as an English list,
/// with the serial comma.
fn and_list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [only] => only.clone(),
        [first, second] => format!("{first} and {second}"),
        [init @ .., last] => format!("{}, and {last}", init.join(", ")),
    }
}

fn diag_walkway_invalid_ident(
    connect: &crate::resolve::ValidatedConnect,
    err: &crate::ids::KeyConstructError,
) -> Diagnostic {
    use crate::ids::{KeyConstructError, KeySegmentRole};
    // Sentence frame, one clause per variant:
    //   primary: walkway `A ↔ B` was dropped because the ROLE id `SEG` PROBLEM
    //   note:    CONSTRAINT; rename the ROLE FIX
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

/// The endpoint(s) of `connect` a diagnostic blames, each in backticks:
/// `` `a` and `b` ``, `` `a` `` or `` `b` ``. At least one side must be at
/// fault.
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

/// Where an `@token` was read from, for the prose of the abstract-token
/// diagnostics. [`Self::MemberSlot`] also carries what gets built without
/// the material, which [`Self::consequence`] words.
///
/// Only `W_ABSTRACT_TOKEN_DEFERRED` reads that payload.
/// [`diag_unknown_abstract_token`] reads [`Self::token_noun`] and
/// [`Self::catalog_note`] alone, because `E_UNKNOWN_ABSTRACT_TOKEN` stops the
/// build and there is no fallback to describe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TokenSite {
    /// A `connect` row's `path=`.
    WalkwayPath,
    /// A member's `mat_slot=` binding, with what becomes of that member
    /// when the binding resolves to nothing.
    MemberSlot(MemberFallback),
}

/// What becomes of a member whose `mat_slot=` resolves to no material.
///
/// A property of the role's painter, so it is derived from the role by
/// [`Self::for_role`] rather than handed in by each caller of
/// [`resolve_member_state`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemberFallback {
    /// A `floor`: [`fill_floor`] is not reached, so its cells stay air.
    Air,
    /// A `walls`: it paints no row, and [`painting_walls`] drops it, so it
    /// claims none in the volume either ([`max_wall_top`]) and a `door` or
    /// `window` cut into it is refused.
    Absent,
    /// A `window`: [`cut_window`] refuses it, so the wall it would have cut
    /// stays, and a port anchored on it is refused with it.
    WallStays,
    /// A `roof`, an eave `stair` or a `pressure_plate`: built from its
    /// default block.
    ///
    /// A `roof` or eave `stair` also gets [`geometry_material_id`]'s
    /// `W_DEFERRED_MEMBER`, which names the concrete id: only the geometry
    /// side knows it, since a roof's depends on its `kind`.
    /// [`plate_id_for_member`] deliberately does not echo; its doc says why.
    DefaultBlock,
}

impl MemberFallback {
    /// The consequence for a member of this role.
    ///
    /// Only the six roles whose painters call [`resolve_member_state`] reach
    /// here. The rest are spelled out with no wildcard, as in the four
    /// `lower_*_member` matches, so a role added later fails the compile
    /// here rather than borrowing another role's sentence: a `door` is
    /// carved and reads no material, a `circuit` reserves a region, a
    /// `place` or `connect` is a site row, `Other` is a keyword the role
    /// table does not know, and a `level` is flattened before any painter
    /// runs.
    fn for_role(role: &MemberRole) -> Self {
        match role {
            MemberRole::Floor => Self::Air,
            MemberRole::Walls => Self::Absent,
            MemberRole::Window => Self::WallStays,
            MemberRole::Roof | MemberRole::Stair | MemberRole::PressurePlate => Self::DefaultBlock,
            MemberRole::Door
            | MemberRole::Level
            | MemberRole::Circuit
            | MemberRole::Place
            | MemberRole::Connect
            | MemberRole::Other(_) => {
                unreachable!(
                    "no painter resolves the `mat_slot=` of a `{}`",
                    MemberRole::keyword(role)
                )
            }
        }
    }
}

impl TokenSite {
    fn token_noun(self) -> &'static str {
        match self {
            Self::WalkwayPath => "abstract path token",
            Self::MemberSlot(_) => "abstract token",
        }
    }

    /// What the build does instead, worded to follow "cannot be lowered
    /// without the registry pack; ".
    fn consequence(self) -> &'static str {
        match self {
            Self::WalkwayPath => "the walkway falls back to air",
            Self::MemberSlot(MemberFallback::Air) => "the cell falls back to air",
            Self::MemberSlot(MemberFallback::Absent) => {
                "the walls are not built and take up no rows, so a door or window cut into them \
                 is refused"
            }
            Self::MemberSlot(MemberFallback::WallStays) => {
                "the window is not cut, and the wall stays"
            }
            Self::MemberSlot(MemberFallback::DefaultBlock) => {
                "the member is built from its default block"
            }
        }
    }

    fn canonical_example(self) -> &'static str {
        match self {
            Self::WalkwayPath => "path=@gravel",
            Self::MemberSlot(_) => "@oak_planks",
        }
    }

    fn catalog_note(self) -> &'static str {
        match self {
            Self::WalkwayPath => {
                "abstract path tokens must be declared in the pack's `materials` catalog"
            }
            Self::MemberSlot(_) => {
                "abstract material tokens must be declared in the pack's `materials` catalog \
                 (see `spec/materials-themes` \"Canonical vocabulary\")"
            }
        }
    }
}

fn diag_abstract_token(span: Span, token: &str, site: TokenSite) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::AbstractTokenDeferred,
        span,
        primary: format!(
            "{} `@{token}` cannot be lowered without the registry pack; {}",
            site.token_noun(),
            site.consequence(),
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: format!(
                "use a canonical block token (e.g. `{}`) until the registry pack ships",
                site.canonical_example(),
            ),
        }],
        data: None,
    }
}

fn diag_unknown_abstract_token(
    span: Span,
    token: &str,
    suggestion: Option<&str>,
    site: TokenSite,
) -> Diagnostic {
    let mut notes = Vec::with_capacity(2);
    notes.extend(suggestion.map(|s| did_you_mean_note(&format!("@{s}"))));
    notes.push(DiagnosticNote {
        span: None,
        message: site.catalog_note().to_owned(),
    });
    Diagnostic {
        code: DiagnosticCode::UnknownAbstractToken,
        span,
        primary: format!(
            "{} `@{token}` is not declared by the registry pack's materials catalog",
            site.token_noun(),
        ),
        notes,
        data: None,
    }
}

/// Say that a state literal was taken as written, when `value` carries one.
///
/// `spec/versioning-editions` "Fail-loud and minimum-version inference"
/// makes an out-of-domain state a hard error, `E_STATE_DOMAIN`, and
/// nothing raises it yet: no table this compiler holds says which
/// properties a block has or which values each takes. Until one does, the
/// literal reaches the palette and the structure file unchanged, right or
/// wrong, and this warning is what keeps that from being silent. It
/// anchors on the value the way `E_UNKNOWN_ID` does, so the mistake right
/// of the `[` is pointed at from the same place as one left of it.
///
/// Only a canonical token folds a `[` into its text, so a state that came
/// from anywhere else — a catalog lookup, a member default — is not one.
fn diag_state_literal_unchecked(value: &ValueWithSpan, state: &BlockState) -> Option<Diagnostic> {
    let ValueKind::Token(text) = &value.value.kind else {
        return None;
    };
    if state.properties.is_empty() || !text.contains('[') {
        return None;
    }
    let pairs = state
        .properties
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(",");
    Some(Diagnostic {
        code: DiagnosticCode::StateLiteralUnchecked,
        span: value.span.clone(),
        primary: format!(
            "`{id}` is written with `{pairs}` unchecked: nothing checks a state literal's \
             properties or values against the target yet",
            id = state.id,
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "a property the block does not have, or a value outside its domain, \
                      reaches the structure file unchanged; check each one against the \
                      block's states in the target edition and version"
                .to_owned(),
        }],
        data: None,
    })
}

/// Report a block id the compile's target does not declare.
///
/// One builder serves both the member and the walkway site: the span
/// differs and nothing else does. The prose fork that matters is not which
/// statement the id sat on but *who chose it* — an author can correct what
/// they typed, while a catalog mapping is not theirs to edit and a message
/// that blames their token sends them to the wrong file.
fn diag_unknown_id(span: Span, unknown: &UnknownId) -> Diagnostic {
    let UnknownId {
        id,
        registry,
        origin,
        suggestion,
        aliases,
    } = unknown;
    let mut notes = Vec::with_capacity(2);
    match origin {
        IdOrigin::Authored => {}
        IdOrigin::Catalog { token } => notes.push(DiagnosticNote {
            span: None,
            message: format!(
                "the registry pack maps `@{token}` onto it; the token is fine and the pack's mapping is not, so this is not a fix you make here",
            ),
        }),
        IdOrigin::Builtin { token } => notes.push(DiagnosticNote {
            span: None,
            message: format!(
                "the registry pack declares no `{token}`, so lowering fell back to the id compiled into the compiler; the pack needs that row",
            ),
        }),
    }
    // One note, and the alias table wins it when it has anything to say.
    // The two answers are not additive: a reader handed "this target calls
    // it X" and "the nearest spelling is Y" has to work out which of the
    // two the compiler believes, and the answer is always the first. The
    // payload still carries both, for a consumer that can show them apart.
    notes.push(DiagnosticNote {
        span: None,
        message: match (aliases.as_slice(), suggestion) {
            ([], None) => format!(
                "no block in `{registry}` is near enough to suggest; compile against a target that declares `{id}`, or pick a block this one has",
            ),
            ([], Some(candidate)) => {
                format!("`{registry}` spells the nearest block `{candidate}`")
            }
            (spellings, _) => format!(
                "`{registry}` spells this block {}, per the registry pack's alias table",
                candidate_list(spellings),
            ),
        },
    });
    Diagnostic {
        code: DiagnosticCode::UnknownId,
        span,
        primary: format!("`{id}` is not a block in `{registry}`"),
        notes,
        data: Some(DiagnosticData::UnknownId {
            id: id.clone(),
            registry: registry.clone(),
            origin: origin.kind().to_owned(),
            token: origin.token().map(str::to_owned),
            suggestion: suggestion.clone(),
            aliases: aliases.clone(),
        }),
    }
}

fn lower_struct<'a>(
    s: &StructIr,
    scope: Option<&'a ScopeResolution>,
    registry: Option<&'a dyn TargetRegistry>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<BlockArray> {
    let Some(size) = s.size.as_ref() else {
        diagnostics.push(diag_struct_no_size(s));
        return None;
    };
    // A struct is never placed, so nothing ever anchors a port to it and
    // its wall column has no second reader.
    let lowered = lower_body_to_block_array(
        BodyDescriptor {
            kind: VoxelSource::Struct,
            scope_label: &s.name,
            size,
            members: &s.members,
            header_span: &s.span,
            source_scope: format!("struct::{}", s.name),
        },
        scope,
        registry,
        diagnostics,
    )?;
    Some(lowered.array)
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
    registry: Option<&'a dyn TargetRegistry>,
    structures: &mut IndexMap<String, BlockArray>,
    placed: &mut IndexMap<String, PlacedBody>,
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
        // A site name is an identifier the lexer produced, and the lexer's
        // `Ident` rule is a strict subset of what `SiteName` accepts. Folding this into the arm above
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
            // Usually the resolver already said why: `E_UNRESOLVED_PLACE_REF`
            // / `E_UNRESOLVED_THEME_REF` / `E_INVALID_PLACE_ORIGIN`, so
            // repeating it here would double the count.
            //
            // Not always, though. A `place` row with no `use=` or no
            // `theme=` takes a silent arm in `resolve_site_placements` (see
            // the two INVARIANT blocks there) and lands here with nothing
            // reported at either layer. Staying silent is still the right
            // call for this arm — lowering has no span for the missing key
            // and no way to tell which one it was — but the gap is the
            // resolver's to close, not evidence that it already did.
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

        // The anchor reads `placed` for prior-place lookups, so the lookup
        // has to happen before *this* placement is inserted. A lookup
        // misses when the prior place never reached `placed`, which is any
        // `continue` arm of this loop: those above, or, below, the anchor
        // deferral, the volume refusal and the `i32` range refusal. Falling
        // back to `(0, 0, 0)` would silently stack the placement on top of
        // `home1`, so the row is deferred and skipped instead; only the
        // origin waits for the lowered dims.
        //
        // An unreadable `gap=` is reported on every path out of this row,
        // placed or not, with a note that says which: see
        // [`read_or_ignore`] for why the finding is never held back.
        let (anchor, gap_unread) = resolve_place_anchor(member, placed, &site.name);

        // The body's findings go straight out, whether or not the row is
        // then placed — also when its anchor did not lower. Lowering a body
        // takes nothing from the row's origin — the voxels are local to
        // the body, and the origin is worked out from them afterwards — so
        // every finding it raises is one the row would have raised had it
        // landed: a defect in the `def` or the theme, which the author has
        // to fix wherever the row ends up. Holding them for a refused row
        // only moved them one compile later, and a `def` that only refused
        // rows place would never have reported them at all.
        let body = lower_body_to_block_array(
            BodyDescriptor {
                kind: VoxelSource::Place,
                scope_label: place_id,
                size: def_size,
                members: &def.members,
                header_span: &member.span,
                source_scope: key,
            },
            Some(scope),
            registry,
            diagnostics,
        );
        // Below the body on purpose, not an early return above it: a row
        // whose anchor did not lower still reports its body's findings.
        let Some(anchor) = anchor else {
            diagnostics.push(diag_deferred_member_reason(
                member,
                "the prior place referenced by `east_of=`/`north_of=` did not lower, so this placement's origin cannot be resolved",
            ));
            report_unread_gap(gap_unread, GapOutcome::NotPlaced, diagnostics);
            continue;
        };
        let Some(LoweredBody { array, walls, cut }) = body else {
            // The extent was refused; the diagnostic names the scope, and
            // recording a placement for a structure that does not exist
            // would leave the lockfile pointing at nothing.
            report_unread_gap(gap_unread, GapOutcome::NotPlaced, diagnostics);
            continue;
        };
        // `array.source_scope` now owns the IR key — read it back so the
        // two map inserts share that one allocation as their canonical
        // key (one extra clone for `placed`, one move into
        // `structures`).
        let dims = array.dims;
        // `north_of` needs this body's own depth, so the origin is finished
        // only now that the body has been sized.
        let origin = match anchor.origin(dims) {
            Ok(origin) => origin,
            Err(refusal) => {
                diagnostics.push(diag_deferred_member_reason(member, &refusal.deferral()));
                report_unread_gap(gap_unread, GapOutcome::OutOfRange, diagnostics);
                continue;
            }
        };
        report_unread_gap(gap_unread, GapOutcome::Placed, diagnostics);
        // First-write-wins, as above. Two `site` blocks of one name put
        // their `place id=` rows into one `site::NAME::` namespace, so
        // only a repeated `id=` collides — and the resolver has already
        // bound the first of those. `placed` and `structures` share the
        // key here, and the lockfile reads a placement's dims beside the
        // structure it names, so the two must agree on which body won.
        placed
            .entry(array.source_scope.clone())
            .or_insert(PlacedBody {
                walls,
                cut,
                placement: Placement {
                    site: placement_site,
                    place_id: placement_id,
                    source_def: use_name.to_owned(),
                    // The theme that governed the build, not the one the
                    // row spelled. A `--edition` pin can bind a different
                    // variant than the `place` named
                    // (`W_THEME_VARIANT_REBOUND`), and the warning
                    // scrolls away while the lockfile is what a later
                    // reader has. Recording the written name there would
                    // name a variant whose materials are not in the
                    // artifact.
                    theme: scope
                        .bound_theme
                        .clone()
                        .unwrap_or_else(|| theme_name.to_owned()),
                    origin,
                    dims,
                },
            });
        structures
            .entry(array.source_scope.clone())
            .or_insert(array);
    }
}

/// Which lowering entry point produced this body. Lets diagnostic messages
/// distinguish a sizeless struct from a sizeless def without adding two
/// near-identical helpers, and lets a future fixtures pass switch on the
/// host when origin conventions differ.
///
/// Distinct from [`crate::intent::BodyKind`], which splits `struct` / `def`
/// from `site` by what the author wrote. This one splits by what is being
/// voxelised, and a `place` voxelises a `def` — so the two disagree on
/// every placement and were worth separate names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VoxelSource {
    Struct,
    Place,
}

/// Inputs shared by the struct and place lowering paths.
struct BodyDescriptor<'a> {
    kind: VoxelSource,
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

/// What lowering one body produced: the voxels, and the wall column they
/// were painted against.
///
/// The column comes back out because the `connect` pass asks a
/// placement's masonry the same question the openings phase asked it,
/// and one answer is what keeps them from disagreeing. Deriving it a
/// second time from the `def` would be this rule written twice — which
/// is how a strip came to be laid to a window the openings phase had
/// deferred, and refused to one it had cut.
struct LoweredBody {
    array: BlockArray,
    walls: WallColumn,
    /// The spans of the `door` and `window` members that painted at
    /// least one cell — the openings the `connect` pass may anchor a port
    /// on. Read off the canvas rather than reported by the generators, so
    /// a new way for a cut to defer cannot forget to say so.
    cut: HashSet<Span>,
}

/// Lower one struct or place body into voxels.
///
/// `None` means the extent the body asks for is past
/// [`MAX_STRUCTURE_VOLUME`]; the diagnostic has already been pushed and the
/// caller drops the scope.
fn lower_body_to_block_array<'a>(
    body: BodyDescriptor<'a>,
    scope: Option<&'a ScopeResolution>,
    registry: Option<&'a dyn TargetRegistry>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<LoweredBody> {
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

    // Flatten `level y=N` grouping once, here. The result is the set of
    // members this body paints, and everything below reads it: the dim math
    // (which has to size a volume that holds them) and the phase buckets
    // (which need each member's y-offset). Deriving the dims from any other
    // list is how a roof came to paint into a volume too short to hold it.
    // `flatten_members` also emits the `W_DEFERRED_MEMBER` for every member
    // it drops, so its side effects need to happen exactly once per call.
    let flattened = flatten_members(body.members, diagnostics);
    // Inflate the struct's footprint by the maximum `overhang=` across all
    // roof members so the roof's eaves and gable-end overhangs have voxel
    // room outside the wall ring. Floors, walls, doors, and windows are
    // authored against the *interior* size and shifted inward by this
    // amount in their respective fill helpers.
    let overhang = max_roof_overhang(&flattened, diagnostics);
    // One walk over the walls, read twice. `resolve_member_state` is not
    // free — the abstract-token miss path builds the whole catalogue to
    // pick a suggestion from — and asking it once per member here is also
    // what makes "the volume and the carve read one list" true of the
    // run and not only of the rule.
    let painting_walls: Vec<(u32, u32)> =
        painting_walls(&flattened, scope, registry, theme_missing).collect();
    let max_wall_top = max_wall_top(&painting_walls);
    let wall_column = WallColumn::from_walls(painting_walls.iter().copied());
    let roof_extra = max_roof_extra_height(&flattened, interior_w, interior_h, overhang);

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
    let mut palette = ScopePalette::new();

    let ctx = StructCtx {
        scope,
        registry,
        theme_missing,
        dims,
        overhang,
        interior_w,
        interior_h,
        wall_top: max_wall_top,
        wall_column,
    };

    let buckets = bucket_members(&flattened, diagnostics);
    let canvas = paint_phases(buckets, &ctx, &mut palette, diagnostics);
    // Before anything reads the grid: a voxel painted after the palette
    // filled up holds air rather than its state, so the array is not the
    // body the source describes.
    let mut palette = match palette.into_palette() {
        Ok(palette) => palette,
        Err(full) => {
            diagnostics.push(diag_palette_too_large(&body, full));
            return None;
        }
    };

    for ((overridden, overriding), voxels) in &canvas.conflicts {
        diagnostics.push(diag_phase_conflict(
            flattened[*overridden as usize].1,
            flattened[*overriding as usize].1,
            *voxels,
        ));
    }
    let cut: HashSet<Span> = flattened
        .iter()
        .zip(&canvas.wrote)
        .filter(|((_, m), wrote)| {
            **wrote && matches!(m.role, MemberRole::Door | MemberRole::Window)
        })
        .map(|((_, m), _)| m.span.clone())
        .collect();
    let (voxels, never_painted) = prune_unreferenced(&mut palette, &canvas);
    debug_assert!(
        never_painted.is_empty(),
        "palette slots {never_painted:?} of `{}` were claimed but no voxel was ever painted \
         with one; a generator is interning a material for geometry it does not emit",
        body.scope_label,
    );

    let mut array = BlockArray {
        dims,
        palette,
        voxels,
        block_entities: Vec::new(),
        entities: Vec::new(),
        source_scope: body.source_scope,
    };
    // Last, and after the prune: the grid is finished, so this is the
    // point where the palette can be a rendering of what the body
    // contains instead of a log of the order the paints ran in. Doing it
    // before the prune would sort entries that are about to be dropped
    // and renumber twice for the same answer.
    array.canonicalize_palette();

    Some(LoweredBody {
        array,
        walls: ctx.wall_column,
        cut,
    })
}

/// Paint one body's members onto a fresh canvas, phase by phase in the
/// order `spec/compilation` "Phase evaluation" fixes.
///
/// That order is the whole content of this helper: a `door` written
/// before `walls` in the source still cuts through the resulting wall
/// because openings run after massing, whatever the lines say. It owns
/// the canvas because a canvas outlives no phase — it is created from
/// the same buckets that are about to be painted onto it, and handing
/// the two to a caller separately is an order for the caller to get
/// right.
///
/// None of the three adjacent swaps — massing/envelope,
/// envelope/openings, openings/fixtures — is observable with today's
/// generators, because no two neighbouring phases write the same cell.
/// A roof starts at `wall_top + 1` and an eave `stair` is shifted a
/// voxel outside the wall line into an overhang it defers without, while
/// every opening is cut into the wall ring at or below `wall_top`; a
/// `pressure_plate` is shifted a voxel in or out for the same reason;
/// and a `floor` is the single interior plane at row 0, which no opening
/// reaches. Separation in y for some of those pairs, in x/z for the
/// others.
///
/// What the corpus does see is massing running after openings — a
/// two-step move rather than a swap — which fills a carved opening back
/// in. So the order here is the spec's, held by the argument above
/// rather than by a test, which is what a generator that grows into a
/// neighbour's cells will change.
fn paint_phases(
    buckets: PhaseBuckets<'_>,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    diagnostics: &mut Vec<Diagnostic>,
) -> Canvas {
    let PhaseBuckets {
        massing,
        envelope,
        openings,
        fixtures,
        phases,
    } = buckets;
    let mut canvas = Canvas::new(ctx.dims, phases);
    run_phase(
        massing,
        lower_massing_member,
        ctx,
        palette,
        &mut canvas,
        diagnostics,
    );
    run_phase(
        envelope,
        lower_envelope_member,
        ctx,
        palette,
        &mut canvas,
        diagnostics,
    );
    run_phase(
        openings,
        lower_opening_member,
        ctx,
        palette,
        &mut canvas,
        diagnostics,
    );
    run_phase(
        fixtures,
        lower_fixture_member,
        ctx,
        palette,
        &mut canvas,
        diagnostics,
    );
    canvas
}

/// The paint set filed by phase, each entry keeping its position in the
/// flattened list so the canvas can name it in a finding.
struct PhaseBuckets<'a> {
    massing: Vec<(u32, u32, &'a Member)>,
    envelope: Vec<(u32, u32, &'a Member)>,
    openings: Vec<(u32, u32, &'a Member)>,
    fixtures: Vec<(u32, u32, &'a Member)>,
    /// One entry per member of the flattened list, in the same order:
    /// which phase evaluates it, or `None` when none does. Handed to the
    /// [`Canvas`] so a voxel can carry its writer's index alone.
    phases: Vec<Option<Phase>>,
}

/// File every member of the paint set under the phase `spec/compilation`
/// "Phase evaluation" evaluates it in, reporting the ones no phase has a
/// reader for.
///
/// Within a bucket the members keep source order, which is what that section's
/// last-wins grant is about; across buckets the order is the spec's, which
/// is what makes the source order-free.
fn bucket_members<'a>(
    flattened: &[(u32, &'a Member)],
    diagnostics: &mut Vec<Diagnostic>,
) -> PhaseBuckets<'a> {
    let mut buckets = PhaseBuckets {
        massing: Vec::new(),
        envelope: Vec::new(),
        openings: Vec::new(),
        fixtures: Vec::new(),
        phases: Vec::with_capacity(flattened.len()),
    };
    for (index, &(y_offset, member)) in flattened.iter().enumerate() {
        // `u32` because that is what a voxel stores. A paint set longer
        // than `u32::MAX` would need a source of `level` blocks measured
        // in gigabytes, and the volume such a body asks for is refused by
        // `MAX_STRUCTURE_VOLUME` long before this point.
        let index = u32::try_from(index).expect("the paint set is far shorter than u32::MAX");
        // Actuator patches (`door[id=X] opened_by=sig.Y`) are metadata
        // overlays on an already-declared physical door — they carry
        // neither `side=` nor `at=` and must not enter the openings
        // phase, or `carve_door`'s `side_of` guard would false-positive
        // "missing side=". The recogniser handles the surface shape
        // here; the wired signal graph is threaded on by the future
        // redstone lowering pipeline (`spec/redstone` "Signal binding").
        if is_actuator_patch(member) {
            recognize_actuator_patch(member, flattened, diagnostics);
            buckets.phases.push(None);
            continue;
        }
        let disposition = member_disposition(&member.role);
        buckets.phases.push(match disposition {
            MemberDisposition::Paints(phase) => Some(phase),
            MemberDisposition::Reserves | MemberDisposition::NotLowered => None,
        });
        match disposition {
            MemberDisposition::Paints(Phase::Massing) => {
                buckets.massing.push((index, y_offset, member));
            }
            MemberDisposition::Paints(Phase::Envelope) => {
                buckets.envelope.push((index, y_offset, member));
            }
            MemberDisposition::Paints(Phase::Openings) => {
                buckets.openings.push((index, y_offset, member));
            }
            MemberDisposition::Paints(Phase::Fixtures) => {
                buckets.fixtures.push((index, y_offset, member));
            }
            // `circuit region=<label> void=<N>` reserves a routing region
            // for the future `logic_synth → logic_place → logic_route`
            // passes (`spec/redstone` "Place-and-route" and "Connection to
            // the IR and phases"). Nothing lands in the block array from this
            // member; the recognizer only checks the surface shape so a valid
            // fixture stays quiet while a malformed one still surfaces a
            // targeted `W_DEFERRED_MEMBER`.
            MemberDisposition::Reserves => recognize_circuit_region(member, diagnostics),
            MemberDisposition::NotLowered => diagnostics.push(diag_deferred_member(member)),
        }
    }
    buckets
}

/// Lower one phase's bucket, opening a [`MemberCanvas`] per member so
/// every write it makes is filed under it. A bucket cannot forget: the
/// borrow is the only route to a write, and it closes with the member.
fn run_phase(
    bucket: Vec<(u32, u32, &Member)>,
    lower: fn(
        &Member,
        u32,
        &StructCtx<'_>,
        &mut ScopePalette,
        &mut MemberCanvas<'_>,
        &mut Vec<Diagnostic>,
    ),
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut Canvas,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (index, y_offset, member) in bucket {
        let mut member_canvas = canvas.for_member(index);
        lower(
            member,
            y_offset,
            ctx,
            palette,
            &mut member_canvas,
            diagnostics,
        );
    }
}

/// Two members of one phase wrote the same cell to different blocks.
///
/// `spec/compilation` "Phase evaluation" settles that by source order —
/// "last-wins applies only to local overrides within the same phase" — and
/// this says so out loud, because an override the author meant and two
/// footprints that happen to intersect look identical from the grid. Anchored
/// at the member that wrote last, the way `E_LOGIC_MULTIPLE_DRIVERS` anchors
/// at the redefinition and notes the first declaration.
fn diag_phase_conflict(overridden: &Member, overriding: &Member, voxels: u32) -> Diagnostic {
    let cell = if voxels == 1 { "voxel" } else { "voxels" };
    Diagnostic {
        code: DiagnosticCode::PhaseConflict,
        span: overriding.span.clone(),
        primary: format!(
            "`{}` overwrites {voxels} {cell} that `{}` painted in the same phase, so which \
             block the build keeps is decided by the order the two lines are written in",
            MemberRole::keyword(&overriding.role),
            MemberRole::keyword(&overridden.role),
        ),
        notes: vec![
            DiagnosticNote {
                span: Some(overridden.span.clone()),
                message: "overwritten member declared here".to_owned(),
            },
            DiagnosticNote {
                span: None,
                message: "If the override is deliberate this is the local-override rule of \
                          spec/compilation \"Phase evaluation\" and the later line wins as \
                          written. If it is not, move one of the two so their footprints do \
                          not meet."
                    .to_owned(),
            },
            DiagnosticNote {
                span: None,
                message: "Members in different phases may overlap freely — a `door` cut \
                          through a `walls` is the evaluation model working — so this is \
                          only reported for two members the same phase evaluates."
                    .to_owned(),
            },
        ],
        data: None,
    }
}

/// Drop the palette slots a later phase covered, and remap the voxels
/// onto the survivors.
///
/// A member may paint a cell a later phase then covers — a `window` set
/// into a `walls` ring takes those cells for good — and when a member's
/// last cell goes, its material is left in the palette describing a block
/// the structure does not contain. That entry is not free: the Java writer
/// emits it into the `.nbt`, `cairn info` reports a portability row for
/// it, and `resolved_ir_hash` covers it, so two sources that differ only
/// in which member lost would hash apart.
///
/// Slot 0 stays whatever happens: [`Palette::new_with_air`] puts air there
/// before any member runs, and a fully paved volume names it nowhere.
///
/// A slot that was **never painted at all** is a generator bug, not a
/// covered cell, and is left in place so `tests/palette_is_referenced`
/// can catch it; such slots are returned for the caller to assert on.
fn prune_unreferenced(palette: &mut Palette, canvas: &Canvas) -> (Vec<PaletteIndex>, Vec<usize>) {
    let mut referenced = vec![false; palette.entries.len()];
    for v in &canvas.voxels {
        referenced[usize::from(v.0)] = true;
    }
    let mut remap: Vec<PaletteIndex> = vec![PaletteIndex::AIR; palette.entries.len()];
    let mut kept = Vec::with_capacity(palette.entries.len());
    let mut never_painted = Vec::new();
    for (slot, state) in palette.entries.drain(..).enumerate() {
        let painted = canvas.painted.get(slot).copied().unwrap_or(false);
        if slot != 0 && !referenced[slot] {
            if painted {
                continue;
            }
            never_painted.push(slot);
        }
        remap[slot] = PaletteIndex(
            u16::try_from(kept.len()).expect("the kept palette is no longer than the original"),
        );
        kept.push(state);
    }
    palette.entries = kept;
    let voxels = canvas
        .voxels
        .iter()
        .map(|v| remap[usize::from(v.0)])
        .collect();
    (voxels, never_painted)
}

/// The palette one body is painted against, refusing rather than
/// panicking once it holds [`PALETTE_CAPACITY`] states.
///
/// Every paint interns its state, including one a later member then
/// covers, so what fills the palette is the states *written*, not the
/// states the finished body keeps, and the overflow has to be caught at
/// paint time. A paint past the capacity gets air in place of its state
/// and records the refusal; [`Self::into_palette`] then answers with it,
/// and the body is refused with `W_PALETTE_TOO_LARGE` before its grid is
/// read. Holding the refusal here, rather than threading a `Result` out
/// of every generator, is what lets the generators keep their infallible
/// `intern`, and the type is what keeps them off the panicking
/// [`Palette::intern`].
struct ScopePalette {
    palette: Palette,
    capacity: usize,
    /// The refusal that overflowed the palette, once one has.
    overflow: Option<PaletteFull>,
}

impl ScopePalette {
    fn new() -> Self {
        Self {
            palette: Palette::new_with_air(),
            capacity: scope_palette_capacity(),
            overflow: None,
        }
    }

    fn intern(&mut self, state: BlockState) -> PaletteIndex {
        // Once a paint has overflowed, `into_palette` discards the palette
        // and the body is refused, so which states the palette holds from
        // here on, and which index a later paint gets, is never read. Air
        // without the scan, which would otherwise walk every entry on each
        // remaining paint of an already refused body.
        if self.overflow.is_some() {
            return PaletteIndex::AIR;
        }
        self.palette
            .try_intern_within(state, self.capacity)
            .unwrap_or_else(|full| {
                self.overflow = Some(full);
                PaletteIndex::AIR
            })
    }

    /// The palette, or the refusal that overflowed it.
    fn into_palette(self) -> Result<Palette, PaletteFull> {
        match self.overflow {
            None => Ok(self.palette),
            Some(full) => Err(full),
        }
    }
}

/// The capacity a body's palette is painted against: [`PALETTE_CAPACITY`],
/// or, in this crate's tests, what the test set.
#[cfg(not(test))]
fn scope_palette_capacity() -> usize {
    PALETTE_CAPACITY
}

#[cfg(test)]
fn scope_palette_capacity() -> usize {
    test_capacity::get()
}

/// Lets an in-crate test refuse a body at a capacity it can reach. Per
/// thread, so tests running in parallel do not see each other's.
#[cfg(test)]
mod test_capacity {
    use std::cell::Cell;

    use super::PALETTE_CAPACITY;

    thread_local! {
        static CAPACITY: Cell<usize> = const { Cell::new(PALETTE_CAPACITY) };
    }

    pub(super) fn get() -> usize {
        CAPACITY.with(Cell::get)
    }

    pub(super) fn set(capacity: usize) {
        CAPACITY.with(|cell| cell.set(capacity));
    }
}

/// A body painted more distinct block states than its palette holds.
///
/// The number is `full`'s, the capacity that was in force, less the slot
/// air holds from the start.
fn diag_palette_too_large(body: &BodyDescriptor<'_>, full: PaletteFull) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::PaletteTooLarge,
        span: body.header_span.clone(),
        primary: format!(
            "`{}` paints more than {} distinct non-air block states, past what one palette \
             can index; block-array lowering skipped it",
            body.scope_label,
            full.capacity.saturating_sub(1),
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "every state a member writes counts, including one a later member \
                      covers; a vanilla registry has far fewer, so check the block ids with \
                      `--edition` and `--target`, and each state literal by hand, since no \
                      target checks its properties (`W_STATE_LITERAL_UNCHECKED`)"
                .to_owned(),
        }],
        data: None,
    }
}

/// The scope asked for more voxels than [`MAX_STRUCTURE_VOLUME`] allows.
///
/// Names the extent rather than only the limit: the numbers an author wrote
/// are `size=`, `height=`, and `overhang=`, and the product is what went out
/// of range, so showing the derived extent is what connects the two.
fn diag_structure_too_large(body: &BodyDescriptor<'_>, dims: Dims) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::StructureTooLarge,
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

/// Where one `place` line lands relative to what has already been placed:
/// the prior placement's origin (and, for `east_of`, its width) and the
/// `gap`, read before this placement's body is lowered.
///
/// Finished by [`PlaceAnchor::origin`] once the new body's dims are known,
/// because `north_of` steps back by the *new* placement's depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaceAnchor {
    /// `at=origin`.
    WorldOrigin,
    /// `east_of=ID gap=N`: the prior placement's origin and inflated
    /// `dims.x`.
    EastOf {
        prior_origin: (i32, i32, i32),
        prior_dims_x: u32,
        gap: i64,
    },
    /// `north_of=ID gap=N`: the prior placement's origin. Its depth plays
    /// no part: the step back is the new body's own depth.
    NorthOf {
        prior_origin: (i32, i32, i32),
        gap: i64,
    },
}

/// A placement [`PlaceAnchor::origin`] works out lies partly outside the
/// `i32` range world coordinates are recorded and addressed in. `axis` is
/// the one that left the range and `coord` the coordinate on it that did,
/// for the message; `corner` says which coordinate that is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PlacementOutOfRange {
    corner: PlacementCorner,
    axis: char,
    coord: i128,
}

/// Which end of a placement's box [`PlacementOutOfRange`] found outside
/// the range, and so what its `coord` holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlacementCorner {
    /// The low-`x`, low-`z` origin the placement records. `coord` is the
    /// origin on the axis its selector moves along: a sum for `east_of`, a
    /// difference for `north_of`.
    Origin,
    /// The body's far edge, `origin + dims − 1`, where its last cell sits.
    /// `coord` is that edge on the first of `x` and `z` that leaves the
    /// range, not the whole high corner.
    FarEdge,
}

impl PlacementOutOfRange {
    /// `coord` as the `i32` it has to fit in, or the refusal that says it
    /// does not.
    fn fit(corner: PlacementCorner, axis: char, coord: i128) -> Result<i32, Self> {
        i32::try_from(coord).map_err(|_| Self {
            corner,
            axis,
            coord,
        })
    }

    /// The `W_DEFERRED_MEMBER` reason for the row this refuses.
    ///
    /// The repair depends on which end left the range. The far edge also
    /// moves with the body's own extent, and on a `north_of` row no `gap=`
    /// on the row itself moves `x`, so its advice names the body's `size=`
    /// and `overhang=` beside the `gap=`.
    fn deferral(self) -> String {
        let Self {
            corner,
            axis,
            coord,
        } = self;
        let (what, recorded, repair) = match corner {
            PlacementCorner::Origin => (
                "origin works out to",
                "a placement's origin is recorded in",
                "shorten the `gap=` on this row or on a row it is placed relative to",
            ),
            PlacementCorner::FarEdge => (
                "body reaches",
                "a placement's cells are addressed in",
                "shrink the body with its `def`'s `size=` or a roof's `overhang=`, or shorten \
                 the `gap=` on this row or on a row it is placed relative to",
            ),
        };
        format!(
            "this placement's {what} {axis}={coord}, past the {} to {} range {recorded}; {repair}",
            i32::MIN,
            i32::MAX,
        )
    }
}

impl PlaceAnchor {
    /// The world-space origin (low-`x`, low-`z` corner) of a placement
    /// whose lowered body has `dims`, per `spec/components-editing-sites`
    /// "Origin selectors": `east_of` is `prior.x + prior.dims.x + gap`,
    /// `north_of` is `prior.z − new.dims.z − gap`. Either way `gap` counts
    /// the empty blocks between the two facing bounding-box faces (each the
    /// wall plus its `overhang=` columns), so `gap=0` makes the boxes touch
    /// whichever of the two is wider or deeper.
    ///
    /// The sum is taken in `i128`, where no `i32` origin, `u32` extent and
    /// `i64` gap can overflow, and refused when it leaves `i32` rather than
    /// saturated: a saturated origin put two placements on one coordinate,
    /// the second stacked inside the first, and nothing said so. The body's
    /// far edge, `origin + dims − 1` on `x` and `z`, is refused the same
    /// way, since a cell past it has no world coordinate either. `y` is not
    /// checked, because every placement's `y` is the `0` of the `at=origin`
    /// row its chain starts from (`east_of` and `north_of` carry the
    /// prior's `y` through) and [`MAX_STRUCTURE_VOLUME`] keeps `dims.y` far
    /// below `i32::MAX`; a selector that moves `y` has to join the check.
    fn origin(self, dims: Dims) -> Result<(i32, i32, i32), PlacementOutOfRange> {
        use PlacementCorner::{FarEdge, Origin};
        let fit = PlacementOutOfRange::fit;
        // The low corner first: the far edge is measured from it, so it
        // has to be in range before the far edge means anything.
        let (x, y, z) = match self {
            Self::WorldOrigin => (0, 0, 0),
            Self::EastOf {
                prior_origin: (x, y, z),
                prior_dims_x,
                gap,
            } => {
                let next_x = i128::from(x) + i128::from(prior_dims_x) + i128::from(gap);
                (fit(Origin, 'x', next_x)?, y, z)
            }
            Self::NorthOf {
                prior_origin: (x, y, z),
                gap,
            } => {
                let next_z = i128::from(z) - i128::from(dims.z) - i128::from(gap);
                (x, y, fit(Origin, 'z', next_z)?)
            }
        };
        // INVARIANT: a body's `x` and `z` extents are its `size=` (a
        // `NonZeroU32`) plus twice its overhang, so neither is 0, and
        // `origin + dims − 1` is the last cell rather than one before the
        // origin.
        debug_assert!(
            dims.x > 0 && dims.z > 0,
            "a placed body has a zero extent: {dims:?}",
        );
        for (axis, low, extent) in [('x', x, dims.x), ('z', z, dims.z)] {
            fit(FarEdge, axis, i128::from(low) + i128::from(extent) - 1)?;
        }
        Ok((x, y, z))
    }
}

/// Read the `at=origin` / `east_of=ID gap=N` / `north_of=ID gap=N`
/// selector of one `place` line into a [`PlaceAnchor`].
///
/// On a relative (`east_of` / `north_of`) row the anchor is `None` when the
/// prior place it names did not lower, which the caller reports before
/// skipping the row. A row with no usable selector
/// never gets here: the resolver refuses it with `E_INVALID_PLACE_ORIGIN`,
/// or with `E_UNRESOLVED_PLACE_REF` when the selector names no prior place,
/// and binds no scope for it. The front-is-`+z` convention of
/// `spec/components-editing-sites` "Multi-building with `site`" is why
/// `north_of` retreats along `-z`.
///
/// On a relative row, a `gap=` that is not an integer is an unreadable
/// value: the anchor carries `gap=0`, and the [`UnreadArgument`] is handed
/// back beside it — also when the anchor is `None` — for the caller to
/// report with the note that matches whether the row was placed. An
/// `at=origin` row returns before `gap=` is read.
fn resolve_place_anchor(
    member: &Member,
    placed: &IndexMap<String, PlacedBody>,
    site_name: &str,
) -> (Option<PlaceAnchor>, Option<UnreadArgument>) {
    if let Some(value) = member.intent_state.get("at")
        && matches!(&value.value.kind, ValueKind::Ident(s) if s == "origin")
    {
        return (Some(PlaceAnchor::WorldOrigin), None);
    }
    let (gap, gap_unread) = match read_or_ignore(
        member,
        "gap",
        |kind| match kind {
            ValueKind::Int(n) => Some(*n),
            _ => None,
        },
        "an integer",
    ) {
        Ok(gap) => (gap.unwrap_or(0), None),
        Err(unread) => (0, Some(unread)),
    };
    let prior = |key: &str| {
        member
            .intent_state
            .get(key)
            .and_then(|v| v.value.as_label_str())
            .and_then(|target| placed.get(&place_scope_key(site_name, target)))
            .map(|body| (body.placement.origin, body.placement.dims.x))
    };
    let anchor = if let Some((prior_origin, prior_dims_x)) = prior("east_of") {
        Some(PlaceAnchor::EastOf {
            prior_origin,
            prior_dims_x,
            gap,
        })
    } else {
        prior("north_of").map(|(prior_origin, _)| PlaceAnchor::NorthOf { prior_origin, gap })
    };
    (anchor, gap_unread)
}

/// Where a `place` row with an unreadable `gap=` ended up, which picks the
/// note its finding carries.
#[derive(Clone, Copy)]
enum GapOutcome {
    /// Placed at the `gap=0` the unreadable value falls back to.
    Placed,
    /// Refused for a reason no `gap=` reaches: its anchor did not lower,
    /// or its body was refused.
    NotPlaced,
    /// Refused because the placement worked out at `gap=0` leaves the
    /// `i32` range: its origin, or its body's far edge. Only that value was
    /// tried, so the note claims nothing about any other `gap=`.
    OutOfRange,
}

/// Report a `place` row's unreadable `gap=`, if it had one, with the note
/// for where the row ended up.
fn report_unread_gap(
    unread: Option<UnreadArgument>,
    outcome: GapOutcome,
    diagnostics: &mut Vec<Diagnostic>,
) {
    diagnostics.extend(unread.map(|unread| {
        unread.report(match outcome {
            GapOutcome::Placed => {
                "the row is placed as `gap=0` places it, edge to edge with the place it is \
                 relative to"
            }
            GapOutcome::NotPlaced => {
                "this row is not placed either way — see the finding on the same line"
            }
            GapOutcome::OutOfRange => {
                "this row is not placed at `gap=0`, the value its origin was worked out \
                 with — see the finding on the same line"
            }
        })
    }));
}

/// Bundle of per-struct context shared by every member-lowering helper.
///
/// Carried as a struct (rather than threaded as 7 positional args) so a new
/// per-struct field (e.g. theme name for selector-binding lookups) lands as
/// one field change instead of touching every helper signature.
struct StructCtx<'a> {
    scope: Option<&'a ScopeResolution>,
    registry: Option<&'a dyn TargetRegistry>,
    theme_missing: bool,
    dims: Dims,
    overhang: u32,
    interior_w: u32,
    interior_h: u32,
    /// Highest wall voxel coordinate: the largest `y_offset + height=`
    /// across the walls members the pass will paint, so a `walls` inside a
    /// `level y=N` raises it by that `N`. `0` when no walls are present.
    ///
    /// This is where the roof starts and how tall the volume has to be.
    /// It is *not* what a member cut into the wall may check itself
    /// against — see [`StructCtx::wall_column`].
    wall_top: u32,
    /// Every row the walls members actually paint, gaps and all.
    ///
    /// A `window` has to land inside masonry and a `door` has to open
    /// into it, neither of which `wall_top` can decide: it says nothing
    /// about the rows below the first course and nothing about the air
    /// between two of them. `walkway::port_world_position` is handed
    /// this same column, so a port and the cut it anchors to read one
    /// answer rather than two that agree.
    wall_column: WallColumn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Massing,
    Envelope,
    Openings,
    Fixtures,
}

/// What the phase model does with a member.
///
/// Three answers rather than `Option<Phase>`, because "no phase" covered
/// three different facts and the caller had to take them apart again
/// behind a catch-all arm — which is the arm a role added later falls
/// into by accident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MemberDisposition {
    /// Evaluated in this phase, and paints voxels there.
    Paints(Phase),
    /// Reserves something a later pass reads, and paints nothing. Its
    /// surface shape is still checked, so a malformed one is reported.
    Reserves,
    /// Nothing in this pass lowers it, and it earns a
    /// `W_DEFERRED_MEMBER` saying so.
    NotLowered,
}

/// Where a member's role puts it, per `spec/compilation` "Phase evaluation".
///
/// The spec's order is `massing (shell: floor/walls/volume) → envelope
/// (roof/exterior) → openings (door/window) → fixtures (furnishings:
/// sign/painting/frame/bed/sensors & actuators)`, and each role below is
/// placed against that sentence rather than against what happens to lower
/// alike:
///
/// - `floor` / `walls` are the shell the sentence names.
/// - `roof` is the envelope; `stair` joins it because `spec/compilation`
///   "Gable roof voxel rules" describes it as an *eave* stair, and an eave is
///   exterior.
/// - `door` / `window` are the openings the sentence names.
/// - `pressure_plate` is a sensor, so it is a fixture. Sharing the openings
///   bucket with `window` meant a contested cell went to whichever line came
///   last, which is the order accident the phase order promises away.
/// - `circuit` reserves a routing region for the redstone phases and
///   writes no voxel.
/// - `place` and `connect` belong to a site body, which this pass does not
///   lower; `Other` is a keyword the role table does not know, which
///   includes the `raw` that same phase order names — not yet a keyword at
///   all, so it is reported as unknown by the allowlist pass on top of the
///   deferral here.
///
/// The four `lower_*_member` matches spell every role out with no wildcard
/// so a role added here fails the compile there rather than reaching a
/// user's source and panicking. `Level` never arrives: flattening runs
/// first, so one that does means a caller skipped it.
fn member_disposition(role: &MemberRole) -> MemberDisposition {
    match role {
        MemberRole::Floor | MemberRole::Walls => MemberDisposition::Paints(Phase::Massing),
        MemberRole::Roof | MemberRole::Stair => MemberDisposition::Paints(Phase::Envelope),
        MemberRole::Door | MemberRole::Window => MemberDisposition::Paints(Phase::Openings),
        MemberRole::PressurePlate => MemberDisposition::Paints(Phase::Fixtures),
        MemberRole::Circuit => MemberDisposition::Reserves,
        MemberRole::Place | MemberRole::Connect | MemberRole::Other(_) => {
            MemberDisposition::NotLowered
        }
        // Spelled out rather than folded into the arm above so that a
        // `Level` reaching here is a loud bug and not a deferral: it means
        // a caller skipped `flatten_members`, and the members it was
        // grouping would go missing without a word.
        MemberRole::Level => unreachable!(
            "`Level` members must be flattened before phase-bucketing (see `flatten_members`)"
        ),
    }
}

/// Flatten level blocks so their children participate in phase-bucketing.
///
/// Returns each contributing member paired with the `y_offset` derived from
/// its enclosing `level y=N` (`0` for members that sit directly under the
/// struct/def/place body).
///
/// The result is the **paint set**, not the source members with the `level`
/// wrappers unwrapped. Dimension derivation reads the same list, and the two
/// have to agree in both directions: a member the dims cannot see paints
/// into a volume too small to hold it, and a member the paint pass drops
/// inflates a volume nothing fills. Deciding participation here, once, is
/// what keeps them from drifting apart — the alternative is the same rule
/// spelled out twice, one copy per reader.
///
/// Dropped here, each with its own `W_DEFERRED_MEMBER`:
///
/// - a `level` whose `y=` is missing or is not a non-negative integer,
///   along with its whole body: there is no offset to place the children at;
/// - a `level` nested inside another, one diagnostic per direct child, so an
///   author who wrote deep nesting sees each dropped subtree rather than one
///   defer that hides how many members went with it;
/// - a member whose role has no lowering above the ground plane, per
///   [`level_offset_role_defer`].
///
/// `level` blocks themselves are consumed and never reach the returned list.
/// `logic` and `assert` items inside a level body are intentionally absent
/// too — they live on the intent IR for the resolver and redstone passes to
/// consume, unaffected by block-array lowering.
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
                if let Some(reason) = level_offset_role_defer(&child.role, y_offset) {
                    diagnostics.push(diag_deferred_member_reason(child, reason));
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

/// Why a role has no lowering at a non-zero `level y=`, or `None` when the
/// role means the same thing at any offset.
///
/// `walls`, `door`, `window`, `stair`, and `pressure_plate` read the offset
/// as the base their own geometry is measured from, which is exactly what
/// `level y=N` asks for — `themed-tower.crn` builds its second storey from
/// the first three. A `floor` and a `roof` are single planes the struct has
/// one of: a second floor slab would land in mid-air, and a second roof
/// would cap the building below its own roof plane. Neither generator has a
/// story for that yet, so the member is dropped rather than painted
/// somewhere unexpected — and, being dropped, it contributes nothing to the
/// volume.
///
/// Every variant is spelled out for the reason [`member_disposition`] spells its
/// own out: a role added later must not fall into "lowers the same at any
/// offset" because that is the arm a wildcard happens to reach. The three
/// that lower to nothing at any offset are listed with the rest — they
/// reach the phase buckets and defer there, with a message that names the
/// role rather than the level.
fn level_offset_role_defer(role: &MemberRole, y_offset: u32) -> Option<&'static str> {
    if y_offset == 0 {
        return None;
    }
    match role {
        MemberRole::Floor => Some("level-scoped `floor` is not yet supported"),
        MemberRole::Roof => Some("level-scoped `roof` is not yet supported"),
        // The first five measure their geometry from the offset. The rest
        // paint no voxels at any offset and are listed rather than left to
        // a wildcard: `place` and `connect` belong to a site body and are
        // reported as misplaced, `Other` is an unknown keyword, and
        // `circuit` marks a region for redstone lowering. `circuit` is the
        // one with something left owing — the region recogniser takes no
        // `y_offset`, so a level-scoped region is not shifted. That is a
        // gap on the redstone side rather than something dropping the
        // member would close: a lost region is not closer to right than an
        // unshifted one.
        MemberRole::Walls
        | MemberRole::Door
        | MemberRole::Window
        | MemberRole::Stair
        | MemberRole::PressurePlate
        | MemberRole::Circuit
        | MemberRole::Place
        | MemberRole::Connect
        | MemberRole::Other(_) => None,
        MemberRole::Level => unreachable!(
            "a nested `level` is dropped by `flatten_members` before this function sees it"
        ),
    }
}

fn lower_massing_member(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &member.role {
        MemberRole::Floor => {
            // Always the struct's ground plane: `flatten_members` drops a
            // `floor` at a non-zero offset before it reaches a phase.
            let Some(idx) = palette_index_for(
                member,
                ctx.scope,
                ctx.registry,
                palette,
                diagnostics,
                ctx.theme_missing,
            ) else {
                return;
            };
            fill_floor(ctx, idx, canvas);
        }
        MemberRole::Walls => {
            let Some(height) = wall_height(member, diagnostics) else {
                return;
            };
            let Some(idx) = palette_index_for(
                member,
                ctx.scope,
                ctx.registry,
                palette,
                diagnostics,
                ctx.theme_missing,
            ) else {
                return;
            };
            fill_walls(ctx, height, y_offset, idx, canvas);
        }
        // No wildcard: see [`member_disposition`].
        MemberRole::Roof
        | MemberRole::Stair
        | MemberRole::Door
        | MemberRole::Window
        | MemberRole::PressurePlate
        | MemberRole::Level
        | MemberRole::Circuit
        | MemberRole::Place
        | MemberRole::Connect
        | MemberRole::Other(_) => {
            unreachable!(
                "the massing bucket holds no `{}`",
                MemberRole::keyword(&member.role)
            )
        }
    }
}

fn lower_envelope_member(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &member.role {
        // Always the struct's own roof plane: `flatten_members` drops a
        // `roof` at a non-zero offset before it reaches a phase.
        MemberRole::Roof => fill_roof(member, ctx, palette, canvas, diagnostics),
        MemberRole::Stair => fill_stair(member, y_offset, ctx, palette, canvas, diagnostics),
        // No wildcard: see [`member_disposition`].
        MemberRole::Floor
        | MemberRole::Walls
        | MemberRole::Door
        | MemberRole::Window
        | MemberRole::PressurePlate
        | MemberRole::Level
        | MemberRole::Circuit
        | MemberRole::Place
        | MemberRole::Connect
        | MemberRole::Other(_) => {
            unreachable!(
                "the envelope bucket holds no `{}`",
                MemberRole::keyword(&member.role)
            )
        }
    }
}

fn lower_opening_member(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &member.role {
        MemberRole::Door => carve_door(member, y_offset, ctx, canvas, diagnostics),
        MemberRole::Window => fill_window(member, y_offset, ctx, palette, canvas, diagnostics),
        // No wildcard: see [`member_disposition`].
        MemberRole::Floor
        | MemberRole::Walls
        | MemberRole::Roof
        | MemberRole::Stair
        | MemberRole::PressurePlate
        | MemberRole::Level
        | MemberRole::Circuit
        | MemberRole::Place
        | MemberRole::Connect
        | MemberRole::Other(_) => {
            unreachable!(
                "the openings bucket holds no `{}`",
                MemberRole::keyword(&member.role)
            )
        }
    }
}

fn lower_fixture_member(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match &member.role {
        MemberRole::PressurePlate => {
            fill_pressure_plate(member, y_offset, ctx, palette, canvas, diagnostics);
        }
        // No wildcard: see [`member_disposition`].
        MemberRole::Floor
        | MemberRole::Walls
        | MemberRole::Roof
        | MemberRole::Stair
        | MemberRole::Door
        | MemberRole::Window
        | MemberRole::Level
        | MemberRole::Circuit
        | MemberRole::Place
        | MemberRole::Connect
        | MemberRole::Other(_) => {
            unreachable!(
                "the fixtures bucket holds no `{}`",
                MemberRole::keyword(&member.role)
            )
        }
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
/// - the value lowered as an abstract token and no `registry` resolver was
///   offered (a `W_ABSTRACT_TOKEN_DEFERRED` warning is emitted),
/// - the value lowered as an abstract token the offered `registry` resolver
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
    registry: Option<&dyn TargetRegistry>,
    diagnostics: &mut Vec<Diagnostic>,
    theme_missing: bool,
) -> Option<BlockState> {
    if theme_missing {
        return None;
    }
    let scope = scope?;
    let binding = scope.members.get(&member.span.start)?;
    // INVARIANT(upstream-diagnosed): every reason `slot_value` is `None`
    // has a reporter on the same diagnostic stream, and that stream is
    // gated before anything is emitted — which is the guarantee, rather
    // than any ordering between the two passes. `W_NO_THEME_BOUND` is
    // pushed from *inside* this pass, and the CLI runs lowering before it
    // merges `check`'s findings in, so "reported first" would be false in
    // two different ways. `ResolvedMemberBinding::slot_value` names the
    // four causes and who owns each; the one that used to have no owner is
    // a member carrying no `mat_slot=` at all, now `check::material`'s for
    // the roles that paint nothing without one.
    //
    // Scoped to this `?` alone. The two above it are different questions —
    // a scope that was never built, and a member missing from one that
    // was — and `lower.rs`'s own comment on the `place` path admits the
    // first is reported by neither layer.
    let slot_value: &ValueWithSpan = binding.slot_value.as_ref()?;
    match resolve_block_state(slot_value, registry) {
        Ok(state) => {
            diagnostics.extend(diag_state_literal_unchecked(slot_value, &state));
            Some(state)
        }
        Err(MaterialDeferred::Abstract(token)) => {
            diagnostics.push(diag_abstract_token(
                member_or_slot_span(member, slot_value),
                &token,
                TokenSite::MemberSlot(MemberFallback::for_role(&member.role)),
            ));
            None
        }
        Err(MaterialDeferred::UnknownAbstract { token, suggestion }) => {
            diagnostics.push(diag_unknown_abstract_token(
                member_or_slot_span(member, slot_value),
                &token,
                suggestion.as_deref(),
                TokenSite::MemberSlot(MemberFallback::for_role(&member.role)),
            ));
            None
        }
        Err(MaterialDeferred::UnknownId(unknown)) => {
            diagnostics.push(diag_unknown_id(slot_value.span.clone(), &unknown));
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
    registry: Option<&dyn TargetRegistry>,
    palette: &mut ScopePalette,
    diagnostics: &mut Vec<Diagnostic>,
    theme_missing: bool,
) -> Option<PaletteIndex> {
    resolve_member_state(member, scope, registry, diagnostics, theme_missing)
        .map(|state| palette.intern(state))
}

/// Will this member put a block anywhere?
///
/// The same question [`palette_index_for`] asks when the massing phase
/// paints, through the same function, so the volume and the paint cannot
/// answer differently. The diagnostics are ignored because the massing
/// phase's own call pushes them for real; keeping them here would say
/// each twice.
fn member_will_paint(
    member: &Member,
    scope: Option<&ScopeResolution>,
    registry: Option<&dyn TargetRegistry>,
    theme_missing: bool,
) -> bool {
    let mut ignored_diagnostics = Vec::new();
    resolve_member_state(
        member,
        scope,
        registry,
        &mut ignored_diagnostics,
        theme_missing,
    )
    .is_some()
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
///
/// Members whose material will not resolve are absent from the list for
/// the same reason a heightless one is: they paint no row. A themeless
/// struct used to reserve the full wall height for walls that lowered to
/// air, so the artifact was as tall as the building it did not contain.
fn max_wall_top(painting: &[(u32, u32)]) -> u32 {
    painting
        .iter()
        .map(|(y_offset, h)| y_offset.saturating_add(*h))
        .max()
        .unwrap_or(0)
}

/// The `(y_offset, height)` pairs of every walls member that will both
/// stand and paint.
///
/// One list behind both the volume and [`WallColumn`], because
/// `spec/compilation` "Level grouping and volume derivation" makes them two
/// readings of a single list and rests the "no member paints past the end of
/// the array" invariant on their agreeing. Filtering one and not the other
/// would leave the window carve writing rows the array no longer has.
/// `max_wall_top` collapses the pairs to their maximum, which sizes the volume
/// and seats the roof; the column keeps the spans, which is what decides
/// whether a rectangle cut into a wall lands in masonry.
fn painting_walls<'a>(
    flattened: &'a [(u32, &'a Member)],
    scope: Option<&'a ScopeResolution>,
    registry: Option<&'a dyn TargetRegistry>,
    theme_missing: bool,
) -> impl Iterator<Item = (u32, u32)> + 'a {
    flattened
        .iter()
        .filter(|(_, m)| matches!(m.role, MemberRole::Walls))
        .filter(move |(_, m)| member_will_paint(m, scope, registry, theme_missing))
        .filter_map(|(y_offset, m)| height_value(m).map(|h| (*y_offset, h)))
}

/// Largest `overhang=` across the roof members the pass will paint.
///
/// Two concerns over one walk, and they cover different members.
///
/// The **contribution** is filtered through [`roof_draws`], the same
/// question [`max_roof_extra_height`] asks: a roof that will not draw must
/// not widen the array. Without the filter it gave the footprint its eaves
/// and the height nothing, so the walls moved inward and a ring of air
/// surrounded them.
///
/// The **validation** is not filtered, because this is the only place
/// `overhang=` is read: a value nothing here looks at is a value nothing
/// anywhere reports. It walks the flattened list for the same reason — a
/// roof this function cannot see is a roof whose `overhang=` nothing
/// validates.
fn max_roof_overhang(flattened: &[(u32, &Member)], diagnostics: &mut Vec<Diagnostic>) -> u32 {
    flattened
        .iter()
        .map(|(_, m)| *m)
        .filter(|m| matches!(m.role, MemberRole::Roof))
        .filter_map(|m| {
            // The consequence differs by member, so it is worked out here
            // rather than written once: a roof that draws is in the build
            // flush with the wall line, and a roof that does not is
            // already earning its own finding from the envelope phase.
            let draws = roof_draws(m);
            let consequence = if draws.is_some() {
                "the roof is drawn flush with the wall line"
            } else {
                "this roof draws nothing either way — see the finding on the same line"
            };
            let read = nonneg_int_or_ignore(m, "overhang", consequence, diagnostics);
            draws.and(read)
        })
        .max()
        .unwrap_or(0)
}

/// Maximum vertical contribution from any roof member the pass will draw
/// ([`roof_draws`]). A roof that draws nothing contributes `0` here; its
/// `W_DEFERRED_MEMBER` warning fires later, during the envelope phase,
/// against the actual member span. Computing the dim from the inflated
/// roof bounding box (interior + 2 * overhang on each axis) keeps the math
/// consistent with each per-kind generator.
fn max_roof_extra_height(
    flattened: &[(u32, &Member)],
    interior_w: u32,
    interior_h: u32,
    overhang: u32,
) -> u32 {
    let roof_w = interior_w.saturating_add(overhang.saturating_mul(2));
    let roof_h = interior_h.saturating_add(overhang.saturating_mul(2));
    flattened
        .iter()
        .map(|(_, m)| *m)
        .filter(|m| matches!(m.role, MemberRole::Roof))
        .filter_map(|m| roof_draws(m).map(|k| roof_extra_height(k, m, roof_w, roof_h)))
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
            match member
                .ident_value("slope_to")
                .and_then(WallSide::from_ident)
            {
                Some(slope_to) => shed_extra_height(shed_slope_span(roof_w, roof_h, slope_to)),
                None => 0,
            }
        }
        RoofKind::Hip => hip_extra_height(roof_w, roof_h),
        RoofKind::Flat => flat_extra_height(),
    }
}

/// The kind this roof will actually draw with, or `None` when it will
/// draw nothing.
///
/// Not [`roof_kind_of`], which answers "does the kind table know this
/// name". A `shed` with no usable `slope_to=` is a name the table knows
/// and a member [`fill_roof_shed`] returns from without painting a voxel,
/// and [`roof_extra_height`]'s shed arm already gives it `0`. Both roof
/// walks go through here so the footprint and the height cannot disagree
/// about which roofs are real — the disagreement that let a roof widen an
/// array it put nothing in.
///
/// Silent, unlike [`shed_slope_to`]: this is the question, and the answer
/// is reported by the pass that tries to draw.
fn roof_draws(member: &Member) -> Option<RoofKind> {
    let kind = roof_kind_of(member)?;
    if matches!(kind, RoofKind::Shed)
        && member
            .ident_value("slope_to")
            .and_then(WallSide::from_ident)
            .is_none()
    {
        return None;
    }
    Some(kind)
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
/// asked for `2^33` — the outcome [`Member::nonneg_u32`] documents as the
/// reason it refuses rather than clamps.
fn height_value(member: &Member) -> Option<u32> {
    let raw = member.intent_state.get("height")?;
    match &raw.value.kind {
        ValueKind::Int(v) if *v > 0 => u32::try_from(*v).ok(),
        _ => None,
    }
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
/// `y="top"`-silently-becomes-`0` gap that the plain [`Member::nonneg_u32`]
/// return type could not.
///
/// Every caller keeps the `Deferred` contract: each one returns or
/// `continue`s, and the `void=` site reads for validation alone. A caller
/// that means to draw the member anyway wants
/// [`nonneg_int_or_ignore`], whose finding says so.
enum NonNegRead {
    Valid(u32),
    Absent,
    Deferred,
}

/// Read `key=` as a non-negative `u32` for a caller that carries on
/// without it.
///
/// [`NonNegRead::Deferred`] means "the caller must return", and
/// `W_DEFERRED_MEMBER` says the member did not lower. A caller that falls
/// back to a default and draws the member anyway needs the other report:
/// the value was unusable, and here is what was used instead.
/// `consequence` is that second half in the caller's own words, because
/// only the caller knows what its fallback does to the output — and, as
/// [`max_roof_overhang`] does, whether the member draws at all.
///
/// `None` covers "absent" as well as "unusable", because a caller with a
/// default treats them alike — the difference is only whether anything is
/// reported.
fn nonneg_int_or_ignore(
    member: &Member,
    key: &'static str,
    consequence: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<u32> {
    read_or_ignore(
        member,
        key,
        |kind| match kind {
            ValueKind::Int(v) => u32::try_from(*v).ok(),
            _ => None,
        },
        NONNEG_U32,
    )
    .unwrap_or_else(|unread| {
        diagnostics.push(unread.report(consequence));
        None
    })
}

/// What [`nonneg_int_or_ignore`] accepts, worded to complete "`key=` must
/// be …".
const NONNEG_U32: &str = "a non-negative integer that fits in u32";

/// Read `key=` through `read` for a caller that falls back to a default,
/// telling "absent" apart from "unreadable".
///
/// - `Ok(Some(v))`: the key is present and `read` accepted its value.
/// - `Ok(None)`: the key was not written; the caller applies its default
///   and nothing is reported.
/// - `Err(unread)`: the key was written and `read` refused it. This is the
///   unreadable value `spec/lint` "Error vs warning" reports as
///   `W_IGNORED_ARGUMENT`. The caller applies its default, or, where the
///   key decides whether the member is built at all, refuses the member
///   instead.
///
/// Every caller reports the [`UnreadArgument`], whether or not the member
/// then reaches the build: the value is unreadable wherever the member
/// ends up, and holding the finding back until a later refusal is repaired
/// only costs the author another compile to learn it. What differs is the
/// note, which [`UnreadArgument::report`] takes from the caller once it
/// knows: the default's effect on a member in the build, that the member
/// is not built either way, or that it was refused instead of given the
/// default. A member dropped before its reader runs at all (a level-scoped
/// roof, say) is not read, and so reports nothing.
///
/// `expected` completes "`key=` must be …".
fn read_or_ignore<'m, T>(
    member: &'m Member,
    key: &'static str,
    read: impl FnOnce(&'m ValueKind) -> Option<T>,
    expected: &'static str,
) -> Result<Option<T>, UnreadArgument> {
    let Some(raw) = member.intent_state.get(key) else {
        return Ok(None);
    };
    match read(&raw.value.kind) {
        Some(v) => Ok(Some(v)),
        None => Err(UnreadArgument {
            span: member_or_slot_span(member, raw),
            key,
            written: raw.value.describe(),
            expected,
        }),
    }
}

/// [`read_or_ignore`] for a key whose value is a bare identifier.
///
/// Which identifiers the caller accepts is still the caller's to say:
/// this only separates "not an identifier at all" (`half="bottom"`,
/// `facing=1`), which is an unreadable value, from an identifier the
/// caller does not support (`half=sideways`), which defers the member.
fn ident_or_ignore<'m>(
    member: &'m Member,
    key: &'static str,
    expected: &'static str,
) -> Result<Option<&'m str>, UnreadArgument> {
    read_or_ignore(
        member,
        key,
        |kind| match kind {
            ValueKind::Ident(name) => Some(name.as_str()),
            _ => None,
        },
        expected,
    )
}

/// A `key=` written with a value its reader cannot use — the third outcome
/// of [`read_or_ignore`], beside "read" and "not written".
///
/// Not yet a [`Diagnostic`]: its note says what became of the member, and
/// only the caller knows whether it reached the build.
#[derive(Debug)]
struct UnreadArgument {
    /// The value's own span, so the finding underlines what was written,
    /// as `check::arguments` does for the same code, and two unreadable
    /// keys on one line underline two places.
    span: Span,
    key: &'static str,
    /// The value as [`crate::ast::Value::describe`] renders it, so a quoted
    /// `"true"` reads as the string it is rather than as the word the
    /// author meant.
    written: String,
    /// What the reader accepts, worded to complete "`key=` must be …".
    expected: &'static str,
}

impl UnreadArgument {
    /// The `W_IGNORED_ARGUMENT` for this value, with `consequence` as its
    /// note.
    ///
    /// The primary stops at "the value was ignored" because whether the
    /// member is in the build is not a fact the reader has. The note
    /// carries it instead, in the caller's words: what the default did to
    /// the output when the member is built, that it is not built either way
    /// when a finding on the same line refused it, or that the finding on
    /// the same line refused it instead of giving it the default.
    fn report(self, consequence: &str) -> Diagnostic {
        let Self {
            span,
            key,
            written,
            expected,
        } = self;
        Diagnostic {
            code: DiagnosticCode::IgnoredArgument,
            span,
            primary: format!("`{key}=` must be {expected}, not {written}; the value was ignored"),
            notes: vec![DiagnosticNote {
                span: None,
                message: consequence.to_owned(),
            }],
            data: None,
        }
    }
}

fn nonneg_int_or_defer(
    member: &Member,
    key: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> NonNegRead {
    if !member.intent_state.contains_key(key) {
        return NonNegRead::Absent;
    }
    if let Some(v) = member.nonneg_u32(key) {
        NonNegRead::Valid(v)
    } else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!("`{key}=` must be a non-negative integer that fits in u32"),
        ));
        NonNegRead::Deferred
    }
}

pub(super) fn size_value(member: &Member, key: &str) -> Option<(u32, u32)> {
    let raw = member.intent_state.get(key)?;
    match &raw.value.kind {
        ValueKind::Size { w, h } => Some((w.get(), h.get())),
        _ => None,
    }
}

/// The voxel grid under construction, plus the two things the phase model
/// needs to know about the writes that built it.
///
/// **Who wrote each cell.** `spec/compilation` "Phase evaluation" promises
/// that a `window` written after a `roof` still lands as an opening, and only
/// says last-wins for "local overrides within the same phase". So a
/// cross-phase overwrite is the model working and a within-phase one is the
/// author's two members contesting a cell with nothing but line order to
/// separate them. The canvas can tell the two apart because it remembers the
/// writer, and reports the second kind rather than resolving it silently.
///
/// **Which palette slots a write has named.** An entry no voxel references
/// reaches the `.nbt`, the `resolved_ir_hash`, and `cairn info`'s per-entry
/// rows, so it is a block the tooling counts for a build that does not
/// contain it. There are two ways to get one and they are not the same
/// mistake: a generator claiming a material for geometry it never emits is
/// a bug in the generator, while a member painting a cell a later phase
/// then covers is the layering doing its job. [`prune_unreferenced`] drops
/// the second and leaves the first where the artifact still shows it,
/// which is why the flag is kept at all rather than deriving everything
/// from the finished grid.
///
/// Nothing paints through a `Canvas` directly: [`Self::for_member`] hands
/// out a [`MemberCanvas`], and that is the only type with a write on it.
struct Canvas {
    dims: Dims,
    voxels: Vec<PaletteIndex>,
    /// Per voxel: `0` while nothing has written it, and `n + 1` once the
    /// member at index `n` of the flattened paint list has.
    ///
    /// One `u32` rather than the `(phase, index)` pair this began as. The
    /// pair is 16 bytes at align 8, which over a volume at the
    /// [`MAX_STRUCTURE_VOLUME`] cap is 256 MB of bookkeeping against
    /// 32 MB of voxels — a cap chosen against the grid alone would no
    /// longer mean what it was set to mean. The phase comes from
    /// [`Self::phases`] instead, which is one entry per member rather
    /// than one per cell.
    owners: Vec<u32>,
    /// Phase of each member of the flattened paint list, by that member's
    /// index. `None` for the members no phase evaluates — they never
    /// paint, so the entry exists only to keep the indices aligned.
    phases: Vec<Option<Phase>>,
    /// Indexed by palette slot: `true` once a write has named that slot.
    painted: Vec<bool>,
    /// Indexed like [`Self::phases`]: `true` once that member has written
    /// a cell, whatever a later write did to it. How the pass knows which
    /// openings were cut, for the `connect` rows that anchor on them.
    wrote: Vec<bool>,
    /// `(overridden, overriding)` member pairs to the number of voxels the
    /// second took from the first. Insertion-ordered so the diagnostics
    /// come out in the order the phases discovered them, then sorted by
    /// span with the rest of the pass's findings.
    conflicts: IndexMap<(u32, u32), u32>,
}

impl Canvas {
    fn new(dims: Dims, phases: Vec<Option<Phase>>) -> Self {
        Self {
            dims,
            voxels: vec![PaletteIndex::AIR; dims.volume()],
            owners: vec![0; dims.volume()],
            wrote: vec![false; phases.len()],
            phases,
            painted: vec![false; 1],
            conflicts: IndexMap::new(),
        }
    }

    /// Borrow the canvas as the member at `index` of the flattened paint
    /// list sees it.
    ///
    /// The only way to reach a write. A generator cannot paint without a
    /// member to file the write under, and cannot file it under the wrong
    /// one by forgetting to update a field: [`run_phase`] opens the scope
    /// once per member and the borrow closes it.
    fn for_member(&mut self, index: u32) -> MemberCanvas<'_> {
        debug_assert!(
            (index as usize) < self.phases.len(),
            "member {index} is not in the flattened paint list this canvas was built for",
        );
        MemberCanvas {
            canvas: self,
            member: index,
        }
    }

    /// Whether two members of the flattened paint list are evaluated in
    /// one phase.
    ///
    /// A member with no phase never paints, so a `None` on either side
    /// describes a write that could not have happened; it answers `false`
    /// rather than letting two of them match each other.
    fn same_phase(&self, a: u32, b: u32) -> bool {
        matches!(
            (
                self.phases.get(a as usize).copied().flatten(),
                self.phases.get(b as usize).copied().flatten(),
            ),
            (Some(x), Some(y)) if x == y
        )
    }
}

/// The canvas as one member sees it: the same grid, with every write
/// already attributed.
struct MemberCanvas<'a> {
    canvas: &'a mut Canvas,
    member: u32,
}

impl MemberCanvas<'_> {
    /// Paint one voxel, claiming its palette slot only once the cell is
    /// known to exist.
    ///
    /// Every generator derives its coordinates from the same [`Dims`] the
    /// volume was allocated against, so a coordinate outside it means the
    /// dim math and the generator disagree. That disagreement is what a
    /// `roof` under `level y=0` used to be: a full set of stair entries
    /// reached the palette and not one stair reached the structure, with
    /// nothing said. The write is still clipped rather than panicking a
    /// release build, and the `debug_assert!` turns the disagreement into
    /// a test failure instead of a silent drop.
    fn paint(&mut self, pos: (u32, u32, u32), claim: impl FnOnce() -> PaletteIndex) {
        if !self.try_paint(pos, claim) {
            debug_assert!(
                false,
                "voxel {pos:?} lies outside {:?}; the dim math and the generator disagree",
                self.canvas.dims,
            );
        }
    }

    /// [`Self::paint`] for the one generator whose coordinate is the
    /// author's to get wrong: a `pressure_plate` anchors at `at=`, so an
    /// out-of-range cell there earns a diagnostic rather than an assertion.
    /// Answers whether the cell existed; `claim` runs only when it did,
    /// which is what keeps the palette free of entries no voxel names.
    fn try_paint(&mut self, pos: (u32, u32, u32), claim: impl FnOnce() -> PaletteIndex) -> bool {
        let member = self.member;
        let canvas = &mut *self.canvas;
        let Some(i) = canvas.dims.index(pos.0, pos.1, pos.2) else {
            return false;
        };
        let after = claim();
        let slot = usize::from(after.0);
        if canvas.painted.len() <= slot {
            canvas.painted.resize(slot + 1, false);
        }
        canvas.painted[slot] = true;
        if let Some(wrote) = canvas.wrote.get_mut(member as usize) {
            *wrote = true;
        }
        let before = canvas.voxels[i];
        canvas.voxels[i] = after;
        let previous = std::mem::replace(&mut canvas.owners[i], member + 1);
        // A write that changes nothing is not a contest: two `walls`
        // members of one material meeting at a corner leave the same block
        // whichever order they run in, and a `window` whose `repeat=` /
        // `step=` stamps overlap covers its own cells with its own
        // material.
        if before == after || previous == 0 {
            return true;
        }
        // The member check is the other half of that and is unreachable
        // from today's generators — none writes one cell to two different
        // blocks, so a same-member pair never gets past the line above.
        // It stays because the finding is a statement about two members:
        // reported against itself it would read "`roof` overwrites 3
        // voxels that `roof` painted", which is a generator emitting two
        // faces onto one cell and not something an author can move.
        let overridden = previous - 1;
        if overridden != member && canvas.same_phase(overridden, member) {
            *canvas.conflicts.entry((overridden, member)).or_default() += 1;
        }
        true
    }
}

fn fill_floor(ctx: &StructCtx<'_>, idx: PaletteIndex, canvas: &mut MemberCanvas<'_>) {
    let y = 0;
    for z_local in 0..ctx.interior_h {
        for x_local in 0..ctx.interior_w {
            let x = ctx.overhang + x_local;
            let z = ctx.overhang + z_local;
            canvas.paint((x, y, z), || idx);
        }
    }
}

fn fill_walls(
    ctx: &StructCtx<'_>,
    height: u32,
    y_offset: u32,
    idx: PaletteIndex,
    canvas: &mut MemberCanvas<'_>,
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
                canvas.paint((x, y, z), || idx);
            }
        }
    }
}

fn fill_roof(
    member: &Member,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(kind) = parse_roof_kind(member, diagnostics) else {
        return;
    };
    // Resolved behind [`roof_draws`], the gate the volume goes through, and
    // not before it. A `shed` with no usable `slope_to=` draws nothing, and
    // resolving it anyway reported a material for a roof that is not there:
    // `W_ABSTRACT_TOKEN_DEFERRED` saying it is built from its default block,
    // and `geometry_material_id`'s `W_DEFERRED_MEMBER` naming that block.
    // `parse_roof_kind` has read a kind, so that shed is the one roof turned
    // away here, and [`shed_slope_to`] is what says why.
    if roof_draws(member).is_none() {
        shed_slope_to(member, diagnostics);
        return;
    }
    let resolved = resolve_member_state(
        member,
        ctx.scope,
        ctx.registry,
        diagnostics,
        ctx.theme_missing,
    );
    let base_id = geometry_material_id(
        member,
        ctx.scope,
        resolved.as_ref(),
        kind.base_block_id(),
        &GeometryMemberDescription {
            subject: format!("`{}` roof", kind.name()),
            states_from: "the geometry",
            requires_stair: kind.paints_stairs(),
        },
        diagnostics,
    );

    match kind {
        RoofKind::Gable => fill_roof_gable(ctx, palette, canvas, base_id),
        RoofKind::Shed => fill_roof_shed(member, ctx, palette, canvas, diagnostics, base_id),
        RoofKind::Hip => fill_roof_hip(ctx, palette, canvas, base_id),
        RoofKind::Flat => fill_roof_flat(ctx, palette, canvas, base_id),
    }
}

/// What a geometry-driven member does with the material it is given.
///
/// Lets one function serve every such member: it can name the member and
/// say where its blockstates came from without knowing which one it is.
struct GeometryMemberDescription {
    /// How the member names itself in a message, already quoted: a roof
    /// renders as `` `gable` roof ``, an eave as `` eave `stair` ``.
    subject: String,
    /// Where its blockstates come from, as a noun phrase: a roof reads the
    /// geometry, an eave stair reads its own arguments.
    states_from: &'static str,
    /// Whether it attaches stair blockstates, and therefore needs a
    /// material from the stair family.
    requires_stair: bool,
}

/// The block id a geometry-driven member paints, given its resolved
/// `mat_slot=` binding.
///
/// One function for the roof and the eave stair: they take the same three
/// decisions on the same grounds, and a member that attaches blockstates
/// has the same obligations wherever it appears. `fallback` is the kind's
/// own id, used when the binding supplies nothing usable:
///
/// - no `mat_slot=` at all — silent, the member asked for nothing;
/// - a `mat_slot=` that resolved to nothing — the resolver already said why
///   (`E_UNRESOLVED_SLOT`, `E_UNKNOWN_ABSTRACT_TOKEN`, …) or the theme is
///   missing entirely (`W_NO_THEME_BOUND` fired against the struct), and the
///   `W_DEFERRED_MEMBER` here anchors the consequence to the member that
///   wears the fallback;
/// - a `mat_slot=` that resolved outside the stair family, when this member
///   needs one. `E_INCOMPATIBLE_MATERIAL`, which stops the build: a whole
///   block has nowhere to put `facing` / `half` / `shape`, and neither
///   answer available without the author is acceptable — attaching them
///   anyway writes a blockstate that does not exist, and substituting the
///   fallback builds the member out of a material nobody chose. The
///   fallback is still returned so the rest of the pass has a coherent
///   palette to finish with; nothing reaches disk, because the finding is
///   an error.
///
/// A resolved state that carries `properties` is reported and its id still
/// used — the member derives its own states and drops the binding's, which
/// is a smaller thing than the material being the wrong shape. Checked
/// after the family, because properties on an id the member is not going to
/// paint are nothing the author needs to hear about.
fn geometry_material_id<'a>(
    member: &Member,
    scope: Option<&ScopeResolution>,
    resolved: Option<&'a BlockState>,
    fallback: &'a str,
    shape: &GeometryMemberDescription,
    diagnostics: &mut Vec<Diagnostic>,
) -> &'a str {
    let Some(state) = resolved else {
        if member.mat_slot.is_some() {
            // The theme is named because it is the thing the author edits,
            // and because without it two placements under two themes write
            // the same sentence. This is the arm sibling-variant softening
            // reaches: with no `--edition` pin the resolver stays silent on
            // a slot only the sibling variant declares, so lowering is the
            // only reporter, and the member line is shared by every
            // placement of the `def`. Two broken themes then differ in
            // nothing at all, and fixing one of them changes no output.
            let under = scope
                .and_then(|scope| scope.bound_theme.as_deref())
                .map_or_else(String::new, |theme| format!(" under theme `{theme}`"));
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "{}'s `mat_slot=` did not resolve to a block id{under}; it falls back to `{fallback}`",
                    shape.subject,
                ),
            ));
        }
        return fallback;
    };
    let slot_value = scope
        .and_then(|scope| scope.members.get(&member.span.start))
        .and_then(|binding| binding.slot_value.as_ref());
    if shape.requires_stair && !is_stair(&state.id) {
        diagnostics.push(diag_incompatible_material(member, state, shape, slot_value));
        return fallback;
    }
    if !state.properties.is_empty() {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "{} derives its blockstates from {}; the `mat_slot=` binding to `{}[...]` also carried properties and was not applied verbatim",
                shape.subject,
                shape.states_from,
                state.id,
            ),
        ));
    }
    &state.id
}

/// A member whose geometry attaches blockstates was bound to a material
/// that cannot carry them.
///
/// Anchored on the theme's slot value when there is one, because that line
/// is where the fix goes — the member itself only names a slot, and every
/// member reading that slot has the same problem for the same reason.
fn diag_incompatible_material(
    member: &Member,
    state: &BlockState,
    shape: &GeometryMemberDescription,
    slot_value: Option<&ValueWithSpan>,
) -> Diagnostic {
    let mut notes = vec![DiagnosticNote {
        span: None,
        message: "bind the slot to a `*_stairs` material — the registry pack's \
                  `roof.dark_wood`, `roof.light_wood`, `roof.warm_wood`, and \
                  `roof.cool_wood` all resolve to one"
            .to_owned(),
    }];
    if let Some(slot) = &member.mat_slot {
        notes.push(DiagnosticNote {
            span: None,
            message: format!(
                "reached through `mat_slot={slot}`, so every member reading that slot has it too",
            ),
        });
    }
    Diagnostic {
        code: DiagnosticCode::IncompatibleMaterial,
        span: slot_value.map_or_else(|| member.span.clone(), |v| member_or_slot_span(member, v)),
        primary: format!(
            "{} derives `facing` / `half` / `shape` from {} and `{}` is not a stair, so those states have nowhere to go",
            shape.subject, shape.states_from, state.id,
        ),
        notes,
        data: Some(DiagnosticData::IncompatibleMaterial {
            id: state.id.clone(),
            required: "stair".to_owned(),
            slot: member.mat_slot.clone(),
            token: slot_value.and_then(|v| match &v.value.kind {
                ValueKind::Token(token) => Some(token.clone()),
                // Anything else here was already refused upstream
                // (`E_UNKNOWN_SLOT_TARGET`), so there is no token to name.
                _ => None,
            }),
        }),
    }
}

fn fill_roof_gable(
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    base_id: &str,
) {
    let roof_w = ctx.dims.x;
    let roof_h = ctx.dims.z;
    let ridge_axis = gable_ridge_axis(roof_w, roof_h);
    // One `palette.intern` per face rather than one per voxel — a 99-voxel
    // cottage roof needs four — but claimed on first use, not up front. An
    // odd ridge span converges on a single row, so a roof that has one (a
    // 3x3 struct) emits three of the five faces and never the apex pair,
    // and an entry no voxel references is not free: it reaches the `.nbt`
    // palette, the `resolved_ir_hash`, and the per-entry counts `cairn info`
    // reports. Claiming on demand also means the palette lays out in the
    // order [`gable_voxels`] first visits each face, which is fixed by the
    // generator's layer iteration, so the layout stays deterministic.
    let mut face_indices: [Option<PaletteIndex>; 5] = [None; 5];
    for GableVoxel { pos, face } in gable_voxels(roof_w, roof_h, ctx.wall_top) {
        let slot = match face {
            StairFace::LowSlope => 0,
            StairFace::HighSlope => 1,
            StairFace::Apex => 2,
            StairFace::ApexLow => 3,
            StairFace::ApexHigh => 4,
        };
        canvas.paint(pos, || {
            *face_indices[slot].get_or_insert_with(|| {
                let mut state = gable_stair_state(ridge_axis, face);
                base_id.clone_into(&mut state.id);
                palette.intern(state)
            })
        });
    }
}

fn fill_roof_shed(
    member: &Member,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
    base_id: &str,
) {
    let Some(slope_to) = shed_slope_to(member, diagnostics) else {
        return;
    };
    // Each face's slot is claimed the first time a voxel needs it, for the
    // reason spelled out in [`fill_roof_gable`]. The face that goes missing
    // here is the slope, not the apex: the layer count is the slope span
    // floored at one, and the topmost layer is always the apex, so a shed
    // one deep is a single apex row with no slope under it.
    let mut face_indices: [Option<PaletteIndex>; 2] = [None; 2];
    for ShedVoxel { pos, face } in shed_voxels(ctx.dims.x, ctx.dims.z, ctx.wall_top, slope_to) {
        let slot = match face {
            ShedFace::Slope => 0,
            ShedFace::Apex => 1,
        };
        canvas.paint(pos, || {
            *face_indices[slot].get_or_insert_with(|| {
                let mut state = shed_stair_state(slope_to, face);
                base_id.clone_into(&mut state.id);
                palette.intern(state)
            })
        });
    }
}

fn fill_roof_hip(
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    base_id: &str,
) {
    let roof_w = ctx.dims.x;
    let roof_h = ctx.dims.z;
    // Hip and gable share the same long-axis-wins-with-x-tiebreak ridge rule
    // (`spec/compilation` "Hip roof voxel rules" falls through to "Gable roof
    // voxel rules"). Reusing `gable_ridge_axis` keeps the two paths from
    // drifting if the tiebreak rule ever changes.
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
        canvas.paint(pos, || {
            let mut state = hip_stair_state(ridge_axis, face);
            base_id.clone_into(&mut state.id);
            palette.intern(state)
        });
    }
}

fn fill_roof_flat(
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    base_id: &str,
) {
    // A deck is one material, so a single slot serves the whole layer — but
    // it is still claimed from inside the first write rather than before
    // the loop, so the rule "no entry without a voxel" holds at every
    // generator rather than at three of the four.
    let mut deck_idx: Option<PaletteIndex> = None;
    for (x, y, z) in flat_voxels(ctx.dims.x, ctx.dims.z, ctx.wall_top) {
        canvas.paint((x, y, z), || {
            *deck_idx.get_or_insert_with(|| {
                let mut deck_state = flat_block_state();
                base_id.clone_into(&mut deck_state.id);
                palette.intern(deck_state)
            })
        });
    }
}

/// Resolve a `roof` member's `kind=` to a [`RoofKind`].
///
/// Pushes a `W_DEFERRED_MEMBER` warning and returns `None` when the
/// `kind=` is missing, typed wrong, or names a kind outside the supported
/// set. Keeping the dispatch table in [`RoofKind::from_ident`] and the
/// diagnostic phrasing here lets each side stay self-contained.
fn parse_roof_kind(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<RoofKind> {
    let Some(raw) = member.ident_value("kind") else {
        // A value of the wrong shape is named by its shape, not by the
        // closed set: `kind="shed"` spells a kind that is in the set, and
        // being told the set again answers a question the author did not
        // ask. `recognize_circuit_region` words the same case the same way.
        // It matters more since `check::arguments` began deferring to this
        // finding — a `kind=` naming no rule is where the conditional
        // argument check stays quiet, so this is the whole repair.
        let reason = match member.intent_state.get("kind") {
            Some(raw) => format!(
                "roof `kind=` must be an identifier naming one of {}, got {}",
                RoofKind::names(),
                raw.value.kind_name(),
            ),
            None => format!("missing `kind=` (expected one of {})", RoofKind::names()),
        };
        diagnostics.push(diag_deferred_member_reason(member, &reason));
        return None;
    };
    if let Some(k) = RoofKind::from_ident(raw) {
        return Some(k);
    }
    diagnostics.push(diag_deferred_member_reason(
        member,
        &format!(
            "unknown roof `kind={raw}` (expected one of {})",
            RoofKind::names(),
        ),
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
    let Some(raw) = member.ident_value("slope_to") else {
        let reason = match member.intent_state.get("slope_to") {
            Some(written) => format!(
                "shed `slope_to=` must be one of front, back, left, right, not {}",
                written.value.describe(),
            ),
            None => "shed roof requires `slope_to=` (one of front, back, left, right)".to_owned(),
        };
        diagnostics.push(diag_deferred_member_reason(member, &reason));
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
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(side) = side_of(member, diagnostics) else {
        return;
    };
    let len = wall_length(side, ctx.interior_w, ctx.interior_h);
    // `at=` is read by `super::walkway::door_anchor_offset`, the function
    // a `connect` port on this door reads it with, and a refusal is worded
    // by `door_at_deferral` from the same classification the port's note
    // uses — so the cut, the walkway and both messages cannot disagree
    // about what `at=` says. Asked before the wall below, the way
    // `fill_window` reads its rectangle before asking where the masonry
    // is, so an `at=` typo is reported as one on a body whose walls are
    // also wrong.
    let at = match door_anchor_offset(member, len) {
        Ok(at) => at,
        Err(written) => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &door_at_deferral(&written),
            ));
            return;
        }
    };
    // Gate and cap both come from the course holding the row this door
    // opens at — `y_offset + 1`, one above the level's base plane, which
    // the floor slab owns — and never from the struct's tallest wall
    // row: what stands over a shorter course is the gap above it or the
    // roof the envelope phase wrote at `wall_top + 1`, so a door judged
    // against `wall_top` carved that air, and one gated on "does the
    // column hold anything" carved it without a word. `fill_window` asks
    // the same column where its rectangle lands, so the two members
    // answer one question.
    let base_row = y_offset.saturating_add(1);
    let Some(course_top) = ctx.wall_column.course_top_at(base_row) else {
        // "Nowhere" and "not here" are different findings. An empty
        // column means no `walls` member paints at all — which a
        // positive `height=` does not settle, since a `mat_slot=` that
        // does not resolve empties it too, and naming only the height
        // would send an author who wrote `height=4` over a themeless
        // struct to the wrong line. Same split as `fill_window`'s.
        let reason = if ctx.wall_column.is_empty() {
            "door requires a `walls` member that paints — a positive `height=` and a `mat_slot=` that resolves — to carve into".to_owned()
        } else {
            format!(
                "door opens at y={base_row}, which is not inside any wall course (the walls occupy {})",
                ctx.wall_column,
            )
        };
        diagnostics.push(diag_deferred_member_reason(member, &reason));
        return;
    };
    // The door block itself (`oak_door`, hinge / half / facing / open) is
    // not yet placed; that landed deferred along with per-theme door
    // materials.
    let door_height = course_top
        .saturating_sub(base_row)
        .saturating_add(1)
        .min(DOOR_HEIGHT);
    for v_local in 0..door_height {
        let v = base_row.saturating_add(v_local);
        let Some((x, y, z)) = wall_local_to_grid(
            side,
            at,
            v,
            ctx.overhang,
            ctx.interior_w,
            ctx.interior_h,
            ctx.dims,
        ) else {
            // INVARIANT(wall-grid-validated): `at` is below `len` by the
            // match above, and every row the loop asks for is at most
            // `course_top` because `door_height` is clamped to the course
            // holding `base_row` — so the helper has nothing to reject.
            // Reaching here means one of those two stopped agreeing with
            // `wall_local_to_grid`'s `u < length` / `v < dims.y`. Loud in
            // debug builds; release builds skip the cell rather than panic
            // over one voxel.
            debug_assert!(
                false,
                "door row y={v} at u={at} on the {} wall is outside the wall grid; \
                 `at=` against `wall_length` / `door_height` against the course \
                 stopped agreeing with `wall_local_to_grid`",
                side_name(side),
            );
            continue;
        };
        canvas.paint((x, y, z), || PaletteIndex::AIR);
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
fn fill_stair(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // A value that is not an identifier at all (`half="bottom"`,
    // `facing=1`) is unreadable rather than unsupported: the band is drawn
    // with the default state. Read before anything can refuse the stair,
    // so every unreadable state is reported in the same compile as the
    // refusal, with a note that says which of the two happened.
    let mut unread = Vec::new();
    let mut ident = |key: &'static str, expected: &'static str, default: &'static str| {
        ident_or_ignore(member, key, expected).unwrap_or_else(|argument| {
            unread.push((argument, default));
            None
        })
    };
    let states = EaveStates {
        facing: ident("facing", "`out` or `in`", "out"),
        half: ident("half", "`top` or `bottom`", "top"),
        shape: ident(
            "shape",
            "`straight`, `outer_left`, or `outer_right`",
            "straight",
        ),
    };
    let built = draw_eave_band(member, states, y_offset, ctx, palette, canvas, diagnostics);
    for (argument, default) in unread {
        let consequence = if built {
            format!(
                "the stair is built with the default `{}={default}`",
                argument.key
            )
        } else {
            "this stair is not built either way — see the finding on the same line".to_owned()
        };
        diagnostics.push(argument.report(&consequence));
    }
}

/// The three state arguments of an eave `stair`, as identifiers: `None`
/// is a key not written, or written with a value [`fill_stair`] could not
/// read, and either way takes the default.
#[derive(Debug, Clone, Copy)]
struct EaveStates<'m> {
    facing: Option<&'m str>,
    half: Option<&'m str>,
    shape: Option<&'m str>,
}

/// [`fill_stair`]'s refusals and paint. Returns whether the band was drawn,
/// which is what the note on an unreadable state has to say.
#[allow(clippy::too_many_lines)] // one linear defer-and-paint chain reads better than 6 tiny helpers
fn draw_eave_band(
    member: &Member,
    states: EaveStates<'_>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let Some(raw_kind) = member.ident_value("kind") else {
        let reason = if member.intent_state.contains_key("kind") {
            "stair `kind=` must be `stairs`"
        } else {
            "stair without `kind=` is not yet supported (currently only `kind=stairs`)"
        };
        diagnostics.push(diag_deferred_member_reason(member, reason));
        return false;
    };
    if raw_kind != "stairs" {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!("stair `kind={raw_kind}` is not yet supported (currently only `kind=stairs`)"),
        ));
        return false;
    }
    let Some(side) = side_of(member, diagnostics) else {
        return false;
    };
    let half = match states.half {
        Some("top") | None => "top",
        Some("bottom") => "bottom",
        Some(other) => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!("stair `half={other}` is not yet supported (use `top` or `bottom`)"),
            ));
            return false;
        }
    };
    let facing = match states.facing {
        Some("out") | None => shed_high_side(side),
        Some("in") => inward_cardinal(side),
        Some(other) => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!("stair `facing={other}` is not yet supported (use `out` or `in`)"),
            ));
            return false;
        }
    };
    let shape = match states.shape {
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
            return false;
        }
    };
    // The gate is the overhang the roof *draws*, not the `overhang=` the
    // author wrote: a roof that will not draw contributes none, so a
    // struct with `overhang=2` and no `kind=` arrives here at zero.
    // Naming only the key would send that author to the wrong line.
    if ctx.overhang == 0 {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "eave `stair` needs a roof that draws an overhang of at least 1 so the band can sit outside the wall — no roof on this struct contributes one",
        ));
        return false;
    }
    let y_local = match nonneg_int_or_defer(member, "y", diagnostics) {
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 0,
        NonNegRead::Deferred => return false,
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
        return false;
    }
    // An eave band is a row of stairs whose `facing` / `half` / `shape`
    // come from the member's own arguments rather than from a slope, but
    // they are states this pass attaches to whatever it paints just the
    // same — so the material has to be able to carry them.
    let resolved = resolve_member_state(
        member,
        ctx.scope,
        ctx.registry,
        diagnostics,
        ctx.theme_missing,
    );
    let stair_id = geometry_material_id(
        member,
        ctx.scope,
        resolved.as_ref(),
        STAIR_BASE_ID,
        &GeometryMemberDescription {
            subject: "eave `stair`".to_owned(),
            states_from: "its own arguments",
            // An eave is a row of stairs by construction — `fill_stair`
            // refuses any `kind=` but `stairs` well above here.
            requires_stair: true,
        },
        diagnostics,
    );
    let idx = palette.intern(stair_state(stair_id, facing, half, shape));
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
            // INVARIANT(wall-grid-validated): `u` walks `0..length`, the
            // same length the helper checks, and any `y_world >= dims.y`
            // was refused above with a diagnostic — so the helper has
            // nothing to reject. Reaching here means one of those stopped
            // agreeing with `wall_local_to_grid`. Loud in debug builds;
            // release builds skip the cell rather than panic.
            debug_assert!(
                false,
                "eave stair cell u={u} y={y_world} on the {} wall is outside the wall grid; \
                 the band's `u < wall_length` / `y_world < dims.y` checks stopped agreeing \
                 with `wall_local_to_grid`",
                side_name(side),
            );
            continue;
        };
        let (x, z) = shift_outward(side, wx, wz);
        canvas.paint((x, y_world, z), || idx);
    }
    true
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
    let (dx, dz) = side.outward_normal();
    (x.saturating_add_signed(dx), z.saturating_add_signed(dz))
}

/// Shift a wall voxel's `(x, z)` by one voxel toward the interior, the
/// cell an `at=inside.<side>` fixture sits on at whatever row it asks for.
///
/// The step saturates at 0 and never checks the result: whether the cell
/// is really inside the building, rather than a block of another wall, is
/// [`inside_plate_refusal`]'s question, asked by the caller.
fn shift_inward(side: WallSide, x: u32, z: u32) -> (u32, u32) {
    let (dx, dz) = side.outward_normal();
    (x.saturating_add_signed(-dx), z.saturating_add_signed(-dz))
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
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
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
    let Some(base_id) = plate_id_for_member(member, ctx, diagnostics) else {
        return;
    };
    // Interned only once the cell is known to exist. `plate_voxel_position`
    // already rejects every out-of-range anchor it can name, so this arm is
    // the guard against a future one it cannot — and a guard that leaves a
    // palette entry behind reports a block the structure does not contain.
    if !canvas.try_paint((x, y_world, z), || {
        palette.intern(BlockState::bare(base_id))
    }) {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "pressure_plate resolved to a voxel outside the struct's block array",
        ));
    }
}

/// Resolve a `pressure_plate` anchor + `offset=` + `y=` into the world
/// voxel `(x, y, z)` the plate should paint onto, or `None` (with a
/// diagnostic already pushed) when any of the inputs is missing / out
/// of range / lands outside the block array. The one `None` without a
/// diagnostic is the `INVARIANT(wall-grid-validated)` arm, which the
/// checks before it make unreachable and which asserts in debug builds.
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
/// `inside.<side>` always shifts one voxel inward and defers unless the
/// shifted cell is strictly inside the wall ring on both horizontal axes
/// (see [`inside_plate_refusal`]), at every row including the floor row:
/// otherwise the plate would sit on the ring, where the walls paint their
/// courses, or outside the building altogether.
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
        // INVARIANT(wall-grid-validated): the two refusals above
        // (`y_world >= dims.y`, `offset >= length`) each pushed their own
        // diagnostic and returned, and they are every rejection
        // `wall_local_to_grid` performs — so it has nothing to reject.
        // Reaching here means one of them stopped agreeing with the
        // helper, which is a compiler bug, not something the author can
        // act on, so it is an assertion rather than a warning. Loud in
        // debug builds; release builds drop the plate.
        debug_assert!(
            false,
            "pressure_plate cell u={offset} y={y_world} on the {} wall is outside the wall grid; \
             the `offset < wall_length` / `y_world < dims.y` checks stopped agreeing with \
             `wall_local_to_grid`",
            side_name(side),
        );
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
                        "pressure_plate `at={}.outside` at y={y_world} has no exterior voxel to sit on (no roof on this struct draws an overhang; the foundation fallback only applies at y=0 so a higher plate would overwrite the wall)",
                        side_name(side),
                    ),
                ));
                None
            }
        }
        PlateAnchor::Inside(_) => {
            let (sx, sz) = shift_inward(side, wx, wz);
            if let Some(mut reason) = inside_plate_refusal(side, offset, y_world, (sx, sz), ctx) {
                if member.binding.is_some() {
                    // The redstone pass reads the binding, not the block
                    // array, so the signal outlives the refused block.
                    reason.push_str(
                        ". Its signal binding still reaches the netlist, with no plate placed to drive it",
                    );
                }
                diagnostics.push(diag_deferred_member_reason(member, &reason));
                return None;
            }
            Some((sx, y_world, sz))
        }
    }
}

/// Why an `at=inside.<side>` plate whose inward step landed on `(sx, sz)`
/// at row `y` has no interior cell to sit on, or `None` when it has one.
///
/// The interior is what the wall ring encloses. The ring is the
/// footprint's outermost row and column, offset by the roof overhang, and
/// it is where `walls` paint their courses, so only a cell strictly inside
/// it can take a plate without replacing a wall block. The ring is decided
/// from the footprint whether or not a `walls` member paints it, so a cell
/// on or outside it is refused either way, and the reason claims a wall
/// only at a row one is painted on.
fn inside_plate_refusal(
    side: WallSide,
    offset: u32,
    y: u32,
    (sx, sz): (u32, u32),
    ctx: &StructCtx<'_>,
) -> Option<String> {
    // Saturation cannot turn an outside cell into an inside one: that
    // needs `overhang + extent > u32::MAX`, where `dims` saturates too and
    // `fits_volume_budget` has already dropped the struct.
    let strictly_inside = |c: u32, extent: u32| {
        c > ctx.overhang && c < ctx.overhang.saturating_add(extent).saturating_sub(1)
    };
    if strictly_inside(sx, ctx.interior_w) && strictly_inside(sz, ctx.interior_h) {
        return None;
    }
    let name = side_name(side);
    let length = wall_length(side, ctx.interior_w, ctx.interior_h);
    // An interior needs one voxel between two walls on each axis, so a
    // size below 3 on either axis has none for any side or offset.
    let thin: Vec<(&str, u32)> = [("w", ctx.interior_w), ("h", ctx.interior_h)]
        .into_iter()
        .filter(|&(_, extent)| extent < 3)
        .collect();
    // With both axes at least 3, every offset but the two ends steps into
    // the interior, so a refusal with no thin axis is always a corner.
    let corner = thin.is_empty() || offset == 0 || offset == length - 1;
    let valid_offsets = if length >= 3 {
        format!("from 1 to {}", length - 2)
    } else {
        "away from both ends of the wall".to_owned()
    };
    if thin.is_empty() {
        let inside_it = if ctx.wall_column.course_top_at(y).is_some() {
            "belongs to the neighbouring wall"
        } else {
            "is not an interior cell of this struct"
        };
        return Some(format!(
            "pressure_plate `at=inside.{name} offset={offset}` is at a corner of the {name} wall, \
             so the voxel inside it {inside_it}; use an `offset=` {valid_offsets} to reach an \
             interior voxel",
        ));
    }
    let also_corner = if corner {
        format!(", and `offset={offset}` is at a corner of the {name} wall besides")
    } else {
        String::new()
    };
    let sizes = thin
        .iter()
        .map(|(axis, extent)| format!("size.{axis} is {extent}"))
        .collect::<Vec<_>>()
        .join(" and ");
    let grow = thin
        .iter()
        .map(|(axis, _)| format!("`size.{axis}` to at least 3"))
        .collect::<Vec<_>>()
        .join(" and ");
    let use_offset = if corner {
        format!(" and use an `offset=` {valid_offsets}")
    } else {
        String::new()
    };
    // Without an overhang `<side>.outside` finds no exterior cell above
    // the floor row and, at it, falls back to the cell under the wall, so
    // it is a remedy only when a roof draws an overhang ring to sit in.
    let outside = if ctx.overhang > 0 {
        format!(", or anchor the plate with `at={name}.outside`")
    } else {
        String::new()
    };
    Some(format!(
        "pressure_plate `at=inside.{name} offset={offset}`: the struct's {sizes}, so it has no \
         interior voxel{also_corner}; an interior needs a size of at least 3 on both axes. \
         Grow {grow}{use_offset}{outside}",
    ))
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
fn plate_id_for_member(
    member: &Member,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    let resolved = resolve_member_state(
        member,
        ctx.scope,
        ctx.registry,
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
    match resolved {
        Some(state) => Some(state.id),
        None => pack_plate_default(member, ctx.registry, diagnostics),
    }
}

/// The pressure plate id for this compile's edition, checked against the
/// target the way an authored id is.
///
/// Asks the pack for [`PRESSURE_PLATE_TOKEN`] and falls back to
/// [`PRESSURE_PLATE_BASE_ID`] when there is no pack to ask, or when the
/// pack declares no such row. A pack that declares it wins, which is how
/// `--edition bedrock` stops emitting the Java-only `oak_pressure_plate`.
///
/// The result goes through [`validated_id`] because it reaches the palette
/// without passing [`resolve_block_state`]. Skipping it would leave one
/// path in the build — the one whose default is edition-specific and
/// whose fallback is a Java spelling — writing an id nothing verified,
/// which is the failure this whole pass exists to remove. Returns `None`
/// after diagnosing, so the plate is dropped rather than painted with an
/// id the target has no block for.
fn pack_plate_default(
    member: &Member,
    registry: Option<&dyn TargetRegistry>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<String> {
    let (state, origin) = match registry.and_then(|r| r.lookup(PRESSURE_PLATE_TOKEN)) {
        Some(state) => (
            state,
            IdOrigin::Catalog {
                token: PRESSURE_PLATE_TOKEN.to_owned(),
            },
        ),
        None => (
            BlockState::bare(PRESSURE_PLATE_BASE_ID),
            IdOrigin::Builtin {
                token: PRESSURE_PLATE_TOKEN,
            },
        ),
    };
    match validated_id(state, registry, &origin) {
        Ok(state) => Some(state.id),
        Err(MaterialDeferred::UnknownId(unknown)) => {
            diagnostics.push(diag_unknown_id(member.span.clone(), &unknown));
            None
        }
        // INVARIANT(validated-id-refuses-one-way): `validated_id` returns either
        // the state or `UnknownId`; it has no path to the three deferral
        // variants, which describe how a `mat_slot=` value failed to
        // resolve and there is no such value here.
        Err(other) => {
            debug_assert!(
                false,
                "validated_id returned {other:?} for the pressure plate default, \
                 which resolves no mat_slot= value",
            );
            None
        }
    }
}

/// Recognise a `circuit region=<label> void=<N>` fixture without emitting
/// voxels.
///
/// `circuit` reserves a routing region for the future
/// `logic_synth → logic_place → logic_route` passes (`spec/redstone`
/// "Place-and-route" and "Connection to the IR and phases"). Nothing lands in
/// the block array at this stage — the physical dust / repeater / cell tiles
/// are decided by the logic layer, which is not part of block-array lowering
/// yet. Recognising the shape
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
/// matching keyword in the role table first (`spec/redstone` "Signal
/// binding").
const ACTUATOR_PATCH_INTENT_KEYS: &[&str] = &["opened_by"];

/// A door member is an **actuator patch** when its surface line uses the
/// selector form (`door[…] …`). The bracketed selector references an
/// already-declared physical door by `id=`; its role in block-array
/// lowering is pure metadata (an `opened_by=` signal binding for the
/// future redstone lowering pipeline, `spec/redstone` "Signal binding").
/// Routing patch lines through `carve_door` would false-positive `side_of`'s
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
/// - `opened_by=` must be present. `spec/redstone` "Signal binding" also
///   defines `lit_by=` on lamps, `powered_by=` on pistons, and
///   `fired_by=` on dispensers, but those keywords are not yet in the
///   role table — door + `opened_by` is the only shape
///   `redstone-door.crn` exercises today. Any other intent-state key
///   defers with a primary that names the offending key so a future
///   `powered_by=` implementation cannot silently change the meaning
///   of existing source (see [`ACTUATOR_PATCH_INTENT_KEYS`]).
/// - The `opened_by=` value must be a two-segment `sig.<name>`
///   `DotRef`. Non-`DotRef` values defer with a "got `<kind>`" primary;
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
    // Only keys a `door` does carry. One outside the role's vocabulary is
    // `check::arguments`' `E_UNKNOWN_ARGUMENT`, which names it, offers the
    // word it may be a typo for, and reaches an unpinned `cairn check`
    // that runs no lowering at all — so repeating it here would bill one
    // repair twice. What is left for this recogniser is the key that
    // passes a `door`'s vocabulary and still has no reader in the patch,
    // `door[side=front]` being the shape.
    let vocabulary = member.role.accepted_arguments().unwrap_or_default();
    let unknown_selector_keys: Vec<&str> = selector
        .keys()
        .map(String::as_str)
        .filter(|k| !ACTUATOR_PATCH_SELECTOR_KEYS.contains(k) && vocabulary.contains(k))
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
    // Which door the brackets pick is read from the one place the
    // redstone front end reads it too, so the patch this defers is the
    // patch that gets no port there.
    if let Err(reason) = actuator_patch_target(member, siblings.iter().map(|&(_, m)| m)) {
        let mut diagnostic = diag_deferred_member_reason(member, &reason.to_string());
        if let Some(fix) = patch_target_fix(&reason) {
            diagnostic.notes.push(DiagnosticNote {
                span: None,
                message: fix,
            });
        }
        diagnostics.push(diagnostic);
        return;
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
                "door actuator patch accepts only `opened_by=sig.<name>` today; unknown \
                 attribute(s): {} (spec/redstone \"Signal binding\" reserves `lit_by=` / \
                 `powered_by=` / `fired_by=` for future keywords)",
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
        ValueKind::DotRef(dotref) if dotref.head() == SIGNAL_HEAD && dotref.tail().len() == 1 => {}
        ValueKind::DotRef(dotref) if dotref.head() == SIGNAL_HEAD => {
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

fn fill_window(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    // An unreadable `sym=` (`sym=yes`, `sym="true"`) draws the window
    // unmirrored, except on a window with `repeat=` greater than 1, which
    // `sym=true` refuses: that one is refused. Read before anything can
    // refuse the window, so it is reported in the same compile as a
    // refusal, with a note that says which of the three [`WindowCut`]
    // outcomes happened.
    let (sym, sym_unread) = match read_or_ignore(
        member,
        "sym",
        |kind| match kind {
            ValueKind::Bool(b) => Some(*b),
            _ => None,
        },
        "`true` or `false`",
    ) {
        Ok(sym) => (Some(sym.unwrap_or(false)), None),
        Err(unread) => (None, Some(unread)),
    };
    let cut = cut_window(member, sym, y_offset, ctx, palette, canvas, diagnostics);
    diagnostics.extend(sym_unread.map(|unread| {
        unread.report(match cut {
            WindowCut::Cut => {
                "the window is drawn without its mirror, as `sym=false` would draw it"
            }
            WindowCut::Refused => {
                "this window is not cut either way — see the finding on the same line"
            }
            WindowCut::RefusedForUnreadSym => {
                "this window has `repeat=` greater than 1, which `sym=true` does not yet \
                 support, so it is not cut on the `sym=false` default while `sym=` is \
                 unreadable — see the finding on the same line"
            }
        })
    }));
}

/// What [`cut_window`] did with the window, which is what the note on an
/// unreadable `sym=` has to say.
#[derive(Clone, Copy)]
enum WindowCut {
    /// The primary rectangle was cut.
    Cut,
    /// The window was refused, and another finding says why. When its
    /// `sym=` is unreadable, that reason holds whatever `sym=` had said.
    Refused,
    /// The window has `repeat=` greater than 1, which `sym=true` refuses,
    /// and its `sym=` is unreadable: it was refused instead of carried on
    /// with the `sym=false` default, which the source may not mean.
    RefusedForUnreadSym,
}

/// [`fill_window`]'s refusals and paint, with `sym=` already read: `None`
/// when the value written is unreadable, in which case the window is
/// painted unmirrored, or refused if its `repeat=` is greater than 1, which
/// `sym=true` refuses.
#[allow(clippy::too_many_lines)] // one linear parse-and-paint chain reads better than 6 tiny helpers
fn cut_window(
    member: &Member,
    sym: Option<bool>,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> WindowCut {
    let Some(side) = side_of(member, diagnostics) else {
        return WindowCut::Refused;
    };
    // `offset=` defaults to 0 (the wall-local axis origin) when absent, so a
    // decorative repeat=N series can be authored as `window ... repeat=N
    // step=M size=WxH` without a redundant `offset=0`; `y=` and `size=` are
    // required. A key that is present but ill-shaped defers, with a reason
    // that says so rather than calling it missing. Read by
    // `super::walkway::read_window_args`, which a `connect` port on this
    // window reads the same three with.
    let WindowArgs {
        offset,
        y: y_start_local,
        width: sw,
        height: sh,
    } = match read_window_args(member) {
        Ok(args) => args,
        Err(fault) => {
            diagnostics.push(diag_deferred_member_reason(member, &fault.deferral()));
            return WindowCut::Refused;
        }
    };
    let y_start = y_start_local.saturating_add(y_offset);
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
            return WindowCut::Refused;
        }
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 1,
        NonNegRead::Deferred => return WindowCut::Refused,
    };
    let step = match nonneg_int_or_defer(member, "step", diagnostics) {
        NonNegRead::Valid(v) => v,
        NonNegRead::Absent => 0,
        NonNegRead::Deferred => return WindowCut::Refused,
    };
    // Before `sym=`: a series without a positive `step=` is refused
    // whatever `sym=` says, so an unreadable `sym=` is not why it is not
    // cut, and the author hears about `step=` now rather than after
    // repairing `sym=`.
    if repeat > 1 && step == 0 {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "window `repeat=` requires a positive `step=` so instances do not overlap",
        ));
        return WindowCut::Refused;
    }
    if repeat > 1 {
        match sym {
            Some(false) => {}
            Some(true) => {
                diagnostics.push(diag_deferred_member_reason(
                    member,
                    "window with both `repeat=` and `sym=true` is not yet supported",
                ));
                return WindowCut::Refused;
            }
            // `sym=true` refuses a window with `repeat=` greater than 1, so
            // carrying on with the `sym=false` default could build a window
            // the source refused.
            None => {
                diagnostics.push(diag_deferred_member_reason(
                    member,
                    "window with `repeat=` greater than 1 is not cut while its `sym=` is \
                     unreadable: `sym=true` with such a `repeat=` is not yet supported, and \
                     `sym=false` is not what was written",
                ));
                return WindowCut::RefusedForUnreadSym;
            }
        }
    }
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
        return WindowCut::Refused;
    }
    // A window is a rectangle cut into a wall, so every row it cuts has
    // to be a row some `walls` member painted — not merely a row below
    // the roof. The three ways to leave the masonry all cut silently
    // before this asked the whole question: below the first course the
    // rectangle carves through the floor slab, above the top course it
    // punches through the roof voxels the envelope phase wrote at
    // `y = wall_top + 1`, and between two `level` courses it hangs its
    // glass in open air.
    if !ctx.wall_column.contains_rows(y_start, sh) {
        let reason = if ctx.wall_column.is_empty() {
            // The column holds the rows the walls will *paint*, so an
            // empty one means "no walls member puts a block anywhere" —
            // a `mat_slot=` that does not resolve empties it as surely
            // as a missing `height=` does, and naming only the height
            // sends an author who wrote one to the wrong line. Same
            // reason `carve_door`'s gate says the same thing.
            format!(
                "window at y={y_start} size={sw}x{sh} has no wall to cut into (this struct declares no `walls` that paints — one with a positive `height=` and a `mat_slot=` that resolves)",
            )
        } else if let Some(last) = y_start.checked_add(sh.saturating_sub(1)) {
            format!(
                "window rows y={y_start}..={last} are not all inside one wall course (size={sw}x{sh}; the walls occupy {})",
                ctx.wall_column,
            )
        } else {
            // Saturating here printed `y=4294967295..=4294967294`, a range
            // that ends before it starts.
            format!(
                "window rows from y={y_start} run past the highest row a build can address (size={sw}x{sh}; the walls occupy {})",
                ctx.wall_column,
            )
        };
        diagnostics.push(diag_deferred_member_reason(member, &reason));
        return WindowCut::Refused;
    }
    // Resolved below the two geometry checks above, not before them: both
    // return without painting, and a palette entry claimed on the way to
    // one of them is a block `cairn info` counts and the `.nbt` ships for a
    // window that was never cut.
    //
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
            ctx.registry,
            palette,
            diagnostics,
            ctx.theme_missing,
        ) else {
            return WindowCut::Refused;
        };
        idx
    } else {
        PaletteIndex::AIR
    };
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
            canvas,
        );
    }
    if sym == Some(true) {
        let mirror_offset = len.saturating_sub(offset).saturating_sub(sw);
        // Reject overlapping mirrors: a `sym=true` window asks for a
        // *pair*, not one wide span. The `span_end` guard above refused
        // `offset + size_w > wall_length`, so neither subtraction
        // saturates, and the two rectangles intersect exactly when the
        // window straddles the wall's midpoint
        // (2*offset < wall_length < 2*(offset + size_w)) — diagnose and
        // skip the mirror so the primary is still emitted cleanly. A
        // centred window (2*offset + size_w == wall_length) has a mirror
        // that is the same rectangle, the fullest overlap there is: the
        // author still asked for two windows and got one, so it is
        // reported like the rest.
        let primary_end = offset.saturating_add(sw);
        let mirror_end = mirror_offset.saturating_add(sw);
        let overlap = offset < mirror_end && mirror_offset < primary_end;
        if overlap {
            let relation = if mirror_offset == offset {
                "coincides with"
            } else {
                "would overlap"
            };
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "`sym=true` window at offset={offset} size={sw}x{sh} on the `{}` wall {relation} its mirror (wall length={len}); the mirror was skipped",
                    side_name(side),
                ),
            ));
            return WindowCut::Cut;
        }
        paint_window_rect(
            ctx,
            WindowRect {
                offset: mirror_offset,
                ..base_rect
            },
            canvas,
        );
    }
    WindowCut::Cut
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

fn paint_window_rect(ctx: &StructCtx<'_>, rect: WindowRect, canvas: &mut MemberCanvas<'_>) {
    for du in 0..rect.width {
        for dv in 0..rect.height {
            let along = rect.offset.saturating_add(du);
            let row = rect.y_start.saturating_add(dv);
            let Some(cell) = wall_local_to_grid(
                rect.side,
                along,
                row,
                ctx.overhang,
                ctx.interior_w,
                ctx.interior_h,
                ctx.dims,
            ) else {
                // INVARIANT(wall-grid-validated): `fill_window` refused any
                // span past the wall (`span_end = offset + step*(repeat-1) +
                // size.w` is at most the wall length, which also bounds the
                // `sym=true` mirror) and any row outside a wall course before
                // it got here — so the helper has nothing to reject. Reaching
                // here means one of those checks stopped agreeing with
                // `wall_local_to_grid`. Loud in debug builds; release builds
                // skip the cell rather than panic.
                debug_assert!(
                    false,
                    "window cell u={along} y={row} on the {} wall is outside the wall grid; \
                     `fill_window`'s span check (`offset + step*(repeat-1) + size.w <= wall \
                     length`, which also bounds the mirror) / wall-course check stopped \
                     agreeing with `wall_local_to_grid`",
                    side_name(rect.side),
                );
                continue;
            };
            canvas.paint(cell, || rect.palette_index);
        }
    }
}

fn side_of(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<WallSide> {
    let Some(raw) = member.ident_value("side") else {
        // Distinguish "missing entirely" (no `side=` key) from "wrong
        // type" (`side=` present but its value is not an identifier). A
        // silent return on the missing case would let a `door at=center`
        // line lower to nothing without telling the author, which breaks
        // the module-level promise that every dropped member surfaces a
        // diagnostic.
        let reason = match member.intent_state.get("side") {
            Some(written) => format!(
                "`side=` must be one of front, back, left, right, not {}",
                written.value.describe(),
            ),
            None => "missing `side=` (expected one of front, back, left, right)".to_owned(),
        };
        diagnostics.push(diag_deferred_member_reason(member, &reason));
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

fn diag_no_theme_bound_generic(kind: VoxelSource, label: &str, header_span: &Span) -> Diagnostic {
    let host = match kind {
        VoxelSource::Struct => "struct",
        VoxelSource::Place => "place",
    };
    Diagnostic {
        code: DiagnosticCode::NoThemeBound,
        span: header_span.clone(),
        primary: format!(
            "{host} `{label}` has no theme bound; every `mat_slot=` will lower to air",
        ),
        notes: vec![DiagnosticNote {
            span: None,
            // Describes what makes a theme bind rather than naming one
            // cause, because this pass cannot tell the causes apart: the
            // module may declare no theme, or several logical ones with no
            // `theme=` to choose between them, or exactly one whose
            // variants the pinned edition cannot bind
            // (`E_THEME_VARIANT_MISSING`, reported by the resolver). Asking
            // for "exactly one `theme NAME:`" told the author of that third
            // file to add what they already had.
            message: "a scope binds a theme when the module declares exactly one logical theme, \
                      or the `place` names one with `theme=`; under a `--edition` pin that theme \
                      must also have a variant for that edition"
                .to_owned(),
        }],
        data: None,
    }
}

fn diag_def_no_size(def: &DefIr) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::DefNoSize,
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

/// The repair for a patch whose selector picks no single member, where
/// the reason alone does not point at one.
///
/// [`PatchTargetError`]'s sentence states the fault and stops, so the
/// repair is said here. A missing or malformed `id=` and an id beside the
/// ones the reason lists already name the edit; an id that two members
/// carry, or a scope whose members carry none, does not.
fn patch_target_fix(reason: &PatchTargetError) -> Option<String> {
    let keyword = reason.keyword();
    match reason {
        PatchTargetError::Ambiguous { count, .. } => Some(format!(
            "Fix: give the {count} `{keyword}` members distinct ids, so the selector names one."
        )),
        PatchTargetError::NoSuchId {
            id,
            known,
            unlabelled,
            ..
        } if known.is_empty() && *unlabelled > 0 => Some(format!(
            "Fix: add `id={id}` to the `{keyword}` this patch is meant to bind."
        )),
        _ => None,
    }
}

fn diag_deferred_member_reason(member: &Member, reason: &str) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::DeferredMember,
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
    use crate::check::Severity;

    /// [`PlaceAnchor::origin`] measures the far edge as `origin + dims − 1`,
    /// which is one cell before the origin when an extent is 0. No body has
    /// one, since `size=` is a `NonZeroU32`, so the only way to show that
    /// the check fails loud rather than measuring from the wrong cell is to
    /// hand it one. Debug-only because that is where `debug_assert!` lives.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "a placed body has a zero extent")]
    fn a_zero_extent_is_not_measured_for_its_far_edge() {
        let _ = PlaceAnchor::WorldOrigin.origin(Dims { x: 0, y: 1, z: 1 });
    }

    #[test]
    fn a_placement_whose_far_edge_leaves_i32_is_refused_on_either_axis() {
        // A source cannot reach the `z` case. The grammar has no negative
        // integer literal in value position, so `gap=-1` is `E_PARSE`, and
        // with a `gap=` of 0 or more no origin's `z` is above the `0` of
        // `at=origin`: `east_of` keeps the prior's `z`, and `north_of`
        // moves back from it by the new body's depth and the `gap=`. The
        // anchor is checked directly, with a negative `gap`, so the `z`
        // half of the rule is held all the same.
        let dims = Dims { x: 5, y: 1, z: 5 };
        let north = |prior_origin, gap| PlaceAnchor::NorthOf { prior_origin, gap };
        // In range: the last cell on each axis is `i32::MAX`.
        assert_eq!(
            north((i32::MAX - 4, 0, i32::MAX), -1).origin(dims),
            Ok((i32::MAX - 4, 0, i32::MAX - 4)),
        );
        // One wider on `x`: a `north_of` row keeps the prior's `x`.
        assert_eq!(
            north((i32::MAX - 3, 0, 0), 0).origin(dims),
            Err(PlacementOutOfRange {
                corner: PlacementCorner::FarEdge,
                axis: 'x',
                coord: i128::from(i32::MAX) + 1,
            }),
        );
        let z_past = north((0, 0, i32::MAX), -2).origin(dims);
        assert_eq!(
            z_past,
            Err(PlacementOutOfRange {
                corner: PlacementCorner::FarEdge,
                axis: 'z',
                coord: i128::from(i32::MAX) + 1,
            }),
        );
        // No source renders the sentence with `z` in it, so it is rendered
        // here.
        assert_eq!(
            z_past.map_err(PlacementOutOfRange::deferral),
            Err(
                "this placement's body reaches z=2147483648, past the -2147483648 to \
                 2147483647 range a placement's cells are addressed in; shrink the body with \
                 its `def`'s `size=` or a roof's `overhang=`, or shorten the `gap=` on this row \
                 or on a row it is placed relative to"
                    .to_owned()
            ),
        );
    }

    /// One placed row as `lower_site` leaves it in `structures` and
    /// `placed`: a body `width` cells wide and one cell deep and tall, every
    /// cell stone, at `origin`.
    fn one_placed_row(
        origin: (i32, i32, i32),
        width: u32,
    ) -> (IndexMap<String, BlockArray>, IndexMap<String, PlacedBody>) {
        let key = "site::s::b".to_owned();
        let dims = Dims {
            x: width,
            y: 1,
            z: 1,
        };
        let mut palette = Palette::new_with_air();
        let stone = palette.intern(BlockState::bare("minecraft:stone"));
        let mut structures = IndexMap::new();
        structures.insert(
            key.clone(),
            BlockArray {
                dims,
                palette,
                voxels: vec![stone; width as usize],
                block_entities: Vec::new(),
                entities: Vec::new(),
                source_scope: key.clone(),
            },
        );
        let mut placed = IndexMap::new();
        placed.insert(
            key,
            PlacedBody {
                placement: Placement {
                    site: SiteName::new("s").expect("a site name"),
                    place_id: PlaceId::new("b").expect("a place id"),
                    source_def: "hut".to_owned(),
                    theme: "t".to_owned(),
                    origin,
                    dims,
                },
                walls: WallColumn::default(),
                cut: HashSet::new(),
            },
        );
        (structures, placed)
    }

    /// A body whose last column is `i32::MAX` lays each of its cells under
    /// a column of its own, and its extent ends on the edge.
    #[test]
    fn a_body_ending_on_the_i32_edge_lays_every_cell_once() {
        let (structures, placed) = one_placed_row((i32::MAX - 2, 0, 0), 3);
        let plan = collect_floor_cells(&structures, &placed);
        assert_eq!(
            plan.cells,
            HashSet::from([(i32::MAX - 2, 0, 0), (i32::MAX - 1, 0, 0), (i32::MAX, 0, 0)]),
        );
        assert!(
            plan.owners.values().all(|owners| owners.len() == 1),
            "{:?}",
            plan.owners,
        );
        assert_eq!(plan.extents["site::s::b"].max_x, i32::MAX);
    }

    /// The guard behind the far-edge refusal, for a body that runs past
    /// `i32` and reaches the floor plan anyway. No source can: the refusal
    /// stops the row first, so the plan is built by hand. A debug build
    /// stops at the guard. A release build lays the cells that have a
    /// coordinate and folds none of the rest onto the edge, where a fold
    /// would give the edge cell a second owner.
    #[test]
    #[cfg_attr(debug_assertions, should_panic(expected = "has no world coordinate"))]
    fn a_body_past_the_i32_edge_folds_nothing_onto_it() {
        let (structures, placed) = one_placed_row((i32::MAX - 1, 0, 0), 3);
        let plan = collect_floor_cells(&structures, &placed);
        assert_eq!(
            plan.cells,
            HashSet::from([(i32::MAX - 1, 0, 0), (i32::MAX, 0, 0)]),
        );
        assert!(
            plan.owners.values().all(|owners| owners.len() == 1),
            "{:?}",
            plan.owners,
        );
        assert_eq!(plan.extents["site::s::b"].max_x, i32::MAX);
    }

    /// The other two guards in [`collect_floor_cells`]: a placement with no
    /// structure, and a body with no row 0. `lower_site` produces neither,
    /// so each is handed in, and each stops a debug build.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "has no structure")]
    fn a_placement_with_no_structure_is_not_a_silent_skip() {
        let (_, placed) = one_placed_row((0, 0, 0), 3);
        let _ = collect_floor_cells(&IndexMap::new(), &placed);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "is outside")]
    fn a_body_with_no_row_0_is_not_a_silent_skip() {
        let (mut structures, placed) = one_placed_row((0, 0, 0), 3);
        structures["site::s::b"].dims.y = 0;
        let _ = collect_floor_cells(&structures, &placed);
    }

    /// The plate is the one hardcoded id a pack *can* redirect, and the
    /// list would be making a false promise if it carried it: the id is a
    /// Java spelling, and Bedrock declares nothing by that name.
    #[test]
    fn the_redirectable_plate_id_is_not_in_that_list() {
        assert!(
            !BUILTIN_BLOCK_IDS.contains(&PRESSURE_PLATE_BASE_ID),
            "BUILTIN_BLOCK_IDS promises every target declares its entries, and no Bedrock \
             target declares `{PRESSURE_PLATE_BASE_ID}`; the plate is covered by the pack \
             tests around `{PRESSURE_PLATE_TOKEN}` instead",
        );
    }
    use crate::{lower, parse, resolve};

    fn lowered(source: &str) -> BlockArrayIr {
        let module = parse(source).expect("parse");
        let ir = lower(&module);
        let resolution = resolve(&ir, None);
        lower_to_block_array(&ir, &resolution, None)
    }

    /// A 1x1 struct whose `walls` members each paint the one ring cell
    /// with a block of their own, so `states` members write `states`
    /// distinct states and the finished body keeps one of them.
    fn overwriting_walls(states: usize) -> String {
        use std::fmt::Write as _;

        let mut source = String::from("theme t:\n");
        for i in 0..states {
            writeln!(source, "  slot s{i} -> @b{i}").expect("writing to a String");
        }
        source.push_str("\nstruct s size=1x1\n");
        for i in 0..states {
            writeln!(source, "  walls mat_slot=s{i} height=1").expect("writing to a String");
        }
        source
    }

    /// A body that paints more states than its palette can hold is refused
    /// with `W_PALETTE_TOO_LARGE` rather than panicking. Run at a capacity
    /// of four (air and three states) so the refusal is reachable without
    /// painting 65,536 states; the boundary itself is pinned on
    /// `Palette::try_intern`. States a later member covers count, which is
    /// why the refused body would have kept a single state.
    #[test]
    fn a_body_painting_more_states_than_its_palette_holds_is_refused() {
        test_capacity::set(4);

        let fits = lowered(&overwriting_walls(3));
        assert!(
            !fits
                .diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::PaletteTooLarge),
            "three states fit: {:#?}",
            fits.diagnostics,
        );
        let array = fits
            .structures
            .get("struct::s")
            .expect("three states build");
        assert_eq!(
            array.palette.entries.len(),
            2,
            "air and the last wall's state: the covered ones are pruned after painting",
        );

        let over = lowered(&overwriting_walls(4));
        let refused: Vec<&Diagnostic> = over
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::PaletteTooLarge)
            .collect();
        assert_eq!(refused.len(), 1, "{:#?}", over.diagnostics);
        assert_eq!(refused[0].severity(), Severity::Warning);
        assert!(
            refused[0]
                .primary
                .starts_with("`s` paints more than 3 distinct non-air block states"),
            "{}",
            refused[0].primary,
        );
        assert!(
            !over.structures.contains_key("struct::s"),
            "the refused body must not reach a writer",
        );

        test_capacity::set(PALETTE_CAPACITY);
    }

    fn lowered_with_resolver(source: &str, resolver: &dyn TargetRegistry) -> BlockArrayIr {
        let module = parse(source).expect("parse");
        let ir = lower(&module);
        let resolution = resolve(&ir, None);
        lower_to_block_array(&ir, &resolution, Some(resolver))
    }

    /// In-memory registry for the lowering tests. Declares no id table,
    /// so these tests exercise lowering the way `cairn check` runs it —
    /// with no target pinned and no id refuted. The `E_UNKNOWN_ID` path
    /// has its own tests in `material.rs` and `cli_block_ids.rs`.
    struct FakeResolver {
        entries: Vec<(&'static str, &'static str)>,
    }

    impl TargetRegistry for FakeResolver {
        fn lookup(&self, token: &str) -> Option<BlockState> {
            self.entries
                .iter()
                .find(|(t, _)| *t == token)
                .map(|(_, id)| BlockState::bare(format!("minecraft:{id}")))
        }

        fn known_tokens(&self) -> Vec<String> {
            self.entries.iter().map(|(t, _)| (*t).to_owned()).collect()
        }

        fn block_ids(&self) -> Option<crate::block_array::BlockIdSet<'_>> {
            None
        }
    }

    /// A registry whose tokens carry blockstates.
    ///
    /// [`FakeResolver`] answers with `BlockState::bare`, as the real
    /// `PackView` does, so the branch that keeps a binding's id while
    /// dropping its properties had no way to run.
    struct StatefulResolver {
        token: &'static str,
        id: &'static str,
        properties: Vec<(&'static str, &'static str)>,
    }

    impl TargetRegistry for StatefulResolver {
        fn lookup(&self, token: &str) -> Option<BlockState> {
            (token == self.token).then(|| BlockState {
                id: format!("minecraft:{}", self.id),
                properties: self
                    .properties
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                    .collect(),
            })
        }

        fn known_tokens(&self) -> Vec<String> {
            vec![self.token.to_owned()]
        }

        fn block_ids(&self) -> Option<crate::block_array::BlockIdSet<'_>> {
            None
        }
    }

    /// A registry with a pinned target, for the `E_UNKNOWN_ID` path.
    ///
    /// Separate from [`FakeResolver`] so the tests above keep running in
    /// the no-target mode they were written for; a table added there would
    /// start refusing ids in a hundred tests that are about something else.
    struct PinnedRegistry {
        entries: Vec<(&'static str, &'static str)>,
        /// Sorted, fully namespaced ids the pinned target declares.
        ids: Vec<String>,
        /// One alias group, in the order a pack wrote it. Empty for a pack
        /// shipping no `aliases` component.
        group: Vec<String>,
    }

    impl PinnedRegistry {
        fn new(entries: Vec<(&'static str, &'static str)>, ids: &[&str]) -> Self {
            let mut ids: Vec<String> = ids.iter().map(|id| (*id).to_owned()).collect();
            ids.sort();
            Self {
                entries,
                ids,
                group: Vec::new(),
            }
        }

        /// Declare one alias group over the same ids.
        fn aliasing(mut self, group: &[&str]) -> Self {
            self.group = group.iter().map(|id| (*id).to_owned()).collect();
            self
        }
    }

    impl TargetRegistry for PinnedRegistry {
        fn lookup(&self, token: &str) -> Option<BlockState> {
            self.entries
                .iter()
                .find(|(t, _)| *t == token)
                .map(|(_, id)| BlockState::bare(format!("minecraft:{id}")))
        }

        fn known_tokens(&self) -> Vec<String> {
            self.entries.iter().map(|(t, _)| (*t).to_owned()).collect()
        }

        fn block_ids(&self) -> Option<crate::block_array::BlockIdSet<'_>> {
            Some(crate::block_array::BlockIdSet::new("test 1.0", &self.ids))
        }

        fn aliases_for(&self, id: &str) -> Vec<String> {
            if !self.group.iter().any(|spelling| spelling == id) {
                return Vec::new();
            }
            self.group
                .iter()
                .filter(|spelling| self.ids.binary_search(spelling).is_ok())
                .cloned()
                .collect()
        }
    }

    struct UnknownIdPayload {
        id: String,
        registry: String,
        origin: String,
        token: Option<String>,
        suggestion: Option<String>,
        aliases: Vec<String>,
    }

    fn unknown_id_payload(out: &BlockArrayIr) -> UnknownIdPayload {
        let found: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::UnknownId)
            .collect();
        assert_eq!(
            found.len(),
            1,
            "expected exactly one E_UNKNOWN_ID, got {:?}",
            out.diagnostics
                .iter()
                .map(|d| d.code.as_str())
                .collect::<Vec<_>>(),
        );
        match found[0].data.clone() {
            Some(DiagnosticData::UnknownId {
                id,
                registry,
                origin,
                token,
                suggestion,
                aliases,
            }) => UnknownIdPayload {
                id,
                registry,
                origin,
                token,
                suggestion,
                aliases,
            },
            other => panic!("expected an UnknownId payload, got {other:?}"),
        }
    }

    /// The payload's `token` is what tells a consumer whose mistake it is,
    /// and it has to come from the origin rather than be a constant.
    ///
    /// `mat_slot=` reaches the same code either way, so the two halves are
    /// asserted against one another: a payload that hardcoded `None` would
    /// pass the authored case on its own.
    #[test]
    fn the_payload_names_the_token_only_when_the_catalog_chose_the_id() {
        let registry = PinnedRegistry::new(
            vec![("floor.stone.smooth", "stone_bricks")],
            &["minecraft:stonebrick"],
        );

        let authored = lowered_with_resolver(
            "theme t:\n  slot floor -> @stone_brick\nstruct s size=2x2\n  floor mat_slot=floor\n",
            &registry,
        );
        let payload = unknown_id_payload(&authored);
        assert_eq!(payload.id, "minecraft:stone_brick");
        assert_eq!(payload.registry, "test 1.0");
        assert_eq!(payload.origin, "authored");
        assert_eq!(payload.token, None, "the author wrote this id themselves");
        assert_eq!(payload.suggestion.as_deref(), Some("minecraft:stonebrick"));

        let from_catalog = lowered_with_resolver(
            "theme t:\n  slot floor -> @floor.stone.smooth\nstruct s size=2x2\n  floor mat_slot=floor\n",
            &registry,
        );
        let payload = unknown_id_payload(&from_catalog);
        assert_eq!(payload.id, "minecraft:stone_bricks");
        assert_eq!(payload.origin, "catalog");
        assert_eq!(
            payload.token.as_deref(),
            Some("floor.stone.smooth"),
            "the catalog produced this id, so the author's token is not the fix site",
        );
    }

    /// The member default that comes from the pack is the one id that
    /// reaches a palette without passing `resolve_block_state`, so it has
    /// its own path through the check and its own origin.
    ///
    /// Both halves matter. A registry that *declares* the token but maps it
    /// onto a missing id is the pack's mapping being wrong; a registry with
    /// no row at all sends lowering to the id compiled into this crate,
    /// which is a Java spelling and wrong on Bedrock. Before this, neither
    /// was checked at all.
    #[test]
    fn a_member_default_from_the_pack_is_checked_like_any_other_id() {
        let plate = "struct s size=3x3\n  pressure_plate id=p at=front.outside\n";

        let mapped_wrong = PinnedRegistry::new(
            vec![(PRESSURE_PLATE_TOKEN, "oak_pressure_plate")],
            &["minecraft:wooden_pressure_plate"],
        );
        let out = lowered_with_resolver(plate, &mapped_wrong);
        let payload = unknown_id_payload(&out);
        assert_eq!(payload.id, "minecraft:oak_pressure_plate");
        assert_eq!(payload.origin, "catalog");
        assert_eq!(payload.token.as_deref(), Some(PRESSURE_PLATE_TOKEN));

        let no_row = PinnedRegistry::new(vec![], &["minecraft:wooden_pressure_plate"]);
        let out = lowered_with_resolver(plate, &no_row);
        let payload = unknown_id_payload(&out);
        assert_eq!(
            payload.id, PRESSURE_PLATE_BASE_ID,
            "a pack with no row sends lowering to the compiled-in default",
        );
        assert_eq!(payload.origin, "builtin");
        assert_eq!(payload.token.as_deref(), Some(PRESSURE_PLATE_TOKEN));

        let right = PinnedRegistry::new(
            vec![(PRESSURE_PLATE_TOKEN, "wooden_pressure_plate")],
            &["minecraft:wooden_pressure_plate"],
        );
        let out = lowered_with_resolver(plate, &right);
        assert!(
            !out.diagnostics
                .iter()
                .any(|d| d.code == DiagnosticCode::UnknownId),
            "a pack that spells it the way the target does must not be refused: {:?}",
            out.diagnostics
                .iter()
                .map(|d| d.primary.as_str())
                .collect::<Vec<_>>(),
        );
    }

    /// A rename reaches the message as a statement, and a typo as a
    /// guess, and the note says which it is holding.
    ///
    /// The wording is the load-bearing part: "spells this block" is the
    /// pack asserting two names are one block, "spells the nearest block"
    /// is a distance search offering its best candidate, and an author who
    /// cannot tell them apart has to go and check either way.
    #[test]
    fn a_rename_and_a_typo_read_as_different_claims() {
        let renamed = PinnedRegistry::new(vec![], &["minecraft:standing_sign"])
            .aliasing(&["minecraft:oak_sign", "minecraft:standing_sign"]);
        let out = lowered_with_resolver(
            "theme t:\n  slot floor -> @oak_sign\nstruct s size=2x2\n  floor mat_slot=floor\n",
            &renamed,
        );
        let payload = unknown_id_payload(&out);
        assert_eq!(payload.aliases, ["minecraft:standing_sign".to_owned()]);
        let note = unknown_id_note(&out);
        assert!(
            note.contains("spells this block `minecraft:standing_sign`")
                && note.contains("alias table"),
            "the note must name the alias table it is quoting, got: {note}",
        );

        let typo = PinnedRegistry::new(vec![], &["minecraft:oak_planks"]);
        let out = lowered_with_resolver(
            "theme t:\n  slot floor -> @oak_plank\nstruct s size=2x2\n  floor mat_slot=floor\n",
            &typo,
        );
        let note = unknown_id_note(&out);
        assert!(
            note.contains("spells the nearest block") && !note.contains("alias table"),
            "a distance guess must not be dressed up as the pack's word, got: {note}",
        );
    }

    /// A split lists the first few candidates and counts the rest, and the
    /// payload keeps all of them.
    ///
    /// The truncation is about the sentence a person reads;
    /// `spec/versioning-editions` "Fail-loud and minimum-version inference"
    /// asks the error to return the closed set of candidates, and the `data`
    /// payload is where a consumer finds it whole.
    #[test]
    fn a_split_is_summarised_in_the_note_and_kept_whole_in_the_payload() {
        let ids: Vec<String> = (0..16)
            .map(|i| format!("minecraft:light_block_{i}"))
            .collect();
        let mut group = vec!["minecraft:light".to_owned()];
        group.extend(ids.iter().cloned());
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let group_refs: Vec<&str> = group.iter().map(String::as_str).collect();
        let registry = PinnedRegistry::new(vec![], &refs).aliasing(&group_refs);
        let out = lowered_with_resolver(
            "theme t:\n  slot floor -> @light\nstruct s size=2x2\n  floor mat_slot=floor\n",
            &registry,
        );
        let payload = unknown_id_payload(&out);
        assert_eq!(payload.aliases.len(), 16, "every candidate reaches a tool");
        let note = unknown_id_note(&out);
        assert!(
            note.contains("and 12 more"),
            "the sentence counts what it does not list, got: {note}",
        );
    }

    /// The one note the `E_UNKNOWN_ID` finding carries about candidates —
    /// the last one, after the origin note the two pack-chosen origins add.
    fn unknown_id_note(out: &BlockArrayIr) -> String {
        let found = out
            .diagnostics
            .iter()
            .find(|d| d.code == DiagnosticCode::UnknownId)
            .expect("an E_UNKNOWN_ID finding");
        found
            .notes
            .last()
            .expect("the candidate note")
            .message
            .clone()
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

    fn primaries_of(out: &BlockArrayIr, code: DiagnosticCode) -> Vec<&str> {
        out.diagnostics
            .iter()
            .filter(|d| d.code == code)
            .map(|d| d.primary.as_str())
            .collect()
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
        // head is not `sig`, so it cannot be a signal reference under the
        // namespace of `spec/redstone` "Signal binding".
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
        // a `powered_by=` on doors would let a later extension that
        // gives the key a meaning silently change the meaning of source
        // that shipped meanwhile. Reject the shape now with a primary
        // that names the offending key(s) and points at `spec/redstone`
        // "Signal binding".
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
            deferred[0].primary.contains("2 physical doors"),
            "expected the primary to flag the ambiguity, got {}",
            deferred[0].primary,
        );
        // The reason states the fault; the repair is a note of its own.
        assert!(
            deferred[0].notes.iter().any(|n| n.message
                == "Fix: give the 2 `door` members distinct ids, so the selector names one."),
            "expected a note telling the author to tell the doors apart, got {:?}",
            deferred[0].notes,
        );
    }

    #[test]
    fn actuator_patch_beside_unlabelled_door_defers_with_add_id_note() {
        // A door declared without `id=` can never be picked, but it is a
        // door: the primary counts it rather than saying none is declared,
        // and the note points at labelling it.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  \
                   walls mat_slot=w height=3\n  \
                   door side=front at=center\n  \
                   door[id=front] opened_by=sig.open\n";
        let out = lowered(src);
        let deferred: Vec<&Diagnostic> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect();
        assert_eq!(deferred.len(), 1);
        assert!(
            deferred[0]
                .primary
                .contains("1 physical door is declared in this scope, without an `id=`"),
            "got {}",
            deferred[0].primary,
        );
        assert!(
            deferred[0]
                .notes
                .iter()
                .any(|n| n.message
                    == "Fix: add `id=front` to the `door` this patch is meant to bind."),
            "got {:?}",
            deferred[0].notes,
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
        // `spec/redstone` "Signal binding". Silently accepting a
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

    /// One arm of the abstract-token deferral tests: members that read
    /// `@wood.dark` from slot `trim` (slot `wall` is `@cobblestone`), what
    /// `W_ABSTRACT_TOKEN_DEFERRED` says the member does instead, and a check
    /// that the voxels say the same. `None` is a member turned away before it
    /// asks for its material, which earns no such warning at all.
    struct DeferralArm {
        members: &'static str,
        consequence: Option<&'static str>,
        voxels: fn(&BlockArrayIr),
    }

    fn check_deferral_arms(arms: &[DeferralArm]) {
        for arm in arms {
            let members = arm.members;
            let out = lowered(&format!(
                "theme t:\n  slot wall -> @cobblestone\n  slot trim -> @wood.dark\n\n\
                 struct s size=5x5\n  {members}\n"
            ));
            let expected: Vec<String> = arm
                .consequence
                .map(|c| {
                    format!(
                        "abstract token `@wood.dark` cannot be lowered without the registry \
                         pack; {c}"
                    )
                })
                .into_iter()
                .collect();
            assert_eq!(
                primaries_of(&out, DiagnosticCode::AbstractTokenDeferred),
                expected,
                "{members}",
            );
            (arm.voxels)(&out);
        }
    }

    fn struct_s(out: &BlockArrayIr) -> &BlockArray {
        out.structures.get("struct::s").expect("struct::s lowered")
    }

    fn has_id(out: &BlockArrayIr, id: &str) -> bool {
        struct_s(out).palette.entries.iter().any(|e| e.id == id)
    }

    fn refused_for_no_wall(out: &BlockArrayIr) -> bool {
        primaries_of(out, DiagnosticCode::DeferredMember)
            .iter()
            .any(|p| p.contains("has no wall to cut into"))
    }

    #[test]
    fn abstract_token_deferral_says_what_each_member_does_instead() {
        let default_block = Some("the member is built from its default block");
        check_deferral_arms(&[
            // The issue's example: the window is not cut, so its wall stays.
            DeferralArm {
                members: "walls mat_slot=wall height=3\n  \
                          window side=front offset=1 y=1 size=1x2 mat_slot=trim",
                consequence: Some("the window is not cut, and the wall stays"),
                voxels: |out| {
                    for y in 1..=2 {
                        assert_eq!(block_id(struct_s(out), 1, y, 4), "minecraft:cobblestone");
                    }
                },
            },
            DeferralArm {
                members: "walls mat_slot=wall height=3\n  floor mat_slot=trim",
                consequence: Some("the cell falls back to air"),
                voxels: |out| assert_eq!(block_id(struct_s(out), 1, 0, 1), BlockState::AIR_ID),
            },
            // The walls' rows are absent rather than air, and a window cut
            // into them is refused.
            DeferralArm {
                members: "floor mat_slot=wall\n  \
                          walls mat_slot=trim height=3\n  \
                          window side=front offset=1 y=1 size=1x2 mat_slot=wall",
                consequence: Some(
                    "the walls are not built and take up no rows, so a door or window cut into \
                     them is refused",
                ),
                voxels: |out| {
                    assert_eq!(struct_s(out).dims.y, 1);
                    assert!(refused_for_no_wall(out));
                },
            },
            DeferralArm {
                members: "walls mat_slot=wall height=3\n  roof kind=gable mat_slot=trim",
                consequence: default_block,
                voxels: |out| assert!(has_id(out, STAIR_BASE_ID)),
            },
            DeferralArm {
                members: "walls mat_slot=wall height=3\n  roof kind=flat mat_slot=trim",
                consequence: default_block,
                voxels: |out| assert!(has_id(out, FLAT_BASE_ID)),
            },
            DeferralArm {
                members: "walls mat_slot=wall height=3\n  \
                          roof kind=shed slope_to=front mat_slot=trim",
                consequence: default_block,
                voxels: |out| assert!(has_id(out, STAIR_BASE_ID)),
            },
            DeferralArm {
                members: "walls mat_slot=wall height=3\n  \
                          roof kind=flat mat_slot=wall overhang=1\n  \
                          stair kind=stairs side=front mat_slot=trim",
                consequence: default_block,
                voxels: |out| assert!(has_id(out, STAIR_BASE_ID)),
            },
            DeferralArm {
                members: "walls mat_slot=wall height=3\n  \
                          pressure_plate at=front.outside offset=2 y=0 mat_slot=trim",
                consequence: default_block,
                voxels: |out| assert!(has_id(out, PRESSURE_PLATE_BASE_ID)),
            },
        ]);
    }

    #[test]
    fn a_member_refused_before_its_material_defers_no_abstract_token() {
        check_deferral_arms(&[
            // `cut_window` refuses a window with no wall before it resolves
            // the material.
            DeferralArm {
                members: "floor mat_slot=wall\n  \
                          window side=front offset=1 y=1 size=1x2 mat_slot=trim",
                consequence: None,
                voxels: |out| {
                    assert_eq!(struct_s(out).dims.y, 1);
                    assert!(refused_for_no_wall(out));
                },
            },
            // A shed with no usable `slope_to=` draws nothing, so it claims no
            // material either: the one finding is the one that says why.
            DeferralArm {
                members: "walls mat_slot=wall height=3\n  roof kind=shed mat_slot=trim",
                consequence: None,
                voxels: |out| {
                    assert_eq!(struct_s(out).dims.y, 4);
                    assert!(!has_id(out, STAIR_BASE_ID));
                    assert_eq!(
                        primaries_of(out, DiagnosticCode::DeferredMember),
                        vec!["shed roof requires `slope_to=` (one of front, back, left, right)"],
                    );
                },
            },
            DeferralArm {
                members: "walls mat_slot=wall height=3\n  \
                          roof kind=shed slope_to=sideways mat_slot=trim",
                consequence: None,
                voxels: |out| {
                    assert_eq!(struct_s(out).dims.y, 4);
                    assert!(!has_id(out, STAIR_BASE_ID));
                    assert_eq!(
                        primaries_of(out, DiagnosticCode::DeferredMember),
                        vec![
                            "unknown shed `slope_to=sideways` (expected one of front, back, left, \
                             right)"
                        ],
                    );
                },
            },
        ]);
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
        assert_eq!(diag.severity(), Severity::Error);
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
        // Exercises the palette/material path directly, below the parser,
        // to lock the canonical-id and property-bag contract on its own.
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

    // ------------------------------------------------------------------
    // A geometry pass may only attach stair states to a stair.
    // ------------------------------------------------------------------

    /// Every distinct palette entry the lowering produced for `struct::s`.
    fn palette_of(out: &BlockArrayIr) -> Vec<BlockState> {
        out.structures
            .get("struct::s")
            .expect("scope lowered")
            .palette
            .entries
            .clone()
    }

    /// Ids of the palette entries that carry blockstate properties.
    fn ids_carrying_properties(out: &BlockArrayIr) -> Vec<String> {
        palette_of(out)
            .into_iter()
            .filter(|e| !e.properties.is_empty())
            .map(|e| e.id)
            .collect()
    }

    fn deferrals(out: &BlockArrayIr) -> Vec<&Diagnostic> {
        out.diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .collect()
    }

    fn refusals(out: &BlockArrayIr) -> Vec<&Diagnostic> {
        out.diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::IncompatibleMaterial)
            .collect()
    }

    /// A struct whose roof and eave read `slot r`, bound to `material`.
    fn roofed(kind: &str, material: &str) -> String {
        format!(
            "theme t:\n  slot w -> @cobblestone\n  slot r -> @{material}\n\n\
             struct s size=9x7\n  walls mat_slot=w height=4\n  \
             roof kind={kind} mat_slot=r overhang=1\n"
        )
    }

    #[test]
    fn a_sloped_roof_refuses_a_material_that_is_not_a_stair() {
        // A whole block has nowhere to carry `facing` / `half` / `shape`,
        // so a palette entry pairing them would name a blockstate that does
        // not exist. Refusing at the binding is the only answer that does
        // not either write it or silently pick a different material.
        for kind in ["gable", "shed", "hip"] {
            let src = roofed(kind, "cobblestone")
                .replace("roof kind=shed", "roof slope_to=front kind=shed");
            let out = lowered(&src);
            let reasons: Vec<&str> = refusals(&out).iter().map(|d| d.primary.as_str()).collect();
            assert!(
                reasons
                    .iter()
                    .any(|r| r.contains("minecraft:cobblestone") && r.contains("is not a stair")),
                "kind={kind}: expected the family refusal, got {reasons:?}",
            );
            assert!(
                !ids_carrying_properties(&out)
                    .iter()
                    .any(|id| id == "minecraft:cobblestone"),
                "kind={kind}: stair states must never land on a non-stair",
            );
            assert!(
                ids_carrying_properties(&out)
                    .iter()
                    .any(|id| id == STAIR_BASE_ID),
                "kind={kind}: the roof still gets built, out of the fallback species",
            );
        }
    }

    #[test]
    fn a_sloped_roof_takes_any_stair_species_without_comment() {
        // The registry pack ships four roof species and every one of them
        // is a stair (`roof.dark_wood` → `dark_oak_stairs`). Choosing one
        // is the point of binding a roof slot, not a deviation from a
        // hardcoded id.
        let out = lowered(&roofed("gable", "dark_oak_stairs"));
        assert!(
            out.diagnostics.is_empty(),
            "a stair species is a legal roof material, got {:?}",
            out.diagnostics,
        );
        // `.all()` over the stated ids would also hold if the roof painted
        // nothing at all, which is the regression the refusal path could
        // plausibly introduce. Pin the presence first.
        let stated = ids_carrying_properties(&out);
        assert!(!stated.is_empty(), "the roof must paint stairs");
        assert!(
            stated.iter().all(|id| id == "minecraft:dark_oak_stairs"),
            "the species must be the one bound, got {stated:?}",
        );
    }

    #[test]
    fn a_roof_with_no_material_binding_keeps_the_fallback_silently() {
        let src = "theme t:\n  slot w -> @cobblestone\n\n\
                   struct s size=9x7\n  walls mat_slot=w height=4\n  \
                   roof kind=gable overhang=1\n";
        let out = lowered(src);
        assert!(out.diagnostics.is_empty(), "got {:?}", out.diagnostics);
        let stated = ids_carrying_properties(&out);
        assert!(!stated.is_empty(), "the roof must paint stairs");
        assert!(
            stated.iter().all(|id| id == STAIR_BASE_ID),
            "got {stated:?}",
        );
    }

    #[test]
    fn a_roof_whose_binding_resolves_to_nothing_still_says_so() {
        // A distinct failure from an unusable material, with a distinct
        // fix: here the slot names nothing at all. Folding the two messages
        // together would send the author looking for the wrong thing.
        let src = "theme t:\n  slot w -> @cobblestone\n\n\
                   struct s size=9x7\n  walls mat_slot=w height=4\n  \
                   roof kind=gable mat_slot=nosuchslot overhang=1\n";
        let out = lowered(src);
        assert!(
            deferrals(&out)
                .iter()
                .any(|d| d.primary.contains("did not resolve")),
            "got {:?}",
            deferrals(&out),
        );
    }

    #[test]
    fn a_roof_reading_a_slot_from_a_theme_that_never_bound_says_so_once() {
        // `geometry_material_id`'s doc claims this arm: the struct already
        // has `W_NO_THEME_BOUND` against it, and the member-level finding
        // is what says *which* member wore the fallback. A module with two
        // logical themes and no `place theme=` binds nothing.
        let src = "theme a:
  slot r -> @dark_oak_stairs

theme b:
  slot r -> @birch_stairs

struct s size=9x7
  roof kind=gable mat_slot=r overhang=1
";
        let out = lowered(src);
        assert!(
            deferrals(&out)
                .iter()
                .any(|d| d.primary.contains("did not resolve")),
            "got {:?}",
            out.diagnostics,
        );
        assert!(refusals(&out).is_empty(), "got {:?}", out.diagnostics);
        let stated = ids_carrying_properties(&out);
        assert!(
            !stated.is_empty() && stated.iter().all(|id| id == STAIR_BASE_ID),
            "the roof wears the fallback, got {stated:?}",
        );
    }

    #[test]
    fn a_flat_roof_takes_any_block_because_it_attaches_no_states() {
        // Including a stair id: a flat roof paints `minecraft:oak_stairs`
        // bare, which is a valid blockstate — every property takes its
        // default. Requiring a family here would refuse a legal build.
        for material in ["stone_bricks", "oak_stairs"] {
            let out = lowered(&roofed("flat", material));
            assert!(
                deferrals(&out).is_empty(),
                "material={material}: {:?}",
                deferrals(&out),
            );
            assert!(
                palette_of(&out)
                    .iter()
                    .any(|e| e.id == format!("minecraft:{material}") && e.properties.is_empty()),
                "material={material}: the flat deck must be that block, bare",
            );
        }
    }

    #[test]
    fn an_eave_stair_refuses_a_material_that_is_not_a_stair() {
        // A `stair` member takes its `facing` / `half` / `shape` from its
        // own arguments rather than from a slope, and attaches them to
        // whatever it paints just the same. The obligation follows the
        // states, not the member kind.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\
                   \x20\x20slot trim -> @cobblestone\n\n\
                   struct s size=9x7\n  walls mat_slot=w height=4\n  \
                   roof kind=gable mat_slot=r overhang=1\n  \
                   stair id=e kind=stairs mat_slot=trim side=front half=top shape=outer_left\n";
        let out = lowered(src);
        assert!(
            refusals(&out)
                .iter()
                .any(|d| d.primary.contains("eave `stair`") && d.primary.contains("is not a stair")),
            "got {:?}",
            out.diagnostics,
        );
        assert!(
            !ids_carrying_properties(&out)
                .iter()
                .any(|id| id == "minecraft:cobblestone"),
        );
    }

    #[test]
    fn an_eave_stair_with_no_binding_falls_back_to_the_stair_id() {
        // No `stair` member without `mat_slot=` exists anywhere else in the
        // repo, so nothing held this argument to the stair constant — and
        // the plank one is a plausible slip that would put
        // `spruce_planks[facing=…]` in a palette, which is the shape this
        // whole check exists to refuse. The roof binds a *different*
        // species so the eave's fallback is identifiable in the palette.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @dark_oak_stairs\n\n\
                   struct s size=9x7\n  walls mat_slot=w height=4\n  \
                   roof kind=gable mat_slot=r overhang=1\n  \
                   stair id=e kind=stairs side=front half=top shape=outer_left\n";
        let out = lowered(src);
        assert!(out.diagnostics.is_empty(), "got {:?}", out.diagnostics);
        let stated = ids_carrying_properties(&out);
        assert!(
            stated.iter().any(|id| id == STAIR_BASE_ID),
            "the eave wears the stair fallback, got {stated:?}",
        );
        assert!(
            stated.iter().all(|id| is_stair(id)),
            "nothing outside the family may carry stair states, got {stated:?}",
        );
    }

    #[test]
    fn an_eave_stair_takes_any_stair_species_without_comment() {
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\
                   \x20\x20slot trim -> @birch_stairs\n\n\
                   struct s size=9x7\n  walls mat_slot=w height=4\n  \
                   roof kind=gable mat_slot=r overhang=1\n  \
                   stair id=e kind=stairs mat_slot=trim side=front half=top shape=outer_left\n";
        let out = lowered(src);
        assert!(deferrals(&out).is_empty(), "got {:?}", deferrals(&out));
        assert!(
            ids_carrying_properties(&out)
                .iter()
                .any(|id| id == "minecraft:birch_stairs"),
            "got {:?}",
            ids_carrying_properties(&out),
        );
    }

    #[test]
    fn a_binding_inside_the_family_keeps_its_id_and_loses_its_states() {
        // The geometry derives `facing` / `half` / `shape` and the binding
        // asked for its own; the id survives and the states do not, which
        // is a smaller thing than the material being the wrong shape and so
        // stays a warning.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @roof.fancy\n\n\
                   struct s size=9x7\n  walls mat_slot=w height=4\n  \
                   roof kind=gable mat_slot=r overhang=1\n";
        let out = lowered_with_resolver(
            src,
            &StatefulResolver {
                token: "roof.fancy",
                id: "birch_stairs",
                properties: vec![("facing", "north")],
            },
        );
        assert!(
            deferrals(&out)
                .iter()
                .any(|d| d.primary.contains("also carried properties")),
            "got {:?}",
            out.diagnostics,
        );
        assert!(refusals(&out).is_empty(), "got {:?}", out.diagnostics);
        let stated = ids_carrying_properties(&out);
        assert!(
            stated.iter().all(|id| id == "minecraft:birch_stairs"),
            "the bound id survives, got {stated:?}",
        );
        // And the states are the geometry's, not the binding's: a gable
        // paints both slope facings, which one hardcoded `facing=north`
        // could not produce.
        let facings: std::collections::BTreeSet<String> = palette_of(&out)
            .iter()
            .filter_map(|e| e.properties.get("facing").cloned())
            .collect();
        assert!(
            facings.len() > 1,
            "the geometry's facings, not the binding's one: {facings:?}",
        );
    }

    #[test]
    fn the_family_is_reported_before_the_properties_it_makes_moot() {
        // A source reaches this through a state literal on a block outside
        // the stair family — `slot r -> @cobblestone[facing=north]` bound
        // to a gable roof — and that literal is the only way in: the
        // registry pack answers `PackView::lookup` with `BlockState::bare`.
        // The state is built here rather than parsed so the test asks
        // about the precedence alone: once the id is refused it is not
        // painted, and reporting that its unused properties were also
        // dropped would ask the author to fix something that is not there.
        let mut properties = IndexMap::new();
        properties.insert("facing".to_owned(), "north".to_owned());
        let state = BlockState {
            id: "minecraft:cobblestone".to_owned(),
            properties,
        };
        let module = parse(&roofed("gable", "cobblestone")).expect("parse");
        let ir = lower(&module);
        let member = ir.structs[0]
            .members
            .iter()
            .find(|m| m.role == MemberRole::Roof)
            .expect("the roof member");
        // A real scope, so the payload carries the theme's slot value the
        // way it does in a build — `None` here would leave `token` empty
        // and quietly weaken every assertion below it.
        let resolution = resolve(&ir, None);
        let scope = resolution.scopes.get("struct::s").expect("scope");
        let mut diagnostics = Vec::new();
        let id = geometry_material_id(
            member,
            Some(scope),
            Some(&state),
            STAIR_BASE_ID,
            &GeometryMemberDescription {
                subject: "`gable` roof".to_owned(),
                states_from: "the geometry",
                requires_stair: true,
            },
            &mut diagnostics,
        );
        assert_eq!(id, STAIR_BASE_ID);
        assert_eq!(diagnostics.len(), 1, "one finding, got {diagnostics:?}");
        assert_eq!(
            diagnostics[0].code,
            DiagnosticCode::IncompatibleMaterial,
            "got {:?}",
            diagnostics[0],
        );
        // The payload is the whole reason this is an error rather than a
        // softer finding: it says where the material came from, so a
        // consumer can tell a source line from a pack mapping without
        // parsing the sentence (`spec/lint` "Machine-readable payload").
        let Some(DiagnosticData::IncompatibleMaterial {
            id,
            required,
            slot,
            token,
        }) = &diagnostics[0].data
        else {
            panic!("expected the payload, got {:?}", diagnostics[0].data);
        };
        assert_eq!(id, "minecraft:cobblestone");
        assert_eq!(required, "stair");
        assert_eq!(slot.as_deref(), Some("r"));
        assert_eq!(
            token.as_deref(),
            Some("cobblestone"),
            "the theme's slot value, so a dotted one reads as a pack mapping",
        );
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
        // A flat deck attaches no blockstates, so it requires nothing of
        // the block it names: the binding lands in the palette verbatim,
        // including a stair id, which is then simply a stair in its
        // default state. `FLAT_BASE_ID` is what a deck falls back to with
        // no binding, not a species it is held to.
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
    fn a_sym_window_that_coincides_with_its_mirror_is_reported() {
        // `spec/syntax` "Selectors": a mirror overlapping the primary is
        // `W_DEFERRED_MEMBER`. A centred window's mirror is the same
        // rectangle, so the author asked for two windows and got one.
        // All three centred shapes on a 5-wide wall, the solutions of
        // 2*offset + width == 5: mirror_offset = 5 - 2 - 1 = 2,
        // 5 - 1 - 3 = 1 and 5 - 0 - 5 = 0. The last puts `mirror_offset`
        // at 0, the floor its `saturating_sub` clamps to.
        for (offset, width) in [(2, 1), (1, 3), (0, 5)] {
            let src = format!(
                "theme t:\n  slot w -> @cobblestone\n  slot g -> @glass_pane\n\n\
                 struct s size=5x5\n  walls mat_slot=w height=3\n  \
                 window side=front offset={offset} y=1 size={width}x1 sym=true mat_slot=g\n"
            );
            let out = lowered(&src);
            let found: Vec<_> = out
                .diagnostics
                .iter()
                .map(|d| (d.code, d.primary.as_str()))
                .collect();
            assert_eq!(
                found,
                vec![(
                    DiagnosticCode::DeferredMember,
                    format!(
                        "`sym=true` window at offset={offset} size={width}x1 on the `front` wall \
                         coincides with its mirror (wall length=5); the mirror was skipped"
                    )
                    .as_str(),
                )],
            );
            // The primary is still painted.
            let ba = out.structures.get("struct::s").unwrap();
            for x in offset..offset + width {
                assert_eq!(block_id(ba, x, 1, 4), "minecraft:glass_pane", "x={x}");
            }
        }
    }

    // The one-row course under a roof, and the struct with no walls at
    // all, live in `tests/door_wall_fit.rs`. Both were asserted here too
    // loosely to hold what this file now decides: the cap test named
    // `wall_top` for a mechanism that is the course's, and the
    // no-walls test matched any message containing "walls", which both
    // branches of the gate satisfy — so the split between "nowhere" and
    // "not here" went unguarded on the side that has its own sentence.

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
        // A theme that binds `slot roof -> @oak_stairs` lands oak_stairs on
        // every gable voxel instead of the hardcoded spruce_stairs.
        // Nothing is reported: the binding is inside the stair family, so
        // it is used verbatim, which is what choosing a species means.
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

    /// Pruning must not become the place generator bugs go to be tidied
    /// away. A slot a write named and a later phase covered is the
    /// layering working; a slot no write ever named is a generator
    /// interning a material for geometry it does not emit, and it stays in
    /// the palette where the artifact, `cairn info`, and
    /// `tests/palette_is_referenced` can all still see it. The caller
    /// turns the returned list into a `debug_assert!`, which is why this
    /// asserts on the list rather than on a panic — the behaviour under
    /// test is what a release build does.
    #[test]
    fn a_slot_no_write_ever_named_is_kept_rather_than_swept_out() {
        let mut palette = Palette::new_with_air();
        palette.intern(BlockState::bare("minecraft:cobblestone"));
        let canvas = Canvas::new(Dims { x: 1, y: 1, z: 1 }, vec![None]);

        let (voxels, never_painted) = prune_unreferenced(&mut palette, &canvas);
        assert_eq!(never_painted, [1]);
        assert_eq!(
            palette
                .entries
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["minecraft:air", "minecraft:cobblestone"],
        );
        assert_eq!(voxels, [PaletteIndex::AIR]);
    }

    /// The other half of the pair: the same unreferenced slot, this time
    /// reached by a write that a later one covered, is dropped without a
    /// word and the survivors renumber onto the gap.
    #[test]
    fn a_slot_a_later_write_covered_is_pruned_and_the_rest_renumber() {
        let mut palette = Palette::new_with_air();
        let covered = palette.intern(BlockState::bare("minecraft:glass_pane"));
        let winner = palette.intern(BlockState::bare("minecraft:cobblestone"));
        let mut canvas = Canvas::new(Dims { x: 1, y: 1, z: 1 }, vec![Some(Phase::Massing)]);
        canvas.for_member(0).paint((0, 0, 0), || covered);
        canvas.for_member(0).paint((0, 0, 0), || winner);

        let (voxels, never_painted) = prune_unreferenced(&mut palette, &canvas);
        assert_eq!(never_painted, Vec::<usize>::new());
        assert_eq!(
            palette
                .entries
                .iter()
                .map(|s| s.id.as_str())
                .collect::<Vec<_>>(),
            ["minecraft:air", "minecraft:cobblestone"],
        );
        assert_eq!(voxels, [PaletteIndex(1)]);
    }

    /// The clip in [`MemberCanvas::paint`] is unreachable from source — every
    /// generator's coordinates come from the dims the volume was built
    /// against — so the only way to show that it fails loud rather than
    /// dropping the write is to call it with the disagreement it exists to
    /// catch. Debug-only because that is where `debug_assert!` lives; a
    /// release build clips and carries on.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "the dim math and the generator disagree")]
    fn painting_outside_the_volume_is_not_a_silent_drop() {
        let mut canvas = Canvas::new(Dims { x: 2, y: 2, z: 2 }, vec![Some(Phase::Massing)]);
        canvas.for_member(0).paint((2, 0, 0), || PaletteIndex(1));
    }

    #[test]
    fn even_span_gable_apex_rows_face_away_from_the_ridge() {
        // The two apex faces agree on `id`, `half`, and `shape`, and differ
        // only in `facing`, so a test that asserts `half=top` cannot tell
        // them apart — and the two rows reach the palette through a face →
        // slot table that a single wrong arm collapses into one entry.
        //
        // The ridge runs between z=1 and z=2, so the low row points -z and
        // the high row +z: each cap turns its open half under the ridge.
        // Pointing them inward instead — which is what the table used to
        // do — leaves that open half on the outer face, a 0.5 x 0.5
        // undercut running the length of the roof.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=8x4\n  walls mat_slot=w height=4\n  roof kind=gable mat_slot=r\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        let apex_low = block_state_at(ba, 0, 6, 1);
        let apex_high = block_state_at(ba, 0, 6, 2);
        assert_eq!(apex_low.properties.get("facing").unwrap(), "north");
        assert_eq!(apex_high.properties.get("facing").unwrap(), "south");
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
            "x = prior.x(0) + prior.dims.x(3) + gap(2)"
        );
        assert_eq!(b.origin.2, 0, "east_of does not move along z");
    }

    #[test]
    fn north_of_subtracts_dims_and_gap_on_z_axis() {
        // north_of retreats along -z by the new placement's full inflated
        // dims.z plus gap. Both cottages are 3 deep, so this pins the
        // arithmetic, not whose depth it reads — the unequal-depth tests
        // below do that. Front-is-+z (`spec/syntax` "Selectors" and
        // `spec/components-editing-sites` "Multi-building with `site`") means
        // north sits at the negative-z half-space.
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
            "z = prior.z(0) - new.dims.z(3) - gap(4)"
        );
    }

    /// A two-place site with `a` built from `a_def` and `b` placed next to
    /// it with `selector=a gap=gap`.
    ///
    /// Each pair a test draws from here differs on exactly the axis its
    /// selector reads: `small` (3x3) against `deep` (3x9) on `z` for
    /// `north_of`, `small` against `wide` (9x3) on `x` for `east_of`.
    /// `eaved` is `deep` with a roof of `overhang=1`, so it lowers to 5x11:
    /// its `dims` differ from its `size=`, which is what tells an origin
    /// taken from the lowered body apart from one taken from `size=` (known
    /// before lowering, and wrong whenever a roof overhangs).
    fn unequal_pair(a_def: &str, b_def: &str, selector: &str, gap: i32) -> BlockArrayIr {
        lowered(&format!(
            concat!(
                "def small size=3x3:\n",
                "  floor id=f mat_slot=floor\n",
                "  walls id=w mat_slot=wall height=2\n",
                "\n",
                "def deep size=3x9:\n",
                "  floor id=f mat_slot=floor\n",
                "  walls id=w mat_slot=wall height=2\n",
                "\n",
                "def eaved size=3x9:\n",
                "  floor id=f mat_slot=floor\n",
                "  walls id=w mat_slot=wall height=2\n",
                "  roof  id=r kind=flat mat_slot=wall overhang=1\n",
                "\n",
                "def wide size=9x3:\n",
                "  floor id=f mat_slot=floor\n",
                "  walls id=w mat_slot=wall height=2\n",
                "\n",
                "theme t:\n",
                "  slot floor -> @oak_planks\n",
                "  slot wall  -> @cobblestone\n",
                "\n",
                "site s:\n",
                "  place id=a use={a_def} theme=t at=origin\n",
                "  place id=b use={b_def} theme=t {selector}=a gap={gap}\n",
            ),
            a_def = a_def,
            b_def = b_def,
            selector = selector,
            gap = gap,
        ))
    }

    #[test]
    fn north_of_leaves_exactly_gap_rows_between_unequal_depths() {
        // `gap` counts the empty rows between `b`'s +z face and `a`'s -z
        // face, whichever of the two is deeper. Stepping back by the prior
        // placement's depth instead buries a small `a` inside a deep `b`
        // at gap=0 and pushes a small `b` six rows too far behind a deep
        // `a`. The `eaved` row fails if the step is taken from `size=`
        // rather than the lowered dims.
        //
        // `b_z` is `b`'s origin at gap=0: `-new.dims.z`, since `a` sits at 0.
        for (a_def, b_def, b_z) in [
            ("small", "deep", -9),
            ("deep", "small", -3),
            ("small", "eaved", -11),
        ] {
            for gap in [0, 3] {
                let out = unequal_pair(a_def, b_def, "north_of", gap);
                let a = out.placements.get("site::s::a").expect("placement a");
                let b = out.placements.get("site::s::b").expect("placement b");
                // The premise: without it, the gap check below holds for
                // either reading.
                assert_ne!(a.dims.z, b.dims.z, "a={a_def}, b={b_def}");
                assert_eq!(
                    b.origin,
                    (0, 0, b_z - gap),
                    "a={a_def}, b={b_def}, gap={gap}"
                );
                let b_front = b.origin.2 + i32::try_from(b.dims.z).unwrap();
                assert_eq!(
                    a.origin.2 - b_front,
                    gap,
                    "a={a_def} at z {}..{}, b={b_def} at z {}..{}, gap={gap}",
                    a.origin.2,
                    a.origin.2 + i32::try_from(a.dims.z).unwrap(),
                    b.origin.2,
                    b_front,
                );
            }
        }
    }

    #[test]
    fn east_of_leaves_exactly_gap_columns_between_unequal_widths() {
        // A guard, not a regression test: `east_of` was already right, and
        // this pins it against being "symmetrised" onto the new body's
        // dims to match `north_of`. It moves past the prior placement's
        // width, so the step is the prior's lowered `dims.x` — `eaved` is 5
        // wide, not its `size=` 3.
        //
        // `b_x` is `b`'s origin at gap=0: `prior.dims.x`, since `a` sits at 0.
        for (a_def, b_def, b_x) in [
            ("small", "wide", 3),
            ("wide", "small", 9),
            ("eaved", "small", 5),
        ] {
            for gap in [0, 3] {
                let out = unequal_pair(a_def, b_def, "east_of", gap);
                let a = out.placements.get("site::s::a").expect("placement a");
                let b = out.placements.get("site::s::b").expect("placement b");
                assert_ne!(a.dims.x, b.dims.x, "a={a_def}, b={b_def}");
                assert_eq!(
                    b.origin,
                    (b_x + gap, 0, 0),
                    "a={a_def}, b={b_def}, gap={gap}"
                );
                let a_east = a.origin.0 + i32::try_from(a.dims.x).unwrap();
                assert_eq!(b.origin.0 - a_east, gap, "a={a_def}, b={b_def}, gap={gap}");
            }
        }
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

    #[test]
    fn a_row_refused_after_the_dedup_check_leaves_its_pair_to_the_next_row() {
        // The first row's `path=` is an abstract token, which lowers to
        // nothing without a registry pack, so that row lays no strip. The
        // second row names the same pair and a concrete block. It is not
        // a duplicate of a walkway that was never laid: it lays the strip,
        // and the only finding is the first row's own deferral.
        let src = village_pair_source(
            "  connect a.entry to b.entry path=@path.gravel\n  connect b.entry to a.entry path=@gravel\n",
        );
        let out = lowered(&src);
        let codes: Vec<DiagnosticCode> = out.diagnostics.iter().map(|d| d.code).collect();
        assert_eq!(
            codes,
            [DiagnosticCode::AbstractTokenDeferred],
            "{:#?}",
            out.diagnostics
        );
        let laid: Vec<&str> = out.walkways.keys().map(WalkwayScopeKey::as_str).collect();
        assert_eq!(laid, ["walkway::s::b.entry__a.entry"]);
        assert_eq!(out.walkways[0].path_material, "minecraft:gravel");
    }

    /// The `W_INVALID_WALKWAY_IDENT` findings on `out`, as `(primary, notes)`.
    fn invalid_walkway_ident_findings(out: &BlockArrayIr) -> Vec<(String, Vec<String>)> {
        out.diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::InvalidWalkwayIdent)
            .map(|d| {
                (
                    d.primary.clone(),
                    d.notes.iter().map(|n| n.message.clone()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn walkway_ids_with_an_edge_underscore_are_named_rather_than_aliased() {
        // `a.p_ → b.p` and `a.p → _b.p` differ as `(from, to)` pairs, so
        // `W_DUPLICATE_WALKWAY` has nothing to say, but a `_` at the edge
        // of `p_` or `_b` merges into the `__` separator: both would
        // encode to `walkway::s::a.p___b.p`, and the second row would
        // replace the first in the structure map with no finding. Each
        // row must instead be dropped with a finding naming its own
        // segment. The third row is sound and must still lay, so the
        // test cannot pass by dropping every walkway.
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def hut size=5x5:\n",
            "  walls id=w mat_slot=wall height=3\n",
            "  door  id=p  side=front at=center\n",
            "  door  id=p_ side=back  at=center\n",
            "\n",
            "site s:\n",
            "  place id=a  use=hut theme=t at=origin\n",
            "  place id=b  use=hut theme=t east_of=a gap=4\n",
            "  place id=_b use=hut theme=t north_of=a gap=4\n",
            "  connect a.p_ to b.p path=@gravel\n",
            "  connect a.p to _b.p path=@gravel\n",
            "  connect a.p to b.p path=@gravel\n",
        );
        let out = lowered(src);
        let findings = invalid_walkway_ident_findings(&out);
        let primaries: Vec<&str> = findings.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            primaries,
            [
                "walkway `a.p_ ↔ b.p` was dropped because the port id `p_` starts or ends \
                 with `_`",
                "walkway `a.p ↔ _b.p` was dropped because the place id `_b` starts or ends \
                 with `_`",
            ],
        );
        let notes: Vec<&[String]> = findings.iter().map(|(_, n)| n.as_slice()).collect();
        let rule = "a walkway's place and port ids may not start or end with `_` in either \
                    position, so the rule does not depend on which way the row is written";
        assert_eq!(
            notes,
            [
                [format!(
                    "{rule}; rename the port so it neither starts nor ends with `_`"
                )],
                [format!(
                    "{rule}; rename the place so it neither starts nor ends with `_`"
                )],
            ],
        );
        let laid: Vec<&str> = out.walkways.keys().map(WalkwayScopeKey::as_str).collect();
        assert_eq!(laid, ["walkway::s::a.p__b.p"]);
        let walkway_structures: Vec<&str> = out
            .structures
            .keys()
            .filter(|k| k.starts_with("walkway::"))
            .map(String::as_str)
            .collect();
        assert_eq!(walkway_structures, ["walkway::s::a.p__b.p"]);
    }

    #[test]
    fn a_port_named_underscore_is_named_rather_than_lowered_to_an_unparseable_key() {
        // A port called `_` gives `walkway::s::a.___b._`, which splits
        // back at the first `__` into an empty port; the artifact
        // namer then tripped a debug assertion on it. The row must be
        // dropped at lowering with a finding naming `_`, beside a sound
        // row that still lays.
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def hut size=5x5:\n",
            "  walls id=w mat_slot=wall height=3\n",
            "  door  id=_ side=front at=center\n",
            "  door  id=q side=back  at=center\n",
            "\n",
            "site s:\n",
            "  place id=a use=hut theme=t at=origin\n",
            "  place id=b use=hut theme=t east_of=a gap=4\n",
            "  connect a._ to b._ path=@gravel\n",
            "  connect a.q to b.q path=@gravel\n",
        );
        let out = lowered(src);
        let findings = invalid_walkway_ident_findings(&out);
        let primaries: Vec<&str> = findings.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            primaries,
            [
                "walkway `a._ ↔ b._` was dropped because the port id `_` starts or ends \
              with `_`"
            ],
        );
        let laid: Vec<&str> = out.walkways.keys().map(WalkwayScopeKey::as_str).collect();
        assert_eq!(laid, ["walkway::s::a.q__b.q"]);
        for key in out.walkways.keys() {
            assert_eq!(
                WalkwayScopeKey::parse(key.as_str()).as_ref(),
                Ok(key),
                "every laid walkway's key must parse back to itself",
            );
        }
    }

    #[test]
    fn a_walkway_id_containing_dunder_is_named_with_the_dunder_repair() {
        // The `__` arm of the same finding, which had no lowering test:
        // its note names the `__` repair, not the edge-`_` one.
        let src = concat!(
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "def hut size=5x5:\n",
            "  walls id=w mat_slot=wall height=3\n",
            "  door  id=b__c side=front at=center\n",
            "\n",
            "site s:\n",
            "  place id=a use=hut theme=t at=origin\n",
            "  place id=b use=hut theme=t east_of=a gap=4\n",
            "  connect a.b__c to b.b__c path=@gravel\n",
        );
        let out = lowered(src);
        let findings = invalid_walkway_ident_findings(&out);
        assert_eq!(findings.len(), 1, "{findings:?}");
        let (primary, notes) = &findings[0];
        assert_eq!(
            primary,
            "walkway `a.b__c ↔ b.b__c` was dropped because the port id `b__c` contains \
             `__`, the separator between the walkway scope key's `from` and `to` halves",
        );
        assert_eq!(
            notes,
            &[
                "a walkway's site, place and port ids may not contain `__`, so no id can be \
                 mistaken for the separator; rename the port so it has no `__`, e.g. by \
                 replacing `__` with `_`"
            ],
        );
        assert!(out.walkways.is_empty());
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
        // The `primary` string is part of the gcc-style
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

    /// The note of the one `W_WALKWAY_BLOCKED` `source` lowers to.
    fn walkway_blocked_note_of(source: &str) -> String {
        let out = lowered(source);
        let blocked: Vec<_> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::WalkwayBlocked)
            .collect();
        assert_eq!(blocked.len(), 1, "diagnostics={:?}", out.diagnostics);
        assert_eq!(blocked[0].notes.len(), 1, "{:?}", blocked[0]);
        blocked[0].notes[0].message.clone()
    }

    #[test]
    fn a_port_on_its_own_placements_pressure_plate_is_not_blamed_on_another_floor() {
        // Each hut lays a pressure plate on the cell in front of its own
        // door, which is that door's port cell. No other placement covers
        // either port, so pulling the placements apart cannot help.
        let note = walkway_blocked_note_of(concat!(
            "def hut size=5x5:\n",
            "  floor id=f mat_slot=wall\n",
            "  walls id=w mat_slot=wall height=3\n",
            "  roof  kind=flat mat_slot=wall overhang=1\n",
            "  door  id=e side=front at=center\n",
            "  pressure_plate id=pp at=front.outside offset=2 y=0\n",
            "\n",
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "site s:\n",
            "  place id=a use=hut theme=t at=origin\n",
            "  place id=b use=hut theme=t east_of=a gap=4\n",
            "  connect a.e to b.e path=@gravel\n",
        ));
        assert_eq!(
            note,
            "port `a.e` sits on the `minecraft:oak_pressure_plate` its own placement `a` lays \
             on that cell; move that block or the door/window; port `b.e` sits on the \
             `minecraft:oak_pressure_plate` its own placement `b` lays on that cell; move that \
             block or the door/window",
        );
    }

    #[test]
    fn a_port_buried_by_another_sites_placement_names_that_site() {
        // `u`'s `c` sits where `walkway_with_unroutable_port_source`'s
        // `c` does — over `a`'s back port — but in another site, so its
        // bare id would name nothing in site `s`.
        let source = walkway_with_unroutable_port_source()
            .replace("  place id=c use=front_home theme=t north_of=a gap=0\n", "")
            + concat!(
                "\n",
                "site u:\n",
                "  place id=o use=front_home theme=t at=origin\n",
                "  place id=c use=front_home theme=t north_of=o gap=0\n",
            );
        let note = walkway_blocked_note_of(&source);
        assert_eq!(
            note,
            "port `a.back` is buried inside the floor of placement `c` in site `u`; move that \
             door/window to an unobstructed wall or pull the placements apart",
        );
    }

    #[test]
    fn a_router_past_the_area_cap_names_the_placements_that_stretch_its_box() {
        // `c` and `d` are four blocks apart; the router's box is wide
        // because `a` and `b` sit two thousand blocks off. `e1` reaches
        // the box's west edge, `e2` sits inside it and sets no edge, and
        // `e3`, north of everything, alone sets its north edge.
        let note = walkway_blocked_note_of(concat!(
            "def hut size=3x3:\n",
            "  floor id=f mat_slot=wall\n",
            "  walls id=w mat_slot=wall height=3\n",
            "  door  id=front side=front at=center\n",
            "  door  id=back  side=back  at=center\n",
            "\n",
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "site s:\n",
            "  place id=a use=hut theme=t at=origin\n",
            "  place id=b use=hut theme=t north_of=a gap=2000\n",
            "  place id=c use=hut theme=t east_of=b gap=2000\n",
            "  place id=d use=hut theme=t north_of=c gap=4\n",
            "  place id=e1 use=hut theme=t north_of=a gap=500\n",
            "  place id=e2 use=hut theme=t east_of=e1 gap=500\n",
            "  place id=e3 use=hut theme=t north_of=e2 gap=1600\n",
            "  connect c.front to d.back path=@gravel\n",
        ));
        assert_eq!(
            note,
            format!(
                "the router searches the box spanning both ports and every placement floor on \
                 the walk plane, here 4238888 cells, past its cap of {ROUTE_AREA_CAP} cells; \
                 the two ports span only 42 of those cells, and the floors of placements `a`, \
                 `b`, `c`, `d`, `e1`, and `e3` stretch the box past them, so bring those \
                 placements closer to the two ports",
            ),
        );
    }

    #[test]
    fn edge_pushers_names_each_floor_that_sets_an_edge_beyond_both_ports() {
        // Ports at x=0..=10, z=0..=10. Each of `w`, `e`, `n` and `s`
        // alone sets one edge of the rectangle, beyond the ports; `in`
        // sets none; `tie` reaches the east edge only where a port does,
        // so it stretches nothing; `other_plane` meets the east edge on
        // another walk plane.
        let extent = |min_x, max_x, min_z, max_z| FloorExtent {
            y: 0,
            min_x,
            max_x,
            min_z,
            max_z,
        };
        let extents: IndexMap<String, FloorExtent> = [
            ("w", extent(-20, -18, 4, 6)),
            ("e", extent(28, 30, 4, 6)),
            ("n", extent(4, 6, -40, -38)),
            ("s", extent(4, 6, 48, 50)),
            ("in", extent(2, 4, 2, 4)),
            ("tie", extent(8, 10, 4, 6)),
            ("other_plane", extent(28, 30, 20, 22)),
        ]
        .into_iter()
        .map(|(k, e)| {
            let e = if k == "other_plane" {
                FloorExtent { y: 7, ..e }
            } else {
                e
            };
            (k.to_owned(), e)
        })
        .collect();
        let rect = SearchRect {
            min_x: -21,
            max_x: 31,
            min_z: -41,
            max_z: 51,
        };
        assert_eq!(
            edge_pushers(rect, (0, 0, 0), (10, 0, 10), &extents),
            vec!["w", "e", "n", "s"],
        );
        // An edge a port sets is not stretched by a floor that only meets
        // it there.
        let to_tie = SearchRect { max_x: 11, ..rect };
        assert_eq!(
            edge_pushers(to_tie, (0, 0, 0), (10, 0, 10), &extents),
            vec!["w", "n", "s"],
        );
    }

    #[test]
    fn a_router_past_the_area_cap_on_distant_ports_blames_the_ports() {
        // The floor plan is `p`'s nine cells; the two ports are 1999
        // apart on each axis, so the straight L is exactly at the cap and
        // the router's margin takes it past. Moving `p` cannot help.
        let note = walkway_blocked_note_of(concat!(
            "def hut size=3x3:\n",
            "  walls id=w mat_slot=wall height=3\n",
            "  door  id=front side=front at=center\n",
            "  door  id=back  side=back  at=center\n",
            "\n",
            "def pad size=3x3:\n",
            "  floor id=f mat_slot=wall\n",
            "  walls id=w mat_slot=wall height=3\n",
            "\n",
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "site s:\n",
            "  place id=a use=hut theme=t at=origin\n",
            "  place id=q use=hut theme=t east_of=a gap=1996\n",
            "  place id=p use=pad theme=t north_of=q gap=500\n",
            "  place id=b use=hut theme=t north_of=q gap=1992\n",
            "  connect a.front to b.back path=@gravel\n",
        ));
        assert_eq!(
            note,
            format!(
                "the router searches the box spanning both ports and every placement floor on \
                 the walk plane, here 4010006 cells, past its cap of {ROUTE_AREA_CAP} cells; \
                 the two ports alone span 4008004 of those cells, so place the two structures \
                 closer together",
            ),
        );
    }

    #[test]
    fn a_straight_l_past_the_area_cap_is_not_laid_and_says_why() {
        // The two ports alone span more than the cap, so the row is
        // refused before the router or the voxel buffer is reached.
        let out = lowered(concat!(
            "def hut size=3x3:\n",
            "  walls id=w mat_slot=wall height=3\n",
            "  door  id=front side=front at=center\n",
            "  door  id=back  side=back  at=center\n",
            "\n",
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "site s:\n",
            "  place id=a use=hut theme=t at=origin\n",
            "  place id=q use=hut theme=t east_of=a gap=2100\n",
            "  place id=b use=hut theme=t north_of=q gap=2100\n",
            "  connect a.front to b.back path=@gravel\n",
        ));
        let blocked: Vec<_> = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::WalkwayBlocked)
            .collect();
        assert_eq!(blocked.len(), 1, "{:?}", out.diagnostics);
        let area = l_path_area((1, 0, 3), (2104, 0, -2104));
        assert_eq!(
            blocked[0].primary,
            format!(
                "walkway `a.front ↔ b.back` spans {area} cells, past the {ROUTE_AREA_CAP}-cell \
                 router cap, and was not laid"
            ),
        );
        let notes: Vec<&str> = blocked[0]
            .notes
            .iter()
            .map(|n| n.message.as_str())
            .collect();
        assert_eq!(
            notes,
            [format!(
                "the walkway search area ({area} cells) exceeds the router's cap of \
                 {ROUTE_AREA_CAP} cells; place the two structures closer together"
            )
            .as_str()],
        );
        assert_eq!(blocked[0].data, None);
        assert!(out.walkways.is_empty());
    }

    #[test]
    fn a_port_on_its_own_plate_and_another_floor_names_both() {
        // `o`, in another site, lays a floor over `a`'s port cell, where
        // `a`'s own plate already sits. Pulling the placements apart
        // leaves the plate, and moving the plate leaves the floor, so the
        // note names both.
        let note = walkway_blocked_note_of(concat!(
            "def hut size=5x5:\n",
            "  floor id=f mat_slot=wall\n",
            "  walls id=w mat_slot=wall height=3\n",
            "  roof  kind=flat mat_slot=wall overhang=1\n",
            "  door  id=e side=front at=center\n",
            "  pressure_plate id=pp at=front.outside offset=2 y=0\n",
            "\n",
            "def slab size=7x7:\n",
            "  floor id=f mat_slot=wall\n",
            "\n",
            "theme t:\n",
            "  slot wall -> @cobblestone\n",
            "\n",
            "site s:\n",
            "  place id=a use=hut theme=t at=origin\n",
            "  place id=b use=hut theme=t east_of=a gap=4\n",
            "  connect a.e to b.e path=@gravel\n",
            "\n",
            "site u:\n",
            "  place id=o use=slab theme=t at=origin\n",
        ));
        assert_eq!(
            note,
            "port `a.e` is buried inside the floor of placement `o` in site `u` and sits on the \
             `minecraft:oak_pressure_plate` its own placement `a` lays on that cell; move that \
             block, and move that door/window to an unobstructed wall or pull the placements \
             apart; port `b.e` sits on the `minecraft:oak_pressure_plate` its own placement `b` \
             lays on that cell; move that block or the door/window",
        );
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
        assert_eq!(diag.severity(), Severity::Warning);
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
        // Pack supplied but does not declare `@walkway.grvl`;
        // `spec/materials-themes` "Canonical vocabulary"
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
        assert_eq!(diag.severity(), Severity::Error);
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
        // Two ways to arrive at an overhang of zero. The first is the
        // author writing none. The second is a roof that will not draw,
        // which contributes none however large its `overhang=` — and an
        // author who wrote `overhang=2` must not be told the key is
        // missing.
        for roof in ["roof kind=flat mat_slot=r", "roof mat_slot=r overhang=2"] {
            let src = format!(
                "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  {roof}\n  stair id=e kind=stairs mat_slot=r side=front half=top facing=out\n"
            );
            let out = lowered(&src);
            let reason = out
                .diagnostics
                .iter()
                .find(|d| d.primary.contains("eave `stair`"))
                .unwrap_or_else(|| {
                    panic!(
                        "expected the eave to defer under `{roof}`, got {:?}",
                        out.diagnostics
                    )
                })
                .primary
                .clone();
            assert!(
                reason.contains("draws an overhang"),
                "the reason must name the overhang a roof draws, not the key the author may well have written: {reason}",
            );
        }
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
    fn window_carve_stops_at_the_top_of_the_wall_not_the_top_of_the_volume() {
        // walls height=3 fills rows 1..=3, roof kind=flat puts a deck at
        // y=4 and takes dims.y to 5. A mat_slot-less window at y=3 size=1x2
        // carves to air, so gating on the volume rather than on the wall
        // would punch a hole through the deck. It must defer.
        let src = "theme t:\n  slot w -> @cobblestone\n  slot r -> @spruce_stairs\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  roof kind=flat mat_slot=r\n  window side=front y=3 size=1x2\n";
        let out = lowered(src);
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary
                    == "window rows y=3..=4 are not all inside one wall course (size=1x2; the walls occupy y=1..=3)"),
            "expected wall-column defer, got {:?}",
            out.diagnostics,
        );
    }

    // --- carve_door course cap ----------------------------------------------

    #[test]
    fn door_defers_when_its_level_sits_at_or_above_the_wall_it_would_carve() {
        // struct walls height=3 (rows 1..=3) with a door inside `level
        // y=3`: the door opens at world y=4, which is above the course
        // and inside no other. The defer names the row and the rows the
        // walls do occupy instead of silently painting AIR over air.
        let src = "theme t:\n  slot w -> @cobblestone\n\nstruct s size=5x5\n  walls mat_slot=w height=3\n  level id=roofline y=3\n    door id=hole side=front at=center\n";
        let out = lowered(src);
        assert!(
            out.diagnostics.iter().any(|d| d.primary
                == "door opens at y=4, which is not inside any wall course (the walls occupy y=1..=3)"),
            "expected a wall-course defer, got {:?}",
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
        // The front wall's middle cell is (1, 0, 2); one step in is the
        // struct's only interior cell.
        let src = "theme t:\n  slot p -> @oak_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=p height=2\n  pressure_plate at=inside.front offset=1 y=0\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(block_id(ba, 1, 0, 1), "minecraft:oak_pressure_plate");
        assert_eq!(deferred_count(&out), 0);
    }

    #[test]
    fn pressure_plate_mat_slot_resolves_into_the_palette() {
        // A `mat_slot=` bound to a canonical id must land in the palette
        // verbatim — the default `oak_pressure_plate` fallback only
        // fires when no binding resolves.
        let src = "theme t:\n  slot fixture -> @spruce_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=fixture height=2\n  pressure_plate mat_slot=fixture at=inside.front offset=1 y=0\n";
        let out = lowered(src);
        let ba = out.structures.get("struct::s").unwrap();
        let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
        assert!(
            ids.contains(&"minecraft:spruce_pressure_plate"),
            "palette should carry the resolved id, got {ids:?}",
        );
        assert_eq!(block_id(ba, 1, 0, 1), "minecraft:spruce_pressure_plate");
    }

    /// Lower `size=<size>` with `members` (each line indented, cobblestone
    /// on the `wall` slot) and one plate line, and return the output with
    /// the `W_DEFERRED_MEMBER` primaries.
    fn lowered_plate(size: &str, members: &str, plate: &str) -> (BlockArrayIr, Vec<String>) {
        let src = format!(
            "theme t:\n  slot wall -> @cobblestone\n\nstruct s size={size}\n{members}  \
             pressure_plate {plate}\n",
        );
        let out = lowered(&src);
        let reasons = out
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::DeferredMember)
            .map(|d| d.primary.clone())
            .collect();
        (out, reasons)
    }

    /// [`lowered_plate`] with `walls height=3` ahead of `extra`.
    fn lowered_inside_plate(size: &str, extra: &str, plate: &str) -> (BlockArrayIr, Vec<String>) {
        lowered_plate(
            size,
            &format!("  walls mat_slot=wall height=3\n{extra}"),
            plate,
        )
    }

    fn plate_count(ba: &BlockArray) -> usize {
        ba.voxels
            .iter()
            .filter(|v| ba.palette.entries[usize::from(v.0)].id == PRESSURE_PLATE_BASE_ID)
            .count()
    }

    /// The wall rings the inside-plate tests run against: the footprint's
    /// own edge, and the ring a roof overhang of 1 or 2 moves inward.
    const OVERHANGS: [u32; 3] = [0, 1, 2];

    /// The member line that gives a struct `overhang`, empty for none.
    fn overhang_roof(overhang: u32) -> String {
        if overhang == 0 {
            String::new()
        } else {
            format!("  roof kind=flat mat_slot=wall overhang={overhang}\n")
        }
    }

    #[test]
    fn pressure_plate_inside_at_a_corner_defers_and_keeps_the_side_wall() {
        // The front wall of a 5x5 struct is z=4; one step in from its
        // offset=0 end is (0, 1, 3), a block of the left wall.
        let (out, reasons) =
            lowered_inside_plate("5x5", "", "id=p at=inside.front offset=0 y=1 -> sig.a");
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(
            reasons,
            vec![
                "pressure_plate `at=inside.front offset=0` is at a corner of the front wall, so \
                 the voxel inside it belongs to the neighbouring wall; use an `offset=` from 1 to \
                 3 to reach an interior voxel. Its signal binding still reaches the netlist, with \
                 no plate placed to drive it"
                    .to_owned()
            ],
        );
        assert_eq!(block_id(ba, 0, 1, 3), "minecraft:cobblestone");
        assert_eq!(plate_count(ba), 0);
    }

    #[test]
    fn pressure_plate_inside_at_a_corner_defers_at_the_floor_row_too() {
        // At y=0 the corner cell is floor under the side wall. The
        // `<side>.outside` foundation fallback is y=0-only; inside has no
        // such exemption. No wall is painted at row 0, so the reason does
        // not claim one.
        let (out, reasons) = lowered_inside_plate("5x5", "", "at=inside.front offset=0 y=0");
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(
            reasons,
            vec![
                "pressure_plate `at=inside.front offset=0` is at a corner of the front wall, so \
                 the voxel inside it is not an interior cell of this struct; use an `offset=` \
                 from 1 to 3 to reach an interior voxel"
                    .to_owned()
            ],
        );
        assert_eq!(plate_count(ba), 0);
    }

    #[test]
    fn pressure_plate_inside_at_a_corner_claims_no_wall_where_none_is_painted() {
        // The ring is decided from the footprint, so a corner is refused
        // at every row — but only a row the walls paint has a wall there
        // to name: not a floor-only struct, and not the air between two
        // courses (rows 1 and 5 here).
        let cases = [
            ("  floor mat_slot=wall\n", 0),
            (
                "  walls mat_slot=wall height=1\n  level y=4\n    walls mat_slot=wall height=1\n",
                3,
            ),
        ];
        for (members, y) in cases {
            let (out, reasons) =
                lowered_plate("5x5", members, &format!("at=inside.front offset=0 y={y}"));
            let ba = out.structures.get("struct::s").unwrap();
            assert_eq!(
                reasons,
                vec![
                    "pressure_plate `at=inside.front offset=0` is at a corner of the front wall, \
                     so the voxel inside it is not an interior cell of this struct; use an \
                     `offset=` from 1 to 3 to reach an interior voxel"
                        .to_owned()
                ],
                "{members:?} y={y}",
            );
            assert_eq!(plate_count(ba), 0);
        }
    }

    #[test]
    fn pressure_plate_inside_at_a_corner_of_a_long_left_wall_names_that_wall() {
        // On a 4x6 footprint the left wall runs along `size.h`, so both
        // its name and its offset range differ from the front wall's.
        let (out, reasons) = lowered_inside_plate("4x6", "", "at=inside.left offset=5 y=1");
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(
            reasons,
            vec![
                "pressure_plate `at=inside.left offset=5` is at a corner of the left wall, so \
                 the voxel inside it belongs to the neighbouring wall; use an `offset=` from 1 to \
                 4 to reach an interior voxel"
                    .to_owned()
            ],
        );
        assert_eq!(plate_count(ba), 0);
    }

    /// The four sides of a 4x5 footprint with the length of each wall:
    /// front and back run along `size.w`, left and right along `size.h`.
    const SIDES_4X5: [(&str, u32); 4] = [("front", 4), ("back", 4), ("left", 5), ("right", 5)];

    #[test]
    fn pressure_plate_inside_defers_at_both_corners_of_every_side() {
        // Each side mirrors `offset=` differently, so both ends of every
        // wall are checked: a refusal keyed on one axis or one end would
        // let the other three walls or the far corner through. Under an
        // overhang the wall ring moves in, so a bound measured from the
        // volume's edge rather than the ring would let the corner in.
        for overhang in OVERHANGS {
            for (side, length) in SIDES_4X5 {
                for offset in [0, length - 1] {
                    let (out, reasons) = lowered_inside_plate(
                        "4x5",
                        &overhang_roof(overhang),
                        &format!("at=inside.{side} offset={offset} y=1"),
                    );
                    let ba = out.structures.get("struct::s").unwrap();
                    let case = format!("overhang={overhang} inside.{side} offset={offset}");
                    assert_eq!(
                        reasons,
                        vec![format!(
                            "pressure_plate `at=inside.{side} offset={offset}` is at a corner of \
                             the {side} wall, so the voxel inside it belongs to the neighbouring \
                             wall; use an `offset=` from 1 to {} to reach an interior voxel",
                            length - 2,
                        )],
                        "{case}",
                    );
                    assert_eq!(plate_count(ba), 0, "{case} painted");
                }
            }
        }
    }

    #[test]
    fn pressure_plate_inside_paints_at_every_non_corner_offset_of_every_side() {
        // The other half of the corner rule: every offset between the two
        // corners reaches a genuine interior cell and paints without a
        // word. A 4x5 footprint keeps the two axes' lengths distinct, and
        // the cells are those of the footprint with no overhang.
        let cells: [&[(u32, u32)]; 4] = [
            &[(1, 3), (2, 3)],
            &[(2, 1), (1, 1)],
            &[(1, 1), (1, 2), (1, 3)],
            &[(2, 3), (2, 2), (2, 1)],
        ];
        for overhang in OVERHANGS {
            for ((side, length), cells) in SIDES_4X5.into_iter().zip(cells) {
                assert_eq!(
                    cells.len(),
                    usize::try_from(length - 2).unwrap(),
                    "{side} has one cell per non-corner offset",
                );
                for (offset, &(x, z)) in (1..length - 1).zip(cells) {
                    let (out, reasons) = lowered_inside_plate(
                        "4x5",
                        &overhang_roof(overhang),
                        &format!("at=inside.{side} offset={offset} y=1"),
                    );
                    let ba = out.structures.get("struct::s").unwrap();
                    let case = format!("overhang={overhang} inside.{side} offset={offset}");
                    assert_eq!(reasons, Vec::<String>::new(), "{case}");
                    let (x, z) = (x + overhang, z + overhang);
                    assert_eq!(
                        block_id(ba, x, 1, z),
                        PRESSURE_PLATE_BASE_ID,
                        "{case} should land on ({x}, 1, {z})",
                    );
                }
            }
        }
    }

    #[test]
    fn pressure_plate_inside_a_two_deep_struct_defers_and_keeps_the_back_wall() {
        // With size.h = 2 the cell one step in from the front wall is the
        // back wall. With no overhang `front.outside` would bury the plate
        // under the wall at y=0 and find no cell above it, so the reason
        // does not offer it.
        let (out, reasons) =
            lowered_inside_plate("5x2", "", "id=p at=inside.front offset=2 y=1 -> sig.a");
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(
            reasons,
            vec![
                "pressure_plate `at=inside.front offset=2`: the struct's size.h is 2, so it has \
                 no interior voxel; an interior needs a size of at least 3 on both axes. Grow \
                 `size.h` to at least 3. Its signal binding still reaches the netlist, with no \
                 plate placed to drive it"
                    .to_owned()
            ],
        );
        assert_eq!(block_id(ba, 2, 1, 0), "minecraft:cobblestone");
        assert_eq!(plate_count(ba), 0);
    }

    #[test]
    fn pressure_plate_inside_a_one_deep_struct_with_an_overhang_defers() {
        // With size.h = 1 the front and back walls are one row (z=1 under
        // `overhang=1`), and the inward step lands in the overhang ring
        // behind the building at z=0.
        let roof = overhang_roof(1);
        let (out, reasons) = lowered_inside_plate("5x1", &roof, "at=inside.front offset=2 y=1");
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(
            reasons,
            vec![
                "pressure_plate `at=inside.front offset=2`: the struct's size.h is 1, so it has \
                 no interior voxel; an interior needs a size of at least 3 on both axes. Grow \
                 `size.h` to at least 3, or anchor the plate with `at=front.outside`"
                    .to_owned()
            ],
        );
        assert_eq!(block_id(ba, 3, 1, 0), "minecraft:air");
        assert_eq!(plate_count(ba), 0);
        // The remedy the reason offers works on this very struct.
        let (out, reasons) = lowered_inside_plate("5x1", &roof, "at=front.outside offset=2 y=1");
        assert_eq!(reasons, Vec::<String>::new());
        assert_eq!(plate_count(out.structures.get("struct::s").unwrap()), 1);
    }

    #[test]
    fn pressure_plate_inside_a_narrow_struct_names_the_narrow_axis() {
        // The front wall of a 2-wide struct is two corners with nothing
        // between them, so no offset can help until the width grows.
        let (out, reasons) = lowered_inside_plate("2x5", "", "at=inside.front offset=1 y=1");
        let ba = out.structures.get("struct::s").unwrap();
        assert_eq!(
            reasons,
            vec![
                "pressure_plate `at=inside.front offset=1`: the struct's size.w is 2, so it has \
                 no interior voxel, and `offset=1` is at a corner of the front wall besides; an \
                 interior needs a size of at least 3 on both axes. Grow `size.w` to at least 3 \
                 and use an `offset=` away from both ends of the wall"
                    .to_owned()
            ],
        );
        assert_eq!(plate_count(ba), 0);
    }

    #[test]
    fn pressure_plate_inside_reports_a_thin_axis_and_a_corner_in_one_pass() {
        // `2x9 at=inside.left offset=0` is both too narrow and a corner:
        // one reason names both, so widening alone is not the whole fix.
        // With both axes thin, both are named.
        let cases = [
            (
                "2x9",
                "at=inside.left offset=0 y=1",
                "pressure_plate `at=inside.left offset=0`: the struct's size.w is 2, so it has \
                 no interior voxel, and `offset=0` is at a corner of the left wall besides; an \
                 interior needs a size of at least 3 on both axes. Grow `size.w` to at least 3 \
                 and use an `offset=` from 1 to 7",
            ),
            (
                "2x2",
                "at=inside.front offset=0 y=1",
                "pressure_plate `at=inside.front offset=0`: the struct's size.w is 2 and size.h \
                 is 2, so it has no interior voxel, and `offset=0` is at a corner of the front \
                 wall besides; an interior needs a size of at least 3 on both axes. Grow \
                 `size.w` to at least 3 and `size.h` to at least 3 and use an `offset=` away \
                 from both ends of the wall",
            ),
        ];
        for (size, plate, expected) in cases {
            let (out, reasons) = lowered_inside_plate(size, "", plate);
            assert_eq!(reasons, vec![expected.to_owned()], "{size} {plate}");
            assert_eq!(plate_count(out.structures.get("struct::s").unwrap()), 0);
        }
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
        // offset=3 is the first column past a 3-length wall: the boundary
        // `wall_local_to_grid` refuses on, so it has to be refused here
        // first — the arm after the helper call asserts rather than reports
        // (`INVARIANT(wall-grid-validated)`).
        let out = lowered(
            "theme t:\n  slot p -> @oak_pressure_plate\n\nstruct s size=3x3\n  walls mat_slot=p height=2\n  pressure_plate at=inside.front offset=3 y=0\n",
        );
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.primary.contains("`offset=3` runs past the front wall")),
            "expected the boundary offset to be refused, got {:?}",
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

    #[test]
    fn nonneg_int_rejects_values_that_do_not_fit_in_u32() {
        // 5_000_000_000 exceeds u32::MAX (~4.29 * 10^9); it defers via
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
