//! A `def` a body's `place` line builds inside it, by a wall or at a cell, turned to face away
//! from the wall or the way its `facing=` names (`spec/components-editing-sites` "A `def` placed
//! in a body").

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticNote};
use crate::furniture;
use crate::intent::{DefIr, Member, MemberRole};

use crate::block_array::Bbox;
use crate::block_array::args::{diag_deferred_member_noted, diag_invalid_member};
use crate::block_array::geometry::Declared;
use crate::block_array::joins::Face;
use crate::block_array::openings::{WallSide, wall_length, wall_local_to_grid};

use super::canvas::{MemberCanvas, ScopePalette};
use super::cell_box::Cell;
use super::fixture::{Anchor, Fixture, Refusal, anchor_of, coordinate};
use super::members::{
    MemberState, StructCtx, resolve_member_state, side_name, unplaced_walls_note,
};
use super::turn::Turn;

/// How a `place` line's findings name it.
const PLACE: Fixture = Fixture {
    keyword: "place",
    noun: "def",
};

/// The note on an `at=` of neither form.
const AT_FORMS: &str = "write `at=inside.<side>` to stand the def against the inside of a wall, \
                        where `<side>` is front, back, left or right, or `at=X,Y,Z` to stand it \
                        at a cell";

/// Build the def `member` names inside the body, or report why it is not built there.
pub(super) fn fill_place(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some((def, library)) = placed_def(member, ctx, diagnostics) else {
        return;
    };
    let built = buildable(member, def)
        .and_then(|boxes| stand(member, def, y_offset, ctx).map(|stand| (boxes, stand)))
        .and_then(|(boxes, stand)| cells(member, def, &boxes, &stand, ctx).map(|c| (stand, c)));
    let (stand, painted) = match built {
        Ok(built) => built,
        Err(refusal) => {
            diagnostics.push(*refusal);
            return;
        }
    };
    canvas.declare(Declared {
        anchor: stand.low,
        bbox: painted
            .iter()
            .flat_map(|(_, cells)| cells.iter().copied())
            .fold(Bbox::cell(stand.low), Bbox::including),
        host_face: stand.host_face,
    });
    let scope = ctx
        .scope
        .and_then(|scope| scope.placed.get(&member.span.start));
    // The library's theme binds every slot its defs read, so a body with no theme still builds it.
    let theme_missing = ctx.theme_missing && !library;
    for (fill, cells) in painted {
        let mut raised = Vec::new();
        let state = resolve_member_state(fill, scope, ctx.registry, &mut raised, theme_missing);
        if library {
            // A library line is no line of the file, so its findings stand on the `place`.
            for finding in &mut raised {
                if finding.span == fill.span {
                    finding.span.clone_from(&member.span);
                }
            }
        }
        diagnostics.extend(raised);
        let MemberState::Bound(state) = state else {
            continue;
        };
        let index = palette.intern(stand.turn.state(&state));
        for cell in cells {
            canvas.paint(cell, || index);
        }
    }
}

/// Whether `member` reads a slot of its body's theme: its own `mat_slot=`, or, on a `place`, the
/// slots of the def of `defs` it names, which only that theme binds.
pub(super) fn reads_a_slot(member: &Member, defs: &[DefIr]) -> bool {
    let places_a_file_def = || {
        matches!(member.role, MemberRole::Place)
            && member
                .arguments
                .get("use")
                .and_then(|written| written.value.as_label_str())
                .is_some_and(|name| defs.iter().any(|def| def.name == name))
    };
    member.read_slot().is_some() || places_a_file_def()
}

/// The def `member`'s `use=` names, and whether it is the furniture library's; `None` when it
/// names none, which is reported here for a missing `use=` and elsewhere otherwise.
fn placed_def<'a>(
    member: &Member,
    ctx: &StructCtx<'a>,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<(&'a DefIr, bool)> {
    let Some(written) = member.arguments.get("use") else {
        diagnostics.push(diag_invalid_member(
            member,
            "place has no `use=`, so nothing says which def it builds".to_owned(),
            vec![
                "name a def of this file or of the furniture library, as in `use=table`".to_owned(),
            ],
        ));
        return None;
    };
    // `check::type_mismatch` reports a `use=` that is not a label, and the resolver one that
    // names no def.
    let name = written.value.as_label_str()?;
    if let Some(def) = ctx.defs.iter().find(|def| def.name == name) {
        return Some((def, false));
    }
    furniture::def(name).map(|def| (def, true))
}

/// A `fill` line of a placed def and the box it names, measured from the def's footprint.
type FillBox<'a> = (&'a Member, Cell, Cell);

/// Each line of `def` with the box it fills, or the refusal of a def a body cannot build.
fn buildable<'a>(member: &Member, def: &'a DefIr) -> Result<Vec<FillBox<'a>>, Refusal> {
    let name = &def.name;
    if def.size.is_none() {
        return Err(Box::new(diag_invalid_member(
            member,
            format!("def `{name}` declares no `size=`, so nothing says how much floor it takes"),
            vec![format!("give it a footprint, as in `def {name} size=1x1:`")],
        )));
    }
    let (w, d) = footprint(def);
    let mut boxes = Vec::with_capacity(def.members.len());
    for line in &def.members {
        let fills = matches!(line.role, MemberRole::Fill) && !line.pours_fluid();
        if !fills || line.read_slot().is_none() {
            let what = if fills {
                "a `fill` with no `mat_slot=`".to_owned()
            } else if matches!(line.role, MemberRole::Fill) {
                "a water `fill`".to_owned()
            } else {
                format!("a `{}` line", line.role.keyword())
            };
            return Err(Box::new(not_buildable(member, def, line, &what)));
        }
        let corner = |key: &str| match line.arguments.get(key).map(|raw| &raw.value.kind) {
            Some(ValueKind::Coord { x, y, z }) => {
                let at = |v: u64| u32::try_from(v).ok();
                Some((at(*x)?, at(*y)?, at(*z)?))
            }
            _ => None,
        };
        let (Some(from), Some(to)) = (corner("from"), corner("to")) else {
            return Err(Box::new(not_buildable(
                member,
                def,
                line,
                "a `fill` whose `from=` and `to=` are not two coordinate triples",
            )));
        };
        let low = (from.0.min(to.0), from.1.min(to.1), from.2.min(to.2));
        let high = (from.0.max(to.0), from.1.max(to.1), from.2.max(to.2));
        if high.0 >= w || high.2 >= d {
            return Err(Box::new(not_buildable(
                member,
                def,
                line,
                &format!("a `fill` reaching past its {w}x{d} footprint"),
            )));
        }
        boxes.push((line, low, high));
    }
    Ok(boxes)
}

/// The refusal of `member` for the `line` of `def` it cannot build, which `what` names.
fn not_buildable(member: &Member, def: &DefIr, line: &Member, what: &str) -> Diagnostic {
    let mut finding = diag_invalid_member(
        member,
        format!(
            "def `{}` holds {what}, which a def placed in a body cannot build",
            def.name
        ),
        vec![
            "a def placed in a body is built from `fill` lines that each write a `mat_slot=` \
             and a box inside its footprint; a def of any other line is placed from a `site`"
                .to_owned(),
        ],
    );
    if furniture::def(&def.name).is_none_or(|library| !std::ptr::eq(library, def)) {
        finding.notes.push(DiagnosticNote {
            span: Some(line.span.clone()),
            message: "the line it cannot build".to_owned(),
        });
    }
    finding
}

/// The width and depth of the footprint `buildable` found `def` to declare.
fn footprint(def: &DefIr) -> (u32, u32) {
    def.size
        .as_ref()
        .map_or((1, 1), |size| (size.w.get(), size.h.get()))
}

/// Where a placed def stands in the body and which way it is turned.
struct Stand {
    /// The low corner of its turned footprint, on its lowest row.
    low: Cell,
    turn: Turn,
    /// The face of the wall it stands against, `None` at a coordinate triple.
    host_face: Option<Face>,
}

/// Where `member` stands `def`, or the refusal of an `at=`, `offset=`, `y=` or `facing=` that
/// puts it nowhere in the body.
fn stand(
    member: &Member,
    def: &DefIr,
    y_offset: u32,
    ctx: &StructCtx<'_>,
) -> Result<Stand, Refusal> {
    let Some(at) = member.arguments.get("at") else {
        return Err(Box::new(diag_invalid_member(
            member,
            "place has no `at=`, so nothing says where the def stands".to_owned(),
            vec![AT_FORMS.to_owned()],
        )));
    };
    match &at.value.kind {
        ValueKind::DotRef(_) => by_the_wall(member, def, y_offset, ctx),
        ValueKind::Coord { x, y, z } => at_the_cell(member, def, (*x, *y, *z), y_offset, ctx),
        _ => Err(Box::new(diag_invalid_member(
            member,
            format!(
                "place `at=` must be `inside.<side>` or a coordinate triple, not {}",
                at.value.describe(),
            ),
            vec![AT_FORMS.to_owned()],
        ))),
    }
}

/// The turn that brings a front away from the `side` wall, into the struct.
fn away_from(side: WallSide) -> Turn {
    let into = match side {
        WallSide::Back => Face::South,
        WallSide::Front => Face::North,
        WallSide::Left => Face::East,
        WallSide::Right => Face::West,
    };
    Turn::to_face(into).expect("a wall faces sideways")
}

/// Where `def` stands against the inside of the wall `member`'s `at=` names.
fn by_the_wall(
    member: &Member,
    def: &DefIr,
    y_offset: u32,
    ctx: &StructCtx<'_>,
) -> Result<Stand, Refusal> {
    let anchor = anchor_of(PLACE, member)?;
    let Anchor::Inside(side) = anchor else {
        return Err(Box::new(diag_invalid_member(
            member,
            format!(
                "place `at={}.outside` stands the def outside its wall, and a def stands only \
                 inside one",
                side_name(anchor.side()),
            ),
            vec![format!(
                "write `at=inside.{}` to stand it against the inside of that wall",
                side_name(anchor.side()),
            )],
        )));
    };
    let offset = coordinate(
        PLACE,
        member,
        "offset",
        "cells along the wall from 0, its left end seen from outside",
    )?;
    let row = coordinate(
        PLACE,
        member,
        "y",
        "rows up from 0, the base row of the struct or of the def's `level`",
    )?;
    let (w, d) = footprint(def);
    let length = wall_length(side, ctx.interior_w, ctx.interior_h);
    let across = wall_length(
        match side {
            WallSide::Front | WallSide::Back => WallSide::Left,
            WallSide::Left | WallSide::Right => WallSide::Front,
        },
        ctx.interior_w,
        ctx.interior_h,
    );
    let last = offset.saturating_add(w).saturating_sub(1);
    if offset == 0 || last.saturating_add(2) > length || d.saturating_add(2) > across {
        let wall = side_name(side);
        let mut notes = vec![format!(
            "a def by a wall stands strictly inside the wall ring: along the {wall} wall, \
             columns 1 to {}, and at most {} rows into the struct",
            length.saturating_sub(2),
            across.saturating_sub(2),
        )];
        if length < 3 || across < 3 {
            notes.push("an interior needs a size of at least 3 on both axes".to_owned());
        }
        return Err(Box::new(diag_invalid_member(
            member,
            format!(
                "def `{}` is {w} wide and {d} deep, so by the {wall} wall at `offset={offset}` it \
                 takes columns {offset} to {last} and {d} rows in",
                def.name,
            ),
            notes,
        )));
    }
    let (dx, dz) = side.outward_normal();
    let mut low: Option<Cell> = None;
    for u in offset..=last {
        let (wall_x, _, wall_z) = wall_local_to_grid(
            side,
            u,
            0,
            ctx.overhang,
            ctx.interior_w,
            ctx.interior_h,
            ctx.dims,
        )
        .expect("the column lies on the wall");
        // The back row, one cell in, and the row `d` cells in from the wall.
        for depth in [1, d] {
            let step = i64::from(depth);
            let cell_x = u32::try_from(i64::from(wall_x) - i64::from(dx) * step).unwrap_or(0);
            let cell_z = u32::try_from(i64::from(wall_z) - i64::from(dz) * step).unwrap_or(0);
            low = Some(low.map_or((cell_x, 0, cell_z), |(lx, _, lz)| {
                (lx.min(cell_x), 0, lz.min(cell_z))
            }));
        }
    }
    let (low_x, _, low_z) = low.expect("a def is at least one column wide");
    Ok(Stand {
        low: (low_x, row.saturating_add(y_offset), low_z),
        turn: away_from(side),
        host_face: Some(anchor.host_face()),
    })
}

/// Where `def` stands at the coordinate triple `member`'s `at=` names, turned by its `facing=`.
fn at_the_cell(
    member: &Member,
    def: &DefIr,
    (x, y, z): (u64, u64, u64),
    y_offset: u32,
    ctx: &StructCtx<'_>,
) -> Result<Stand, Refusal> {
    let turn = match member.written("facing") {
        None => Turn::to_face(Face::South).expect("south is sideways"),
        Some(raw) => {
            let face = match &raw.value.kind {
                ValueKind::Ident(word) => Face::from_name(word),
                _ => None,
            };
            face.and_then(Turn::to_face).ok_or_else(|| {
                Box::new(diag_invalid_member(
                    member,
                    format!(
                        "place `facing=` must be a compass word, not {}",
                        raw.value.describe()
                    ),
                    vec![
                        "`facing=` names the way the def's front looks: `north`, `east`, \
                         `south` or `west`, and `south` when absent"
                            .to_owned(),
                    ],
                ))
            })?
        }
    };
    let (across, deep) = turn.footprint(footprint(def));
    let (width, depth) = (ctx.interior_w, ctx.interior_h);
    let reach = |at: u64, extent: u32| at.saturating_add(u64::from(extent));
    if reach(x, across) > u64::from(width) || reach(z, deep) > u64::from(depth) {
        return Err(Box::new(diag_invalid_member(
            member,
            format!(
                "def `{}` takes {across}x{deep} cells as it faces, so at `at={x},{y},{z}` it \
                 reaches past the struct's `size={width}x{depth}` footprint",
                def.name,
            ),
            vec![format!(
                "the low corner of its footprint lies at `at=`, and the struct's `x` runs from 0 \
                 to {} and its `z` from 0 to {}",
                width.saturating_sub(1),
                depth.saturating_sub(1),
            )],
        )));
    }
    let grid = |v: u64, by: u32| u32::try_from(v).unwrap_or(u32::MAX).saturating_add(by);
    Ok(Stand {
        low: (
            grid(x, ctx.overhang),
            grid(y, y_offset),
            grid(z, ctx.overhang),
        ),
        turn,
        host_face: None,
    })
}

/// Each line of `boxes` with the body cells it fills once `stand` turns and places it, or the
/// finding on a def that reaches a row the body does not.
fn cells<'a>(
    member: &Member,
    def: &DefIr,
    boxes: &[FillBox<'a>],
    stand: &Stand,
    ctx: &StructCtx<'_>,
) -> Result<Vec<(&'a Member, Vec<Cell>)>, Refusal> {
    let footprint = footprint(def);
    let mut painted = Vec::with_capacity(boxes.len());
    for &(line, low, high) in boxes {
        let mut held = Vec::new();
        for y in low.1..=high.1 {
            for z in low.2..=high.2 {
                for x in low.0..=high.0 {
                    let (tx, ty, tz) = stand.turn.cell((x, y, z), footprint);
                    held.push((
                        stand.low.0.saturating_add(tx),
                        stand.low.1.saturating_add(ty),
                        stand.low.2.saturating_add(tz),
                    ));
                }
            }
        }
        painted.push((line, held));
    }
    let top = boxes
        .iter()
        .map(|&(_, _, high)| stand.low.1.saturating_add(high.1))
        .max()
        .unwrap_or(stand.low.1);
    match past_the_rows(member, def, top, ctx) {
        Some(finding) => Err(Box::new(finding)),
        None => Ok(painted),
    }
}

/// The finding on a def whose top row is `top` when the body is built below it: a refusal above
/// every row the struct as written reaches, and a deferral below one.
fn past_the_rows(
    member: &Member,
    def: &DefIr,
    top: u32,
    ctx: &StructCtx<'_>,
) -> Option<Diagnostic> {
    let written = &ctx.written;
    let above_written = top >= written.height;
    let primary = format!(
        "def `{}` placed here reaches row {top}, which does not fit in the struct (dims.y={})",
        def.name, ctx.dims.y,
    );
    if above_written && !written.unplaced_walls && top >= written.reach {
        let mut notes = vec![format!(
            "the struct's top row is {}",
            written.height.saturating_sub(1)
        )];
        notes.extend(ctx.unread_roof_note());
        notes.push("stand the def lower, or raise the `walls` to reach its rows".to_owned());
        return Some(diag_invalid_member(member, primary, notes));
    }
    let (rows, built) = ("these rows", "the def is built");
    let note = if above_written && written.unplaced_walls {
        unplaced_walls_note(rows, built)
    } else if above_written {
        format!(
            "a `roof` whose `kind=` or `slope_to=` does not read may rise to {rows}; {built} once \
             that roof draws"
        )
    } else if top >= ctx.dims.y {
        ctx.shortfall().note(rows, built)
    } else {
        return None;
    };
    Some(diag_deferred_member_noted(member, &primary, &note))
}
