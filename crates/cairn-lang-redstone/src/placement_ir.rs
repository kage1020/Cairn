//! Placement IR: edition-tagged cells laid out inside their scope's `circuit region` reservation.
//!
//! The placement pass ([`crate::placement::compile_placement`]) looks up each scope's
//! [`cairn_lang_core::CircuitRegion`] and gives every
//! [`crate::edition_netlist_ir::EditionCellNode`] a [`CellCoord`] inside it, the first of the five
//! stages of `spec/redstone` "Place-and-route" (placement, Steiner routing, delay insertion,
//! crossing legalization, edition legalization). The next three passes fill in the same IR:
//! routing writes [`PlacedCellNode::wire_length`], delay insertion
//! [`PlacedCellNode::local_delay_ticks`] (per `spec/redstone` "Time model", delay is first
//! determined here, once wire length is known), and crossing legalization
//! [`PlacedCellNode::buffer_coords`]. They are optional fields rather than new ones, so a JSON
//! consumer sees one shape.
//!
//! Every cell names the last pass to touch it with a [`PlacementStage`] tag, by which
//! [`PlacedCellNode`]'s [`Deserialize`] picks the phase and refuses a cell whose keys disagree
//! with it.
//!
//! Placement refuses a missing reservation (`E_NO_CIRCUIT_REGION`) and an over-full one
//! (`E_ROUTE_CONGESTION`). `E_ATTENUATION_LIMIT` is checked against
//! [`crate::delay::MAX_ATTENUATION_SEGMENT`] twice: on the straight line from a driver to its sink
//! before routing to it, in every place-and-route pass, and on the routed segment in
//! [`crate::delay::compile_delay`].

use std::fmt;

use cairn_lang_core::Edition;
use cairn_lang_core::ast::DottedRef;
use cairn_lang_core::error::Span;
use indexmap::IndexMap;
use serde::de::{self, IntoDeserializer, MapAccess, Visitor};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::edition_netlist_ir::EditionCell;
use crate::logic_ir::{ScopeKind, Scoped, ScopedEntry};
use crate::netlist_ir::{CellPortDriver, NetRef, NetlistInput, PortName};

mod phase;

pub(crate) use phase::CellIdentity;
pub use phase::{PlacementPhase, PlacementPhaseTransitionError, PlacementStage};

/// Which of the three pseudo-2.5D layers of `spec/redstone` "Place-and-route" a coordinate lives
/// on: `Plane` the ground layer, `Bridge` the escape layer above it, `Via` the vertical tap
/// between them.
///
/// A cell is always on `Plane`. Everything else takes its layer from its height through
/// [`CellCoord::new`], so two things at one voxel are one map key. Serialised as the words
/// [`Self::as_str`] spells.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(strum::EnumIter))]
pub enum RouteLayer {
    /// Ground layer: every cell, every pad, and every wire coord the router did not climb to. The
    /// default, so a forgotten layer lands on the ground rather than an escape layer.
    #[default]
    Plane,
    /// Escape layer above the ground: wire the router climbed to get past a block or past the dust
    /// of an earlier net, and the buffer repeaters standing on it.
    Bridge,
    /// The vertical `Plane` ↔ `Bridge` tap, which nothing produces: a climb is a step between two
    /// coords rather than a coord of its own. Kept so the enum names the whole vocabulary of
    /// `spec/redstone` "Place-and-route".
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

/// Coordinate inside a scope's `circuit region` reservation.
///
/// Placement puts the cell with topological index `i` at `x = 1 + 2i`, `y = 0`, `z = 1`. Pads sit
/// on the plane at the reservation edges: in a scope with cells at `z = i` below the cell row and
/// `z = i + 1` from it on, saturating at `depth - 1`, and in a scope without at `z = i`. The
/// saturation can put a pad on the cell row only in a region placement refuses. Wire and buffer
/// coords may have `y >= 1`, and are then on [`RouteLayer::Bridge`].
///
/// `layer` takes part in `Eq` and `Hash`, but [`Self::new`] derives it from the height, so for
/// every coord a pass builds `Eq` is voxel identity. The fields are public, so a struct literal
/// can still spell a coord `new` would not; the geometry module puts everything it holds through
/// `new` before comparing.
///
/// `layer` is serialised only off [`RouteLayer::Plane`]. Reading a coord back goes through
/// [`Self::new`] too: an absent `layer` is the one the height gives, an explicit one must agree
/// with it (which refuses every explicit `via`), and an unknown key is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "CellCoordWire")]
pub struct CellCoord {
    /// Column along the region's x-axis. Zero at the region's origin.
    pub x: u32,
    /// Height along the region's y-axis: `0` on the plane, `>= 1` within the `void=<N>` budget for
    /// wire lifted onto a bridge and the buffers standing on it.
    pub y: u32,
    /// Row along the region's z-axis. Cells sit at `1`, one row in, so every cell has a clear lane
    /// on each side.
    pub z: u32,
    /// Pseudo-2.5D layer, derived from `y` by [`Self::new`].
    #[serde(skip_serializing_if = "RouteLayer::is_plane")]
    pub layer: RouteLayer,
}

/// [`CellCoord`]'s wire form as read, before [`CellCoord::new`] judges
/// the `layer` against the height.
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
    ///
    /// The only constructor, and the one a read-back coord goes through, so the layer is a
    /// function of the height. One rule rather than one per producer, because `(x, 1, z, Plane)`
    /// and `(x, 1, z, Bridge)` are distinct map keys for one voxel, and two rules would put two
    /// things in one place.
    #[must_use]
    pub const fn new(x: u32, y: u32, z: u32) -> Self {
        let layer = if y == 0 {
            RouteLayer::Plane
        } else {
            RouteLayer::Bridge
        };
        Self { x, y, z, layer }
    }

    /// Coord with an explicit [`RouteLayer`], for tests: an assertion written with
    /// [`Self::new`]'s rule would agree with a broken rule.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn with_layer(x: u32, y: u32, z: u32, layer: RouteLayer) -> Self {
        Self { x, y, z, layer }
    }
}

/// One implicit buffer repeater the crossing-legalization pass placed, tagged with the segment it
/// serves.
///
/// **An attribution list, not a block list.** There is one entry per segment per repeater that
/// segment's signal passes through, so [`Self::coord`] repeats when one repeater serves several
/// segments: two ports of a cell reading one signal, two cells hanging off one point of the routed
/// tree, a cell and an actuator on one net. **To count blocks, deduplicate by `coord`**; to get a
/// segment's repeaters, filter by `port`. The delay pass charges ticks per driving net for the
/// same reason, so `local_delay_ticks - base_delay_ticks` is `BUFFER_REPEATER_TICKS` per distinct
/// block, not per entry.
///
/// **Order.** In [`PlacedCellNode::buffer_coords`], entries follow [`PlacedCellNode::drivers`],
/// and within one driver the order the routed path visits them, source first. That is not an
/// x → z → y sweep: the route hangs off the trunk laid for a nearer sink and turns wherever it
/// goes round a block.
///
/// Serialises as `{"port": "<a|b|sel|out>", "coord": {...}}`, the `{port, ...}` shape of
/// [`CellPortDriver`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BufferCoord {
    /// Which segment this buffer stands on: a driver port of the owning
    /// cell, or the wire out to an actuator pad.
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

/// Which wire a buffer repeater stands on, written `"a"`, `"b"` or `"sel"` for a driver port and
/// `"out"` for the wire to an actuator.
///
/// Separate from [`PortName`] rather than a variant of it, because a [`CellPortDriver`] must not
/// be able to say `out`: a cell has no such input. `#[non_exhaustive]` like [`PortName`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BufferSegment {
    /// A driver port feeding the owning cell.
    Port(PortName),
    /// The wire from a driver to an actuator's pad, attributed to the [`PlacedOutputNode`] whose
    /// pad it runs to.
    Out,
}

// Hand-written so both cases are one flat word, where a derive would tag `Port(..)`.
impl Serialize for BufferSegment {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Port(port) => port.serialize(serializer),
            Self::Out => serializer.serialize_str("out"),
        }
    }
}

impl BufferSegment {
    /// Every segment word, for the error an unknown word earns. Only the error reads it: what is
    /// accepted is [`PortName`]'s own set plus `out`, and a test holds this list to it.
    const WORDS: &'static [&'static str] = &["a", "b", "sel", "out"];
}

/// Reads back the flat word [`Serialize`] writes: `out`, or any word [`PortName`] accepts. An
/// unknown word is refused naming every segment word, not only the ports.
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
///
/// Copied onto every non-empty [`PlacementIr`], so a dump describes itself and routing reads the
/// budget from it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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
    /// Total blocks reserved for routing: `width * depth * void`, saturating.
    ///
    /// `u64` holds the product of two `u32`s but not of three, and a `size=` from a `.crn` can
    /// overflow it. A wrapped product is either a plausible wrong reservation
    /// (`size=4294967295x4294967295 void=3`) or zero (`size=2147483648x2147483648 void=4`), which
    /// the congestion check divides by. Saturating suits every caller, which compares this with a
    /// budget or divides by it: a reservation that large constrains nothing, and such a scope's
    /// refusal comes from the attenuation cap.
    #[must_use]
    pub const fn reserved_area(&self) -> u64 {
        (self.width as u64)
            .saturating_mul(self.depth as u64)
            .saturating_mul(self.void as u64)
    }
}

/// One placed cell: an [`EditionCell`] at its [`CellCoord`] inside the reservation.
///
/// What routing, delay insertion and crossing legalization add (`wire_length`,
/// `local_delay_ticks`, `buffer_coords`) lives in a crate-private [`PlacementPhase`] field, read
/// through [`Self::wire_length`], [`Self::local_delay_ticks`], [`Self::buffer_coords`] and
/// [`Self::stage`].
///
/// [`Serialize`] flattens the phase into
/// `{stage, cell, drivers, coord[, wire_length][, local_delay_ticks][, buffer_coords]}`, so one
/// stage's dump is a subset of the next one's apart from the `stage` value (see
/// [`PlacementStage`] for why the tag exists).
///
/// [`Deserialize`] reads the flat form back, picking the phase by `stage`, never by which keys are
/// present. It refuses a payload key the named stage has not written (`buffer_coords` under
/// `"route"`) or one it must have written (`local_delay_ticks` under `"delay"`); `buffer_coords`
/// alone may be absent at its own stage, meaning no buffers. It also refuses a missing `stage`, an
/// unknown `stage` or `layer` word, a `layer` that disagrees with the height (see [`CellCoord`]),
/// a buffer on a segment the cell has no driver for, and an unknown key at any depth. Every
/// refusal is a deserializer error, not a panic.
///
/// The reader is the only way another crate can build a `PlacedCellNode`. It builds a cell as its
/// stage's pass left it, so hand it to the pass after that stage; the phase transitions still
/// panic on a phase they do not expect.
///
/// [`Self::span`] is not serialised, so a cell read back has `0..0`, and `PartialEq` compares it:
/// a round trip is equal once the original's span is set to `0..0`.
///
/// Read-back needs a self-describing format such as JSON: the flat form has four to seven keys
/// depending on the phase, so a positional format (bincode, postcard) is ambiguous, and a non-map
/// input is refused. A repeated key is refused when the deserializer hands over the map as written
/// (`serde_json::from_str`, `from_slice`, `from_reader`), but not through a `serde_json::Value`,
/// which has already kept the last duplicate.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlacedCellNode {
    /// Edition-specific realisation, from the source
    /// [`crate::edition_netlist_ir::EditionCellNode`].
    pub cell: EditionCell,
    /// Port → net drivers, in the Netlist IR's port order.
    pub drivers: Vec<CellPortDriver>,
    /// Coordinate assigned by the placement pass.
    pub coord: CellCoord,
    /// Pipeline state, written only through [`PlacementPhase::route`], [`PlacementPhase::delay`]
    /// and [`PlacementPhase::legalize`], whose guards keep the illegal states out.
    pub(crate) phase: PlacementPhase,
    /// Byte range inherited from the source [`crate::edition_netlist_ir::EditionCellNode`].
    pub span: Span,
}

/// The read-only projections of a placed node's [`PlacementPhase`],
/// spelled once for the two node types that carry one.
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

/// Writes the flat form both placed node types share: `stage` first, so a truncated dump still
/// names its pass, then the node's `identity_fields` (written by `identity`), then the phase's
/// fields as far as they are filled.
///
/// Hand-written because a derive would tag the phase enum. Binary formats need the announced field
/// count to be exact, so every write goes through a [`CountedFields`] and the count is checked
/// before `end`. A new visible field on either node type, or a stage whose payload the JSON must
/// show, is added here along with a [`PlacementStage`] variant.
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

/// A [`SerializeStruct`] that counts the fields written through it, so
/// [`serialize_phased`] can hold the announced field count to the writes.
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

/// The keys of [`PlacedCellNode`]'s flat wire form, in the order
/// [`serialize_phased`] writes them — what an unknown key's error lists.
const PLACED_CELL_FIELDS: &[&str] = &[
    "stage",
    "cell",
    "drivers",
    "coord",
    "wire_length",
    "local_delay_ticks",
    "buffer_coords",
];

/// The phase half of a placed node's flat wire form, as read off the
/// map before anything is checked — so the keys may arrive in any order
/// and are judged against the `stage` tag only once all of them are in.
#[derive(Default)]
struct PhaseFields {
    stage: Option<PlacementStage>,
    wire_length: Option<u32>,
    local_delay_ticks: Option<u32>,
    buffer_coords: Option<Vec<BufferCoord>>,
}

impl PhaseFields {
    /// Store `key`'s value if it is one of the four phase keys,
    /// refusing a second occurrence of it. Returns `false` for any
    /// other key, which the caller reads itself.
    fn read<'de, A: MapAccess<'de>>(&mut self, key: &str, map: &mut A) -> Result<bool, A::Error> {
        fn once<'de, T: Deserialize<'de>, A: MapAccess<'de>>(
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
        match key {
            "stage" => once(&mut self.stage, "stage", map)?,
            "wire_length" => once(&mut self.wire_length, "wire_length", map)?,
            "local_delay_ticks" => once(&mut self.local_delay_ticks, "local_delay_ticks", map)?,
            "buffer_coords" => once(&mut self.buffer_coords, "buffer_coords", map)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    /// The [`PlacementPhase`] the `stage` tag names, holding the
    /// payload keys to exactly what that stage carries.
    ///
    /// Each payload key belongs to the pass that writes it: a stage
    /// before that pass must not carry it, and the pass's own stage
    /// and every later one must — except `buffer_coords`, which
    /// [`serialize_phased`] leaves out when it is empty, so its absence
    /// at the crossing stage reads back as no buffers.
    fn into_phase<E: de::Error>(self) -> Result<PlacementPhase, E> {
        use PlacementStage::{Crossing, Delay, Placement, Route};

        let stage = self.stage.ok_or_else(|| E::missing_field("stage"))?;
        // A payload key the tagged stage has not reached yet.
        let early = |key: &str, writer: PlacementStage, present: bool| {
            if present {
                Err(E::custom(format_args!(
                    "`{key}` is written by the {writer} stage, which a cell tagged \
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
                    "a cell tagged `stage: \"{stage}\"` has been through the {writer} \
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

/// Refuses a buffer a cell could not carry: every [`BufferCoord`] on a cell stands on a
/// [`BufferSegment::Port`] one of its drivers feeds. The crossing pass debug-asserts this where it
/// writes them; it is checked here in every build so a dump cannot bring in what the pass never
/// emits.
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
                        "cell" if cell.is_some() => return Err(de::Error::duplicate_field("cell")),
                        "cell" => cell = Some(map.next_value()?),
                        "drivers" if drivers.is_some() => {
                            return Err(de::Error::duplicate_field("drivers"));
                        }
                        "drivers" => drivers = Some(map.next_value()?),
                        "coord" if coord.is_some() => {
                            return Err(de::Error::duplicate_field("coord"));
                        }
                        "coord" => coord = Some(map.next_value()?),
                        other => return Err(de::Error::unknown_field(other, PLACED_CELL_FIELDS)),
                    }
                }
                let drivers = drivers.ok_or_else(|| de::Error::missing_field("drivers"))?;
                let phase = phase.into_phase()?;
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

/// An actuator-driven net sink, placed at its output pad.
///
/// The placement counterpart of [`crate::netlist_ir::NetlistOutput`]. It exists because the wire
/// from a driver to an actuator is wire like any other: it attenuates, needs buffer repeaters
/// past [`crate::DUST_ATTENUATION_LIMIT`], and they stand inside the reservation, so a driver far
/// from its actuator costs more delay than one beside it.
///
/// It carries the same [`PlacementPhase`] as a cell, filled by the same passes. Its
/// `local_delay_ticks` is the buffers on the segment alone, with no base delay, since a pad is not
/// a cell.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlacedOutputNode {
    /// Dotted signal reference the actuator consumes, from
    /// [`crate::netlist_ir::NetlistOutput::name`].
    pub name: DottedRef,
    /// The net driving this port, from [`crate::netlist_ir::NetlistOutput::driver`].
    pub driver: NetRef,
    /// Coordinate the placement pass assigned this actuator's pad, per the `x = width - 1`
    /// convention the routing geometry uses.
    pub pad: CellCoord,
    /// Pipeline state, written only through the transition methods, as with
    /// [`PlacedCellNode::phase`].
    pub(crate) phase: PlacementPhase,
    /// Byte range of the originating `opened_by=` argument.
    pub span: Span,
}

impl PlacedOutputNode {
    /// A placed actuator pad, before routing.
    ///
    /// Crate-visible because the pad is placement's to assign from the reservation: a coordinate
    /// from outside is checked against nothing.
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

/// The Placement IR for one struct/def body, one per non-empty
/// [`crate::edition_netlist_ir::EditionNetlistIr`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct PlacementIr {
    /// Edition this IR was compiled for, which every cell's [`EditionCell::edition`] matches.
    pub edition: Edition,
    /// Reservation the cells and pads were placed into; absent only on an empty IR, which
    /// [`ScopedPlacementIr::push`] drops.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<CircuitRegionReservation>,
    /// Sensor-driven nets, copied from the source Edition Netlist IR.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<NetlistInput>,
    /// Actuator-driven nets, each placed at its output pad, so the wire out to an actuator is
    /// routed, delayed and buffered like the wire into a cell.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<PlacedOutputNode>,
    /// Placed cells, in the same topological order as the source
    /// [`crate::edition_netlist_ir::EditionNetlistIr::cells`]
    /// (`NetRef::Cell(j)` in `cells[i]` still satisfies `j < i`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<PlacedCellNode>,
    /// `sig.NAME` → the net that defines it, copied from the source Edition Netlist IR and
    /// serialised the same way.
    #[serde(
        skip_serializing_if = "IndexMap::is_empty",
        serialize_with = "crate::logic_ir::serialize_signal_defs"
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

/// Every scope's [`PlacementIr`], in source order across the whole
/// module.
pub type ScopedPlacementIr = Scoped<PlacementIr>;

/// One entry in a [`ScopedPlacementIr`]: the Placement IR laid out from
/// one scope's Edition Netlist IR.
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
