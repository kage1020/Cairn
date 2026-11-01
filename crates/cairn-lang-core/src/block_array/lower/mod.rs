//! Intent IR → block-array IR lowering, total: what cannot lower is a warning, not a failure.
//!
//! Members paint in `spec/compilation` "Phase evaluation" order (massing, envelope, openings,
//! fixtures), after [`flatten_members`] folds `level` blocks into one list.
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
mod paint;
mod phase;
mod plate;
mod roof_fill;
mod roof_kind;
mod site_floor;
mod sizing;
mod stair;
mod token_diag;
mod window;
mod without_target;
mod written;

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
use body::{BodyDescriptor, BodyInputs, LoweredBody, lower_body_to_block_array};
use connect::{ConnectInputs, lower_connects};
use diag::{VoxelSource, diag_def_no_size, diag_struct_no_size};
use site_floor::{PlacedBody, collect_floor_cells};

pub use without_target::findings_without_target;

/// Block ids lowering can put in a palette that no pack can redirect.
///
/// Each must exist on every supported target; a pack-side test holds them to that.
pub const BUILTIN_BLOCK_IDS: &[&str] = &[BlockState::AIR_ID, STAIR_BASE_ID, FLAT_BASE_ID];

/// Lower `intent`'s structs, placements and walkways into block arrays.
///
/// Without a `registry`, an abstract token is a `W_ABSTRACT_TOKEN_DEFERRED` warning.
#[must_use]
pub fn lower_to_block_array(
    intent: &IntentModule,
    resolution: &Resolution,
    registry: Option<&dyn TargetRegistry>,
) -> BlockArrayIr {
    lower_to_block_array_within(intent, resolution, registry, PALETTE_CAPACITY)
}

/// [`lower_to_block_array`] with each body's palette capped at `palette_capacity` entries.
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
        // A `None` is diagnosed, unless its palette is one only a target's pack can size.
        if let Some(ba) = lower_struct(s, scope, bodies, &mut diagnostics) {
            // First write wins on a duplicate name, as `resolution.scopes` bound the first body.
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
    // After every site, so the collision set covers every floor tile a strip might cross.
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

    // Report in source order; the sort is stable, so findings on one span keep pass order.
    sort_by_position(&mut diagnostics);
    // A `def` body lowers once per `place`, so its findings repeat byte-for-byte; keep the first.
    // Findings that are not repeats must differ in their text, as `geometry_material_id`'s do.
    let mut said: HashSet<Diagnostic> = HashSet::new();
    diagnostics.retain(|d| said.insert(d.clone()));

    BlockArrayIr {
        structures,
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

/// Lower every `place` in `site` into a [`BlockArray`] and a [`Placement`], keyed
/// `site::SITE::PLACE_ID`.
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
            continue;
        }
        if !matches!(member.role, MemberRole::Place) {
            continue;
        }
        let Some(place_id) = member.id.as_deref() else {
            continue;
        };
        // Only the resolver's `E_INVALID_PLACE_ID` constrains `id=`, and it has reported this one.
        let Ok(placement_id) = PlaceId::new(place_id) else {
            continue;
        };
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
            // `resolve_site_placements` built no scope for this row and has already said why.
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

        // Resolved before this place is inserted: the anchor looks only at prior places.
        let (anchor, gap_unread) = resolve_place_anchor(member, placed, &site.name);

        // Lowered even without an anchor: the body's findings stand wherever the row lands.
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
            // Refused for its extent or palette: diagnosed unless only a pack can size the palette.
            report_unread_gap(gap_unread, GapOutcome::NotPlaced, diagnostics);
            continue;
        };
        let dims = array.dims;
        // `north_of` needs this body's own depth.
        let origin = match anchor.origin(dims) {
            Ok(origin) => origin,
            Err(refusal) => {
                diagnostics.push(diag_deferred_member_reason(member, &refusal.deferral()));
                report_unread_gap(gap_unread, GapOutcome::NotPlaced, diagnostics);
                continue;
            }
        };
        report_unread_gap(gap_unread, GapOutcome::Placed, diagnostics);
        // First write wins, as above; `placed` and `structures` must agree on the winner.
        placed
            .entry(array.source_scope.clone())
            .or_insert(PlacedBody {
                walls,
                cut,
                placement: Placement {
                    site: placement_site,
                    place_id: placement_id,
                    source_def: use_name.to_owned(),
                    // The theme that governed the build, which an `--edition` pin can rebind.
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
