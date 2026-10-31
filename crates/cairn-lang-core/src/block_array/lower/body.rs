//! Lowering one `struct` or `place` body: sizing its volume, then painting
//! its members phase by phase.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::intent::{Member, MemberRole, Size};
use crate::resolve::ScopeResolution;

use crate::block_array::material::TargetRegistry;
use crate::block_array::wall_column::WallColumn;
use crate::block_array::{BlockArray, Dims, MAX_STRUCTURE_VOLUME, PaletteFull, PaletteIndex};

use super::canvas::{Canvas, Overflow, ScopePalette, prune_unreferenced};
use super::circuit::{is_actuator_patch, recognize_actuator_patch, recognize_circuit_region};
use super::diag::{VoxelSource, diag_deferred_member, diag_no_theme_bound_generic};
use super::members::{MemberDisposition, StructCtx, flatten_members, member_disposition};
use super::paint::paint_member;
use super::phase::Phase;
use super::sizing::{
    max_roof_extra_height, max_roof_overhang, max_wall_top, painting_walls, written,
};

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

/// What lowering one body produced: the voxels, and the wall column they were painted against.
pub(super) struct LoweredBody {
    pub(super) array: BlockArray,
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
    let written = written(body.members);
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
        written_height: written.height(roof_extra),
        written_walls: written.walls,
        written_overhang: written.overhang,
        written_floor: written.floor,
    };

    let buckets = bucket_members(&flattened, diagnostics);
    let canvas = paint_phases(buckets, &ctx, &mut palette, diagnostics);
    let stand_in = palette.stand_in();
    // Before anything reads the grid: a voxel painted after the palette filled holds air.
    let mut palette = match palette.into_palette() {
        Ok(palette) => palette,
        Err(Overflow::Full(full)) => {
            diagnostics.push(diag_palette_too_large(&body, full));
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
    // After the prune, so dropped entries are not sorted and the grid is renumbered once.
    array.canonicalize_palette();

    Some(LoweredBody {
        array,
        walls: ctx.wall_column,
        cut,
    })
}

/// Paint one body's members onto a fresh canvas, phase by phase in `spec/compilation` "Phase
/// evaluation" order.
///
/// No test pins the order: neighbouring phases write disjoint cells, so only a two-step swap shows.
fn paint_phases(
    buckets: PhaseBuckets<'_>,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    diagnostics: &mut Vec<Diagnostic>,
) -> Canvas {
    let PhaseBuckets { by_phase, phases } = buckets;
    let mut canvas = Canvas::new(ctx.dims, phases);
    for bucket in by_phase {
        run_phase(bucket, ctx, palette, &mut canvas, diagnostics);
    }
    canvas
}

/// The paint set filed by phase, each entry keeping its index in the flattened list.
struct PhaseBuckets<'a> {
    /// Indexed by [`Phase::index`]: `(index in the flattened list, y_offset, member)` per phase.
    by_phase: [Vec<(u32, u32, &'a Member)>; Phase::COUNT],
    /// The phase that evaluates each member of the flattened list, in order, or `None`.
    phases: Vec<Option<Phase>>,
}

/// File every member of the paint set under its phase, reporting the ones no phase reads.
fn bucket_members<'a>(
    flattened: &[(u32, &'a Member)],
    diagnostics: &mut Vec<Diagnostic>,
) -> PhaseBuckets<'a> {
    let mut buckets = PhaseBuckets {
        by_phase: Default::default(),
        phases: Vec::with_capacity(flattened.len()),
    };
    for (index, &(y_offset, member)) in flattened.iter().enumerate() {
        let index = u32::try_from(index).expect("the paint set is far shorter than u32::MAX");
        // An actuator patch overlays a door declared elsewhere, with no `side=`; only its shape is
        // checked here (`spec/redstone` "Signal binding").
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
            MemberDisposition::Paints(phase) => {
                buckets.by_phase[phase.index()].push((index, y_offset, member));
            }
            // A `circuit` region is left for `spec/redstone` "Place-and-route".
            MemberDisposition::Reserves => recognize_circuit_region(member, diagnostics),
            MemberDisposition::NotLowered => diagnostics.push(diag_deferred_member(member)),
        }
    }
    buckets
}

/// Paint one phase's bucket, each member through its own [`MemberCanvas`] so its writes are
/// filed under it.
///
/// [`MemberCanvas`]: super::canvas::MemberCanvas
pub(super) fn run_phase(
    bucket: Vec<(u32, u32, &Member)>,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut Canvas,
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
            diagnostics,
        );
    }
}

/// Two members of one phase wrote the same cell to different blocks (`spec/compilation` "Phase
/// evaluation"); anchored at the later.
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

/// A body painted more distinct block states than its palette holds.
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
