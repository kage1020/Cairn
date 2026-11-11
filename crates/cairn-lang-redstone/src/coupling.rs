//! `spec/redstone` "Place-and-route": where each sensor's signal enters the circuit under the base
//! plane, where each actuator's leaves it, and which columns the top layer may wire under.

use std::collections::HashMap;

use cairn_lang_core::block_array::{BlockArray, Face, ResolvedBlock, ResolvedMember};
use cairn_lang_core::error::Span;
use cairn_lang_core::intent::{Actuator, Member, SENSOR_HOSTS};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::edition_netlist_ir::EditionNetlistIr;
use crate::geometry::Voxel;
use crate::netlist_ir::NetRef;
use crate::synth::{actuator_host, patch_candidates, signal_named_by};
use crate::tile::PadKind;

/// A cell of the built array, `(x, y, z)`, its `y = 0` the base plane.
type Cell = (u32, u32, u32);

/// What paints a cell, and the line of the member that does.
#[derive(Debug, Clone, Copy)]
struct Paint<'a> {
    painter: Painter,
    span: &'a Span,
}

/// What paints a cell of the base plane or the row over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Painter {
    /// A component a circuit reads.
    Sensor,
    /// A component power moves.
    Actuator,
    /// A Tier 0 part or a raw member, whose block no table describes.
    Unknown,
}

impl Painter {
    fn of(keyword: &str) -> Option<Self> {
        if SENSOR_HOSTS.contains(&keyword) {
            Some(Self::Sensor)
        } else if Actuator::ALL
            .iter()
            .any(|actuator| actuator.keyword() == keyword)
        {
            Some(Self::Actuator)
        } else if matches!(
            keyword,
            "repeater"
                | "comparator"
                | "redstone_torch"
                | "redstone_dust"
                | "raw_block"
                | "raw_fill"
        ) {
            Some(Self::Unknown)
        } else {
            None
        }
    }

    /// Whether a block beside it may be powered, or power what is under it, through it.
    const fn signals(self) -> bool {
        matches!(self, Self::Sensor | Self::Unknown)
    }
}

/// One column of the base plane, as the top layer under it sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Column {
    /// The base-plane cell holds a block.
    pub(crate) filled: bool,
    /// The cell is air, or a block no component paints.
    pub(crate) plain: bool,
    /// The cell is plain and nothing beside or over it signals into it, so dust may run under it.
    pub(crate) quiet: bool,
    /// The column of the one component beside or over the cell that signals into it, if one does
    /// and no other.
    pub(crate) heard: Option<(i32, i32)>,
}

impl Column {
    /// Whether dust may run under the cell for a pad whose sensor stands in column `own`, which
    /// may signal into the cell since its signal is the pad's.
    pub(crate) fn quiet_but_for(self, own: (i32, i32)) -> bool {
        self.quiet || (self.plain && self.heard == Some(own))
    }
}

/// The base plane over a reservation, column by column.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Surface {
    pub(crate) width: u32,
    pub(crate) depth: u32,
    columns: Vec<Column>,
}

impl Surface {
    /// The column at `(x, z)`, or `None` outside the reservation.
    pub(crate) fn column(&self, x: i32, z: i32) -> Option<Column> {
        let x = u32::try_from(x).ok().filter(|x| *x < self.width)?;
        let z = u32::try_from(z).ok().filter(|z| *z < self.depth)?;
        let index = usize::try_from(z * self.width + x).ok()?;
        self.columns.get(index).copied()
    }
}

/// Where a sensor's signal enters the circuit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pad {
    /// The input of the netlist the sensor drives.
    pub(crate) input: usize,
    pub(crate) kind: PadKind,
    /// The pad's origin, in the top layer.
    pub(crate) origin: Voxel,
    /// The sensor's line.
    pub(crate) span: Span,
    /// The sensor as a finding names it.
    pub(crate) named: String,
}

/// Where an actuator's signal leaves the circuit: the torch under the block that drives it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Riser {
    /// The output of the netlist that drives the actuator.
    pub(crate) output: usize,
    /// The riser's top torch, in the top layer.
    pub(crate) top: Voxel,
    /// The actuator's line.
    pub(crate) span: Span,
    /// The actuator as a finding names it.
    pub(crate) named: String,
}

/// A scope's terminals and the surface over its reservation.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Coupled {
    pub(crate) surface: Surface,
    pub(crate) pads: Vec<Pad>,
    pub(crate) risers: Vec<Riser>,
}

/// One component as built.
struct Component<'a> {
    keyword: &'a str,
    /// Its lowest cell.
    cell: Cell,
    cells: Vec<Cell>,
    facing: Option<Face>,
    span: Span,
    named: String,
}

/// The terminals of `netlist` in `array`, the struct `members` lowered to, over a circuit `layers`
/// deep; a component that cannot reach the circuit is a finding.
pub(crate) fn couple(
    netlist: &EditionNetlistIr,
    members: &[Member],
    array: &BlockArray,
    resolved: &[ResolvedMember],
    layers: u32,
    scope: &str,
) -> (Coupled, Vec<Diagnostic>) {
    let painted = painted(resolved);
    let surface = surface(array, &painted);
    let top = i32::try_from(layers).unwrap_or(i32::MAX) - 1;
    let candidates = patch_candidates(members);
    let built: HashMap<(usize, usize), &ResolvedMember> = resolved
        .iter()
        .map(|member| ((member.span.start, member.span.end), member))
        .collect();
    let filled = |cell: Cell| filled(array, cell);
    let mut coupled = Coupled {
        surface,
        ..Coupled::default()
    };
    let mut diagnostics = Vec::new();

    for (input, sensor) in netlist.inputs.iter().enumerate() {
        if !has_sinks(netlist, NetRef::Input(crate::saturating_index(input))) {
            continue;
        }
        let Some(member) = candidates.iter().find(|member| {
            member
                .binding
                .as_ref()
                .is_some_and(|binding| binding.span == sensor.span)
        }) else {
            continue;
        };
        let Some(component) = component(member, &built) else {
            continue;
        };
        match pad_for(&component, filled) {
            Ok((kind, (x, z))) => coupled.pads.push(Pad {
                input,
                kind,
                origin: Voxel::new(coord(x), top, coord(z)),
                span: component.span.clone(),
                named: component.named.clone(),
            }),
            Err(reason) => diagnostics.push(unreachable(&component, scope, &reason, &[])),
        }
    }

    for (output, actuator) in netlist.outputs.iter().enumerate() {
        let Some(host) = candidates.iter().find_map(|member| {
            member
                .written_fields()
                .any(|(_, value)| {
                    value.span == actuator.span && signal_named_by(&value.value).is_some()
                })
                .then(|| actuator_host(member, &candidates).ok())
                .flatten()
        }) else {
            continue;
        };
        let Some(component) = component(host, &built) else {
            continue;
        };
        let shared = riser_for(&component, filled)
            .map_err(|reason| (reason, Vec::new()))
            .and_then(|(x, z)| {
                let beside = [
                    x.checked_sub(1).map(|x| (x, 0, z)),
                    Some((x + 1, 0, z)),
                    z.checked_sub(1).map(|z| (x, 0, z)),
                    Some((x, 0, z + 1)),
                    Some((x, 1, z)),
                ];
                let mut sharing: Vec<&Span> = beside
                    .into_iter()
                    .flatten()
                    .filter(|cell| !component.cells.contains(cell))
                    .filter_map(|cell| painted.get(&cell))
                    .filter(|paint| matches!(paint.painter, Painter::Actuator | Painter::Unknown))
                    .map(|paint| paint.span)
                    .collect();
                sharing.sort_by_key(|span| (span.start, span.end));
                sharing.dedup();
                if sharing.is_empty() {
                    Ok((x, z))
                } else {
                    Err((
                        Unreachable {
                            reason: "the block that drives it from under the floor would power the component beside that block too",
                            fix: "move it, or the component beside the block under it, so that no other actuator, Tier 0 part or raw member touches that block",
                        },
                        sharing,
                    ))
                }
            });
        match shared {
            Ok((x, z)) => coupled.risers.push(Riser {
                output,
                top: Voxel::new(coord(x), top, coord(z)),
                span: component.span.clone(),
                named: component.named.clone(),
            }),
            Err((reason, sharing)) => {
                diagnostics.push(unreachable(&component, scope, &reason, &sharing));
            }
        }
    }
    (coupled, diagnostics)
}

/// Whether anything reads `net`.
fn has_sinks(netlist: &EditionNetlistIr, net: NetRef) -> bool {
    netlist
        .cells
        .iter()
        .any(|cell| cell.drivers.iter().any(|driver| driver.net == net))
        || netlist.outputs.iter().any(|output| output.driver == net)
}

/// What paints each cell that a sensor, an actuator, a Tier 0 part or a raw member holds.
fn painted(resolved: &[ResolvedMember]) -> HashMap<Cell, Paint<'_>> {
    let mut by_cell = HashMap::new();
    for member in resolved {
        let Some(painter) = Painter::of(&member.role) else {
            continue;
        };
        for cell in member.resolved_state.iter().flat_map(ResolvedBlock::cells) {
            by_cell.insert(
                cell,
                Paint {
                    painter,
                    span: &member.span,
                },
            );
        }
    }
    by_cell
}

fn filled(array: &BlockArray, (x, y, z): Cell) -> bool {
    array
        .dims
        .index(x, y, z)
        .and_then(|index| array.voxels.get(index))
        .is_some_and(|slot| slot.0 != 0)
}

fn surface(array: &BlockArray, painted: &HashMap<Cell, Paint<'_>>) -> Surface {
    let (width, depth) = (array.dims.x, array.dims.z);
    let mut columns = Vec::new();
    for z in 0..depth {
        for x in 0..width {
            let filled = filled(array, (x, 0, z));
            let plain = !painted.contains_key(&(x, 0, z));
            let beside = [
                x.checked_sub(1).map(|x| (x, 0, z)),
                Some((x + 1, 0, z)),
                z.checked_sub(1).map(|z| (x, 0, z)),
                Some((x, 0, z + 1)),
                Some((x, 1, z)),
            ];
            let mut signalling: Vec<(i32, i32)> = beside
                .into_iter()
                .flatten()
                .filter(|cell| {
                    painted
                        .get(cell)
                        .is_some_and(|paint| paint.painter.signals())
                })
                .map(|(x, _, z)| (coord(x), coord(z)))
                .collect();
            signalling.sort_unstable();
            signalling.dedup();
            columns.push(Column {
                filled,
                plain,
                quiet: plain && signalling.is_empty(),
                heard: match signalling.as_slice() {
                    [one] => Some(*one),
                    _ => None,
                },
            });
        }
    }
    Surface {
        width,
        depth,
        columns,
    }
}

fn component<'a>(
    member: &'a Member,
    built: &HashMap<(usize, usize), &'a ResolvedMember>,
) -> Option<Component<'a>> {
    let resolved = built.get(&(member.span.start, member.span.end))?;
    let cells: Vec<Cell> = resolved
        .resolved_state
        .iter()
        .flat_map(ResolvedBlock::cells)
        .collect();
    let cell = cells.iter().copied().min_by_key(|&(x, y, z)| (y, z, x))?;
    let facing = resolved
        .resolved_state
        .first()
        .and_then(|block| block.state.properties.get("facing"))
        .and_then(|name| Face::from_name(name));
    let keyword = member.role.keyword();
    let named = match &member.id {
        Some(id) => format!("the {keyword} `{id}`"),
        None => format!("the {keyword}"),
    };
    Some(Component {
        keyword,
        cell,
        cells,
        facing,
        span: member.span.clone(),
        named,
    })
}

/// Why a component cannot reach the circuit, and how to move it so it does.
struct Unreachable {
    reason: &'static str,
    fix: &'static str,
}

/// The pad a sensor's signal enters by, and the column it stands in.
fn pad_for(
    sensor: &Component<'_>,
    filled: impl Fn(Cell) -> bool,
) -> Result<(PadKind, (u32, u32)), Unreachable> {
    let (x, y, z) = sensor.cell;
    let on_floor = y == 1 && filled((x, 0, z));
    let kind = match sensor.keyword {
        "pressure_plate" if y == 0 => PadKind::Block,
        "pressure_plate" if on_floor => PadKind::Dust,
        "pressure_plate" => {
            return Err(Unreachable {
                reason: "a pressure plate reaches the layer under the floor from the floor row `y = 0`, or from `y = 1` standing on a block, which it powers whole",
                fix: "set `y=0` on the plate, or `y=1` over a floor block",
            });
        }
        "lever" | "button" if y == 0 => PadKind::Dust,
        "lever" | "button" => {
            return Err(Unreachable {
                reason: "a lever or button reaches the layer under the floor only from the floor row `y = 0`, the row over it",
                fix: "set `y=0` on it",
            });
        }
        "daylight" if y == 0 => PadKind::Repeater,
        "daylight" => {
            return Err(Unreachable {
                reason: "a daylight detector reaches the layer under the floor only from the floor row `y = 0`, the row over it",
                fix: "set `y=0` on it",
            });
        }
        "observer" if sensor.facing == Some(Face::Up) && (y == 0 || on_floor) => PadKind::Observer,
        _ => {
            return Err(Unreachable {
                reason: "an observer reaches the layer under the floor only looking up, so that its back powers the cell under it, from the floor row `y = 0` or from `y = 1` over a block",
                fix: "write `facing=up` on the observer, and set `y=0`, or `y=1` over a floor block",
            });
        }
    };
    Ok((kind, (x, z)))
}

/// The column of the block, or the actuator itself, that the riser's top torch powers.
fn riser_for(
    actuator: &Component<'_>,
    filled: impl Fn(Cell) -> bool,
) -> Result<(u32, u32), Unreachable> {
    let (x, y, z) = actuator.cell;
    if actuator.keyword == "piston" && actuator.facing == Some(Face::Down) {
        return Err(Unreachable {
            reason: "a piston looking down would push into the torch that drives it from under the floor",
            fix: "turn the piston with another `facing=`",
        });
    }
    let on_floor = y == 1 && filled((x, 0, z));
    match actuator.keyword {
        "door" if on_floor => Ok((x, z)),
        "door" => Err(Unreachable {
            reason: "a door is driven through the block under its lower half, so it must stand at `y = 1` over a floor block",
            fix: "stand the door on the floor row's blocks",
        }),
        _ if y == 0 || on_floor => Ok((x, z)),
        _ => Err(Unreachable {
            reason: "an actuator is driven from the layer under the floor, so it must stand in the floor row `y = 0`, or at `y = 1` over a floor block",
            fix: "set `y=0` on it, or `y=1` over a floor block",
        }),
    }
}

/// `component`'s refusal, with a note at each member in `sharing` that the block under it touches.
fn unreachable(
    component: &Component<'_>,
    scope: &str,
    why: &Unreachable,
    sharing: &[&Span],
) -> Diagnostic {
    let diagnostic = Diagnostic::new(
        DiagnosticCode::RouteUnreachableComponent,
        component.span.clone(),
        format!(
            "{scope} {named} at y={y} cannot reach the circuit under the floor: {reason}",
            named = component.named,
            y = component.cell.1,
            reason = why.reason,
        ),
    );
    sharing
        .iter()
        .fold(diagnostic, |diagnostic, span| {
            diagnostic.with_note(
                (*span).clone(),
                "the block that drives it touches this member, which it would power too",
            )
        })
        .with_footer(format!("Fix: {}", why.fix))
}

fn coord(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}
