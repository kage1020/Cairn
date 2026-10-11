//! Placement IR — edition-tagged cells laid out inside their scope's
//! `circuit region` reservation.
//!
//! Sits one lowering step above the [Edition Netlist IR](crate::edition_netlist_ir)
//! and one below the routed block-array IR that later passes will grow
//! on top. The placement pass ([`crate::placement::compile_placement`])
//! walks every [`crate::edition_netlist_ir::ScopedEditionNetlistIrEntry`]
//! once, looks up the scope's [`cairn_lang_core::CircuitRegion`], and
//! assigns each [`crate::edition_netlist_ir::EditionCellNode`] an
//! integer [`CellCoord`] inside the reservation — the first stage of
//! the five-stage pipeline `spec/redstone` "Place-and-route" lays out
//! (Placement → Steiner routing → Delay insertion → Crossing
//! legalization → Edition legalization).
//!
//! Delay is not carried at the placement stage itself. Per
//! `spec/redstone` "Time model" delay is determined "for the first
//! time in the Placement IR" once wire length is known; wire length is
//! the output of the routing pass (Steiner routing, stage 2 of that
//! pipeline) and is folded into `local_delay_ticks` by the
//! delay-insertion pass
//! ([`crate::delay::compile_delay`], stage 3), so
//! [`PlacedCellNode::wire_length`] and [`PlacedCellNode::local_delay_ticks`]
//! are reserved as `Option`s that the placement pass leaves `None`.
//! Adding values in a follow-up pass is a field-write, not a schema
//! change, so downstream JSON consumers see a stable wire shape. Which
//! pass a given dump came out of is named outright by the `stage` tag
//! every cell carries ([`PlacementStage`]) rather than inferred from
//! which optional keys are present, because that inference cannot
//! separate a stage-4 dump with nothing to legalize from its stage-3
//! input. The tag is also what makes a dumped cell readable again:
//! [`PlacedCellNode`]'s [`Deserialize`] picks the phase by it and
//! refuses a cell whose keys disagree with it.
//!
//! `circuit region=... void=N` congestion detection (`E_ROUTE_CONGESTION`)
//! and missing-reservation refusal (`E_NO_CIRCUIT_REGION`) fire here —
//! this is the first pass with a physical footprint to measure against.
//! Attenuation limits are measured later and in two places, both
//! against [`crate::delay::MAX_ATTENUATION_SEGMENT`]:
//! [`crate::delay::compile_delay`], stage 3, fires `E_ATTENUATION_LIMIT`
//! against the routed segment length, and `crate::pass::lay_nets`,
//! which every place-and-route pass calls, fires the same code against
//! the straight line between a driver and its sink before routing to
//! it.

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

/// Which of the three pseudo-2.5D layers in `spec/redstone`
/// "Place-and-route" a coordinate lives on. `Plane` is the ground
/// layer every cell coord and every pad sits on; `Bridge` is the
/// horizontal escape layer above it; `Via` is the vertical tap between
/// the two.
///
/// Cell coords are `Plane` by construction — the placement pass never
/// stamps `Bridge` / `Via` on a cell body. Everything else the
/// pipeline puts inside the reservation takes its layer from its
/// height through [`CellCoord::new`]: routed wire that climbs to get
/// past a block, or past another net's dust. One rule, so two things
/// at one voxel are one map key rather than two.
/// Serialising as the enum's stable lowercase string (`plane` /
/// `bridge` / `via`) keeps the JSON wire form small and matches the
/// vocabulary `spec/redstone` "Place-and-route" uses. Reading one back
/// is derived, so it accepts exactly the variant names in snake case —
/// the words [`Self::as_str`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(strum::EnumIter))]
pub enum RouteLayer {
    /// Ground layer — every cell coord, every pad, and every coord
    /// the router did not have to climb to sits here. Default so that
    /// any code path which forgets to set the layer explicitly
    /// defaults to the ground layer rather than an escape layer.
    #[default]
    Plane,
    /// Escape layer above the ground: wire the routing pass had to
    /// climb to get past a block or past the dust of a net laid before
    /// it. A buffer repeater standing on such a coord carries the
    /// layer with it, rather than choosing one of its own.
    Bridge,
    /// Reserved for the vertical `Plane` ↔ `Bridge` tap. No producer
    /// materialises `Via` in v1: a climb is a step between two coords
    /// rather than a coord of its own, so there is no ramp to name.
    /// Kept in the enum so a downstream consumer can match exhaustively
    /// against the full `spec/redstone` "Place-and-route" vocabulary; a
    /// subsequent pass that grows `Via` producers is
    /// `#[non_exhaustive]`-safe.
    Via,
}

impl RouteLayer {
    /// `true` for the default [`Self::Plane`] layer. Used by the
    /// `serde(skip_serializing_if = "…")` attribute on
    /// [`CellCoord::layer`] so the JSON wire form of a `Plane` coord
    /// omits the `layer` field entirely — the routed / delayed IR
    /// shape is an additive subset of the legalized IR shape apart
    /// from the `stage` tag ([`PlacementStage`]).
    #[must_use]
    pub const fn is_plane(&self) -> bool {
        matches!(self, Self::Plane)
    }

    /// Stable lowercase string form used in the JSON wire format and
    /// matched by downstream tooling. Mirrors the vocabulary
    /// `spec/redstone` "Place-and-route" uses when it introduces the
    /// three concepts.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plane => "plane",
            Self::Bridge => "bridge",
            Self::Via => "via",
        }
    }
}

impl Serialize for RouteLayer {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Coordinate inside a scope's `circuit region` reservation.
///
/// `Copy` because a coordinate is a value type consumers pass around
/// by value. Cell coords stamped by [`crate::placement::compile_placement`]
/// use `x = 1 + 2 * topological index`, `y = 0`, `z = 1`,
/// [`RouteLayer::Plane`]. Pad coords derived by the routing pass at the
/// reservation edges use `y = 0` on the plane, and in a scope with cells
/// `z = i` below the cell row and `z = i + 1` from it on, saturating at
/// `depth-1`; in a scope with none, `z = i`. No pad the placement pass
/// emits stands on the cell row: the saturation can put one there, but
/// only in a region that pass refuses. Routed wire
/// coords and buffer-repeater coords may use `y >= 1`, and take
/// [`RouteLayer::Bridge`] when they do — see [`Self::new`].
///
/// The `layer` field participates in `Eq` and `Hash`, so
/// `(x, y, z, Plane)` and `(x, y, z, Bridge)` are distinct map keys.
/// Nothing the pipeline builds is ever both: [`Self::new`] derives the
/// layer from the height, so one voxel has one spelling and `Eq` is
/// voxel identity for every coord a pass produces. The fields are
/// public, though, so a struct literal can still write a coord the
/// constructor would not — which is why the geometry module puts
/// everything it holds through that same rule before comparing it.
///
/// `layer` serialises only when it differs from [`RouteLayer::Plane`]
/// so a placement / routing / delay JSON dump omits the `layer` key
/// entirely: the legalized IR shape is an additive superset of the
/// earlier stages' shape apart from the `stage` tag
/// ([`PlacementStage`]), whose value changes rather than appears.
///
/// Reading a coord back goes through [`Self::new`] too, so a read-back
/// coord is one the pipeline could have built: an absent `layer` is the
/// one the height gives, an explicit `layer` that agrees with it is
/// accepted, and one that disagrees is refused rather than read as a
/// second spelling of the voxel. That refuses every explicit `via`,
/// deliberately: no producer writes it and [`Self::new`] cannot spell
/// it, so the reader grows it when the rule does. A key the wire form
/// does not have is refused as well.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "CellCoordWire")]
pub struct CellCoord {
    /// Column along the region's x-axis. Zero at the region's origin.
    pub x: u32,
    /// Row along the region's y-axis. `0` for `Plane` coords; the
    /// routing pass sets `y >= 1` for wire that climbs onto a `Bridge`
    /// layer inside the reservation's `void=<N>` budget, and a buffer
    /// repeater standing on that wire carries the height with it.
    pub y: u32,
    /// Row along the region's z-axis. `1` for cell coords stamped by
    /// the placement pass — one row in, so every cell has a clear lane
    /// on each side; pad coords step from `z = 0` and skip the cell
    /// row (saturating at `depth-1`), and buffer coords take the row of
    /// the wire they stand on.
    pub z: u32,
    /// Pseudo-2.5D layer this coord lives on. Cell coords are
    /// [`RouteLayer::Plane`] by construction; wire the routing pass
    /// climbs above the ground layer carries [`RouteLayer::Bridge`],
    /// by the one rule [`Self::new`] holds.
    /// Serialised only when it differs from the default so a
    /// `Plane` coord's JSON omits the `layer` field.
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
    /// The coord at `(x, y, z)`: [`RouteLayer::Plane`] on the ground
    /// layer, [`RouteLayer::Bridge`] above it.
    ///
    /// The only constructor, and the one a coord read back from a dump
    /// goes through, so the layer is a function of the height for
    /// everything the pipeline builds. One rule rather than one
    /// per producer, because `(x, 1, z, Plane)` and
    /// `(x, 1, z, Bridge)` are distinct map keys and one voxel: two
    /// producers with two rules would key past each other and put two
    /// things in the same place. The lifted coords are the routing
    /// pass's, whose wire climbs to get past a block or past another
    /// net; a buffer repeater takes the layer of the route coord it
    /// stands on rather than choosing one.
    #[must_use]
    pub const fn new(x: u32, y: u32, z: u32) -> Self {
        let layer = if y == 0 {
            RouteLayer::Plane
        } else {
            RouteLayer::Bridge
        };
        Self { x, y, z, layer }
    }

    /// Coord with an explicit [`RouteLayer`]. Test-only: production
    /// code reaches a non-`Plane` layer through [`Self::routed`], which
    /// derives it from the height, and an assertion written with that
    /// same rule would agree with a broken rule. This is the
    /// independent spelling the assertions are written in.
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn with_layer(x: u32, y: u32, z: u32, layer: RouteLayer) -> Self {
        Self { x, y, z, layer }
    }
}

/// One implicit buffer repeater the crossing-legalization pass
/// materialised, tagged with the driver port on the owning cell it
/// belongs to.
///
/// The driver attribution is copied verbatim from
/// [`PlacedCellNode::drivers`]`[i].port` at push time, so a downstream
/// consumer can group buffers by their source segment without
/// re-walking the routed tree.
///
/// **The vector is an attribution list, not a block list.** There is
/// one entry per segment per repeater that segment's signal passes
/// through, so [`Self::coord`] repeats whenever one repeater serves
/// more than one segment: two ports of a cell reading one signal, two
/// cells hanging off the same point of the routed tree, a cell and an
/// actuator on one net. That is the shape the `{port, coord}` pair
/// asks for — the answer to "which segments does this block serve" is
/// a list, and the entries carry it one at a time. **A consumer that
/// wants the number of blocks deduplicates by `coord`**; a consumer
/// that wants a segment's repeaters filters by `port`. The delay pass
/// charges ticks per driving net for the same reason, so
/// `local_delay_ticks - base_delay_ticks` is `BUFFER_REPEATER_TICKS`
/// per deduplicated block and not per entry.
///
/// **Order contract on `Vec<BufferCoord>`** (as returned by
/// [`PlacedCellNode::buffer_coords`]): entries appear in
/// [`PlacedCellNode::drivers`] iteration order, and within one
/// driver's segment in the order the routed path visits them. A
/// consumer that reads the vector positionally therefore sees driver
/// 0's buffers before driver 1's, and closer-to-source buffers before
/// closer-to-sink ones. That order is *not* one x → z → y sweep: the
/// route hangs off the trunk laid for a nearer sink and turns wherever
/// it has to go round a block.
///
/// **Coord layer.** [`Self::coord`]`.layer` records the crossing
/// pass's placement decision — see [`RouteLayer`] for the full
/// vocabulary and which variants a producer emits today.
///
/// Serialises as `{"port": "<a|b|sel|out>", "coord": {...}}`, matching the
/// `{port, ...}` shape [`CellPortDriver`] uses on the netlist side.
/// The nested `coord` still elides its `layer` field when it stays on
/// [`RouteLayer::Plane`], so the JSON footprint of a plane buffer stays
/// compact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BufferCoord {
    /// Which segment this buffer stands on: a driver port of the owning
    /// cell, or the wire out to an actuator pad.
    pub port: BufferSegment,
    /// Coordinate the crossing pass chose for the buffer. The
    /// `layer` field records the placement decision — see
    /// [`RouteLayer`] for the vocabulary.
    pub coord: CellCoord,
}

impl BufferCoord {
    /// New `(segment, coord)` attribution pair. Mirrors the
    /// [`CellCoord::new`] constructor convention so the crossing pass
    /// and its tests build entries without struct-literal noise.
    #[must_use]
    pub const fn new(port: BufferSegment, coord: CellCoord) -> Self {
        Self { port, coord }
    }
}

/// Which wire a buffer repeater stands on.
///
/// Separate from [`PortName`] rather than a variant of it, because a
/// [`CellPortDriver`] must not be able to say `out`: a cell has no such
/// input, and public fields plus no validation is all it would take.
/// Here the two are the same kind of answer — "which segment" — so they
/// share one flat wire form: `"a"` / `"b"` / `"sel"` for a driver port,
/// `"out"` for the wire to an actuator.
///
/// `#[non_exhaustive]` for the reason [`PortName`] is: a future cell
/// with more ports, or a future segment kind, is additive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum BufferSegment {
    /// A driver port feeding the owning cell. Copies
    /// [`CellPortDriver::port`] from the driver the crossing pass was
    /// walking when it emitted the entry.
    Port(PortName),
    /// The wire leaving a driver for an actuator's output pad. Has no
    /// owning cell: it is attributed to the [`PlacedOutputNode`] whose
    /// pad it runs to.
    Out,
}

// Hand-written so the two cases share one flat string vocabulary
// instead of the externally-tagged shape a derive would give `Port(..)`.
// The wire form is the contract `BufferCoord`'s doc states.
impl Serialize for BufferSegment {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Port(port) => port.serialize(serializer),
            Self::Out => serializer.serialize_str("out"),
        }
    }
}

impl BufferSegment {
    /// Every word a segment is written as — the driver ports as
    /// [`PortName`]'s derived `Serialize` spells them, then `out` — for
    /// the error an unknown word earns. Only the error reads it: the
    /// acceptance set is [`PortName`]'s own, so a port added there reads
    /// back without touching this list, and a test holds the list to
    /// every word a segment is written as.
    const WORDS: &'static [&'static str] = &["a", "b", "sel", "out"];
}

/// Reads back the one flat string vocabulary [`Serialize`] writes:
/// `out`, or any word [`PortName`]'s own `Deserialize` accepts. An
/// unknown word is refused naming every segment word rather than only
/// the ports.
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

/// The `circuit region=<label> void=<N>` reservation the enclosing
/// scope declared, together with the footprint that reservation sits
/// inside.
///
/// Copied verbatim onto every non-empty [`PlacementIr`] so a JSON dump
/// self-describes and downstream routing does not re-derive the
/// budget. Extending this struct with a new field (region origin,
/// per-region attenuation budget, ...) is `#[non_exhaustive]`-safe.
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
    /// Total blocks reserved for routing: `width * depth * void`,
    /// saturating.
    ///
    /// `u64` holds the product of two `u32`s and not of three —
    /// `u32::MAX` squared is already within a rounding error of
    /// `u64::MAX`, `2^33 - 2` short of it — so a reservation taken from
    /// a hostile `size=` can carry it over, though not every `void`
    /// above 1 does: `2147483647x2147483647` still fits at `void=4`.
    /// What overflowed is reachable from a `.crn` either way.
    ///
    /// The workspace sets no `overflow-checks`, so what that cost
    /// depended on the build. A debug build panicked. A release build
    /// wrapped, and wrapping is the worse of the two, because it is
    /// silent exactly where the number is still plausible:
    /// `size=4294967295x4294967295 void=3` wraps to ~1.8e19, which no
    /// caller distinguishes from a real reservation, and the run exits
    /// 0. Where the product is an exact multiple of `2^64` it wraps to
    /// **zero** instead — `size=2147483648x2147483648 void=4` is the
    /// smallest such shape — and a zero reservation is not merely
    /// wrong: the congestion test it then fails divides by it in
    /// `area_ratio_tenths`, which takes the process down.
    ///
    /// Saturating rather than widening, because every caller compares
    /// this against a budget or divides into it, and `u64::MAX` is the
    /// right answer to both: a reservation that large is not the
    /// constraint on anything. The refusal such a scope earns comes
    /// from the attenuation cap, which its own span and its own
    /// sentence are about.
    #[must_use]
    pub const fn reserved_area(&self) -> u64 {
        (self.width as u64)
            .saturating_mul(self.depth as u64)
            .saturating_mul(self.void as u64)
    }
}

/// Which place-and-route pass last wrote to a [`PlacedCellNode`],
/// projected out of its [`PlacementPhase`] and emitted as the `stage`
/// key of the JSON wire form.
///
/// Without it a JSON consumer has to infer the stage from which
/// optional keys are present, and that inference is not total:
/// [`PlacementPhase::Delayed`] and a [`PlacementPhase::Legalized`]
/// whose crossing pass materialised zero buffers carry exactly the
/// same keys, because an empty `buffer_coords` serde-skips. The tag
/// makes the two distinguishable without turning the empty vector
/// into a sentinel.
///
/// The strings are the same vocabulary `cairn synth --stage <s>`
/// accepts, so a dump names the flag that produced it and a consumer
/// can round-trip the two. Keep them in step with the CLI's
/// `SynthStage` value names — the CLI test suite asserts the
/// equality per stage. Reading a tag back is derived, so it accepts
/// exactly the variant names in snake case, which are the words
/// [`Self::as_str`] writes; a test walks every variant through both.
///
/// `#[non_exhaustive]` for the same reason [`PlacementPhase`] is: the
/// fifth stage (Edition legalization) has no variant yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(strum::EnumIter))]
#[non_exhaustive]
pub enum PlacementStage {
    /// Produced by [`crate::placement::compile_placement`] — paired
    /// with [`PlacementPhase::Unrouted`].
    Placement,
    /// Produced by [`crate::routing::compile_routing`] — paired with
    /// [`PlacementPhase::Routed`].
    Route,
    /// Produced by [`crate::delay::compile_delay`] — paired with
    /// [`PlacementPhase::Delayed`].
    Delay,
    /// Produced by [`crate::crossing::compile_crossing`] — paired with
    /// [`PlacementPhase::Legalized`], whether or not that pass had any
    /// buffer to materialise.
    Crossing,
}

impl PlacementStage {
    /// Stable lowercase string form used in the JSON wire format and
    /// accepted by `cairn synth --stage <s>`. Mirrors
    /// [`RouteLayer::as_str`], which fixes the other wire-level enum in
    /// this module to one authoritative spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Placement => "placement",
            Self::Route => "route",
            Self::Delay => "delay",
            Self::Crossing => "crossing",
        }
    }
}

impl Serialize for PlacementStage {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

/// Progressive state of a [`PlacedCellNode`] as it moves through the
/// first four of the five stages of the pipeline `spec/redstone`
/// "Place-and-route" lays out (Placement → Steiner routing → Delay
/// insertion → Crossing legalization → Edition legalization; the
/// fifth stage is future work and does not yet have a variant).
///
/// Every legal `(wire_length, local_delay_ticks, buffer_coords)` combination
/// the pipeline can produce is one of these variants:
///
/// | Producer                                             | Variant                                                                     | `stage` tag   |
/// |------------------------------------------------------|-----------------------------------------------------------------------------|---------------|
/// | [`crate::placement::compile_placement`] (Stage 1)    | [`Self::Unrouted`]                                                          | `placement`   |
/// | [`crate::routing::compile_routing`]     (Stage 2)    | [`Self::Routed`] `{ wire_length }`                                          | `route`       |
/// | [`crate::delay::compile_delay`]         (Stage 3)    | [`Self::Delayed`] `{ wire_length, local_delay_ticks }`                      | `delay`       |
/// | [`crate::crossing::compile_crossing`]   (Stage 4)    | [`Self::Legalized`] `{ wire_length, local_delay_ticks, buffer_coords }`     | `crossing`    |
///
/// The rightmost column is [`Self::stage`] — see [`PlacementStage`]
/// for why the JSON dump names its stage outright instead of leaving
/// a consumer to infer it from which optional keys are present.
///
/// Illegal shapes such as "have `local_delay_ticks` but no `wire_length`" or
/// "carry `buffer_coords` before the delay pass has run" are
/// unrepresentable — each transition is expressed by the mutation
/// methods [`Self::route`], [`Self::delay`], and [`Self::legalize`],
/// which pattern-match the current variant and panic on any out-of-order
/// call. Each has a fallible mirror ([`Self::try_route`],
/// [`Self::try_delay`], [`Self::try_legalize`]) that returns
/// [`PlacementPhaseTransitionError`] instead, for the callers to whom
/// an out-of-order transition is recoverable input rather than a bug.
/// `buffer_coords` on [`Self::Legalized`] is allowed to be empty:
/// the crossing pass materialises one entry per implicit buffer the
/// delay pass counted, and a scope whose delay pass counted zero
/// buffers is still legalized (transitions to [`Self::Legalized`] with
/// an empty vector).
///
/// `#[non_exhaustive]` so a future Stage-5 `EditionLegalized` variant
/// is additive: downstream `match` sites in other crates must carry a
/// `_ => …` arm today.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlacementPhase {
    /// Fresh out of the placement pass — no wire, no delay, no buffers.
    Unrouted,
    /// After Steiner routing: wire length is known, delay and buffers
    /// are not yet.
    Routed {
        /// Routed wire length from every driver into this cell, per
        /// [`crate::routing::compile_routing`].
        wire_length: u32,
    },
    /// After delay insertion: delay ticks have been folded in over the
    /// routed wire length, per [`crate::delay::compile_delay`].
    Delayed {
        /// Preserved from [`Self::Routed`].
        wire_length: u32,
        /// The owning node's base delay (`spec/redstone` "Time model";
        /// zero for an actuator pad) plus `BUFFER_REPEATER_TICKS` per
        /// implicit buffer repeater on each distinct net feeding it.
        /// A local wire cost, not an arrival time — see the
        /// [`crate::delay`] module doc.
        local_delay_ticks: u32,
    },
    /// After crossing legalization: buffer coordinates for the implicit
    /// repeaters the delay pass counted have been materialised, per
    /// [`crate::crossing::compile_crossing`].
    Legalized {
        /// Preserved from [`Self::Routed`].
        wire_length: u32,
        /// Preserved from [`Self::Delayed`].
        local_delay_ticks: u32,
        /// One entry per implicit buffer repeater the delay pass
        /// counted; the crossing pass populates each entry with the
        /// coord it chose and the driver port that buffer belongs to.
        /// Empty when the delay pass counted zero buffers.
        ///
        /// See [`BufferCoord`] for the per-entry shape, ordering
        /// contract, and layer semantics.
        buffer_coords: Vec<BufferCoord>,
    },
}

impl PlacementPhase {
    /// Wire length once routing has run, `None` otherwise.
    #[must_use]
    pub const fn wire_length(&self) -> Option<u32> {
        match self {
            Self::Unrouted => None,
            Self::Routed { wire_length }
            | Self::Delayed { wire_length, .. }
            | Self::Legalized { wire_length, .. } => Some(*wire_length),
        }
    }

    /// Local delay ticks once delay insertion has run, `None`
    /// otherwise. See the `local_delay_ticks` field on
    /// [`Self::Delayed`].
    #[must_use]
    pub const fn local_delay_ticks(&self) -> Option<u32> {
        match self {
            Self::Unrouted | Self::Routed { .. } => None,
            Self::Delayed {
                local_delay_ticks, ..
            }
            | Self::Legalized {
                local_delay_ticks, ..
            } => Some(*local_delay_ticks),
        }
    }

    /// Buffer coordinates once crossing legalization has run, an empty
    /// slice otherwise. Legalized-with-zero-buffers also returns an
    /// empty slice so callers can treat "nothing to place" and "not
    /// yet legalized" identically for read-only purposes; use the
    /// enum discriminant if the distinction matters.
    ///
    /// All variants are matched explicitly rather than via `_ => &[]`
    /// so that adding a Stage-5 variant becomes a compile error here
    /// instead of a silent empty-slice return that would drop the new
    /// stage's data from the JSON dump.
    #[must_use]
    pub fn buffer_coords(&self) -> &[BufferCoord] {
        match self {
            Self::Unrouted | Self::Routed { .. } | Self::Delayed { .. } => &[],
            Self::Legalized { buffer_coords, .. } => buffer_coords,
        }
    }

    /// Which pass last wrote to this cell — the discriminant projected
    /// onto the [`PlacementStage`] the JSON wire form carries. Unlike
    /// the three value accessors above this one is total: every phase
    /// belongs to exactly one stage, including a [`Self::Legalized`]
    /// that has no buffer coords to show for itself.
    ///
    /// All variants are matched explicitly rather than via a `_ =>`
    /// catch-all so that adding a Stage-5 variant becomes a compile
    /// error here instead of a silent mislabelling of the new stage's
    /// dumps as stage 4.
    #[must_use]
    pub const fn stage(&self) -> PlacementStage {
        match self {
            Self::Unrouted => PlacementStage::Placement,
            Self::Routed { .. } => PlacementStage::Route,
            Self::Delayed { .. } => PlacementStage::Delay,
            Self::Legalized { .. } => PlacementStage::Crossing,
        }
    }

    /// [`Self::Unrouted`] → [`Self::Routed`].
    ///
    /// Carries no caller context, so an out-of-order call names only
    /// the phase it tripped on. Prefer [`Self::route_at`] from a
    /// pipeline pass, which names the offending cell as well.
    ///
    /// # Panics
    ///
    /// Panics if the phase is not [`Self::Unrouted`]. Routing must run
    /// exactly once per placement, and the producer↔variant table on
    /// this enum forbids re-routing. Use [`Self::try_route`] where an
    /// out-of-order call is a recoverable input condition rather than
    /// a caller-side bug.
    #[track_caller]
    pub fn route(&mut self, wire_length: u32) {
        or_panic(self.try_route(wire_length), None);
    }

    /// [`Self::Unrouted`] → [`Self::Routed`], naming `context` in the
    /// panic an out-of-order call raises.
    ///
    /// `#[track_caller]` alone puts the calling `.rs:line` in the
    /// backtrace but says nothing about *which* cell tripped the
    /// guard. Pipeline passes pass a crate-internal `CellIdentity`; any
    /// [`fmt::Display`] works.
    ///
    /// # Panics
    ///
    /// Panics under exactly the conditions [`Self::route`] does.
    #[track_caller]
    pub fn route_at(&mut self, wire_length: u32, context: impl fmt::Display) {
        or_panic(self.try_route(wire_length), Some(&context));
    }

    /// [`Self::Unrouted`] → [`Self::Routed`], refusing an out-of-order
    /// call with an [`Err`] instead of a panic.
    ///
    /// [`Self::route`] is the form the pipeline passes use: a
    /// wrong-order transition in a fresh compile is a caller-side bug
    /// with no recovery path, and panicking is what surfaces it. This
    /// form is for the consumers that do have one — a cache validator
    /// that rebuilds from scratch, an IR ingest that refuses a
    /// malformed dump with a diagnostic, a language server that cannot
    /// take a process down over one bad call.
    ///
    /// A refused call leaves the phase untouched, so a caller that
    /// recovers can go on using it.
    ///
    /// All non-`Unrouted` variants are listed explicitly (rather than a
    /// `_ =>` catch-all) so a Stage-5 variant addition becomes a
    /// compile error here — the author must decide whether Stage 5 can
    /// be re-routed rather than silently refusing.
    ///
    /// # Errors
    ///
    /// Returns [`PlacementPhaseTransitionError::RouteOnNonUnrouted`],
    /// carrying the phase it found, whenever the phase is not
    /// [`Self::Unrouted`].
    pub fn try_route(&mut self, wire_length: u32) -> Result<(), PlacementPhaseTransitionError> {
        match self {
            Self::Unrouted => {
                *self = Self::Routed { wire_length };
                Ok(())
            }
            Self::Routed { .. } | Self::Delayed { .. } | Self::Legalized { .. } => {
                Err(PlacementPhaseTransitionError::RouteOnNonUnrouted {
                    current: self.clone(),
                })
            }
        }
    }

    /// [`Self::Routed`] → [`Self::Delayed`].
    ///
    /// Carries no caller context — see [`Self::delay_at`] for the form
    /// a pipeline pass should use.
    ///
    /// # Panics
    ///
    /// Panics if the phase is not [`Self::Routed`]. Delay insertion
    /// must run exactly once per routed IR, and the producer↔variant
    /// table on this enum forbids re-writing a `local_delay_ticks` that was
    /// already committed. See [`Self::try_delay`] for the fallible
    /// mirror.
    #[track_caller]
    pub fn delay(&mut self, local_delay_ticks: u32) {
        or_panic(self.try_delay(local_delay_ticks), None);
    }

    /// [`Self::Routed`] → [`Self::Delayed`], naming `context` in the
    /// panic an out-of-order call raises, as [`Self::route_at`] does.
    ///
    /// # Panics
    ///
    /// Panics under exactly the conditions [`Self::delay`] does.
    #[track_caller]
    pub fn delay_at(&mut self, local_delay_ticks: u32, context: impl fmt::Display) {
        or_panic(self.try_delay(local_delay_ticks), Some(&context));
    }

    /// [`Self::Routed`] → [`Self::Delayed`], refusing an out-of-order
    /// call with an [`Err`] instead of a panic. See [`Self::try_route`]
    /// for which callers want this form and why the arms are
    /// enumerated explicitly.
    ///
    /// # Errors
    ///
    /// Returns [`PlacementPhaseTransitionError::DelayOnNonRouted`],
    /// carrying the phase it found, whenever the phase is not
    /// [`Self::Routed`].
    pub fn try_delay(
        &mut self,
        local_delay_ticks: u32,
    ) -> Result<(), PlacementPhaseTransitionError> {
        let wire_length = match self {
            Self::Routed { wire_length } => *wire_length,
            Self::Unrouted | Self::Delayed { .. } | Self::Legalized { .. } => {
                return Err(PlacementPhaseTransitionError::DelayOnNonRouted {
                    current: self.clone(),
                });
            }
        };
        *self = Self::Delayed {
            wire_length,
            local_delay_ticks,
        };
        Ok(())
    }

    /// [`Self::Delayed`] → [`Self::Legalized`].
    ///
    /// Carries no caller context — see [`Self::legalize_at`] for the
    /// form a pipeline pass should use.
    ///
    /// # Panics
    ///
    /// Panics if the phase is not [`Self::Delayed`]. Crossing
    /// legalization must run exactly once per delayed IR, so a caller
    /// who chained `compile_crossing(&legalized.scoped)` trips here
    /// rather than silently producing a stale-but-plausible IR, and one
    /// who reached for it before delay insertion ran trips here too.
    /// See [`Self::try_legalize`] for the fallible mirror.
    #[track_caller]
    pub fn legalize(&mut self, buffer_coords: Vec<BufferCoord>) {
        or_panic(self.try_legalize(buffer_coords), None);
    }

    /// [`Self::Delayed`] → [`Self::Legalized`], naming `context` in the
    /// panic an out-of-order call raises, as [`Self::route_at`] does.
    ///
    /// # Panics
    ///
    /// Panics under exactly the conditions [`Self::legalize`] does.
    #[track_caller]
    pub fn legalize_at(&mut self, buffer_coords: Vec<BufferCoord>, context: impl fmt::Display) {
        or_panic(self.try_legalize(buffer_coords), Some(&context));
    }

    /// [`Self::Delayed`] → [`Self::Legalized`], refusing an
    /// out-of-order call with an [`Err`] instead of a panic. See
    /// [`Self::try_route`] for which callers want this form and why the
    /// arms are enumerated explicitly.
    ///
    /// A refused call drops `buffer_coords` unwritten and leaves the
    /// phase exactly as it was found. The vector is taken by value, so
    /// a caller that needs it after a refusal has to clone it before
    /// calling.
    ///
    /// # Errors
    ///
    /// Returns
    /// [`PlacementPhaseTransitionError::LegalizeOnNonDelayed`],
    /// carrying the phase it found, whenever the phase is not
    /// [`Self::Delayed`].
    pub fn try_legalize(
        &mut self,
        buffer_coords: Vec<BufferCoord>,
    ) -> Result<(), PlacementPhaseTransitionError> {
        let (wire_length, local_delay_ticks) = match self {
            Self::Delayed {
                wire_length,
                local_delay_ticks,
            } => (*wire_length, *local_delay_ticks),
            Self::Unrouted | Self::Routed { .. } | Self::Legalized { .. } => {
                return Err(PlacementPhaseTransitionError::LegalizeOnNonDelayed {
                    current: self.clone(),
                });
            }
        };
        *self = Self::Legalized {
            wire_length,
            local_delay_ticks,
            buffer_coords,
        };
        Ok(())
    }
}

/// The panicking transition forms are their `try_*` mirror plus this,
/// so the guard itself is stated once and the two forms cannot disagree
/// about which transitions are legal.
#[track_caller]
fn or_panic(result: Result<(), PlacementPhaseTransitionError>, context: Option<&dyn fmt::Display>) {
    if let Err(error) = result {
        transition_panic(&error, context);
    }
}

/// Why a [`PlacementPhase`] transition refused: the transition that
/// was attempted, plus the phase it found instead of the one it
/// consumes.
///
/// Returned by [`PlacementPhase::try_route`] /
/// [`PlacementPhase::try_delay`] / [`PlacementPhase::try_legalize`],
/// and the same value the panicking forms render into their payload —
/// so a recovered refusal and a release-loud one read alike.
///
/// The whole offending phase is carried rather than just its variant
/// name: a consumer diagnosing a stale cache entry or a malformed IR
/// dump wants the `wire_length` / `local_delay_ticks` the phase got as far
/// as, and it is what lets [`fmt::Display`] reproduce the panic
/// wording exactly.
///
/// `#[non_exhaustive]` for the same reason [`PlacementPhase`] is: a
/// Stage-5 transition brings a fourth refusal with it, and that must
/// be an additive change for downstream `match` sites.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlacementPhaseTransitionError {
    /// [`PlacementPhase::try_route`] was called on a phase that had
    /// already been routed, delayed, or legalized.
    RouteOnNonUnrouted {
        /// The phase found instead of [`PlacementPhase::Unrouted`].
        current: PlacementPhase,
    },
    /// [`PlacementPhase::try_delay`] was called on a phase that had not
    /// been routed yet, or had already been delayed or legalized.
    DelayOnNonRouted {
        /// The phase found instead of [`PlacementPhase::Routed`].
        current: PlacementPhase,
    },
    /// [`PlacementPhase::try_legalize`] was called on a phase that had
    /// not been delayed yet, or had already been legalized.
    LegalizeOnNonDelayed {
        /// The phase found instead of [`PlacementPhase::Delayed`].
        current: PlacementPhase,
    },
}

impl PlacementPhaseTransitionError {
    /// The phase the refused transition found.
    ///
    /// Total across every variant, so a consumer can read the offending
    /// phase without matching — which under `#[non_exhaustive]` would
    /// otherwise cost it a `_ =>` arm.
    #[must_use]
    pub const fn current(&self) -> &PlacementPhase {
        match self {
            Self::RouteOnNonUnrouted { current }
            | Self::DelayOnNonRouted { current }
            | Self::LegalizeOnNonDelayed { current } => current,
        }
    }

    /// Render this refusal with a caller-supplied identity clause
    /// spliced in, in the same place and format
    /// [`PlacementPhase::route_at`] and friends put theirs.
    ///
    /// A consumer that recovered from the refusal still knows what it
    /// was working on — the index of the cell in the dump it is
    /// ingesting, the path of the cache entry it is validating, the
    /// document position it is reporting on — and this is how it says
    /// so without hand-assembling a message that would drift from the
    /// panicking forms'. Any [`fmt::Display`] works, and it is taken by
    /// value like the context the `*_at` transitions take.
    #[must_use]
    pub fn with_context<'a, C: fmt::Display + 'a>(&'a self, context: C) -> impl fmt::Display + 'a {
        TransitionMessage {
            error: self,
            context: Some(context),
        }
    }

    /// The transition method that refused, as it is spelled in the
    /// message.
    const fn method(&self) -> &'static str {
        match self {
            Self::RouteOnNonUnrouted { .. } => "route",
            Self::DelayOnNonRouted { .. } => "delay",
            Self::LegalizeOnNonDelayed { .. } => "legalize",
        }
    }

    /// The pipeline pass that transition belongs to.
    const fn pass(&self) -> &'static str {
        match self {
            Self::RouteOnNonUnrouted { .. } => "routing",
            Self::DelayOnNonRouted { .. } => "delay insertion",
            Self::LegalizeOnNonDelayed { .. } => "crossing legalization",
        }
    }

    /// What that pass consumes exactly one of.
    const fn source(&self) -> &'static str {
        match self {
            Self::RouteOnNonUnrouted { .. } => "placement",
            Self::DelayOnNonRouted { .. } => "routed IR",
            Self::LegalizeOnNonDelayed { .. } => "delayed IR",
        }
    }
}

impl fmt::Display for PlacementPhaseTransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        TransitionMessage::<&dyn fmt::Display> {
            error: self,
            context: None,
        }
        .fmt(f)
    }
}

impl std::error::Error for PlacementPhaseTransitionError {}

/// The cardinality clause every refused [`PlacementPhase`] transition
/// carries, between the pass that tripped the guard and the phase that
/// pass consumes.
///
/// Spelled once so a transition added beside `route` / `delay` /
/// `legalize` cannot invent its own strength, which holds only as long
/// as every transition reports through [`TransitionMessage`] rather
/// than writing its own message.
///
/// "Exactly" rather than "at most": each transition accepts one source
/// variant and refuses every other, so a phase that never reached that
/// source trips the same guard a re-run does. A skipped stage is as
/// much a bug as a repeated one, and only "exactly once" says so.
const TRANSITION_CARDINALITY: &str = "must run exactly once per";

/// The one rendering of a [`PlacementPhaseTransitionError`], with the
/// identity clause optional.
///
/// Every message this module produces — the `Display` of the error, the
/// `with_context` form, and both shapes of transition panic — goes
/// through here, so the fallible and panicking forms cannot drift
/// apart. An absent context drops the whole ` for {context}` clause
/// rather than rendering an empty one, so no stray separator or double
/// space reaches the message.
///
/// The context is a type parameter rather than a `&dyn fmt::Display` so
/// that the public [`PlacementPhaseTransitionError::with_context`] can
/// own the value its caller passes, matching how the `*_at` transitions
/// take theirs. The panic path instantiates it at `&dyn fmt::Display`,
/// which is what it already holds.
struct TransitionMessage<'a, C: fmt::Display> {
    error: &'a PlacementPhaseTransitionError,
    context: Option<C>,
}

impl<C: fmt::Display> fmt::Display for TransitionMessage<'_, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PlacementPhase::{method} called on {current:?}",
            method = self.error.method(),
            current = self.error.current(),
        )?;
        if let Some(context) = &self.context {
            write!(f, " for {context}")?;
        }
        write!(
            f,
            " — {pass} {TRANSITION_CARDINALITY} {source}",
            pass = self.error.pass(),
            source = self.error.source(),
        )
    }
}

/// Raise the release-loud panic an out-of-order [`PlacementPhase`]
/// transition owes its caller.
///
/// The panicking transition forms are their `try_*` mirror plus this
/// call, so the message is assembled from the same error value a
/// recovering caller would have received.
///
/// `#[track_caller]` on every layer between the pass and here means
/// the reported location is still the pipeline pass's `.rs:line`, not
/// this function's.
#[track_caller]
fn transition_panic(
    error: &PlacementPhaseTransitionError,
    context: Option<&dyn fmt::Display>,
) -> ! {
    panic!("{}", TransitionMessage { error, context });
}

/// Identity breadcrumb a pipeline pass hands to
/// [`PlacementPhase::route_at`] / [`PlacementPhase::delay_at`] /
/// [`PlacementPhase::legalize_at`] so an out-of-order transition panic
/// names the cell that tripped it.
///
/// [`PlacedCellNode`] carries no source-level name, so a cell's only
/// stable identity is its position in [`PlacementIr::cells`], the
/// coord the placement pass stamped on it, and the scope that owns it.
/// Rendered in the same vocabulary the pass diagnostics already use
/// (`cell #{index}`, `({x},{y},{z})`, ``{kind} `{name}` ``) so a panic
/// and an `E_*` diagnostic about the same cell read alike.
///
/// [`CellCoord::layer`] renders only when it is not
/// [`RouteLayer::Plane`], which for a cell coord is never: the
/// placement pass stamps `Plane` and no later pass moves a cell body
/// off it — only wire, and the buffer repeaters that stand on it, ever
/// reach a `Bridge` layer. Suppressing the default rather than
/// dropping the field outright keeps the common rendering short
/// without letting a hand-built IR that breaks the invariant print a
/// coord that silently reads as a plane coord. Enforcing the
/// invariant with an assertion here instead was rejected: this type
/// is built on the happy path for every cell of every pass, and a
/// breadcrumb whose whole purpose is to improve a panic must not be
/// able to raise one of its own.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CellIdentity<'a> {
    index: usize,
    coord: CellCoord,
    subject: PlacedSubject,
    scope: &'a ScopedPlacementIrEntry,
}

/// Which list the index in a [`CellIdentity`] indexes into. A panic
/// that says "cell #0" about an actuator pad sends the reader to the
/// wrong line of the source, and both lists start at zero.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PlacedSubject {
    /// An entry of [`PlacementIr::cells`].
    Cell,
    /// An entry of [`PlacementIr::outputs`].
    Output,
}

impl PlacedSubject {
    const fn label(self) -> &'static str {
        match self {
            Self::Cell => "cell",
            Self::Output => "output",
        }
    }
}

impl<'a> CellIdentity<'a> {
    /// Breadcrumb for the cell at position `index` of the scope being
    /// transitioned, whose placement coord is `coord`.
    ///
    /// `coord` is passed in rather than read back out of
    /// `scope.ir.cells[index]` because a pass transitions the cells of
    /// its own working clone of the IR, not the entry's: taking the
    /// coord from the very cell being transitioned cannot go stale or
    /// index out of bounds, whereas reaching back into `scope.ir`
    /// could do both if a pass ever adds or drops a cell.
    pub(crate) const fn new(
        index: usize,
        coord: CellCoord,
        scope: &'a ScopedPlacementIrEntry,
    ) -> Self {
        Self {
            index,
            coord,
            subject: PlacedSubject::Cell,
            scope,
        }
    }

    /// Breadcrumb for the actuator pad at position `index`, whose
    /// placement coord is `pad`.
    pub(crate) const fn output(
        index: usize,
        pad: CellCoord,
        scope: &'a ScopedPlacementIrEntry,
    ) -> Self {
        Self {
            index,
            coord: pad,
            subject: PlacedSubject::Output,
            scope,
        }
    }
}

impl fmt::Display for CellIdentity<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{subject} #{index} at ({x},{y},{z}",
            subject = self.subject.label(),
            index = self.index,
            x = self.coord.x,
            y = self.coord.y,
            z = self.coord.z,
        )?;
        if !self.coord.layer.is_plane() {
            write!(f, " on {layer}", layer = self.coord.layer.as_str())?;
        }
        write!(
            f,
            ") in {kind} `{name}`",
            kind = self.scope.kind.label(),
            name = self.scope.name,
        )
    }
}

/// One placed cell — an [`EditionCell`] plus its assigned [`CellCoord`]
/// inside the reservation.
///
/// The progressive fields the routing, delay-insertion, and
/// crossing-legalization passes produce (`wire_length`, `local_delay_ticks`,
/// `buffer_coords`) live inside the `phase` field as a
/// [`PlacementPhase`] — see that enum's docstring for the four
/// legitimate states and the transition methods each pass calls.
/// Read-only convenience accessors [`Self::wire_length`],
/// [`Self::local_delay_ticks`], and [`Self::buffer_coords`] project the
/// phase back onto the three flat fields the JSON wire form exposes,
/// so consumers that only care about the values do not need to match
/// on the phase enum; [`Self::stage`] projects the discriminant
/// itself.
///
/// The custom [`Serialize`] impl flattens that `phase` field onto
/// `{stage, cell, drivers, coord[, wire_length][, local_delay_ticks][, buffer_coords]}`.
/// The three optionals carry the shape earlier revisions of this
/// struct produced via `skip_serializing_if`, so the JSON dump of a
/// stage-N output is an additive subset of the stage-(N+1) dump *apart
/// from the `stage` tag* on scopes whose stage-(N+1) pass had nothing
/// to write. The tag is the one field whose value changes rather than
/// appears, and it exists precisely because the subset relation made
/// a zero-buffer [`PlacementPhase::Legalized`] indistinguishable from
/// a [`PlacementPhase::Delayed`] — see [`PlacementStage`].
///
/// The matching [`Deserialize`] reads that flat form back. It picks the
/// [`PlacementPhase`] variant by the `stage` tag, never by which
/// optional keys are present, and refuses a dump whose keys disagree
/// with its tag: a payload key the named stage has not written yet
/// (`buffer_coords` under `"route"`), or one it must have written and
/// is missing (`local_delay_ticks` under `"delay"`). `buffer_coords`
/// is the one payload key that may be absent at its own stage, because
/// [`Serialize`] leaves it out when the crossing pass placed nothing.
/// A missing `stage`, an unknown `stage` or `layer` word, a `layer`
/// that disagrees with the coord's height (see [`CellCoord`]), a buffer
/// on a segment the cell has no driver for, and a key the wire form
/// does not have — at any depth — are refused too. Every refusal is a
/// deserializer error, not a panic.
///
/// This reader is the first way code outside this crate can build a
/// `PlacedCellNode` at all: the struct is `#[non_exhaustive]` with a
/// crate-private phase and has no public constructor. What it builds is
/// a cell as its `stage`'s pass left it, so hand it to the pass *after*
/// that stage. The phase transitions still panic on a phase they do
/// not expect, as they do for a cell the pipeline built: the promise
/// above that a bad dump is an error rather than a panic is this
/// reader's, not theirs.
///
/// [`Self::span`] is not part of the wire form, so a cell read back
/// carries `0..0` — which is also a real empty span at the start of a
/// file, and `PartialEq` compares it. A round trip is therefore equal
/// except for the span: compare against the original with its span set
/// to `0..0`.
///
/// Read-back is for self-describing formats (JSON and the like). The
/// reader takes a map; the field count of the flat form varies with the
/// phase, from four keys to seven, so a positional encoding such as
/// bincode or postcard cannot be read back unambiguously, and a
/// non-map input is refused rather than guessed at. A repeated key is
/// refused only where the deserializer hands over the map as written —
/// `serde_json::from_str`, `from_slice`, `from_reader`. Through a
/// `serde_json::Value` it cannot be: the `Value` has already folded the
/// duplicates, last one winning, before this reader sees the map.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlacedCellNode {
    /// Edition-specific realisation chosen for this cell (copied verbatim
    /// from the source [`crate::edition_netlist_ir::EditionCellNode`]).
    pub cell: EditionCell,
    /// Port → net drivers, in canonical port order carried across from
    /// the Netlist IR (`[A, B]` for two-input gates, `[A]` for `Not`,
    /// `[Sel, A, B]` for `Mux`).
    pub drivers: Vec<CellPortDriver>,
    /// Coordinate assigned by the placement pass.
    pub coord: CellCoord,
    /// Progressive pipeline state — see [`PlacementPhase`].
    ///
    /// Crate-visible so the three pipeline passes (routing, delay,
    /// crossing) can call the phase-transition methods, but hidden from
    /// downstream crates: writes must go through
    /// [`PlacementPhase::route`] / [`PlacementPhase::delay`] /
    /// [`PlacementPhase::legalize`], and reads through the flat
    /// accessor methods on this struct. Direct field assignment would
    /// bypass the phase-transition guards and re-open the illegal
    /// states the enum exists to forbid.
    pub(crate) phase: PlacementPhase,
    /// Byte range of the originating `logic ...` sub-expression, inherited
    /// from the source [`crate::edition_netlist_ir::EditionCellNode`].
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

/// The flat wire form both placed node types share: `stage` first, so a
/// truncated or eyeballed dump still says which pass produced it, then
/// the node's own `identity_fields` (written by `identity`), then the
/// phase's fields as far as the pipeline has filled them.
///
/// Hand-written rather than derived because the derived form would tag
/// the phase enum and reshape the flat form the integration tests lock
/// byte-for-byte. Binary formats (bincode, postcard, msgpack) rely on the
/// announced field count being exact, so every write goes through a
/// [`CountedFields`] and the count is checked against it before `end`;
/// a new visible field on either node type, or a Stage-5 variant whose
/// payload the JSON must expose, is added here and given a
/// [`PlacementStage`] variant.
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

/// Refuse a buffer a cell could not carry: every [`BufferCoord`] on a
/// cell stands on [`BufferSegment::Port`] of one of that cell's own
/// drivers. It is the contract the crossing pass debug-asserts where it
/// writes them, held here in every build so a dump cannot bring in what
/// the pass itself would never emit — `out` belongs to a
/// [`PlacedOutputNode`], and a port the cell has no driver on would
/// group buffers under a driver that does not exist.
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
/// The placement counterpart of [`crate::netlist_ir::NetlistOutput`],
/// and the reason it
/// exists is that the wire from a driver to an actuator is wire like any
/// other: it attenuates, it needs buffer repeaters past
/// [`crate::DUST_ATTENUATION_LIMIT`], and those repeaters occupy coords
/// inside the reservation. Before this node existed, the pass counted
/// buffers on the segments *into* cells and none on the segment out of
/// the last one, so a driver sitting a hundred blocks from its actuator
/// reported the same delay as one sitting next to it.
///
/// It carries the same [`PlacementPhase`] the cells do, so the four
/// stages fill it by the same rules and a dump reads alike on both
/// sides: `wire_length` after routing, `local_delay_ticks` after delay
/// insertion, `buffer_coords` after legalization. `local_delay_ticks` here is
/// the wire's own contribution — the buffers standing on the segment —
/// with no base delay to add, because a pad is not a cell.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PlacedOutputNode {
    /// Dotted signal reference the actuator consumes, carried across
    /// from [`crate::netlist_ir::NetlistOutput::name`].
    pub name: DottedRef,
    /// Which net drives this port, carried across from
    /// [`crate::netlist_ir::NetlistOutput::driver`].
    pub driver: NetRef,
    /// Coordinate the placement pass assigned this actuator's pad, per
    /// the `x = width - 1` convention the routing geometry uses.
    pub pad: CellCoord,
    /// Progressive pipeline state — see [`PlacementPhase`]. Crate-
    /// visible for the same reason [`PlacedCellNode::phase`] is: writes
    /// go through the transition methods so the illegal shapes stay
    /// unrepresentable.
    pub(crate) phase: PlacementPhase,
    /// Byte range of the originating `opened_by=` argument.
    pub span: Span,
}

impl PlacedOutputNode {
    /// A freshly placed actuator pad, before routing has run. Mirrors
    /// the [`CellCoord::new`] / [`BufferCoord::new`] convention so the
    /// placement pass and the fixtures build one without struct-literal
    /// noise, and so the starting phase is chosen in one place rather
    /// than at every construction site.
    ///
    /// Crate-visible: the pad is the placement pass's to assign, from
    /// the reservation, and a coordinate handed in from outside is one
    /// nothing checks against the region. [`PlacedCellNode`] is
    /// unconstructible from another crate for the same reason — it has
    /// no constructor and the struct is `#[non_exhaustive]`.
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

/// The Placement IR for one struct/def body.
///
/// One instance per [`crate::edition_netlist_ir::EditionNetlistIr`]
/// handed to the pass whose scope produced at least one cell or one
/// actuator. The target edition is pinned so a JSON dump makes clear
/// which library the cells were selected from before placement ran.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct PlacementIr {
    /// Edition this IR was compiled for. Equal to every cell's
    /// [`EditionCell::edition`] by construction.
    pub edition: Edition,
    /// Reservation the cells were placed into. Absent only when the
    /// scope had zero cells (an empty scope is elided by
    /// [`ScopedPlacementIr::push`], so a serialised entry always has a
    /// reservation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<CircuitRegionReservation>,
    /// Sensor-driven nets, copied verbatim from the source Edition
    /// Netlist IR.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub inputs: Vec<NetlistInput>,
    /// Actuator-driven nets, each placed at its output pad. Carried
    /// across from the source Edition Netlist IR and given a coordinate
    /// by the placement pass, so the wire out to an actuator is routed,
    /// delayed, and buffered by the same rules as the wire into a cell.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<PlacedOutputNode>,
    /// Placed cells, in the same topological order as the source
    /// [`crate::edition_netlist_ir::EditionNetlistIr::cells`]
    /// (`NetRef::Cell(j)` in `cells[i]` still satisfies `j < i`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<PlacedCellNode>,
    /// `sig.NAME` → the net that defines it, copied verbatim from the
    /// source Edition Netlist IR.
    ///
    /// Serialises as a JSON object keyed by the flattened dotted name —
    /// the same custom serialisation the Logic / Netlist / Edition
    /// Netlist IRs use — so consumers see a uniform shape across every
    /// IR in the pipeline.
    #[serde(
        skip_serializing_if = "IndexMap::is_empty",
        serialize_with = "crate::logic_ir::serialize_signal_defs"
    )]
    pub signal_defs: IndexMap<DottedRef, NetRef>,
}

impl PlacementIr {
    /// Empty Placement IR for the given edition — no reservation, no
    /// inputs, no outputs, no cells.
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

    /// `true` when this scope produced zero inputs, zero outputs, and
    /// zero cells. Matches [`crate::edition_netlist_ir::EditionNetlistIr::is_empty`]
    /// so scope elision behaves the same across every IR in the
    /// pipeline.
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
    /// Append one scope's Placement IR. Empty scopes are elided so a
    /// module without any redstone renders as `[]`, matching the
    /// sibling IRs' elision.
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
