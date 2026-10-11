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
//!
//! [`flatten_members`]: members::flatten_members

mod anchor;
mod body;
mod canvas;
mod circuit;
mod connect;
mod diag;
mod door;
mod members;
mod plate;
mod roof_fill;
mod site_floor;
mod sizing;
mod stair;
mod token_diag;
mod window;

use std::collections::HashSet;

use indexmap::IndexMap;

use crate::diagnostic::{Diagnostic, sort_by_position};
use crate::ids::{PlaceId, SiteName, WalkwayScopeKey};
use crate::intent::{DefIr, IntentModule, MemberRole, SiteIr, StructIr};
use crate::resolve::{Resolution, ScopeResolution, place_scope_key};

use super::args::diag_deferred_member_reason;
use super::material::TargetRegistry;
use super::roof::{FLAT_BASE_ID, STAIR_BASE_ID};
use super::{BlockArray, BlockArrayIr, BlockState, PALETTE_CAPACITY, Placement, Walkway};

use anchor::{GapOutcome, report_unread_gap, resolve_place_anchor};
use body::{BodyDescriptor, BodyInputs, LoweredBody, VoxelSource, lower_body_to_block_array};
use connect::{ConnectInputs, lower_connects};
use diag::{diag_def_no_size, diag_struct_no_size};
use site_floor::{PlacedBody, collect_floor_cells};

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
/// `cairn-lang-formats/tests/packs/ids_exist.rs::every_id_the_examples_intern_exists_in_its_target`,
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
    lower_to_block_array_within(intent, resolution, registry, PALETTE_CAPACITY)
}

/// [`lower_to_block_array`], with each body's palette refusing a new
/// state once it holds `palette_capacity` entries, air included, rather
/// than [`PALETTE_CAPACITY`]; a larger capacity is clamped to that one.
///
/// [`lower_to_block_array`] passes [`PALETTE_CAPACITY`], the only capacity
/// the compiler lowers at. This module's tests pass one small enough to
/// reach `W_PALETTE_TOO_LARGE` without painting 65,536 states, and so run
/// the same code the public entry point does.
fn lower_to_block_array_within(
    intent: &IntentModule,
    resolution: &Resolution,
    registry: Option<&dyn TargetRegistry>,
    palette_capacity: usize,
) -> BlockArrayIr {
    let mut structures: IndexMap<String, BlockArray> = IndexMap::new();
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let bodies = BodyInputs {
        registry,
        palette_capacity,
    };

    for s in &intent.structs {
        let key = format!("struct::{}", s.name);
        let scope = resolution.scopes.get(&key);
        // `lower_struct` returns `None` only after it has already pushed a
        // diagnostic (no `size=`, etc.), so the skip here is silent on
        // purpose — diagnosing twice would teach a reader the struct had
        // two unrelated problems instead of one.
        if let Some(ba) = lower_struct(s, scope, bodies, &mut diagnostics) {
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
            bodies,
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

    // Report in source order, the way `check::DiagnosticSink` does. This
    // pass emits in pass order — flatten before dims before phases, scope
    // after scope — so which finding comes first has never tracked which
    // line comes first, and a reader working down a file has to jump
    // around. The sort is stable, so two findings on one span keep the
    // order the passes raised them in.
    sort_by_position(&mut diagnostics);
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
    // its theme for exactly this reason. The `lowering` module of
    // `tests/resolve/def_member.rs` holds the cases that must stay apart, and `resolve::resolver`'s module
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

fn lower_struct<'a>(
    s: &StructIr,
    scope: Option<&'a ScopeResolution>,
    bodies: BodyInputs<'a>,
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
        bodies,
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
    bodies: BodyInputs<'a>,
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
            bodies,
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
            // The body was refused, for its extent or for its palette; the
            // diagnostic names the scope, and recording a placement for a
            // structure that does not exist would leave the lockfile
            // pointing at nothing.
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

#[cfg(test)]
mod tests;
