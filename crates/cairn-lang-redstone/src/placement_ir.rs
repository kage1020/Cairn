//! Placement IR: one scope's circuit as tiles laid in its `circuit` reservation, which routing,
//! delay insertion and legalization each fill further (`spec/redstone` "Connection to the IR and
//! phases").

use cairn_lang_core::Edition;
use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::block_array::{BlockState, Face};
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::Serialize;

use crate::coupling::Surface;
use crate::edition_netlist_ir::EditionCell;
use crate::geometry::{Rel, Rotation, Voxel, serialize_face};
use crate::logic_ir::{ScopeKind, Scoped, ScopedEntry};
use crate::netlist_ir::NetRef;
use crate::part::Part;
use crate::tile::{self, PadKind, Template, TilePort};

/// How far through place-and-route a [`CircuitLayout`] has come, serialized as its `--stage` word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LayoutStage {
    /// Every tile has its voxel and turn.
    Placed,
    /// Every net has its dust.
    Routed,
    /// Every net carries the repeaters its strength needs.
    Delayed,
    /// Every voxel of the reservation has its block.
    Legalized,
}

impl LayoutStage {
    /// The stage's `--stage` word.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Placed => "placement",
            Self::Routed => "route",
            Self::Delayed => "delay",
            Self::Legalized => "legalize",
        }
    }
}

impl Serialize for LayoutStage {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// The room a `circuit` line reserves: `void` layers under the whole footprint of the struct, the
/// circuit in the top `layers` of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Reservation {
    /// `region=<label>`.
    pub label: String,
    /// `void=<N>`, the layers under the base plane.
    pub void: u32,
    /// The layers the circuit lies in, the top ones: `void`, or at most
    /// [`crate::placement::MAX_CIRCUIT_LAYERS`], the layers under them left empty.
    pub layers: u32,
    /// The built struct's extent along `x`.
    pub width: u32,
    /// The built struct's extent along `z`.
    pub depth: u32,
    /// The `circuit` line.
    #[serde(skip)]
    pub span: Span,
}

impl Reservation {
    /// The top layer, the one under the base plane.
    #[must_use]
    pub fn top(&self) -> i32 {
        i32::try_from(self.layers).unwrap_or(i32::MAX) - 1
    }

    /// The empty layers under the circuit's.
    #[must_use]
    pub const fn under(&self) -> u32 {
        self.void.saturating_sub(self.layers)
    }

    /// Whether `voxel` lies in the circuit's layers.
    #[must_use]
    pub fn contains(&self, voxel: Voxel) -> bool {
        let within = |c: i32, extent: u32| u32::try_from(c).is_ok_and(|c| c < extent);
        within(voxel.x, self.width) && within(voxel.y, self.layers) && within(voxel.z, self.depth)
    }
}

/// What a tile realises.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "tile", rename_all = "snake_case")]
pub enum TileKind {
    /// An Edition Cell of the netlist.
    Cell {
        /// The cell.
        cell: EditionCell,
    },
    /// The pad a sensor's signal enters by.
    Pad {
        /// Its shape.
        pad: PadKind,
    },
    /// The torches that carry a signal up to an actuator.
    Riser,
}

impl TileKind {
    /// The pattern the tile is laid from.
    pub(crate) fn template(self) -> Template {
        match self {
            Self::Cell { cell } => tile::cell_template(cell),
            Self::Pad { pad } => tile::pad_template(pad),
            Self::Riser => tile::riser_template(),
        }
    }
}

/// One input of a tile and the net that drives it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct TileInput {
    /// What the input takes.
    pub port: TilePort,
    /// Index into [`CircuitLayout::nets`].
    pub net: usize,
}

/// One tile, laid at a voxel of the reservation and turned.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlacedTile {
    /// What it realises.
    #[serde(flatten)]
    pub kind: TileKind,
    /// Where its origin lies.
    pub origin: Voxel,
    /// The way its east-facing pattern is turned.
    pub rotation: Rotation,
    /// Its inputs.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<TileInput>,
    /// The net its output drives, if anything reads it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<usize>,
    /// The cell's `logic` line, or the component's line.
    #[serde(skip)]
    pub span: Span,
}

impl PlacedTile {
    /// The voxel the pattern's `rel` lands on.
    #[must_use]
    pub const fn at(&self, rel: Rel) -> Voxel {
        self.rotation.place(self.origin, rel)
    }

    /// Every part of the tile, turned and placed.
    pub(crate) fn parts(&self, template: &Template) -> Vec<(Voxel, Part)> {
        template
            .parts
            .iter()
            .map(|(rel, part)| (self.at(*rel), part.turned(self.rotation)))
            .collect()
    }
}

/// One input a net feeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct NetSink {
    /// Index into [`CircuitLayout::tiles`].
    pub tile: usize,
    /// The input of that tile.
    pub port: TilePort,
}

/// One repeater a net's strength needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Buffer {
    /// Where it stands.
    pub at: Voxel,
    /// The way it drives.
    #[serde(serialize_with = "serialize_face")]
    pub facing: Face,
}

/// The dust a net runs along, beyond its tiles' own.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct Wire {
    /// Every voxel of dust, a repeater's included.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub dust: Vec<Voxel>,
    /// The blocks laid under dust above the bottom layers.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub supports: Vec<Voxel>,
    /// The voxels a climb needs empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub air: Vec<Voxel>,
    /// The repeaters in place of dust.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub buffers: Vec<Buffer>,
}

/// One signal from the tile that drives it to the inputs it feeds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CircuitNet {
    /// The netlist's net.
    pub source: NetRef,
    /// Index into [`CircuitLayout::tiles`] of the tile that drives it.
    pub driver: usize,
    /// The inputs it feeds.
    pub sinks: Vec<NetSink>,
    /// Its dust, once routed.
    #[serde(skip_serializing_if = "is_unrouted")]
    pub wire: Wire,
}

fn is_unrouted(wire: &Wire) -> bool {
    wire.dust.is_empty()
}

/// One block of the reservation as legalization writes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CircuitBlock {
    /// Where it stands.
    pub at: Voxel,
    /// What it is.
    pub state: BlockState,
    /// A comparator's output strength, which its block entity holds.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_signal: Option<u8>,
}

/// One scope's circuit, from placement to legalization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CircuitLayout {
    /// The edition every tile realises.
    pub edition: Edition,
    /// The built struct the circuit lies under, keyed as the block array keys it.
    pub structure: String,
    /// How far place-and-route has come.
    pub stage: LayoutStage,
    /// The reservation.
    pub region: Reservation,
    /// Every tile: the cells', then the pads and the risers.
    pub tiles: Vec<PlacedTile>,
    /// Every net that has an input to feed.
    pub nets: Vec<CircuitNet>,
    /// Every non-air block, once legalized.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub blocks: Vec<CircuitBlock>,
    /// `sig.NAME` → the net that defines it.
    #[serde(
        skip_serializing_if = "IndexMap::is_empty",
        serialize_with = "crate::logic_ir::serialize_signal_defs"
    )]
    pub signal_defs: IndexMap<DottedRef, NetRef>,
    /// The base plane over the reservation.
    #[serde(skip)]
    pub(crate) surface: Surface,
    /// The clear columns kept around each cell's tile when it was laid.
    #[serde(skip)]
    pub(crate) margin: i32,
}

impl CircuitLayout {
    /// The tile realising the netlist's cell `index`, the cells' tiles laid in the netlist's order.
    #[must_use]
    pub fn cell_tile(&self, index: usize) -> Option<&PlacedTile> {
        self.tiles
            .iter()
            .filter(|tile| matches!(tile.kind, TileKind::Cell { .. }))
            .nth(index)
    }
}

/// Every scope's [`CircuitLayout`], one per built struct its circuit lies under.
pub type ScopedCircuitLayout = Scoped<CircuitLayout>;

/// One entry in a [`ScopedCircuitLayout`].
pub type ScopedCircuitLayoutEntry = ScopedEntry<CircuitLayout>;

impl ScopedCircuitLayout {
    /// Appends the circuit laid under one built struct of a scope.
    pub fn push(&mut self, kind: ScopeKind, name: String, ir: CircuitLayout) {
        self.scopes
            .push(ScopedCircuitLayoutEntry { kind, name, ir });
    }
}
