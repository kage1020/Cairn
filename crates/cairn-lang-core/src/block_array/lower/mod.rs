//! Intent IR → block-array IR lowering.
//!
//! The pass is total: every struct ends up in
//! [`BlockArrayIr::structures`] and every issue is a warning on
//! [`BlockArrayIr::diagnostics`], so `cairn lower` on a half-finished
//! module still shows what did lower and says what was skipped and why.
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
//! This pass implements those four. The section goes on to three redstone
//! phases, which `cairn-lang-redstone` owns, and ends with `raw`, which the
//! surface does not accept: a `raw` line is `E_UNKNOWN_KEYWORD` from the
//! allowlist pass.
//!
//! Within a phase source order wins (the last-wins rule for local
//! overrides), and two members of one phase contesting a voxel earn
//! `W_PHASE_CONFLICT`. Other roles emit `W_DEFERRED_MEMBER` and skip.
//! `level y=N` blocks are flattened into their children before sizing, so
//! a nested member reaches both the dim math and its phase with the
//! level's `y=` applied as an authored offset. That one flattened list is
//! the pass's paint set (see [`flatten_members`]).
//!
//! `walls` and `roof` shape the volume (the walls its height, the roof its
//! overhang and headroom); `floor`, `door`, `window`, `stair` and
//! `pressure_plate` are authored against that volume and checked against
//! it when their phase paints them. So being in the list does not mean the
//! dims read a member, only that the dims and the paint pass see the same
//! members.
//!
//! A `def` lowers only through a `site`'s `place ... use=def_name`, once
//! per placement under the theme that placement bound, so its members are
//! reached once per placement; `lower_to_block_array` therefore drops
//! findings that repeat.
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
/// Nothing checks these against the target the way `E_UNKNOWN_ID` checks
/// an authored id, and no pack can respell them, since none reads a
/// catalog token. So they must be valid on every supported target of every
/// edition, and a pack-side test holds them to that.
///
/// A hardcoded id missing from this list is caught by
/// `cairn-lang-formats/tests/packs/ids_exist.rs::every_id_the_examples_intern_exists_in_its_target`,
/// which reads the palettes of lowered examples; this list covers the ids
/// no example is obliged to reach.
///
/// `PRESSURE_PLATE_BASE_ID` is absent because it is redirectable (through
/// `PRESSURE_PLATE_TOKEN`), which makes it a question for the pack-side
/// tests.
pub const BUILTIN_BLOCK_IDS: &[&str] = &[BlockState::AIR_ID, STAIR_BASE_ID, FLAT_BASE_ID];

/// Lower every `struct` in `intent` into a [`BlockArray`].
///
/// Material lookups go through each struct's [`ScopeResolution`] in
/// `resolution`, the theme bindings `cairn check` and `cairn info` see.
/// Members are processed in phase order (massing → envelope → openings →
/// fixtures), so a `door` written before `walls` still cuts the wall.
/// Other roles are reported as `W_DEFERRED_MEMBER` and skipped.
///
/// `registry` is the registry-pack view of the compile's target. `Some`
/// turns `@floor.wood.broadleaf`-style tokens into concrete ids, fails loud
/// on misses, and, when the run pinned a `--target`, refuses any id that
/// target does not declare (`E_UNKNOWN_ID`). With `None` every abstract
/// token degrades to a `W_ABSTRACT_TOKEN_DEFERRED` warning, so library
/// callers without a pack still get a partial build.
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
/// Tests pass a capacity small enough to reach `W_PALETTE_TOO_LARGE`
/// without painting 65,536 states, through the same code the public entry
/// point runs.
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
        // `lower_struct` has already diagnosed a `None`.
        if let Some(ba) = lower_struct(s, scope, bodies, &mut diagnostics) {
            // First write wins on a duplicate name, matching `resolve`'s
            // `FIRST_BINDING_WINS`: `resolution.scopes` bound the first
            // body, so taking the last would paint it with the first
            // body's materials.
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
    // Walkways come after every site so the collision set covers every
    // floor tile a strip might cross.
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

    // The passes emit in pass order (flatten, dims, phases, scope after
    // scope); report in source order, as `check::DiagnosticSink` does. The
    // sort is stable, so findings on one span keep pass order.
    sort_by_position(&mut diagnostics);
    // Two diagnostics equal in code, span, message, notes and data are one
    // finding reported twice: a `def` body is lowered once per `place`, so
    // a finding about the def, or the theme `slot` line it reads, comes
    // back byte-for-byte once per placement.
    //
    // This holds only while every finding that is not a repeat differs in
    // its own text, an obligation on the messages rather than a property
    // of the walk: `geometry_material_id`'s deferral names its theme for
    // this reason. The `lowering` module of `tests/resolve/def_member.rs`
    // holds the cases that must stay apart, and `resolve::resolver`'s
    // module doc says why that stage keeps ledgers instead.
    //
    // `retain` keeps the survivors in order.
    let mut said: HashSet<Diagnostic> = HashSet::new();
    diagnostics.retain(|d| said.insert(d.clone()));

    BlockArrayIr {
        structures,
        // The wall columns stop here: they let the `connect` pass ask a
        // placement's masonry what the openings phase asked, and that pass
        // has run.
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

/// Lower every `place` in `site` into its own [`BlockArray`] and a
/// [`Placement`] carrying its world-space origin, both keyed
/// `site::SITE::PLACE_ID`.
///
/// The resolver has applied each place's `theme=` (the `place_scope_key`
/// scope) and emitted every fail-loud diagnostic (`E_UNRESOLVED_PLACE_REF`,
/// `E_UNRESOLVED_THEME_REF`, `E_DUPLICATE_PLACE_ID`,
/// `E_INVALID_PLACE_ORIGIN`); this pass solves `at=origin`,
/// `east_of=ID gap=N` and `north_of=ID gap=N` into absolute origins.
/// `connect` rows are left to [`lower_connects`].
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
            // Lowered after every placement lands, so walkways
            // collide-check against the finished floor plan.
            continue;
        }
        if !matches!(member.role, MemberRole::Place) {
            continue;
        }
        let Some(place_id) = member.id.as_deref() else {
            continue;
        };
        // `id=` takes a string literal, so only the resolver's
        // `E_INVALID_PLACE_ID` constrains it: reachable, already reported,
        // skipped silently.
        let Ok(placement_id) = PlaceId::new(place_id) else {
            continue;
        };
        // A site name is a lexer `Ident`, a strict subset of what
        // `SiteName` accepts. The `debug_assert!` makes a change to the
        // site-name grammar fail loud in tests rather than silently drop
        // every place in the site.
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
            // `resolve_site_placements` built no scope for this row and has
            // already said why: a missing or mistyped `use=`/`theme=`, an
            // unresolved reference or an invalid origin.
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
            diagnostics.push(diag_def_no_size(def));
            continue;
        };

        // The anchor looks prior places up in `placed`, so before this one
        // is inserted. A prior place that took any `continue` arm of this
        // loop is missing there; falling back to `(0, 0, 0)` would stack
        // this placement on the anchor's origin, so the row is deferred
        // instead. Only the origin waits for the lowered dims.
        //
        // An unreadable `gap=` is reported on every path out of this row,
        // with a note saying which: see `read_or_ignore` for why it is
        // never held back.
        let (anchor, gap_unread) = resolve_place_anchor(member, placed, &site.name);

        // Lowered even when the anchor did not lower, and its findings go
        // straight out. The voxels are body-local and take nothing from the
        // origin, so each finding is a defect in the `def` or theme the
        // author must fix wherever the row lands. Holding them back would
        // delay them a compile, and a `def` placed only by refused rows
        // would never report them.
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
        let Some(anchor) = anchor else {
            diagnostics.push(diag_deferred_member_reason(
                member,
                "the prior place referenced by `east_of=`/`north_of=` did not lower, so this placement's origin cannot be resolved",
            ));
            report_unread_gap(gap_unread, GapOutcome::NotPlaced, diagnostics);
            continue;
        };
        let Some(LoweredBody { array, walls, cut }) = body else {
            // Refused for its extent or palette, already diagnosed. A
            // placement for a structure that does not exist would leave the
            // lockfile pointing at nothing.
            report_unread_gap(gap_unread, GapOutcome::NotPlaced, diagnostics);
            continue;
        };
        let dims = array.dims;
        // `north_of` needs this body's own depth.
        let origin = match anchor.origin(dims) {
            Ok(origin) => origin,
            Err(refusal) => {
                diagnostics.push(diag_deferred_member_reason(member, &refusal.deferral()));
                report_unread_gap(gap_unread, GapOutcome::OutOfRange, diagnostics);
                continue;
            }
        };
        report_unread_gap(gap_unread, GapOutcome::Placed, diagnostics);
        // First write wins, as above: a repeated `id=` (also across two
        // `site` blocks of one name) collides, and the resolver bound the
        // first. The lockfile reads a placement's dims beside the structure
        // it names, so `placed` and `structures` must agree on the winner.
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
                    // row spelled: an `--edition` pin can bind another
                    // variant (`W_THEME_VARIANT_REBOUND`), and the lockfile
                    // outlives the warning. The written name could name a
                    // variant whose materials are not in the artifact.
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
