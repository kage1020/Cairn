//! Placing a `pressure_plate` member.

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticNote};
use crate::intent::Member;
use crate::suggest::{did_you_mean, nearest_match};

use crate::block_array::BlockState;
use crate::block_array::args::{
    diag_deferred_member_noted, diag_deferred_member_reason, diag_invalid_member,
};
use crate::block_array::material::{IdOrigin, MaterialDeferred, TargetRegistry, validated_id};
use crate::block_array::openings::{WallSide, wall_length, wall_local_to_grid};

use super::canvas::{MemberCanvas, ScopePalette};
use super::members::{StructCtx, resolve_member_state, side_name, unplaced_walls_note};
use super::stair::shift_outward;
use super::token_diag::diag_unknown_id;

/// Catalog token a `pressure_plate`'s default material comes from; the pack spells it per edition.
pub(super) const PRESSURE_PLATE_TOKEN: &str = "pressure_plate.default";

/// Pressure plate id used when the pack cannot supply one: the Java spelling, Java being the base
/// edition (`spec/versioning-editions` "Backend = data tables").
pub(super) const PRESSURE_PLATE_BASE_ID: &str = "minecraft:oak_pressure_plate";

/// The note on an `at=` that names no anchor, saying what one is.
const ANCHOR_FORMS: &str = "write `at=<side>.outside` to sit outside a wall or `at=inside.<side>` \
                            to sit inside it, where `<side>` is front, back, left or right";

/// The note on every plate left unplaced that carries a `->` binding.
const BINDING_STILL_READ: &str =
    "its signal binding still reaches the netlist, with no plate placed to drive it";

/// The note on a plate deferred to a row only a roof whose shape does not read may rise to.
const ROOF_SHAPE_UNREAD: &str = "a `roof` whose `kind=` or `slope_to=` does not read may rise to \
                                 this row; the plate fits once that roof draws";

/// The note on an `at=<side>.outside` plate deferred because a roof does not draw its overhang.
const ROOF_DRAWS_NOTHING: &str = "a `roof` written with an `overhang=` draws nothing, so the struct \
                                  has no overhang to hold the plate; it fits once that roof draws";

/// The note on an `at=inside.<side>` plate on a struct with no interior.
const INTERIOR_NEEDS: &str = "an interior needs a size of at least 3 on both axes";

/// The finding that leaves a plate unplaced.
type Refusal = Box<Diagnostic>;

/// The side names an anchor takes.
const SIDE_NAMES: [&str; 4] = ["front", "back", "left", "right"];

/// The words that say which face of the wall an anchor sits on.
const PLACE_NAMES: [&str; 2] = ["outside", "inside"];

/// Shift a wall voxel's `(x, z)` one voxel toward the interior, saturating at 0 and unchecked.
fn shift_inward(side: WallSide, x: u32, z: u32) -> (u32, u32) {
    let (dx, dz) = side.outward_normal();
    (x.saturating_add_signed(-dx), z.saturating_add_signed(-dz))
}

/// Which side of the wall a `pressure_plate at=<side>.outside | inside.<side>` sits on.
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

/// `refusal`, with [`BINDING_STILL_READ`] after its notes when the plate has a `->` binding.
fn with_binding_note(member: &Member, refusal: Refusal) -> Diagnostic {
    let mut refusal = *refusal;
    // Synthesis reads the binding off the member line, never the block array.
    if member.binding.is_some() {
        refusal.notes.push(DiagnosticNote {
            span: None,
            message: BINDING_STILL_READ.to_owned(),
        });
    }
    refusal
}

/// Parse a `pressure_plate at=` value, or the refusal of any other shape.
fn plate_anchor_of(member: &Member) -> Result<PlateAnchor, Refusal> {
    let refuse = |primary: String| {
        Box::new(diag_invalid_member(
            member,
            primary,
            vec![ANCHOR_FORMS.to_owned()],
        ))
    };
    let Some(raw) = member.intent_state.get("at") else {
        return Err(refuse(
            "pressure_plate has no `at=`, so nothing says which wall it sits by".to_owned(),
        ));
    };
    let ValueKind::DotRef(dotref) = &raw.value.kind else {
        let mut refusal = refuse(format!(
            "pressure_plate `at=` must be a two-segment reference, not {}",
            raw.value.describe(),
        ));
        if let ValueKind::Ident(word) = &raw.value.kind
            && let Some(side) = known_or_nearest(word, &SIDE_NAMES)
        {
            refusal.notes.push(DiagnosticNote {
                span: None,
                message: format!("did you mean `at={side}.outside` or `at=inside.{side}`?"),
            });
        }
        return Err(refusal);
    };
    let segments = dotref.segments();
    if segments.len() != 2 {
        return Err(refuse(format!(
            "pressure_plate `at={dotref}` has {} segments, and an anchor has two",
            segments.len(),
        )));
    }
    let (head, tail) = (segments[0].as_str(), segments[1].as_str());
    let refuse_with_suggestion = |primary: String, note: String| {
        let mut notes = vec![note];
        notes.extend(intended_anchor(head, tail).map(|anchor| did_you_mean(&anchor)));
        Box::new(diag_invalid_member(member, primary, notes))
    };
    let not_a_side = |name: &str, example: &str| {
        refuse_with_suggestion(
            format!("pressure_plate `at={dotref}`: `{name}` is not a wall side"),
            format!("a side is one of front, back, left, right, as in `at={example}`"),
        )
    };
    if tail == "outside" {
        return WallSide::from_ident(head)
            .map(PlateAnchor::Outside)
            .ok_or_else(|| not_a_side(head, "front.outside"));
    }
    if head == "inside" {
        return WallSide::from_ident(tail)
            .map(PlateAnchor::Inside)
            .ok_or_else(|| not_a_side(tail, "inside.front"));
    }
    Err(refuse_with_suggestion(
        format!("pressure_plate `at={dotref}` is not an anchor"),
        ANCHOR_FORMS.to_owned(),
    ))
}

/// `word` as it appears in `vocabulary`, or the entry one typo from it.
fn known_or_nearest(word: &str, vocabulary: &[&'static str]) -> Option<&'static str> {
    vocabulary
        .iter()
        .copied()
        .find(|known| *known == word)
        .or_else(|| nearest_match(word, vocabulary.iter().copied()))
}

/// The `at=` anchor `head.tail` was likely meant as, when a segment is misspelled or the two are
/// swapped.
fn intended_anchor(head: &str, tail: &str) -> Option<String> {
    let (side, place) = match (
        known_or_nearest(head, &SIDE_NAMES),
        known_or_nearest(tail, &PLACE_NAMES),
    ) {
        (Some(side), Some(place)) => (side, place),
        _ => (
            known_or_nearest(tail, &SIDE_NAMES)?,
            known_or_nearest(head, &PLACE_NAMES)?,
        ),
    };
    Some(if place == "outside" {
        format!("at={side}.outside")
    } else {
        format!("at=inside.{side}")
    })
}

/// Paint a `pressure_plate` fixture as a single plate voxel.
pub(super) fn fill_pressure_plate(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Err(refusal) = place_plate(member, y_offset, ctx, palette, canvas, diagnostics) {
        diagnostics.push(with_binding_note(member, refusal));
    }
}

/// [`fill_pressure_plate`], answering the finding that refuses the plate.
fn place_plate(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<(), Refusal> {
    let anchor = plate_anchor_of(member)?;
    let (x, y_world, z) = plate_voxel_position(member, y_offset, anchor, ctx)?;
    let base_id = plate_id_for_member(member, ctx, diagnostics)?;
    // Interned only once the cell exists, so a refusal leaves no palette entry.
    if canvas.try_paint((x, y_world, z), || {
        palette.intern(BlockState::bare(base_id))
    }) {
        Ok(())
    } else {
        Err(Box::new(diag_deferred_member_reason(
            member,
            "pressure_plate resolved to a voxel outside the struct's block array",
        )))
    }
}

/// Read a plate's `key=` as a non-negative integer, `0` when absent.
fn plate_coordinate(member: &Member, key: &str, counts: &str) -> Result<u32, Refusal> {
    let Some(raw) = member.intent_state.get(key) else {
        return Ok(0);
    };
    member.nonneg_u32(key).ok_or_else(|| {
        Box::new(diag_invalid_member(
            member,
            format!(
                "pressure_plate `{key}=` must be a non-negative integer that fits in u32, not {}",
                raw.value.describe(),
            ),
            vec![format!("`{key}=` counts {counts}")],
        ))
    })
}

/// The world voxel a plate paints onto, or the finding that leaves it unplaced.
///
/// `<side>.outside` lands only in the overhang ring, and `inside.<side>` only strictly inside the
/// wall ring.
fn plate_voxel_position(
    member: &Member,
    y_offset: u32,
    anchor: PlateAnchor,
    ctx: &StructCtx<'_>,
) -> Result<(u32, u32, u32), Refusal> {
    let side = anchor.side();
    let offset = plate_coordinate(
        member,
        "offset",
        "cells along the wall from 0, its left end seen from outside",
    )?;
    let row = PlateRow {
        y: plate_coordinate(
            member,
            "y",
            "rows up from 0, the base row of the struct or of the plate's `level`",
        )?,
        level: y_offset,
    };
    let y_world = row.world();
    // The plate's own line is asked first, so its fault is an error even when another member is
    // missing too.
    let length = wall_length(side, ctx.interior_w, ctx.interior_h);
    if offset >= length {
        return Err(Box::new(past_the_wall(member, anchor, offset, length, ctx)));
    }
    if let PlateAnchor::Inside(_) = anchor
        && let Some((primary, notes)) = inside_plate_refusal(side, offset, length, y_world, ctx)
    {
        return Err(Box::new(diag_invalid_member(member, primary, notes)));
    }
    let written = &ctx.written;
    let above_written = y_world >= written.height;
    if above_written && !written.unplaced_walls && y_world >= written.reach {
        return Err(Box::new(above_the_struct(member, anchor, row, ctx)));
    }
    let outside = matches!(anchor, PlateAnchor::Outside(_));
    if outside && written.overhang == 0 {
        return Err(Box::new(no_exterior_voxel(member, side, length, row, ctx)));
    }
    // From here the repair is on another member, so the plate only defers.
    let deferral = if above_written && written.unplaced_walls {
        Some((
            row_past_the_struct(row, ctx),
            unplaced_walls_note("this row", "the plate fits"),
        ))
    } else if above_written {
        Some((row_past_the_struct(row, ctx), ROOF_SHAPE_UNREAD.to_owned()))
    } else if y_world >= ctx.dims.y {
        Some((
            row_past_the_struct(row, ctx),
            ctx.shortfall().note("the plate fits"),
        ))
    } else if outside && ctx.overhang == 0 {
        // An overhang a roof is written with and does not draw is the roof's to repair.
        Some((
            no_exterior_primary(side, row),
            ROOF_DRAWS_NOTHING.to_owned(),
        ))
    } else {
        None
    };
    if let Some((primary, note)) = deferral {
        return Err(Box::new(diag_deferred_member_noted(
            member, &primary, &note,
        )));
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
        // INVARIANT(wall-grid-validated): the `offset` and `dims.y` refusals above are every
        // rejection `wall_local_to_grid` makes.
        debug_assert!(
            false,
            "pressure_plate cell u={offset} y={y_world} on the {} wall is outside the wall grid; \
             the `offset < wall_length` / `y_world < dims.y` checks stopped agreeing with \
             `wall_local_to_grid`",
            side_name(side),
        );
        return Err(Box::new(diag_deferred_member_reason(
            member,
            "pressure_plate resolved to a voxel outside the struct's block array",
        )));
    };
    let (sx, sz) = match anchor {
        PlateAnchor::Outside(_) => shift_outward(side, wx, wz),
        PlateAnchor::Inside(_) => shift_inward(side, wx, wz),
    };
    Ok((sx, y_world, sz))
}

/// A plate's `y=` and the `level y=` it is written under.
#[derive(Debug, Clone, Copy)]
struct PlateRow {
    y: u32,
    level: u32,
}

impl PlateRow {
    /// The struct row the plate stands on.
    fn world(self) -> u32 {
        self.y.saturating_add(self.level)
    }

    /// The row as a message names it: `y=N`, or under a raised level the level and world row too.
    fn named(self) -> String {
        if self.level == 0 {
            format!("y={}", self.y)
        } else {
            format!(
                "y={} under `level y={}` (row {})",
                self.y,
                self.level,
                self.world()
            )
        }
    }
}

/// The primary of a plate whose row is at or above `dims.y`.
fn row_past_the_struct(row: PlateRow, ctx: &StructCtx<'_>) -> String {
    format!(
        "pressure_plate {} does not fit in the struct (dims.y={})",
        row.named(),
        ctx.dims.y,
    )
}

/// The refusal of a plate whose row is above every row the struct as written reaches.
fn above_the_struct(
    member: &Member,
    anchor: PlateAnchor,
    row: PlateRow,
    ctx: &StructCtx<'_>,
) -> Diagnostic {
    let top = ctx.written.height.saturating_sub(1);
    let level = row.level;
    let runs_to = |highest: u32| {
        if highest == 0 {
            "`y=` takes only 0".to_owned()
        } else {
            format!("`y=` runs from 0 to {highest}")
        }
    };
    let mut notes = vec![if level == 0 {
        format!("{}, the struct's top row", runs_to(top))
    } else if let Some(highest) = top.checked_sub(level) {
        format!(
            "{} under `level y={level}`, the struct's top row being {top}",
            runs_to(highest),
        )
    } else {
        format!("the struct's top row is {top}, below this plate's `level y={level}`")
    }];
    notes.extend(ctx.unread_roof_note());
    let built = ctx.dims.y;
    if built <= top {
        let rows = if built == top {
            format!("row {built} waits")
        } else {
            format!("rows {built} to {top} wait")
        };
        notes.push(format!("{rows} on {}", ctx.shortfall().members()));
    }
    // Under a level above the top row, no `y=` brings the plate down far enough.
    notes.push(if level > top {
        "lower the plate's `level`, or raise the `walls` to reach its row".to_owned()
    } else {
        "lower the plate, or raise the `walls` to reach its row".to_owned()
    });
    notes.extend(outside_needs_a_roof(anchor, ctx));
    diag_invalid_member(member, row_past_the_struct(row, ctx), notes)
}

/// The fix in a struct's body an `at=<side>.outside` plate needs too, when no roof on it is
/// written with an overhang.
fn outside_needs_a_roof(anchor: PlateAnchor, ctx: &StructCtx<'_>) -> Option<String> {
    match anchor {
        PlateAnchor::Outside(side) if ctx.written.overhang == 0 => Some(format!(
            "an `at={}.outside` plate also needs a `roof` in the struct's body with an \
             `overhang=` of at least 1",
            side_name(side),
        )),
        _ => None,
    }
}

/// The refusal of a plate whose `offset=` is at or past its wall's length.
fn past_the_wall(
    member: &Member,
    anchor: PlateAnchor,
    offset: u32,
    length: u32,
    ctx: &StructCtx<'_>,
) -> Diagnostic {
    let name = side_name(anchor.side());
    let thin = thin_axes(ctx);
    let notes = match anchor {
        PlateAnchor::Outside(_) => std::iter::once(format!(
            "an `at={name}.outside` plate takes an `offset=` from 0 to {}",
            length - 1,
        ))
        .chain(outside_needs_a_roof(anchor, ctx))
        .collect(),
        PlateAnchor::Inside(_) if thin.is_empty() => vec![format!(
            "an `at=inside.{name}` plate takes an `offset=` {}, clear of the wall's corners",
            inside_offsets(length),
        )],
        PlateAnchor::Inside(_) => vec![
            format!(
                "the struct's {}, so no `offset=` reaches an interior voxel; {INTERIOR_NEEDS}",
                thin_sizes(&thin),
            ),
            format!(
                "grow {} and use an `offset=` {}",
                thin_growth(&thin),
                inside_offsets(length),
            ),
        ],
    };
    diag_invalid_member(
        member,
        format!("pressure_plate `offset={offset}` runs past the {name} wall (length {length})"),
        notes,
    )
}

/// The size axes of `ctx` under 3, each with its extent, `w` before `h`.
fn thin_axes(ctx: &StructCtx<'_>) -> Vec<(&'static str, u32)> {
    [("w", ctx.interior_w), ("h", ctx.interior_h)]
        .into_iter()
        .filter(|&(_, extent)| extent < 3)
        .collect()
}

/// `thin` as "size.w is 2 and size.h is 1".
fn thin_sizes(thin: &[(&str, u32)]) -> String {
    thin.iter()
        .map(|(axis, extent)| format!("size.{axis} is {extent}"))
        .collect::<Vec<_>>()
        .join(" and ")
}

/// `thin` as the growth that gives the struct an interior, completing "grow …".
fn thin_growth(thin: &[(&str, u32)]) -> String {
    thin.iter()
        .map(|(axis, _)| format!("`size.{axis}` to at least 3"))
        .collect::<Vec<_>>()
        .join(" and ")
}

/// The `offset=` values an `at=inside.<side>` plate on a wall `length` long takes, completing
/// "an `offset=` …".
fn inside_offsets(length: u32) -> String {
    match length {
        0..=2 => "away from both ends of the wall".to_owned(),
        3 => "of 1".to_owned(),
        _ => format!("from 1 to {}", length - 2),
    }
}

/// The primary of an `at=<side>.outside` plate with no exterior voxel beside it.
fn no_exterior_primary(side: WallSide, row: PlateRow) -> String {
    format!(
        "pressure_plate `at={}.outside` at {} has no exterior voxel to sit on",
        side_name(side),
        row.named(),
    )
}

/// The refusal of an `at=<side>.outside` plate beside no roof written with an overhang, on a wall
/// `length` long.
fn no_exterior_voxel(
    member: &Member,
    side: WallSide,
    length: u32,
    row: PlateRow,
    ctx: &StructCtx<'_>,
) -> Diagnostic {
    let roof = "give a `roof` in the struct's body an `overhang=` of at least 1";
    let fix = if thin_axes(ctx).is_empty() {
        // On row 0 an inside plate would replace a floor block, one below the floor's top.
        let (raised, on_the_floor) = if row.world() == 0 && ctx.written.floor {
            (" y=1", ", on top of the floor")
        } else {
            ("", "")
        };
        format!(
            "{roof}, or use `at=inside.{}{raised}` with an `offset=` {}{on_the_floor}",
            side_name(side),
            inside_offsets(length),
        )
    } else {
        roof.to_owned()
    };
    diag_invalid_member(
        member,
        no_exterior_primary(side, row),
        vec![
            "no roof on this struct draws an overhang, so it has no voxel outside its walls"
                .to_owned(),
            fix,
        ],
    )
}

/// Why an `at=inside.<side>` plate at an `offset=` within its wall's `length` has no interior
/// cell, as a primary and its notes, or `None`.
///
/// The interior is strictly inside the footprint's wall ring, whether or not walls paint it.
fn inside_plate_refusal(
    side: WallSide,
    offset: u32,
    length: u32,
    y: u32,
    ctx: &StructCtx<'_>,
) -> Option<(String, Vec<String>)> {
    let name = side_name(side);
    let thin = thin_axes(ctx);
    let corner = offset == 0 || offset == length - 1;
    if thin.is_empty() && !corner {
        return None;
    }
    let valid_offsets = inside_offsets(length);
    if thin.is_empty() {
        let inside_it = if ctx.written.walls.course_top_at(y).is_some() {
            "belongs to the neighbouring wall"
        } else {
            "is not an interior cell of this struct"
        };
        return Some((
            format!(
                "pressure_plate `at=inside.{name} offset={offset}` is at a corner of the {name} \
                 wall, so the voxel inside it {inside_it}",
            ),
            vec![format!(
                "use an `offset=` {valid_offsets} to reach an interior voxel"
            )],
        ));
    }
    let also_corner = if corner {
        format!(", and `offset={offset}` is at a corner of the {name} wall besides")
    } else {
        String::new()
    };
    let use_offset = if corner {
        format!(" and use an `offset=` {valid_offsets}")
    } else {
        String::new()
    };
    // `<side>.outside` finds a cell only in a drawn overhang.
    let outside = if ctx.overhang > 0 && y < ctx.dims.y {
        format!(", or anchor the plate with `at={name}.outside`")
    } else {
        String::new()
    };
    Some((
        format!(
            "pressure_plate `at=inside.{name} offset={offset}`: the struct's {}, so it has no \
             interior voxel{also_corner}",
            thin_sizes(&thin),
        ),
        vec![
            INTERIOR_NEEDS.to_owned(),
            format!("grow {}{use_offset}{outside}", thin_growth(&thin)),
        ],
    ))
}

/// The block id a `pressure_plate` paints: its `mat_slot=`, or the pack's default.
///
/// A binding with a state literal defers and paints nothing. An unresolved binding was already
/// reported, so falling back adds no finding about the binding.
fn plate_id_for_member(
    member: &Member,
    ctx: &StructCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<String, Refusal> {
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
        return Err(Box::new(diag_deferred_member_reason(
            member,
            &format!(
                "pressure_plate does not honour bracketed state literals; the `mat_slot=` binding to `{}[...]` was not applied",
                state.id,
            ),
        )));
    }
    match resolved {
        Some(state) => Ok(state.id),
        None => pack_plate_default(member, ctx.registry),
    }
}

/// The pack's pressure plate id, or [`PRESSURE_PLATE_BASE_ID`], checked against the target; the
/// `E_UNKNOWN_ID` for an id the target lacks.
fn pack_plate_default(
    member: &Member,
    registry: Option<&dyn TargetRegistry>,
) -> Result<String, Refusal> {
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
        Ok(state) => Ok(state.id),
        Err(MaterialDeferred::UnknownId(unknown)) => {
            Err(Box::new(diag_unknown_id(member.span.clone(), &unknown)))
        }
        // INVARIANT(validated-id-refuses-one-way): `validated_id` answers the state or `UnknownId`.
        Err(other) => {
            debug_assert!(
                false,
                "validated_id returned {other:?} for the pressure plate default, \
                 which resolves no mat_slot= value",
            );
            Err(Box::new(diag_deferred_member_reason(
                member,
                "pressure_plate default resolved to no block id",
            )))
        }
    }
}
