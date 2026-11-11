//! The physical tiles of `spec/redstone` "Edition differences": each Edition Cell, sensor pad and
//! actuator riser as a fixed pattern of parts facing east, turned and placed whole.

use cairn_lang_core::block_array::Face;

use crate::edition_netlist_ir::EditionCell;
use crate::geometry::{Rel, SIDES};
use crate::netlist_ir::PortName;
use crate::part::{Part, RepeaterDelay};

/// The most stages a `pulse` cell holds: its inverted operand runs one block of dust past each of
/// its repeaters, and dust carries 15 blocks.
pub const MAX_PULSE_STAGES: u32 = 6;

/// The most stages a `counter` cell holds: its operand and the operand inverted each run along
/// every stage on one strand of dust.
pub const MAX_COUNTER_STAGES: u32 = 4;

/// The delay of each repeater a `delay` or `pulse` runs its operand through: a repeater stretches
/// a change shorter than its delay, and one as long meets its own tick, so only a one-tick
/// repeater passes a change held for a stage as it came under every update order.
const MACRO_REPEATER: RepeaterDelay = RepeaterDelay::ONE;

/// Which input a tile's port takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TilePort {
    /// A cell's operand.
    Cell(PortName),
    /// A `counter`'s own output, fed back.
    Feedback,
    /// The signal an actuator's riser carries up to it.
    Actuator,
}

impl TilePort {
    /// The port's name in the JSON form: the operand's, `feedback` or `actuator`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cell(port) => port.as_str(),
            Self::Feedback => "feedback",
            Self::Actuator => "actuator",
        }
    }
}

impl serde::Serialize for TilePort {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// One input of a tile: the dust a wire ends on and the strength it must arrive there with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TemplatePort {
    /// The dust the wire ends on.
    pub(crate) at: Rel,
    /// What the port takes.
    pub(crate) port: TilePort,
    /// The weakest signal that still drives the tile.
    pub(crate) need: u8,
}

/// The strength a tile's output leaves with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Strength {
    /// Full strength, from a torch or a repeater.
    Full,
    /// What reaches the port at this index of [`Template::ports`].
    Rear(usize),
}

/// A tile facing east, its origin at `(0, 0, 0)`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Template {
    /// Every part, dust included.
    pub(crate) parts: Vec<(Rel, Part)>,
    /// The inputs.
    pub(crate) ports: Vec<TemplatePort>,
    /// The dust wires leave from.
    pub(crate) outs: Vec<Rel>,
    /// The dust each part driving the output powers, one group per part.
    pub(crate) groups: Vec<Vec<Rel>>,
    /// The strength [`Self::groups`] hold while the output is on.
    pub(crate) strength: Option<Strength>,
    /// Torches unlit and diodes powered at rest.
    pub(crate) seeds: Vec<(Rel, bool)>,
    /// Cells the tile's own dust climbs past, which must stay air.
    pub(crate) air: Vec<Rel>,
    /// The torch whose light is the tile's output, for a riser.
    pub(crate) probe: Option<Rel>,
}

/// The cells a template's parts keep clear around them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Keepout {
    /// Cells no dust and no diode may take: a torch or a block it would power, the sides of
    /// dust pointing into a block.
    pub(crate) no_dust: Vec<Rel>,
    /// Cells that must stay air: over a torch, which would power a block there.
    pub(crate) air: Vec<Rel>,
}

impl Template {
    /// The part at `rel`.
    pub(crate) fn part(&self, rel: Rel) -> Option<Part> {
        self.parts
            .iter()
            .find_map(|(at, part)| (*at == rel).then_some(*part))
    }

    /// The port taking `port`.
    pub(crate) fn port(&self, port: TilePort) -> Option<&TemplatePort> {
        self.ports.iter().find(|p| p.port == port)
    }

    /// The cells around the parts that nothing else may take.
    pub(crate) fn keepout(&self) -> Keepout {
        let mut out = Keepout {
            no_dust: Vec::new(),
            air: self.air.clone(),
        };
        let free = |rel: Rel| self.part(rel).is_none() && !self.air.contains(&rel);
        for &(at, part) in &self.parts {
            match part {
                Part::Torch { facing } => {
                    for face in SIDES {
                        if face != facing.opposite() && free(at.step(face)) {
                            out.no_dust.push(at.step(face));
                        }
                    }
                    if free(at.step(Face::Down)) {
                        out.no_dust.push(at.step(Face::Down));
                    }
                    if free(at.step(Face::Up)) {
                        out.air.push(at.step(Face::Up));
                    }
                }
                Part::Block => {
                    for face in SIDES.into_iter().chain([Face::Up]) {
                        if free(at.step(face)) {
                            out.no_dust.push(at.step(face));
                        }
                    }
                }
                Part::Dust => {
                    for face in SIDES {
                        if self.part(at.step(face)) != Some(Part::Block) {
                            continue;
                        }
                        let side = face.clockwise().unwrap_or(Face::North);
                        for across in [side, side.opposite()] {
                            let beside = at.step(across);
                            for cell in [beside, beside.step(Face::Up), beside.step(Face::Down)] {
                                if free(cell) {
                                    out.no_dust.push(cell);
                                }
                            }
                        }
                    }
                }
                Part::Comparator { facing, .. } => {
                    let side = facing.clockwise().unwrap_or(Face::North);
                    for across in [side, side.opposite()] {
                        if free(at.step(across)) {
                            out.no_dust.push(at.step(across));
                        }
                    }
                }
                Part::Repeater { .. } => {}
            }
        }
        out.no_dust
            .sort_unstable_by_key(|rel| (rel.x, rel.y, rel.z));
        out.no_dust.dedup();
        out.air.sort_unstable_by_key(|rel| (rel.x, rel.y, rel.z));
        out.air.dedup();
        out.no_dust.retain(|rel| !out.air.contains(rel));
        out
    }
}

/// Lays a [`Template`] out part by part.
#[derive(Default)]
struct Builder {
    template: Template,
}

impl Builder {
    fn put(&mut self, x: i32, y: i32, z: i32, part: Part) -> &mut Self {
        self.template.parts.push((Rel::new(x, y, z), part));
        self
    }

    fn block(&mut self, x: i32, z: i32) -> &mut Self {
        self.put(x, 0, z, Part::Block)
    }

    fn dust(&mut self, x: i32, z: i32) -> &mut Self {
        self.put(x, 0, z, Part::Dust)
    }

    /// Dust along `x` from `from` to `to`, both included, at `z`.
    fn row(&mut self, from: i32, to: i32, z: i32) -> &mut Self {
        for x in from.min(to)..=from.max(to) {
            self.dust(x, z);
        }
        self
    }

    fn torch(&mut self, x: i32, z: i32, facing: Face) -> &mut Self {
        self.put(x, 0, z, Part::Torch { facing })
    }

    fn repeater(&mut self, x: i32, z: i32, facing: Face, delay: RepeaterDelay) -> &mut Self {
        self.put(x, 0, z, Part::Repeater { facing, delay })
    }

    fn port_at(&mut self, at: Rel, port: TilePort, need: u8) -> &mut Self {
        self.template.parts.push((at, Part::Dust));
        self.template.ports.push(TemplatePort { at, port, need });
        self
    }

    fn port(&mut self, x: i32, z: i32, port: PortName, need: u8) -> &mut Self {
        self.port_at(Rel::flat(x, z), TilePort::Cell(port), need)
    }

    fn out(&mut self, x: i32, y: i32, z: i32) -> &mut Self {
        self.template.parts.push((Rel::new(x, y, z), Part::Dust));
        self.template.outs.push(Rel::new(x, y, z));
        self
    }

    /// The output dust each driving part powers, `cells` of the tile's own layer per group.
    fn groups(&mut self, groups: &[&[(i32, i32)]]) -> &mut Self {
        self.template.groups = groups
            .iter()
            .map(|cells| cells.iter().map(|&(x, z)| Rel::flat(x, z)).collect())
            .collect();
        self
    }

    fn seed(&mut self, x: i32, y: i32, z: i32, on: bool) -> &mut Self {
        self.template.seeds.push((Rel::new(x, y, z), on));
        self
    }

    fn done(&mut self) -> Template {
        let mut template = std::mem::take(&mut self.template);
        template.strength.get_or_insert(Strength::Full);
        if template.groups.is_empty() {
            template.groups = template.outs.iter().map(|out| vec![*out]).collect();
        }
        template
    }
}

/// The tile realising `cell`.
pub(crate) fn cell_template(cell: EditionCell) -> Template {
    match cell {
        EditionCell::JavaComparatorAnd => comparator_and(),
        EditionCell::BedrockTorchAnd => torch_and(),
        EditionCell::JavaRepeaterOr | EditionCell::BedrockRepeaterOr => repeater_or(),
        EditionCell::JavaInverterTorch | EditionCell::BedrockInverterTorch => inverter(),
        EditionCell::JavaTorchNor | EditionCell::BedrockTorchNor => nor(),
        EditionCell::JavaTorchNand | EditionCell::BedrockTorchNand => nand(),
        EditionCell::JavaTorchXor | EditionCell::BedrockTorchXor => xor(),
        EditionCell::JavaTorchMux | EditionCell::BedrockTorchMux => mux(),
        EditionCell::JavaRepeaterDelay { stages }
        | EditionCell::BedrockRepeaterDelay { stages } => delay(stages.get()),
        EditionCell::JavaTorchPulse { stages } | EditionCell::BedrockTorchPulse { stages } => {
            pulse(stages.get())
        }
        EditionCell::JavaTorchEdgeRising | EditionCell::BedrockTorchEdgeRising => edge_rising(),
        EditionCell::JavaTorchEdgeFalling | EditionCell::BedrockTorchEdgeFalling => edge_falling(),
        EditionCell::JavaTorchLatch | EditionCell::BedrockTorchLatch => latch(),
        EditionCell::JavaRepeaterCounter { stages }
        | EditionCell::BedrockRepeaterCounter { stages } => counter(stages.get()),
    }
}

/// The columns a `delay` of `stages` takes in its one row: its port, its repeaters and its output.
pub(crate) fn delay_columns(stages: u32) -> u64 {
    (2 * u64::from(stages)).div_ceil(u64::from(MACRO_REPEATER.ticks())) + 2
}

/// How many repeaters of [`MACRO_REPEATER`] take `ticks` in all.
fn repeaters(ticks: u32) -> i32 {
    i32::try_from(ticks.div_ceil(u32::from(MACRO_REPEATER.ticks()))).unwrap_or(i32::MAX)
}

/// `count` as a coordinate.
fn coord(count: usize) -> i32 {
    i32::try_from(count).unwrap_or(i32::MAX)
}

/// A torch on a block powered from its west side: its output is `not a`.
fn inverter() -> Template {
    Builder::default()
        .port(-1, 0, PortName::A, 2)
        .dust(0, 0)
        .block(1, 0)
        .torch(2, 0, Face::East)
        .out(3, 0, 0)
        .done()
}

/// The inverter, with `b` powering its block from the south.
fn nor() -> Template {
    Builder::default()
        .port(-1, 0, PortName::A, 2)
        .dust(0, 0)
        .block(1, 0)
        .torch(2, 0, Face::East)
        .out(3, 0, 0)
        .dust(1, 1)
        .port(1, 2, PortName::B, 2)
        .done()
}

/// Two inverters side by side onto one strand of dust.
fn nand() -> Template {
    Builder::default()
        .port(-1, 0, PortName::A, 2)
        .dust(0, 0)
        .block(1, 0)
        .torch(2, 0, Face::East)
        .port(-1, 2, PortName::B, 2)
        .dust(0, 2)
        .block(1, 2)
        .torch(2, 2, Face::East)
        .out(3, 0, 0)
        .out(3, 0, 1)
        .out(3, 0, 2)
        .groups(&[&[(3, 0)], &[(3, 2)]])
        .done()
}

/// The two inverters' strand inverted once more.
fn torch_and() -> Template {
    Builder::default()
        .port(-1, 0, PortName::A, 2)
        .dust(0, 0)
        .block(1, 0)
        .torch(2, 0, Face::East)
        .port(-1, 2, PortName::B, 2)
        .dust(0, 2)
        .block(1, 2)
        .torch(2, 2, Face::East)
        .row(3, 3, 0)
        .row(3, 3, 1)
        .row(3, 3, 2)
        .dust(4, 1)
        .block(5, 1)
        .torch(6, 1, Face::East)
        .out(7, 0, 1)
        .done()
}

/// Two repeaters driving one block of dust, from the west and from the south.
fn repeater_or() -> Template {
    Builder::default()
        .port(-1, 0, PortName::A, 1)
        .repeater(0, 0, Face::East, RepeaterDelay::ONE)
        .out(1, 0, 0)
        .repeater(1, 1, Face::North, RepeaterDelay::ONE)
        .port(1, 2, PortName::B, 1)
        .done()
}

/// A comparator subtracting `not b`, at full strength on its side, from `a` at its rear.
fn comparator_and() -> Template {
    let mut builder = Builder::default();
    builder
        .port(-1, 0, PortName::A, 1)
        .put(
            0,
            0,
            0,
            Part::Comparator {
                facing: Face::East,
                subtract: true,
            },
        )
        .out(1, 0, 0)
        .dust(0, 1)
        .torch(0, 2, Face::North)
        .block(0, 3)
        .dust(0, 4)
        .port(0, 5, PortName::B, 2);
    builder.template.strength = Some(Strength::Rear(0));
    builder.done()
}

/// `n` the NOR of `a` and `b`, then the NORs of `a` and `n` and of `b` and `n` onto one strand.
fn xor() -> Template {
    Builder::default()
        .port(-1, 0, PortName::A, 6)
        .row(0, 4, 0)
        .block(5, 0)
        .torch(6, 0, Face::East)
        .port(-1, 4, PortName::B, 6)
        .row(0, 4, 4)
        .block(5, 4)
        .torch(6, 4, Face::East)
        .dust(2, 1)
        .dust(2, 3)
        .block(2, 2)
        .torch(3, 2, Face::East)
        .row(4, 5, 2)
        .dust(5, 1)
        .dust(5, 3)
        .out(7, 0, 0)
        .out(7, 0, 1)
        .out(7, 0, 2)
        .out(7, 0, 3)
        .out(7, 0, 4)
        .groups(&[&[(7, 0)], &[(7, 4)]])
        .done()
}

/// The NOR of `not a` and `sel` and the NOR of `not b` and `not sel`, onto one strand.
fn mux() -> Template {
    let mut builder = Builder::default();
    builder
        .port(-1, 0, PortName::A, 2)
        .dust(0, 0)
        .block(1, 0)
        .torch(2, 0, Face::East)
        .row(3, 5, 0)
        .block(6, 0)
        .torch(7, 0, Face::East)
        .port(-1, 2, PortName::Sel, 9)
        .row(0, 6, 2)
        .dust(6, 1)
        .dust(2, 3)
        .block(2, 4)
        .torch(3, 4, Face::East)
        .row(4, 6, 4)
        .dust(6, 5)
        .port(-1, 6, PortName::B, 2)
        .dust(0, 6)
        .block(1, 6)
        .torch(2, 6, Face::East)
        .row(3, 5, 6)
        .block(6, 6)
        .torch(7, 6, Face::East);
    for z in 0..=6 {
        builder.out(8, 0, z);
    }
    builder.groups(&[&[(8, 0)], &[(8, 6)]]).done()
}

/// Repeaters in series, two ticks a stage.
fn delay(stages: u32) -> Template {
    let mut builder = Builder::default();
    builder.port(-1, 0, PortName::A, 1);
    let r = repeaters(stages.saturating_mul(2));
    for x in 0..r {
        builder.repeater(x, 0, Face::East, MACRO_REPEATER);
    }
    builder.out(r, 0, 0).done()
}

/// The NOR of `not a` and of `a` through repeaters of a tick more than `stages` stages, matching
/// the inverter, so it is on for `stages` stages.
fn pulse(stages: u32) -> Template {
    let r = repeaters(stages.saturating_mul(2).saturating_add(1));
    let mut builder = Builder::default();
    builder.port(-1, 0, PortName::A, 3).dust(0, 0);
    for x in 1..=r {
        builder.repeater(x, 0, Face::East, MACRO_REPEATER);
    }
    builder
        .dust(r + 1, 0)
        .block(r + 2, 0)
        .torch(r + 3, 0, Face::East)
        .out(r + 4, 0, 0)
        .dust(0, 1)
        .block(0, 2)
        .torch(1, 2, Face::East)
        .row(2, r + 2, 2)
        .dust(r + 2, 1)
        .done()
}

/// A pulse of one stage.
fn edge_rising() -> Template {
    pulse(1)
}

/// The NOR of `a` and of `not a` through a repeater, so `not a` arrives a tick after the output
/// turns on and holds it on for a stage.
fn edge_falling() -> Template {
    Builder::default()
        .port(-1, 0, PortName::A, 5)
        .row(0, 3, 0)
        .block(4, 0)
        .torch(5, 0, Face::East)
        .out(6, 0, 0)
        .dust(0, 1)
        .block(0, 2)
        .torch(1, 2, Face::East)
        .repeater(2, 2, Face::East, RepeaterDelay::ONE)
        .row(3, 4, 2)
        .dust(4, 1)
        .done()
}

/// The output torch the NOR of `reset` and a second torch, which is the NOR of the output and
/// of `set` gated by `reset` held two ticks past its fall.
fn latch() -> Template {
    Builder::default()
        .port(-6, 2, PortName::Set, 2)
        .dust(-5, 2)
        .block(-4, 2)
        .torch(-3, 2, Face::East)
        .dust(-2, 2)
        .block(-1, 2)
        .torch(0, 2, Face::East)
        .dust(1, 2)
        .block(2, 2)
        .torch(3, 2, Face::East)
        .dust(4, 2)
        .block(5, 2)
        .torch(5, 1, Face::North)
        .out(5, 0, 0)
        .out(4, 0, 0)
        .out(3, 0, 0)
        .out(2, 0, 0)
        .dust(2, 1)
        .repeater(-1, 4, Face::North, RepeaterDelay::TWO)
        .dust(-1, 3)
        .port(-2, 5, PortName::Reset, 10)
        .row(-1, 5, 5)
        .dust(5, 4)
        .dust(5, 3)
        .groups(&[&[(5, 0)]])
        .seed(0, 0, 2, false)
        .seed(5, 0, 1, false)
        .done()
}

/// `stages` master–slave pairs of repeaters, masters locked by `a` and slaves by `not a`, the
/// last slave fed back inverted to the first master over the inverter of `a`.
fn counter(stages: u32) -> Template {
    let n = coord(usize::try_from(stages.clamp(1, MAX_COUNTER_STAGES)).unwrap_or(1));
    let need = u8::try_from(2 * n + 6).unwrap_or(15);
    let mut builder = Builder::default();
    builder
        .port(-5, -2, PortName::A, need)
        .row(-4, 2 * n, -2)
        .dust(-4, -1)
        .block(-4, 0)
        .torch(-4, 1, Face::South)
        .row(-4, 2 * n + 1, 2)
        .port_at(Rel::new(-5, 1, 0), TilePort::Feedback, 1)
        .block(-5, 0)
        .put(
            -4,
            1,
            0,
            Part::Repeater {
                facing: Face::East,
                delay: RepeaterDelay::ONE,
            },
        )
        .put(-3, 1, 0, Part::Dust)
        .block(-3, 0)
        .put(-2, 1, 0, Part::Dust)
        .block(-2, 0)
        .dust(-1, 0)
        .block(0, 0)
        .torch(1, 0, Face::East);
    builder.template.air.push(Rel::new(-1, 1, 0));
    for i in 1..=n {
        builder
            .repeater(2 * i, 0, Face::East, RepeaterDelay::ONE)
            .repeater(2 * i + 1, 0, Face::East, RepeaterDelay::ONE)
            .repeater(2 * i, -1, Face::South, RepeaterDelay::TWO)
            .repeater(2 * i + 1, 1, Face::North, RepeaterDelay::ONE)
            .seed(2 * i + 1, 0, 1, true);
    }
    builder.seed(2, 0, 0, true).out(2 * n + 2, 0, 0).done()
}

/// The kind of pad a sensor's signal enters the circuit by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PadKind {
    /// Dust under the sensor, or under the block it powers.
    Dust,
    /// A block under a plate on the floor row, with dust under it.
    Block,
    /// Dust under a daylight detector and a repeater, since its strength varies with the sky.
    Repeater,
    /// Dust under an observer's back and a two-tick repeater, which holds its one-tick pulse for a
    /// stage.
    Observer,
}

/// The pad of `kind`, its origin the top layer's cell under the sensor or its block.
pub(crate) fn pad_template(kind: PadKind) -> Template {
    let mut builder = Builder::default();
    match kind {
        PadKind::Dust => builder.out(0, 0, 0),
        PadKind::Block => builder.put(0, 0, 0, Part::Block).out(0, -1, 0),
        PadKind::Repeater | PadKind::Observer => builder
            .put(0, 0, 0, Part::Dust)
            .put(
                1,
                0,
                0,
                Part::Repeater {
                    facing: Face::East,
                    delay: if kind == PadKind::Observer {
                        RepeaterDelay::TWO
                    } else {
                        RepeaterDelay::ONE
                    },
                },
            )
            .out(2, 0, 0),
    };
    builder.done()
}

/// Two torches carrying a signal up to the top layer's cell under an actuator, the origin.
pub(crate) fn riser_template() -> Template {
    let mut builder = Builder::default();
    builder
        .put(0, 0, 0, Part::Torch { facing: Face::East })
        .put(-1, 0, 0, Part::Block)
        .put(-1, -1, 0, Part::Torch { facing: Face::East })
        .put(-2, -1, 0, Part::Block)
        .put(-3, -1, 0, Part::Dust)
        .port_at(Rel::new(-4, -1, 0), TilePort::Actuator, 2)
        .seed(0, 0, 0, false);
    builder.template.probe = Some(Rel::new(0, 0, 0));
    builder.template.strength = None;
    builder.template.clone()
}

/// The ticks a riser takes from its port to its torch.
pub const RISER_TICKS: u64 = 2;

#[cfg(test)]
mod tests;
