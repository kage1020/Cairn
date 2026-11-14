//! Lowering one `struct` or `place` body: sizing its volume, then painting
//! its members phase by phase.

use std::collections::{HashMap, HashSet};

use indexmap::IndexMap;

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::intent::{Member, MemberRole, Size, role_of};
use crate::resolve::ScopeResolution;

use crate::block_array::geometry::Declared;
use crate::block_array::material::TargetRegistry;
use crate::block_array::wall_column::WallColumn;
use crate::block_array::{
    Bbox, BlockArray, Dims, MAX_STRUCTURE_VOLUME, Palette, PaletteFull, PaletteIndex,
    ResolvedBlock, ResolvedMember,
};

use super::canvas::{Canvas, Overflow, ScopePalette, prune_unreferenced};
use super::cell_box::Cell;
use super::circuit::{
    patched_actuator, recognize_actuator_patch, recognize_circuit_region, refused_sensor_selector,
};
use super::connections::derive_connections;
use super::diag::{VoxelSource, diag_deferred_member, diag_no_theme_bound_generic};
use super::door::{HungDoor, hang_doors, split_doors};
use super::fill::pour_fluids;
use super::members::{MemberDisposition, StructCtx, flatten_members, member_disposition};
use super::paint::paint_member;
use super::phase::Phase;
use super::sizing::{max_roof_extra_height, max_roof_overhang, max_wall_top, painting_walls};
use super::token_diag::{NamedBy, diag_derived_state_domain, diag_state_unmapped};
use super::written::{Written, written_members};

/// What every body of one run is lowered against.
#[derive(Clone, Copy)]
pub(super) struct BodyInputs<'a> {
    /// The compile target's registry view.
    pub(super) registry: Option<&'a dyn TargetRegistry>,
    /// The entries each body's palette may hold, air included.
    pub(super) palette_capacity: usize,
}

/// Inputs shared by the struct and place lowering paths.
pub(super) struct BodyDescriptor<'a> {
    pub(super) kind: VoxelSource,
    /// Display name for diagnostics (`cottage`, `home1`).
    pub(super) scope_label: &'a str,
    pub(super) size: &'a Size,
    pub(super) members: &'a [Member],
    /// Span the `W_NO_THEME_BOUND` warning anchors at.
    pub(super) header_span: &'a Span,
    /// IR key written into the resulting [`BlockArray::source_scope`].
    pub(super) source_scope: String,
}

/// What lowering one body produced: the voxels, who built them, and the wall column they were
/// painted against.
pub(super) struct LoweredBody {
    pub(super) array: BlockArray,
    pub(super) members: Vec<ResolvedMember>,
    pub(super) walls: WallColumn,
    /// The spans of the `door` and `window` members that painted at least one cell.
    pub(super) cut: HashSet<Span>,
}

/// Lower one struct or place body into voxels, or `None` after the diagnostic refusing it.
pub(super) fn lower_body_to_block_array<'a>(
    body: BodyDescriptor<'a>,
    scope: Option<&'a ScopeResolution>,
    bodies: BodyInputs<'a>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<LoweredBody> {
    let BodyInputs {
        registry,
        palette_capacity,
    } = bodies;
    let (interior_w, interior_h) = (body.size.w.get(), body.size.h.get());

    let theme_missing = scope.is_none_or(|sc| sc.bound_theme.is_none());

    // Flattened once: sizing and the phase buckets must read the same paint set.
    let flattened = flatten_members(body.members, diagnostics);
    // Unflattened: a roof a `level` drops is still written.
    let written_members = written_members(body.members);
    // A scope that reads no `mat_slot=` loses nothing to a missing theme (`spec/lint` "Materials
    // and targets"), so it is not reported.
    if theme_missing && flattened.iter().any(|(_, m)| m.read_slot().is_some()) {
        diagnostics.push(diag_no_theme_bound_generic(
            body.kind,
            body.scope_label,
            body.header_span,
        ));
    }
    let overhang = max_roof_overhang(&flattened, diagnostics);
    // One walk over the walls, read twice, so the volume and the carve see one list.
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
    if !dims.fits_volume_budget() {
        diagnostics.push(diag_structure_too_large(&body, dims));
        return None;
    }
    let mut palette = ScopePalette::with_capacity(palette_capacity);

    let (buckets, patched) = bucket_members(&flattened, &written_members, diagnostics);
    let ctx = StructCtx {
        scope,
        registry,
        theme_missing,
        dims,
        overhang,
        interior_w,
        interior_h,
        wall_top: max_wall_top,
        rise: roof_extra,
        wall_column,
        written: Written::of(&written_members, interior_w, interior_h),
        patched,
    };
    let (canvas, split_doors, derived) = paint_phases(buckets, &ctx, &mut palette, diagnostics);
    let stand_in = palette.stand_in();
    // Before anything reads the grid: a voxel painted after the palette filled holds air.
    let mut palette = match palette.into_palette() {
        Ok(palette) => palette,
        Err(Overflow::Full(full)) => {
            let checked = registry.is_some_and(|registry| registry.block_ids().is_some());
            diagnostics.push(diag_palette_too_large(&body, full, checked));
            return None;
        }
        Err(Overflow::StandIn) => return None,
    };

    // A stand-in differs from air at every target, but from another block only as the pack says.
    let pack_decides = |&(before, after): &(PaletteIndex, PaletteIndex)| {
        (Some(before) == stand_in && after != PaletteIndex::AIR)
            || (Some(after) == stand_in && before != PaletteIndex::AIR)
    };
    for ((overridden, overriding), contested) in &canvas.conflicts {
        if contested.keys().any(pack_decides) {
            continue;
        }
        diagnostics.push(diag_phase_conflict(
            flattened[*overridden as usize].1,
            flattened[*overriding as usize].1,
            contested.values().sum(),
            split_doors.contains(overridden),
        ));
    }
    let cut = cut_openings(&flattened, &canvas.wrote);
    let (voxels, never_painted) = prune_unreferenced(&mut palette, &canvas);
    debug_assert!(
        never_painted.is_empty(),
        "palette slots {never_painted:?} of `{}` were claimed but no voxel was ever painted \
         with one; a generator is interning a material for geometry it does not emit",
        body.scope_label,
    );

    let array = finished_array(dims, palette, voxels, body.source_scope, &derived);
    let members = resolved_members(&flattened, &canvas, &array);
    if let Some(registry) = registry {
        diagnostics.extend(state_findings(&members, registry));
    }

    Some(LoweredBody {
        array,
        members,
        walls: ctx.wall_column,
        cut,
    })
}

/// The body's array over the pruned `voxels`, its palette put in canonical order, with the keys
/// `derived` set at its cells.
fn finished_array(
    dims: Dims,
    palette: Palette,
    voxels: Vec<PaletteIndex>,
    source_scope: String,
    derived: &HashMap<Cell, Vec<String>>,
) -> BlockArray {
    let mut array = BlockArray {
        dims,
        palette,
        voxels,
        block_entities: Vec::new(),
        entities: Vec::new(),
        source_scope,
        derived: Vec::new(),
    };
    // After the prune, so dropped entries are not sorted and the grid is renumbered once.
    array.canonicalize_palette();
    array.derived = derived_by_slot(&array, derived);
    array
}

/// Per slot of `array`, the keys `derived` set at every cell that holds it, without the run of
/// slots past the last that has one.
fn derived_by_slot(array: &BlockArray, derived: &HashMap<Cell, Vec<String>>) -> Vec<Vec<String>> {
    if derived.is_empty() {
        return Vec::new();
    }
    let mut by_slot: Vec<Option<Vec<String>>> = vec![None; array.palette.entries.len()];
    for (index, slot) in array.voxels.iter().enumerate() {
        let cell = array
            .dims
            .position(index)
            .expect("a voxel's index lies in the dims it was laid out by");
        let here = derived.get(&cell).map_or(&[][..], Vec::as_slice);
        match &mut by_slot[usize::from(slot.0)] {
            Some(keys) => keys.retain(|key| here.contains(key)),
            unseen @ None => *unseen = Some(here.to_vec()),
        }
    }
    let mut by_slot: Vec<Vec<String>> =
        by_slot.into_iter().map(Option::unwrap_or_default).collect();
    while by_slot.last().is_some_and(Vec::is_empty) {
        by_slot.pop();
    }
    by_slot
}

/// The spans of the doors and windows of `flattened` that `wrote` says cut their opening.
fn cut_openings(flattened: &[(u32, &Member)], wrote: &[bool]) -> HashSet<Span> {
    flattened
        .iter()
        .zip(wrote)
        .filter(|((_, m), wrote)| {
            **wrote && matches!(m.role, MemberRole::Door | MemberRole::Window)
        })
        .map(|((_, m), _)| m.span.clone())
        .collect()
}

/// The findings on each state a member resolved to that `registry` cannot carry: a derived pair
/// outside the block's states, or a state its state map has no form for.
fn state_findings<'a>(
    members: &'a [ResolvedMember],
    registry: &'a dyn TargetRegistry,
) -> impl Iterator<Item = Diagnostic> + 'a {
    members.iter().flat_map(move |member| {
        member.resolved_state.iter().flat_map(move |block| {
            let outside = diag_derived_state_domain(&member.span, &block.state, registry);
            let named_by = match role_of(&member.role) {
                MemberRole::RawBlock | MemberRole::RawFill => NamedBy::RawMat,
                _ => NamedBy::Binding,
            };
            let unmapped =
                diag_state_unmapped(member.span.clone(), &block.state, registry, named_by);
            outside.into_iter().chain(unmapped)
        })
    })
}

/// Each member of `flattened` that owns a cell of `array`, with the states its cells hold and its
/// geometry, in the order of the first cell each owns.
fn resolved_members(
    flattened: &[(u32, &Member)],
    canvas: &Canvas,
    array: &BlockArray,
) -> Vec<ResolvedMember> {
    let owners = canvas.owners();
    debug_assert_eq!(
        owners.len(),
        array.voxels.len(),
        "the canvas and the array lay out one grid",
    );
    // Keyed by first cell, not source line: `resolved_ir_hash` reads this order, and
    // `spec/compilation` "Within-phase conflicts and the palette" keeps it free of line order.
    let mut built: IndexMap<u32, (Bbox, IndexMap<PaletteIndex, ResolvedBlock>)> = IndexMap::new();
    for (index, (&owner, &state)) in owners.iter().zip(&array.voxels).enumerate() {
        let Some(member) = owner.checked_sub(1) else {
            continue;
        };
        let cell = array
            .dims
            .position(index)
            .expect("a voxel's index lies in the dims it was laid out by");
        let (held, blocks) = built
            .entry(member)
            .or_insert_with(|| (Bbox::cell(cell), IndexMap::new()));
        *held = held.including(cell);
        blocks
            .entry(state)
            .or_insert_with(|| ResolvedBlock {
                state: array.palette.entries[usize::from(state.0)].clone(),
                runs: Vec::new(),
            })
            .push(cell);
    }
    built
        .into_iter()
        .map(|(index, (held, blocks))| {
            let (_, member) = flattened[index as usize];
            let declared = canvas.declared(index).unwrap_or_else(|| {
                debug_assert!(
                    false,
                    "`{}` painted cells without declaring its geometry; its painter must call \
                     `MemberCanvas::declare`",
                    member.role.keyword(),
                );
                Declared::boxed(held, None)
            });
            ResolvedMember {
                role: member.role.keyword().to_owned(),
                id: member.id.clone(),
                resolved_state: blocks.into_values().collect(),
                geometry: declared.holding(held),
                span: member.span.clone(),
            }
        })
        .collect()
}

/// Paint one body's members onto a fresh canvas, phase by phase in `spec/compilation` "Phase
/// evaluation" order, then set the connection states the painted blocks derive; with the `door`
/// members left holding one of their two cells, and the keys derived at each cell.
fn paint_phases(
    buckets: PhaseBuckets<'_>,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    diagnostics: &mut Vec<Diagnostic>,
) -> (Canvas, Vec<u32>, HashMap<Cell, Vec<String>>) {
    let PhaseBuckets { by_phase, phases } = buckets;
    let walls: Vec<u32> = by_phase[Phase::Massing.index()]
        .iter()
        .filter(|(_, _, member)| matches!(member.role, MemberRole::Walls))
        .map(|&(index, _, _)| index)
        .collect();
    let mut canvas = Canvas::new(ctx.dims, phases);
    let mut hung = Vec::new();
    for (phase, bucket) in Phase::ALL.into_iter().zip(by_phase) {
        match phase {
            Phase::Massing | Phase::Envelope | Phase::Fixtures | Phase::Raw => {
                run_phase(bucket, ctx, palette, &mut canvas, &mut hung, diagnostics);
            }
            Phase::Openings => {
                run_phase(bucket, ctx, palette, &mut canvas, &mut hung, diagnostics);
                // A door's hinge reads the wall beside it, which a later opening may cut.
                hang_doors(&hung, &walls, palette, &mut canvas);
            }
            Phase::Fluids => pour_fluids(bucket, ctx, palette, &mut canvas, diagnostics),
        }
    }
    let derived = ctx.registry.map_or_else(HashMap::new, |registry| {
        derive_connections(&mut canvas, palette, registry)
    });
    let split = split_doors(&hung, &canvas);
    (canvas, split, derived)
}

/// The paint set filed by phase, each entry keeping its index in the flattened list.
struct PhaseBuckets<'a> {
    /// Indexed by [`Phase::index`]: `(index in the flattened list, y_offset, member)` per phase.
    by_phase: [Vec<(u32, u32, &'a Member)>; Phase::COUNT],
    /// The phase that evaluates each member of the flattened list, in order, or `None`.
    phases: Vec<Option<Phase>>,
}

/// File every member of the paint set under its phase, reporting the ones no phase reads and the
/// sensor selector lines, which place nothing; with the span starts of the members a well-formed
/// actuator patch binds a signal to.
///
/// An actuator patch picks its actuator among `written`, every member the body writes.
fn bucket_members<'a>(
    flattened: &[(u32, &'a Member)],
    written: &[(Option<u32>, &Member)],
    diagnostics: &mut Vec<Diagnostic>,
) -> (PhaseBuckets<'a>, HashSet<usize>) {
    let mut buckets = PhaseBuckets {
        by_phase: Default::default(),
        phases: Vec::with_capacity(flattened.len()),
    };
    let mut patched = HashSet::new();
    for (index, &(y_offset, member)) in flattened.iter().enumerate() {
        let index = u32::try_from(index).expect("the paint set is far shorter than u32::MAX");
        // An actuator patch binds a component declared elsewhere; only its shape is checked here
        // (`spec/redstone` "Signal binding").
        if let Some(patch) = patched_actuator(member) {
            if let Some(target) = recognize_actuator_patch(member, patch, written, diagnostics) {
                patched.insert(target.span.start);
            }
            buckets.phases.push(None);
            continue;
        }
        if let Some(refusal) = refused_sensor_selector(member) {
            diagnostics.push(refusal);
            buckets.phases.push(None);
            continue;
        }
        let disposition = member_disposition(member);
        buckets.phases.push(match disposition {
            MemberDisposition::Paints(phase) => Some(phase),
            MemberDisposition::Reserves | MemberDisposition::NotLowered => None,
        });
        match disposition {
            MemberDisposition::Paints(phase) => {
                buckets.by_phase[phase.index()].push((index, y_offset, member));
            }
            // A `circuit` region is left for `spec/redstone` "Place-and-route".
            MemberDisposition::Reserves => recognize_circuit_region(member, diagnostics),
            MemberDisposition::NotLowered => diagnostics.push(diag_deferred_member(member)),
        }
    }
    (buckets, patched)
}

/// Paint one phase's bucket, each member through its own [`MemberCanvas`] so its writes are
/// filed under it; each door that stands is added to `hung`.
///
/// [`MemberCanvas`]: super::canvas::MemberCanvas
pub(super) fn run_phase(
    bucket: Vec<(u32, u32, &Member)>,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut Canvas,
    hung: &mut Vec<HungDoor>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for (index, y_offset, member) in bucket {
        let mut member_canvas = canvas.for_member(index);
        paint_member(
            member,
            y_offset,
            ctx,
            palette,
            &mut member_canvas,
            hung,
            diagnostics,
        );
    }
}

/// Two members of one phase wrote the same cell to different blocks (`spec/compilation` "Phase
/// evaluation"); anchored at the later.
fn diag_phase_conflict(
    overridden: &Member,
    overriding: &Member,
    voxels: u32,
    splits_a_door: bool,
) -> Diagnostic {
    let cell = if voxels == 1 { "voxel" } else { "voxels" };
    let mut notes = vec![DiagnosticNote {
        span: Some(overridden.span.clone()),
        message: "overwritten member declared here".to_owned(),
    }];
    if splits_a_door {
        notes.push(DiagnosticNote {
            span: None,
            message: "the `door` is left with one of its two blocks, and the game removes a door \
                      block whose other half is missing"
                .to_owned(),
        });
    }
    notes.extend([
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
    ]);
    Diagnostic {
        code: DiagnosticCode::PhaseConflict,
        span: overriding.span.clone(),
        primary: format!(
            "`{}` overwrites {voxels} {cell} that `{}` painted in the same phase, so which \
             block the build keeps is decided by the order the two lines are written in",
            MemberRole::keyword(&overriding.role),
            MemberRole::keyword(&overridden.role),
        ),
        notes,
        data: None,
    }
}

/// A body painted more distinct block states than its palette holds; `checked` when a pinned
/// target checked its block ids.
fn diag_palette_too_large(
    body: &BodyDescriptor<'_>,
    full: PaletteFull,
    checked: bool,
) -> Diagnostic {
    let fix = if checked {
        "the pinned target declares each block id, so split the members across more than one \
         `struct`, each painting fewer"
    } else {
        "a vanilla registry has far fewer, so check the block ids and state literals by pinning \
         a shipped version with `--edition` and `--target`"
    };
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
            message: format!(
                "every state a member writes counts, including one a later member covers; {fix}"
            ),
        }],
        data: None,
    }
}

/// The scope asked for more voxels than [`MAX_STRUCTURE_VOLUME`] allows.
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
