//! A `def` a body's `place` line builds inside it, by a wall or at a cell, turned to face away
//! from the wall or the way its `facing=` names (`spec/components-editing-sites` "A `def` placed
//! in a body").

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticNote};
use crate::furniture::{self, Placed};
use crate::intent::{DefIr, Member, MemberRole};

use crate::block_array::Bbox;
use crate::block_array::args::{diag_deferred_member_noted, diag_invalid_member};
use crate::block_array::geometry::Declared;
use crate::block_array::joins::{Compass, Face};
use crate::block_array::openings::{WallSide, wall_length, wall_local_to_grid};
use crate::block_array::roof::inward_compass;

use super::canvas::{MemberCanvas, ScopePalette};
use super::cell_box::{Cell, CellBox};
use super::fixture::{Anchor, AnchorForms, Fixture, Refusal, anchor_in, coordinate};
use super::members::{MemberState, RowFit, StructCtx, resolve_member_state, side_name};
use super::turn::Turn;

/// How a `place` line's findings name it.
const PLACE: Fixture = Fixture {
    keyword: "place",
    noun: "def",
};

/// The anchors a body's `place` stands a def by, and the note on an `at=` of neither form.
const PLACE_ANCHORS: AnchorForms = AnchorForms {
    outside: false,
    note: "write `at=inside.<side>` to stand the def against the inside of a wall, where `<side>` \
           is front, back, left or right, or `at=X,Y,Z` to stand it at a cell",
};

/// Build the def `member` names inside the body, or report why it is not built there.
pub(super) fn fill_place(
    member: &Member,
    y_offset: u32,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(placing) = Placing::of(member, y_offset, ctx, diagnostics) else {
        return;
    };
    let (stand, painted) = match placing.built(canvas) {
        Ok(built) => built,
        Err(refusal) => {
            diagnostics.push(*refusal);
            return;
        }
    };
    let cells = painted.iter().flat_map(|(_, cells)| cells.iter().copied());
    canvas.declare(Declared {
        anchor: stand.low,
        bbox: Bbox::around(cells).unwrap_or_else(|| Bbox::cell(stand.low)),
        host_face: stand.host_face,
    });
    let scope = ctx
        .scope
        .and_then(|scope| scope.placed.get(&member.span.start));
    // The library's theme binds every slot its defs read, so a body with no theme still builds it.
    let theme_missing = ctx.theme_missing && !placing.library;
    for (fill, cells) in painted {
        let mut raised = Vec::new();
        let state = resolve_member_state(fill, scope, ctx.registry, &mut raised, theme_missing);
        if placing.library {
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
                .used_def()
                .and_then(|name| furniture::placed(name, defs))
                .is_some_and(|placed| matches!(placed, Placed::File(_)))
    };
    member.read_slot().is_some() || places_a_file_def()
}

/// One `place` line and the def it builds, as each step of building it reads them.
struct Placing<'a> {
    member: &'a Member,
    def: &'a DefIr,
    /// Whether the def is the furniture library's.
    library: bool,
    /// The def's `(width, depth)` before it turns.
    footprint: (u32, u32),
    /// The `y` of the `level` the line stands under.
    y_offset: u32,
    ctx: &'a StructCtx<'a>,
}

/// A `fill` line of a placed def and the box it names, measured from the def's footprint.
type FillBox<'a> = (&'a Member, CellBox);

/// Each `fill` line of a placed def with the body cells it paints.
type Painted<'a> = Vec<(&'a Member, Vec<Cell>)>;

/// Where a placed def stands in the body and which way it is turned.
struct Stand {
    /// The low corner of its turned footprint, on its lowest row.
    low: Cell,
    turn: Turn,
    /// The face of the wall it stands against, `None` at a coordinate triple.
    host_face: Option<Face>,
}

impl Stand {
    /// The body cell that `cell` of a def with `footprint` lands in.
    fn at(&self, cell: Cell, footprint: (u32, u32)) -> Cell {
        let (x, y, z) = self.turn.cell(cell, footprint);
        (
            self.low.0.saturating_add(x),
            self.low.1.saturating_add(y),
            self.low.2.saturating_add(z),
        )
    }
}

impl<'a> Placing<'a> {
    /// The def `member`'s `use=` names, or `None` when it names none, which is reported here for
    /// a missing `use=` and elsewhere otherwise.
    fn of(
        member: &'a Member,
        y_offset: u32,
        ctx: &'a StructCtx<'a>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Option<Self> {
        if !member.arguments.contains_key("use") {
            diagnostics.push(diag_invalid_member(
                member,
                "place has no `use=`, so nothing says which def it builds".to_owned(),
                vec![
                    "name a def of this file or of the furniture library, as in `use=table`"
                        .to_owned(),
                ],
            ));
            return None;
        }
        // `check::type_mismatch` reports a `use=` that is not a label, and the resolver one that
        // names no def.
        let placed = furniture::placed(member.used_def()?, ctx.defs)?;
        let def = placed.def();
        Some(Self {
            member,
            def,
            library: matches!(placed, Placed::Library(_)),
            footprint: def
                .size
                .as_ref()
                .map_or((1, 1), |size| (size.w.get(), size.h.get())),
            y_offset,
            ctx,
        })
    }

    /// Where the def stands and the body cells each of its lines paints, or the finding that
    /// leaves it unbuilt.
    fn built(&self, canvas: &MemberCanvas<'_>) -> Result<(Stand, Painted<'a>), Refusal> {
        let boxes = self.buildable()?;
        let stand = self.stand()?;
        // Before any cell is listed, so a def as tall as a `u32` costs nothing.
        self.fits_the_rows(&boxes, &stand)?;
        let painted: Painted<'a> = boxes
            .iter()
            .map(|&(line, fill)| {
                let cells = fill.cells().map(|cell| stand.at(cell, self.footprint));
                (line, cells.collect())
            })
            .collect();
        self.clear_of_openings(&painted, canvas)?;
        Ok((stand, painted))
    }

    /// Each line of the def with the box it fills, or the refusal of a def a body cannot build.
    fn buildable(&self) -> Result<Vec<FillBox<'a>>, Refusal> {
        let def = self.def;
        let name = &def.name;
        if def.size.is_none() {
            return Err(Box::new(diag_invalid_member(
                self.member,
                format!(
                    "def `{name}` declares no `size=`, so nothing says how much floor it takes"
                ),
                vec![format!("give it a footprint, as in `def {name} size=1x1:`")],
            )));
        }
        let (w, d) = self.footprint;
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
                return Err(self.not_buildable(line, &what));
            }
            let corner = |key: &str| match line.arguments.get(key).map(|raw| &raw.value.kind) {
                Some(ValueKind::Coord { x, y, z }) => {
                    let at = |v: u64| u32::try_from(v).ok();
                    Some((at(*x)?, at(*y)?, at(*z)?))
                }
                _ => None,
            };
            let (Some(from), Some(to)) = (corner("from"), corner("to")) else {
                return Err(self.not_buildable(
                    line,
                    "a `fill` whose `from=` and `to=` are not two coordinate triples",
                ));
            };
            let fill = CellBox::spanning(from, to);
            let (_, high) = fill.corners();
            if high.0 >= w || high.2 >= d {
                return Err(self.not_buildable(
                    line,
                    &format!("a `fill` reaching past its {w}x{d} footprint"),
                ));
            }
            boxes.push((line, fill));
        }
        Ok(boxes)
    }

    /// The refusal of the place for the `line` of its def it cannot build, which `what` names.
    fn not_buildable(&self, line: &Member, what: &str) -> Refusal {
        let mut finding = diag_invalid_member(
            self.member,
            format!(
                "def `{}` holds {what}, which a def placed in a body cannot build",
                self.def.name
            ),
            vec![
                "a def placed in a body is built from `fill` lines that each write a `mat_slot=` \
                 and a box inside its footprint; a def of any other line is placed from a `site`"
                    .to_owned(),
            ],
        );
        // A library line is no line of the file, so no note can point at it.
        if !self.library {
            finding.notes.push(DiagnosticNote {
                span: Some(line.span.clone()),
                message: "the line it cannot build".to_owned(),
            });
        }
        Box::new(finding)
    }

    /// Where the line stands the def, or the refusal of an `at=`, `offset=`, `y=` or `facing=`
    /// that puts it nowhere in the body.
    fn stand(&self) -> Result<Stand, Refusal> {
        let member = self.member;
        let Some(at) = member.arguments.get("at") else {
            return Err(Box::new(diag_invalid_member(
                member,
                "place has no `at=`, so nothing says where the def stands".to_owned(),
                vec![PLACE_ANCHORS.note.to_owned()],
            )));
        };
        match &at.value.kind {
            ValueKind::DotRef(_) => self.by_the_wall(),
            ValueKind::Coord { x, y, z } => self.at_the_cell((*x, *y, *z)),
            _ => Err(Box::new(diag_invalid_member(
                member,
                format!(
                    "place `at=` must be `inside.<side>` or a coordinate triple, not {}",
                    at.value.describe(),
                ),
                vec![PLACE_ANCHORS.note.to_owned()],
            ))),
        }
    }

    /// Where the def stands against the inside of the wall the line's `at=` names.
    fn by_the_wall(&self) -> Result<Stand, Refusal> {
        let (member, ctx) = (self.member, self.ctx);
        let anchor = anchor_in(PLACE, member, PLACE_ANCHORS)?;
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
        let (w, d) = self.footprint;
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
                    "def `{}` is {w} wide and {d} deep, so by the {wall} wall at `offset={offset}` \
                     it takes columns {offset} to {last} and {d} rows in",
                    self.def.name,
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
            low: (low_x, row.saturating_add(self.y_offset), low_z),
            turn: Turn::toward(inward_compass(side)),
            host_face: Some(anchor.host_face()),
        })
    }

    /// Where the def stands at the coordinate triple the line's `at=` names, turned by its
    /// `facing=`.
    fn at_the_cell(&self, (x, y, z): (u64, u64, u64)) -> Result<Stand, Refusal> {
        let (member, ctx) = (self.member, self.ctx);
        let toward = match member.written("facing") {
            None => Compass::South,
            Some(raw) => {
                let compass = match &raw.value.kind {
                    ValueKind::Ident(word) => Face::from_name(word).and_then(Compass::of),
                    _ => None,
                };
                compass.ok_or_else(|| {
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
        let turn = Turn::toward(toward);
        let (across, deep) = turn.footprint(self.footprint);
        let (width, depth) = (ctx.interior_w, ctx.interior_h);
        let reach = |at: u64, extent: u32| at.saturating_add(u64::from(extent));
        if reach(x, across) > u64::from(width) || reach(z, deep) > u64::from(depth) {
            return Err(Box::new(diag_invalid_member(
                member,
                format!(
                    "def `{}` takes {across}x{deep} cells as it faces, so at `at={x},{y},{z}` it \
                     reaches past the struct's `size={width}x{depth}` footprint",
                    self.def.name,
                ),
                vec![format!(
                    "the low corner of its footprint lies at `at=`, and the struct's `x` runs \
                     from 0 to {} and its `z` from 0 to {}",
                    width.saturating_sub(1),
                    depth.saturating_sub(1),
                )],
            )));
        }
        let grid = |v: u64, by: u32| u32::try_from(v).unwrap_or(u32::MAX).saturating_add(by);
        Ok(Stand {
            low: (
                grid(x, ctx.overhang),
                grid(y, self.y_offset),
                grid(z, ctx.overhang),
            ),
            turn,
            host_face: None,
        })
    }

    /// The finding on a def whose top row the body does not reach as built: a refusal above
    /// every row the struct as written reaches, and a deferral below one.
    fn fits_the_rows(&self, boxes: &[FillBox<'_>], stand: &Stand) -> Result<(), Refusal> {
        let ctx = self.ctx;
        let base = u64::from(stand.low.1);
        let top = boxes
            .iter()
            .map(|(_, fill)| base.saturating_add(u64::from(fill.corners().1.1)))
            .max()
            .unwrap_or(base);
        let primary = || {
            format!(
                "def `{}` placed here reaches row {top}, which does not fit in the struct \
                 (dims.y={})",
                self.def.name, ctx.dims.y,
            )
        };
        match ctx.row_fit(top, "these rows", "the def is built") {
            RowFit::Fits => Ok(()),
            RowFit::Above => {
                let mut notes = vec![format!(
                    "the struct's top row is {}",
                    ctx.written.height.saturating_sub(1)
                )];
                notes.extend(ctx.unread_roof_note());
                notes
                    .push("stand the def lower, or raise the `walls` to reach its rows".to_owned());
                Err(Box::new(diag_invalid_member(self.member, primary(), notes)))
            }
            RowFit::Waits(note) => Err(Box::new(diag_deferred_member_noted(
                self.member,
                &primary(),
                &note,
            ))),
        }
    }

    /// The refusal of a def that takes a cell a `door`, `window` or `arch` holds, which painting
    /// it would erase.
    fn clear_of_openings(
        &self,
        painted: &Painted<'_>,
        canvas: &MemberCanvas<'_>,
    ) -> Result<(), Refusal> {
        let opening = painted
            .iter()
            .flat_map(|(_, cells)| cells)
            .find_map(|&cell| canvas.opening_at(cell))
            .and_then(|index| self.ctx.paint_set.get(index as usize));
        let Some(opening) = opening else {
            return Ok(());
        };
        let keyword = opening.role.keyword();
        let mut finding = diag_invalid_member(
            self.member,
            format!(
                "def `{}` placed here takes a cell the `{keyword}` holds, and would erase it",
                self.def.name,
            ),
            Vec::new(),
        );
        finding.notes = vec![
            DiagnosticNote {
                span: Some(opening.span.clone()),
                message: format!("the `{keyword}` whose cell it takes"),
            },
            DiagnosticNote {
                span: None,
                message: "stand the def clear of every `door`, `window` and `arch`".to_owned(),
            },
        ];
        Err(Box::new(finding))
    }
}
