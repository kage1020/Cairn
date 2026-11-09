//! `spec/redstone` "Edition differences": the components of a built struct that read one another
//! through a block update, a dependence on update order that no cell of the library absorbs.

use std::collections::{HashMap, HashSet};

use cairn_lang_core::Edition;
use cairn_lang_core::block_array::{
    BlockArray, BlockArrayIr, Dims, Face, ResolvedBlock, ResolvedMember,
};
use cairn_lang_core::error::Span;
use cairn_lang_core::intent::{Actuator, IntentModule, Member, SENSOR_HOSTS};

use crate::diagnostic::{Diagnostic, DiagnosticCode};
use crate::logic_ir::ScopeKind;
use crate::synth::{actuator_host, patch_candidates, signal_named_by};

/// A cell of a built struct, `(x, y, z)`.
type Cell = (u32, u32, u32);

/// An `E_NO_PORTABLE_IMPL` for each component of `built` whose behaviour on `edition` depends on
/// block-update order: an observer reading back an actuator the circuit drives, and on Java a
/// driven piston or dispenser that a sensor powers through the cell above it.
#[must_use]
pub fn verify_portable(
    module: &IntentModule,
    built: &BlockArrayIr,
    edition: Edition,
) -> Vec<Diagnostic> {
    let lines = DeclaredLines::of(module);
    let mut diagnostics: Vec<Diagnostic> = Vec::new();
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    for (key, members) in &built.members {
        let Some(array) = built.structures.get(key) else {
            continue;
        };
        let body = Body::of(members, array, &lines);
        for diagnostic in body.findings(edition) {
            if seen.insert((diagnostic.span.start, diagnostic.span.end)) {
                diagnostics.push(diagnostic);
            }
        }
    }
    diagnostics.sort_by_key(|d| (d.span.start, d.span.end));
    diagnostics
}

/// Every member line of the module, by span, with the scope that declares it and the actuators
/// the module's bindings drive.
struct DeclaredLines<'a> {
    members: HashMap<(usize, usize), (&'a Member, String)>,
    /// Each driven actuator's span, with the signal that drives it and where it is bound.
    driven: HashMap<(usize, usize), (String, Span)>,
}

impl<'a> DeclaredLines<'a> {
    fn of(module: &'a IntentModule) -> Self {
        let mut lines = Self {
            members: HashMap::new(),
            driven: HashMap::new(),
        };
        let bodies = module
            .structs
            .iter()
            .map(|s| (ScopeKind::Struct, &s.name, &s.members))
            .chain(
                module
                    .defs
                    .iter()
                    .map(|d| (ScopeKind::Def, &d.name, &d.members)),
            );
        for (kind, name, members) in bodies {
            let scope = format!("{}={name}:", kind.label());
            let candidates = patch_candidates(members);
            for member in &candidates {
                lines
                    .members
                    .insert(key_of(&member.span), (*member, scope.clone()));
                for (key, value) in member.written_fields() {
                    let drives = member.role.actuator().map(Actuator::key) == Some(key);
                    let Some(signal) = signal_named_by(&value.value).filter(|_| drives) else {
                        continue;
                    };
                    if let Ok(host) = actuator_host(member, &candidates) {
                        lines
                            .driven
                            .entry(key_of(&host.span))
                            .or_insert_with(|| (signal.to_string(), value.span.clone()));
                    }
                }
            }
        }
        lines
    }
}

fn key_of(span: &Span) -> (usize, usize) {
    (span.start, span.end)
}

/// One sensor or actuator as built: its line, the cells it holds and the way it was turned.
struct Component<'a> {
    member: &'a Member,
    scope: &'a str,
    cells: Vec<Cell>,
    facing: Option<Face>,
    /// The face a lever or button hangs by: `wall`, `floor` or `ceiling`.
    face: Option<&'a str>,
    /// The signal that drives it and where it is bound, for an actuator a binding drives.
    driven_by: Option<(&'a str, &'a Span)>,
}

impl Component<'_> {
    fn keyword(&self) -> &str {
        self.member.role.keyword()
    }

    fn cell(&self) -> Option<Cell> {
        self.cells.first().copied()
    }

    /// What this component is called in a finding: its keyword and its `id=`, if it has one.
    fn named(&self) -> String {
        match &self.member.id {
            Some(id) => format!("the {} `{id}`", self.keyword()),
            None => format!("the {}", self.keyword()),
        }
    }
}

/// One built body's components and the array they stand in.
struct Body<'a> {
    components: Vec<Component<'a>>,
    array: &'a BlockArray,
}

impl<'a> Body<'a> {
    fn of(
        members: &'a [ResolvedMember],
        array: &'a BlockArray,
        lines: &'a DeclaredLines<'_>,
    ) -> Self {
        let components = members
            .iter()
            .filter_map(|resolved| {
                let (member, scope) = lines.members.get(&key_of(&resolved.span))?;
                let keyword = member.role.keyword();
                if member.role.actuator().is_none() && !SENSOR_HOSTS.contains(&keyword) {
                    return None;
                }
                let first = resolved.resolved_state.first()?;
                let property = |key: &str| first.state.properties.get(key).map(String::as_str);
                Some(Component {
                    member,
                    scope,
                    cells: resolved
                        .resolved_state
                        .iter()
                        .flat_map(ResolvedBlock::cells)
                        .collect(),
                    facing: property("facing").and_then(Face::from_name),
                    face: property("face"),
                    driven_by: lines
                        .driven
                        .get(&key_of(&member.span))
                        .map(|(signal, bound)| (signal.as_str(), bound)),
                })
            })
            .collect();
        Self { components, array }
    }

    fn findings(&self, edition: Edition) -> Vec<Diagnostic> {
        let mut found: Vec<Diagnostic> = self
            .components
            .iter()
            .filter_map(|observer| self.read_back(observer))
            .collect();
        if edition == Edition::Java {
            found.extend(
                self.components
                    .iter()
                    .filter_map(|actuator| self.quasi_connected(actuator)),
            );
        }
        found
    }

    /// The observer `observer`'s finding when it watches a component the circuit drives.
    fn read_back(&self, observer: &Component<'_>) -> Option<Diagnostic> {
        let senses = observer.member.selector.is_none()
            && observer
                .member
                .binding
                .as_ref()
                .is_some_and(|binding| signal_named_by(binding).is_some());
        if observer.keyword() != "observer" || !senses {
            return None;
        }
        let watched = step(observer.cell()?, observer.facing?, self.array.dims)?;
        let actuator = self.components.iter().find(|actuator| {
            actuator.driven_by.is_some()
                && (actuator.cells.contains(&watched)
                    || head_cell(actuator, self.array.dims) == Some(watched))
        })?;
        let (signal, bound) = actuator.driven_by?;
        let named = actuator.named();
        Some(
            Diagnostic::new(
                DiagnosticCode::NoPortableImpl,
                observer.member.span.clone(),
                format!(
                    "{} this observer watches {named}, which the circuit drives, so its signal \
                     is the circuit's own change read back through a block update: this circuit \
                     requires update-order (BUD) semantics",
                    observer.scope,
                ),
            )
            .with_note(bound.clone(), format!("{named} is driven by {signal} here"))
            .with_footer(format!(
                "Fix: redesign the logic to be order-independent: an observer pulses on each \
                 change, as `edge_rising({signal}) or edge_falling({signal})` does in `logic`; \
                 or drop to Tier 0 with an `@edition` guard"
            )),
        )
    }

    /// The driven piston or dispenser `actuator`'s finding when a sensor powers the cell above it
    /// and not the actuator itself.
    fn quasi_connected(&self, actuator: &Component<'_>) -> Option<Diagnostic> {
        if !matches!(actuator.keyword(), "piston" | "dispenser") || actuator.driven_by.is_none() {
            return None;
        }
        let at = actuator.cell()?;
        let above = step(at, Face::Up, self.array.dims)?;
        // A piston takes no power through its front; a dispenser takes it through every face.
        let front = (actuator.keyword() == "piston")
            .then(|| actuator.facing.and_then(|f| step(at, f, self.array.dims)))
            .flatten();
        let sensor = self.components.iter().find(|sensor| {
            let sources = self.sources(sensor);
            let reaches = |target: Cell, barred: Option<Cell>| {
                sources
                    .iter()
                    .any(|(from, into)| into.contains(&target) && Some(*from) != barred)
            };
            reaches(above, None) && !reaches(at, front)
        })?;
        let named = actuator.named();
        let source = sensor.named();
        Some(
            Diagnostic::new(
                DiagnosticCode::NoPortableImpl,
                actuator.member.span.clone(),
                format!(
                    "{} on java, {named} also takes power from {source} through the cell above \
                     it, and acts on it only once a block update reaches it: this circuit \
                     requires update-order (quasi-connectivity) semantics",
                    actuator.scope,
                ),
            )
            .with_note(
                sensor.member.span.clone(),
                format!("{source} powers the cell above {named} from here"),
            )
            .with_footer(
                "bedrock has no quasi-connectivity, so this is no dependence on order there",
            )
            .with_footer(format!(
                "Fix: move {source} so that it powers no block beside the cell above {named}, \
                 or redesign the logic to be order-independent; or drop to Tier 0 with an \
                 `@edition java` guard"
            )),
        )
    }

    /// The cells `sensor` powers, each with the cell the power leaves from: those beside the
    /// sensor that it gives off a signal into, and those beside the block it powers whole.
    fn sources(&self, sensor: &Component<'_>) -> Vec<(Cell, Vec<Cell>)> {
        let keyword = sensor.keyword();
        let Some(at) = sensor.cell().filter(|_| SENSOR_HOSTS.contains(&keyword)) else {
            return Vec::new();
        };
        let dims = self.array.dims;
        let behind = || sensor.facing.and_then(|f| step(at, f.opposite(), dims));
        let (gives_off, whole) = match keyword {
            "observer" => (behind().into_iter().collect(), behind()),
            "daylight" => (beside(at, dims), None),
            "pressure_plate" => (beside(at, dims), step(at, Face::Down, dims)),
            _ => {
                let hangs_on = match sensor.face {
                    Some("floor") => step(at, Face::Down, dims),
                    Some("ceiling") => step(at, Face::Up, dims),
                    _ => behind(),
                };
                (beside(at, dims), hangs_on)
            }
        };
        let mut sources = vec![(at, gives_off)];
        if let Some(block) = whole.filter(|&cell| self.conducts(cell)) {
            sources.push((block, beside(block, dims)));
        }
        sources
    }

    /// Whether the block at `cell` passes on the power a sensor gives it: any block but air, a
    /// sensor and an actuator, since no table records which blocks conduct.
    fn conducts(&self, cell: Cell) -> bool {
        let filled = self
            .array
            .dims
            .index(cell.0, cell.1, cell.2)
            .and_then(|i| self.array.voxels.get(i))
            .is_some_and(|slot| slot.0 != 0);
        filled
            && !self
                .components
                .iter()
                .any(|component| component.cells.contains(&cell))
    }
}

/// The cell a driven piston's head moves into.
fn head_cell(actuator: &Component<'_>, dims: Dims) -> Option<Cell> {
    if actuator.keyword() != "piston" {
        return None;
    }
    step(actuator.cell()?, actuator.facing?, dims)
}

/// The six cells beside `cell` that lie in `dims`.
fn beside(cell: Cell, dims: Dims) -> Vec<Cell> {
    [
        Face::Down,
        Face::Up,
        Face::North,
        Face::South,
        Face::West,
        Face::East,
    ]
    .into_iter()
    .filter_map(|face| step(cell, face, dims))
    .collect()
}

/// The cell beside `cell` toward `face`, if it lies in `dims`.
fn step(cell: Cell, face: Face, dims: Dims) -> Option<Cell> {
    let (x, y, z) = cell;
    let next = match face {
        Face::Down => (x, y.checked_sub(1)?, z),
        Face::Up => (x, y.checked_add(1)?, z),
        Face::North => (x, y, z.checked_sub(1)?),
        Face::South => (x, y, z.checked_add(1)?),
        Face::West => (x.checked_sub(1)?, y, z),
        Face::East => (x.checked_add(1)?, y, z),
    };
    (next.0 < dims.x && next.1 < dims.y && next.2 < dims.z).then_some(next)
}
