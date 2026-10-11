//! Lowering one `struct` or `place` body: sizing its volume, then painting
//! its members phase by phase.

use std::collections::HashSet;

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::intent::{Member, MemberRole, Size};
use crate::resolve::ScopeResolution;

use crate::block_array::material::TargetRegistry;
use crate::block_array::wall_column::WallColumn;
use crate::block_array::{BlockArray, Dims, MAX_STRUCTURE_VOLUME, PaletteFull};

use super::canvas::{Canvas, ScopePalette, prune_unreferenced};
use super::circuit::{is_actuator_patch, recognize_actuator_patch, recognize_circuit_region};
use super::diag::{diag_deferred_member, diag_no_theme_bound_generic};
use super::members::{
    MemberDisposition, Phase, StructCtx, flatten_members, member_disposition, paint_member,
};
use super::sizing::{max_roof_extra_height, max_roof_overhang, max_wall_top, painting_walls};

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
pub(super) enum VoxelSource {
    Struct,
    Place,
}

/// What every body of one run is lowered against, whether a `struct` or
/// a `place`'s `def`: threaded unchanged from
/// [`lower_to_block_array_within`] down to [`lower_body_to_block_array`].
///
/// [`lower_to_block_array_within`]: super::lower_to_block_array_within
#[derive(Clone, Copy)]
pub(super) struct BodyInputs<'a> {
    /// The compile target's registry view; see [`lower_to_block_array`].
    ///
    /// [`lower_to_block_array`]: super::lower_to_block_array
    pub(super) registry: Option<&'a dyn TargetRegistry>,
    /// The entries each body's palette may hold, air included:
    /// [`PALETTE_CAPACITY`] from [`lower_to_block_array`].
    ///
    /// [`PALETTE_CAPACITY`]: crate::block_array::PALETTE_CAPACITY
    /// [`lower_to_block_array`]: super::lower_to_block_array
    pub(super) palette_capacity: usize,
}

/// Inputs shared by the struct and place lowering paths.
pub(super) struct BodyDescriptor<'a> {
    pub(super) kind: VoxelSource,
    /// Display name for diagnostics (`cottage`, `home1`).
    pub(super) scope_label: &'a str,
    pub(super) size: &'a Size,
    pub(super) members: &'a [Member],
    /// Span the `W_NO_THEME_BOUND` warning anchors at (struct/def header
    /// for a struct, `place` line for a place).
    pub(super) header_span: &'a Span,
    /// IR key written into the resulting [`BlockArray::source_scope`].
    pub(super) source_scope: String,
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
pub(super) struct LoweredBody {
    pub(super) array: BlockArray,
    pub(super) walls: WallColumn,
    /// The spans of the `door` and `window` members that painted at
    /// least one cell — the openings the `connect` pass may anchor a port
    /// on. Read off the canvas rather than reported by the generators, so
    /// a new way for a cut to defer cannot forget to say so.
    pub(super) cut: HashSet<Span>,
}

/// Lower one struct or place body into voxels.
///
/// `None` means the body was refused, and the diagnostic saying why has
/// already been pushed: the extent it asks for is past
/// [`MAX_STRUCTURE_VOLUME`] (`W_STRUCTURE_TOO_LARGE`), or it paints more
/// states than its palette holds (`W_PALETTE_TOO_LARGE`). The caller drops
/// the scope.
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
    let interior_w = body.size.w.get();
    let interior_h = body.size.h.get();

    let theme_missing = scope.is_none_or(|sc| sc.bound_theme.is_none());

    // Flatten `level y=N` grouping once, here. The result is the set of
    // members this body paints, and everything below reads it: the dim math
    // (which has to size a volume that holds them) and the phase buckets
    // (which need each member's y-offset). Deriving the dims from any other
    // list is how a roof came to paint into a volume too short to hold it.
    // `flatten_members` also emits the `W_DEFERRED_MEMBER` for every member
    // it drops, so its side effects need to happen exactly once per call.
    let flattened = flatten_members(body.members, diagnostics);
    // `spec/lint` "Materials and targets": a scope that reads no
    // `mat_slot=` has nothing to lower to air, so a missing theme costs it
    // nothing and is not reported. "Reads" is `Member::read_slot`, so a
    // `mat_slot=` on a role that reads none, such as a `door`'s, does not
    // count. Asked of the flattened list because a `level` is where the only
    // reader can sit, and a member it drops paints nothing whatever the
    // theme.
    if theme_missing && flattened.iter().any(|(_, m)| m.read_slot().is_some()) {
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
    let PhaseBuckets { by_phase, phases } = buckets;
    let mut canvas = Canvas::new(ctx.dims, phases);
    // `by_phase` is indexed by `Phase::index`, so front to back is the
    // spec's order.
    for bucket in by_phase {
        run_phase(bucket, ctx, palette, &mut canvas, diagnostics);
    }
    canvas
}

/// The paint set filed by phase, each entry keeping its position in the
/// flattened list so the canvas can name it in a finding.
struct PhaseBuckets<'a> {
    /// Indexed by [`Phase::index`]: the members each phase paints, as
    /// `(index in the flattened list, y_offset, member)`.
    by_phase: [Vec<(u32, u32, &'a Member)>; Phase::COUNT],
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
        by_phase: Default::default(),
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
            MemberDisposition::Paints(phase) => {
                buckets.by_phase[phase.index()].push((index, y_offset, member));
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

/// Paint one phase's bucket with [`paint_member`], opening a
/// [`MemberCanvas`] per member so every write it makes is filed under it.
/// A bucket cannot forget: the borrow is the only route to a write, and it
/// closes with the member.
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
