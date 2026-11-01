//! Placement IR: edition-tagged cells laid out in their scope's `circuit region` reservation,
//! which routing, delay insertion and crossing legalization each fill further.

use std::fmt;

use cairn_lang_core::Edition;
use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::de::{self, IntoDeserializer, MapAccess, Visitor};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::edition_netlist_ir::EditionCell;
use crate::logic_ir::{ScopeKind, Scoped, ScopedEntry, dotted_ref_from_segments};
use crate::netlist_ir::{CellPortDriver, NetRef, NetlistInput, PortName};

mod phase;

pub(crate) use phase::CellIdentity;
use phase::PlacedSubject;
pub use phase::{PlacementPhase, PlacementPhaseTransitionError, PlacementStage};

/// Which pseudo-2.5D layer of `spec/redstone` "Place-and-route" a coordinate lives on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(strum::EnumIter))]
pub enum RouteLayer {
    /// Ground layer: every cell, every pad, and every wire coord the router did not climb to.
    #[default]
    Plane,
    /// Escape layer above the ground: wire the router climbed, and the repeaters on it.
    Bridge,
    /// The vertical `Plane` ↔ `Bridge` tap, which nothing produces: a climb is a step, not a coord.
    Via,
}

impl RouteLayer {
    /// `true` for [`Self::Plane`], whose coords omit `layer` from the JSON.
    #[must_use]
    pub const fn is_plane(&self) -> bool {
        matches!(self, Self::Plane)
    }

    /// The layer's word in the JSON form and in `spec/redstone` "Place-and-route".
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plane => "plane",
            Self::Bridge => "bridge",
            Self::Via => "via",
        }
    }
}

/// Coordinate inside a scope's `circuit region` reservation. Build it with [`Self::new`], which
/// derives the layer from the height, so `Eq` is voxel identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "CellCoordWire")]
pub struct CellCoord {
    /// Column along the region's x-axis. Zero at the region's origin.
    pub x: u32,
    /// Height: `0` on the plane, `>= 1` on a bridge within the `void=<N>` budget.
    pub y: u32,
    /// Row along the region's z-axis.
    pub z: u32,
    /// Pseudo-2.5D layer, derived from `y` by [`Self::new`].
    #[serde(skip_serializing_if = "RouteLayer::is_plane")]
    pub layer: RouteLayer,
}

/// [`CellCoord`]'s wire form as read, before [`CellCoord::new`] judges the `layer`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CellCoordWire {
    x: u32,
    y: u32,
    z: u32,
    #[serde(default)]
    layer: Option<RouteLayer>,
}

impl TryFrom<CellCoordWire> for CellCoord {
    type Error = String;

    fn try_from(wire: CellCoordWire) -> Result<Self, Self::Error> {
        let coord = Self::new(wire.x, wire.y, wire.z);
        match wire.layer {
            Some(layer) if layer != coord.layer => Err(format!(
                "a coord at `y: {y}` is on the `{want}` layer, not `{got}`",
                y = wire.y,
                want = coord.layer.as_str(),
                got = layer.as_str(),
            )),
            _ => Ok(coord),
        }
    }
}

impl CellCoord {
    /// The coord at `(x, y, z)`: [`RouteLayer::Plane`] at `y == 0`, [`RouteLayer::Bridge`] above.
    #[must_use]
    pub const fn new(x: u32, y: u32, z: u32) -> Self {
        let layer = if y == 0 {
            RouteLayer::Plane
        } else {
            RouteLayer::Bridge
        };
        Self { x, y, z, layer }
    }

    /// Coord with an explicit [`RouteLayer`], so a test need not assert with [`Self::new`]'s rule.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn with_layer(x: u32, y: u32, z: u32, layer: RouteLayer) -> Self {
        Self { x, y, z, layer }
    }
}

/// One implicit buffer repeater, tagged with the segment it serves. An attribution list, not a
/// block list: a repeater serving several segments appears once per segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BufferCoord {
    /// Which segment this buffer stands on.
    pub port: BufferSegment,
    /// Coordinate of the buffer, on the layer of the wire it stands on.
    pub coord: CellCoord,
}

impl BufferCoord {
    /// A `(segment, coord)` attribution pair.
    #[must_use]
    pub const fn new(port: BufferSegment, coord: CellCoord) -> Self {
        Self { port, coord }
    }
}

/// Which wire a buffer repeater stands on: a driver port, or `out` for the wire to an actuator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BufferSegment {
    /// A driver port feeding the owning cell.
    Port(PortName),
    /// The wire from a driver to an actuator's pad.
    Out,
}

// Hand-written so both cases serialise as one flat word.
impl Serialize for BufferSegment {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Port(port) => port.serialize(serializer),
            Self::Out => serializer.serialize_str("out"),
        }
    }
}

impl BufferSegment {
    /// Every segment word, for the error an unknown word earns.
    const WORDS: &'static [&'static str] = &["a", "b", "sel", "out"];

    /// The segment's word in the JSON form, one of [`Self::WORDS`].
    const fn as_str(self) -> &'static str {
        match self {
            Self::Port(port) => port.as_str(),
            Self::Out => "out",
        }
    }
}

/// Reads back the flat word [`Serialize`] writes.
impl<'de> Deserialize<'de> for BufferSegment {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let word = String::deserialize(deserializer)?;
        if word == "out" {
            return Ok(Self::Out);
        }
        PortName::deserialize(IntoDeserializer::<de::value::Error>::into_deserializer(
            word.as_str(),
        ))
        .map(Self::Port)
        .map_err(|_| de::Error::unknown_variant(&word, Self::WORDS))
    }
}

/// The `circuit region=<label> void=<N>` reservation a scope declared, with the footprint it sits
/// in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct CircuitRegionReservation {
    /// `region=<label>` value the circuit member declared.
    pub label: String,
    /// `void=<N>` service-layer height (`>= 1`).
    pub void: u32,
    /// Width of the enclosing scope's footprint, copied from `size=WxH`.
    pub width: u32,
    /// Depth of the enclosing scope's footprint, copied from `size=WxH`.
    pub depth: u32,
    /// Byte range of the originating `circuit region=...` line.
    #[serde(skip)]
    pub span: Span,
}

impl CircuitRegionReservation {
    /// Total blocks reserved for routing: `width * depth * void`, saturating, since three `u32`s
    /// can overflow a `u64`.
    #[must_use]
    pub const fn reserved_area(&self) -> u64 {
        (self.width as u64)
            .saturating_mul(self.depth as u64)
            .saturating_mul(self.void as u64)
    }
}

/// One placed cell: an [`EditionCell`] at its [`CellCoord`]. It serialises flat, and reads back
/// only from a self-describing format, its phase picked by the `stage` tag.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlacedCellNode {
    /// Edition-specific realisation, from the source cell.
    pub cell: EditionCell,
    /// Port → net drivers, in the Netlist IR's port order.
    pub drivers: Vec<CellPortDriver>,
    /// Coordinate assigned by the placement pass.
    pub coord: CellCoord,
    /// Pipeline state, which the passes advance only through the [`PlacementPhase`] transitions.
    pub(crate) phase: PlacementPhase,
    /// Byte range inherited from the source [`crate::edition_netlist_ir::EditionCellNode`].
    pub span: Span,
}

/// The read-only projections of a placed node's [`PlacementPhase`].
macro_rules! phase_projections {
    ($subject:literal, $wire_length:literal) => {
        #[doc = $wire_length]
        /// See [`PlacementPhase::wire_length`].
        #[must_use]
        pub const fn wire_length(&self) -> Option<u32> {
            self.phase.wire_length()
        }

        /// Local delay ticks once delay insertion has run. See
        /// [`PlacementPhase::local_delay_ticks`].
        #[must_use]
        pub const fn local_delay_ticks(&self) -> Option<u32> {
            self.phase.local_delay_ticks()
        }

        /// Buffer coordinates once crossing legalization has run. See
        /// [`PlacementPhase::buffer_coords`].
        #[must_use]
        pub fn buffer_coords(&self) -> &[BufferCoord] {
            self.phase.buffer_coords()
        }

        #[doc = concat!("Which pass last wrote to this ", $subject, ". See")]
        /// [`PlacementPhase::stage`].
        #[must_use]
        pub const fn stage(&self) -> PlacementStage {
            self.phase.stage()
        }
    };
}

impl PlacedCellNode {
    phase_projections!("cell", "Wire length once routing has run.");
}

/// Writes the flat form both placed node types share: `stage`, `identity`'s fields, then the
/// phase's filled fields.
fn serialize_phased<S: Serializer>(
    serializer: S,
    name: &'static str,
    phase: &PlacementPhase,
    identity_fields: usize,
    identity: impl FnOnce(&mut CountedFields<S::SerializeStruct>) -> Result<(), S::Error>,
) -> Result<S::Ok, S::Error> {
    let wire_length = phase.wire_length();
    let local_delay_ticks = phase.local_delay_ticks();
    let buffer_coords = phase.buffer_coords();
    let field_count = 1
        + identity_fields
        + usize::from(wire_length.is_some())
        + usize::from(local_delay_ticks.is_some())
        + usize::from(!buffer_coords.is_empty());

    let mut state = CountedFields {
        inner: serializer.serialize_struct(name, field_count)?,
        written: 0,
    };
    state.serialize_field("stage", &phase.stage())?;
    identity(&mut state)?;
    if let Some(wl) = wire_length {
        state.serialize_field("wire_length", &wl)?;
    }
    if let Some(dt) = local_delay_ticks {
        state.serialize_field("local_delay_ticks", &dt)?;
    }
    if !buffer_coords.is_empty() {
        state.serialize_field("buffer_coords", buffer_coords)?;
    }
    debug_assert_eq!(
        state.written, field_count,
        "{name}: announced field_count ({field_count}) diverges from the serialize_field \
         call count ({}); binary formats such as bincode / postcard would produce \
         malformed output",
        state.written,
    );
    state.end()
}

/// A [`SerializeStruct`] that counts its writes: binary formats need the announced count exact.
struct CountedFields<T> {
    inner: T,
    written: usize,
}

impl<T: SerializeStruct> SerializeStruct for CountedFields<T> {
    type Ok = T::Ok;
    type Error = T::Error;

    fn serialize_field<V: ?Sized + Serialize>(
        &mut self,
        key: &'static str,
        value: &V,
    ) -> Result<(), T::Error> {
        self.written += 1;
        self.inner.serialize_field(key, value)
    }

    fn end(self) -> Result<T::Ok, T::Error> {
        self.inner.end()
    }
}

impl Serialize for PlacedCellNode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_phased(serializer, "PlacedCellNode", &self.phase, 3, |state| {
            state.serialize_field("cell", &self.cell)?;
            state.serialize_field("drivers", &self.drivers)?;
            state.serialize_field("coord", &self.coord)
        })
    }
}

/// The keys of [`PlacedCellNode`]'s flat wire form, for an unknown key's error.
const PLACED_CELL_FIELDS: &[&str] = &[
    "stage",
    "cell",
    "drivers",
    "coord",
    "wire_length",
    "local_delay_ticks",
    "buffer_coords",
];

/// Stores the map's next value in `slot`, refusing a `key` the map has already given.
fn read_once<'de, T: Deserialize<'de>, A: MapAccess<'de>>(
    slot: &mut Option<T>,
    key: &'static str,
    map: &mut A,
) -> Result<(), A::Error> {
    if slot.is_some() {
        return Err(de::Error::duplicate_field(key));
    }
    *slot = Some(map.next_value()?);
    Ok(())
}

/// The phase keys as read off the map, judged against `stage` only once all are in.
#[derive(Default)]
struct PhaseFields {
    stage: Option<PlacementStage>,
    wire_length: Option<u32>,
    local_delay_ticks: Option<u32>,
    buffer_coords: Option<Vec<BufferCoord>>,
}

impl PhaseFields {
    /// Stores `key`'s value if it is a phase key, refusing a repeat; `false` for any other key.
    fn read<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<bool, A::Error> {
        match key {
            "stage" => read_once(&mut self.stage, "stage", map)?,
            "wire_length" => read_once(&mut self.wire_length, "wire_length", map)?,
            "local_delay_ticks" => {
                read_once(&mut self.local_delay_ticks, "local_delay_ticks", map)?;
            }
            "buffer_coords" => read_once(&mut self.buffer_coords, "buffer_coords", map)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// The [`PlacementPhase`] the `stage` tag names, holding the payload keys to what it carries.
    fn into_phase<E: de::Error>(self, subject: PlacedSubject) -> Result<PlacementPhase, E> {
        use PlacementStage::{Crossing, Delay, Placement, Route};

        let stage = self.stage.ok_or_else(|| E::missing_field("stage"))?;
        let node = subject.with_article();
        // A payload key the tagged stage has not reached yet.
        let early = |key: &str, writer: PlacementStage, present: bool| {
            if present {
                Err(E::custom(format_args!(
                    "`{key}` is written by the {writer} stage, which {node} tagged \
                     `stage: \"{stage}\"` has not reached",
                    writer = writer.as_str(),
                    stage = stage.as_str(),
                )))
            } else {
                Ok(())
            }
        };
        // A payload key the tagged stage has been through and must carry.
        let due = |key: &str, writer: PlacementStage, value: Option<u32>| {
            value.ok_or_else(|| {
                E::custom(format_args!(
                    "{node} tagged `stage: \"{stage}\"` has been through the {writer} \
                     stage and must carry `{key}`",
                    writer = writer.as_str(),
                    stage = stage.as_str(),
                ))
            })
        };
        let wire_length = |v| due("wire_length", Route, v);
        let local_delay_ticks = |v| due("local_delay_ticks", Delay, v);
        let buffer_coords = |present| early("buffer_coords", Crossing, present);

        Ok(match stage {
            Placement => {
                early("wire_length", Route, self.wire_length.is_some())?;
                early("local_delay_ticks", Delay, self.local_delay_ticks.is_some())?;
                buffer_coords(self.buffer_coords.is_some())?;
                PlacementPhase::Unrouted
            }
            Route => {
                early("local_delay_ticks", Delay, self.local_delay_ticks.is_some())?;
                buffer_coords(self.buffer_coords.is_some())?;
                PlacementPhase::Routed {
                    wire_length: wire_length(self.wire_length)?,
                }
            }
            Delay => {
                buffer_coords(self.buffer_coords.is_some())?;
                PlacementPhase::Delayed {
                    wire_length: wire_length(self.wire_length)?,
                    local_delay_ticks: local_delay_ticks(self.local_delay_ticks)?,
                }
            }
            Crossing => PlacementPhase::Legalized {
                wire_length: wire_length(self.wire_length)?,
                local_delay_ticks: local_delay_ticks(self.local_delay_ticks)?,
                buffer_coords: self.buffer_coords.unwrap_or_default(),
            },
        })
    }
}

/// Refuses a buffer on a segment none of the cell's drivers feeds.
fn check_buffers_stand_on_drivers<E: de::Error>(
    phase: &PlacementPhase,
    drivers: &[CellPortDriver],
) -> Result<(), E> {
    for (i, buffer) in phase.buffer_coords().iter().enumerate() {
        match buffer.port {
            BufferSegment::Port(port) if drivers.iter().any(|d| d.port == port) => {}
            BufferSegment::Port(_) => {
                return Err(E::custom(format_args!(
                    "`buffer_coords[{i}]` stands on a port no entry of this cell's `drivers` feeds"
                )));
            }
            BufferSegment::Out => {
                return Err(E::custom(format_args!(
                    "`buffer_coords[{i}]` stands on the `out` segment, which runs to a placed \
                     output and is never a cell's"
                )));
            }
        }
    }
    Ok(())
}

impl<'de> Deserialize<'de> for PlacedCellNode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CellVisitor;

        impl<'de> Visitor<'de> for CellVisitor {
            type Value = PlacedCellNode;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a placed cell in the flat Placement IR wire form")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<PlacedCellNode, A::Error> {
                let mut phase = PhaseFields::default();
                let mut cell: Option<EditionCell> = None;
                let mut drivers: Option<Vec<CellPortDriver>> = None;
                let mut coord: Option<CellCoord> = None;
                while let Some(key) = map.next_key::<String>()? {
                    if phase.read(&key, &mut map)? {
                        continue;
                    }
                    match key.as_str() {
                        "cell" => read_once(&mut cell, "cell", &mut map)?,
                        "drivers" => read_once(&mut drivers, "drivers", &mut map)?,
                        "coord" => read_once(&mut coord, "coord", &mut map)?,
                        other => return Err(de::Error::unknown_field(other, PLACED_CELL_FIELDS)),
                    }
                }
                let drivers = drivers.ok_or_else(|| de::Error::missing_field("drivers"))?;
                let phase = phase.into_phase(PlacedSubject::Cell)?;
                check_buffers_stand_on_drivers(&phase, &drivers)?;
                Ok(PlacedCellNode {
                    cell: cell.ok_or_else(|| de::Error::missing_field("cell"))?,
                    drivers,
                    coord: coord.ok_or_else(|| de::Error::missing_field("coord"))?,
                    phase,
                    span: Span::default(),
                })
            }
        }

        deserializer.deserialize_struct("PlacedCellNode", PLACED_CELL_FIELDS, CellVisitor)
    }
}

/// An actuator-driven net sink placed at its output pad, dumped flat with the same `stage` and
/// phase keys as a [`PlacedCellNode`]; its `local_delay_ticks` has no port delay.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlacedOutputNode {
    /// Dotted signal reference the actuator consumes.
    pub name: DottedRef,
    /// The net driving this port, from [`crate::netlist_ir::NetlistOutput::driver`].
    pub driver: NetRef,
    /// Coordinate of this actuator's pad.
    pub pad: CellCoord,
    /// Pipeline state, which the passes advance only through the [`PlacementPhase`] transitions.
    pub(crate) phase: PlacementPhase,
    /// Byte range of the originating `opened_by=` argument.
    pub span: Span,
}

impl PlacedOutputNode {
    /// A placed actuator pad, before routing.
    #[must_use]
    pub(crate) fn new(name: DottedRef, driver: NetRef, pad: CellCoord, span: Span) -> Self {
        Self {
            name,
            driver,
            pad,
            phase: PlacementPhase::Unrouted,
            span,
        }
    }

    phase_projections!(
        "output",
        "Routed length of the segment from this output's driver to its pad, once routing has run."
    );
}

impl Serialize for PlacedOutputNode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serialize_phased(serializer, "PlacedOutputNode", &self.phase, 3, |state| {
            state.serialize_field("name", &self.name)?;
            state.serialize_field("driver", &self.driver)?;
            state.serialize_field("pad", &self.pad)
        })
    }
}

/// The keys of [`PlacedOutputNode`]'s flat wire form, for an unknown key's error.
const PLACED_OUTPUT_FIELDS: &[&str] = &[
    "stage",
    "name",
    "driver",
    "pad",
    "wire_length",
    "local_delay_ticks",
    "buffer_coords",
];

/// Refuses a buffer on any segment but `out`, the one wire an output's buffers stand on.
fn check_buffers_stand_on_out<E: de::Error>(phase: &PlacementPhase) -> Result<(), E> {
    for (i, buffer) in phase.buffer_coords().iter().enumerate() {
        match buffer.port {
            BufferSegment::Out => {}
            BufferSegment::Port(_) => {
                return Err(E::custom(format_args!(
                    "`buffer_coords[{i}]` stands on `{word}`, a cell's driver port, and an \
                     output's buffers stand on the `out` segment",
                    word = buffer.port.as_str(),
                )));
            }
        }
    }
    Ok(())
}

impl<'de> Deserialize<'de> for PlacedOutputNode {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct OutputVisitor;

        impl<'de> Visitor<'de> for OutputVisitor {
            type Value = PlacedOutputNode;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a placed output in the flat Placement IR wire form")
            }

            fn visit_map<A: MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<PlacedOutputNode, A::Error> {
                let mut phase = PhaseFields::default();
                let mut name: Option<Vec<String>> = None;
                let mut driver: Option<NetRef> = None;
                let mut pad: Option<CellCoord> = None;
                while let Some(key) = map.next_key::<String>()? {
                    if phase.read(&key, &mut map)? {
                        continue;
                    }
                    match key.as_str() {
                        "name" => read_once(&mut name, "name", &mut map)?,
                        "driver" => read_once(&mut driver, "driver", &mut map)?,
                        "pad" => read_once(&mut pad, "pad", &mut map)?,
                        other => return Err(de::Error::unknown_field(other, PLACED_OUTPUT_FIELDS)),
                    }
                }
                let name = name.ok_or_else(|| de::Error::missing_field("name"))?;
                let phase = phase.into_phase(PlacedSubject::Output)?;
                check_buffers_stand_on_out(&phase)?;
                Ok(PlacedOutputNode {
                    name: dotted_ref_from_segments(name)?,
                    driver: driver.ok_or_else(|| de::Error::missing_field("driver"))?,
                    pad: pad.ok_or_else(|| de::Error::missing_field("pad"))?,
                    phase,
                    span: Span::default(),
                })
            }
        }

        deserializer.deserialize_struct("PlacedOutputNode", PLACED_OUTPUT_FIELDS, OutputVisitor)
    }
}

/// The Placement IR for one struct/def body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[non_exhaustive]
pub struct PlacementIr {
    /// Edition this IR was compiled for, matched by the [`EditionCell::edition`] of every cell the
    /// placement pass places.
    pub edition: Edition,
    /// Reservation the cells and pads were placed into, which the placement pass leaves absent
    /// only on an empty IR.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<CircuitRegionReservation>,
    /// Sensor-driven nets, copied from the source Edition Netlist IR.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<NetlistInput>,
    /// Actuator-driven nets, each placed at its output pad.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<PlacedOutputNode>,
    /// Placed cells, in the source's topological order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<PlacedCellNode>,
    /// `sig.NAME` → the net that defines it.
    #[serde(
        default,
        skip_serializing_if = "IndexMap::is_empty",
        serialize_with = "crate::logic_ir::serialize_signal_defs",
        deserialize_with = "crate::logic_ir::deserialize_signal_defs"
    )]
    pub signal_defs: IndexMap<DottedRef, NetRef>,
}

impl PlacementIr {
    /// An empty Placement IR for `edition`.
    #[must_use]
    pub fn new(edition: Edition) -> Self {
        Self {
            edition,
            region: None,
            inputs: Vec::new(),
            outputs: Vec::new(),
            cells: Vec::new(),
            signal_defs: IndexMap::new(),
        }
    }

    /// `true` when the IR has no inputs, outputs or cells.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty() && self.outputs.is_empty() && self.cells.is_empty()
    }
}

/// Every scope's [`PlacementIr`], in source order; its `cairn synth --stage <s>` dump reads back
/// equal to it but for its spans.
pub type ScopedPlacementIr = Scoped<PlacementIr>;

/// One entry in a [`ScopedPlacementIr`].
pub type ScopedPlacementIrEntry = ScopedEntry<PlacementIr>;

impl ScopedPlacementIr {
    /// Appends one scope's Placement IR, skipping an empty one.
    pub fn push(&mut self, kind: ScopeKind, name: String, ir: PlacementIr) {
        if ir.is_empty() {
            debug_assert!(
                ir.signal_defs.is_empty(),
                "empty PlacementIr carries orphan signal_defs entries: {:?}",
                ir.signal_defs.keys().collect::<Vec<_>>(),
            );
            debug_assert!(
                ir.region.is_none(),
                "empty PlacementIr carries an orphan region: {:?}",
                ir.region,
            );
            return;
        }
        self.scopes.push(ScopedPlacementIrEntry { kind, name, ir });
    }
}

#[cfg(test)]
mod tests;
