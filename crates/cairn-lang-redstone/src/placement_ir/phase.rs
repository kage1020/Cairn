//! A placed node's phase through the place-and-route passes, its `stage` tag, and the transitions,
//! which refuse a pass run out of order.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::{BufferCoord, CellCoord, ScopedPlacementIrEntry};

/// Which place-and-route pass last wrote to a placed node: the JSON form's `stage` key, in the
/// words `cairn synth --stage <s>` accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(test, derive(strum::EnumIter))]
#[non_exhaustive]
pub enum PlacementStage {
    /// Produced by [`crate::placement::compile_placement`].
    Placement,
    /// Produced by [`crate::routing::compile_routing`].
    Route,
    /// Produced by [`crate::delay::compile_delay`].
    Delay,
    /// Produced by [`crate::crossing::compile_crossing`], buffers or none.
    Crossing,
}

impl PlacementStage {
    /// The stage's word in the JSON form and in `cairn synth --stage <s>`.
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

/// A placed node's state through the first four stages of `spec/redstone` "Place-and-route", one
/// variant per legal combination of `wire_length`, `local_delay_ticks` and `buffer_coords`.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlacementPhase {
    /// Fresh out of the placement pass — no wire, no delay, no buffers.
    Unrouted,
    /// After Steiner routing: wire length is known.
    Routed {
        /// Routed wire length from every driver into this node.
        wire_length: u32,
    },
    /// After delay insertion.
    Delayed {
        /// Preserved from [`Self::Routed`].
        wire_length: u32,
        /// The slowest driven port's delay plus buffer ticks on each distinct feeding net; not an
        /// arrival time.
        local_delay_ticks: u32,
    },
    /// After crossing legalization: the buffer repeaters have coords.
    Legalized {
        /// Preserved from [`Self::Routed`].
        wire_length: u32,
        /// Preserved from [`Self::Delayed`].
        local_delay_ticks: u32,
        /// The buffer repeaters the delay pass counted, possibly none.
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

    /// Local delay ticks once delay insertion has run, `None` otherwise.
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

    /// Buffer coordinates once crossing legalization has run, empty otherwise.
    #[must_use]
    pub fn buffer_coords(&self) -> &[BufferCoord] {
        match self {
            Self::Unrouted | Self::Routed { .. } | Self::Delayed { .. } => &[],
            Self::Legalized { buffer_coords, .. } => buffer_coords,
        }
    }

    /// Which pass last wrote to this node.
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
    /// # Panics
    ///
    /// If the phase is not [`Self::Unrouted`]; [`Self::try_route`] returns an error instead.
    #[track_caller]
    pub fn route(&mut self, wire_length: u32) {
        or_panic(self.try_route(wire_length), None);
    }

    /// [`Self::route`], naming `context` in the panic.
    ///
    /// # Panics
    ///
    /// As [`Self::route`].
    #[track_caller]
    pub fn route_at(&mut self, wire_length: u32, context: impl fmt::Display) {
        or_panic(self.try_route(wire_length), Some(&context));
    }

    /// [`Self::Unrouted`] → [`Self::Routed`], leaving the phase untouched on an out-of-order call.
    ///
    /// # Errors
    ///
    /// [`PlacementPhaseTransitionError::RouteOnNonUnrouted`] when the phase is not
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
    /// # Panics
    ///
    /// If the phase is not [`Self::Routed`]; [`Self::try_delay`] returns an error instead.
    #[track_caller]
    pub fn delay(&mut self, local_delay_ticks: u32) {
        or_panic(self.try_delay(local_delay_ticks), None);
    }

    /// [`Self::delay`], naming `context` in the panic.
    ///
    /// # Panics
    ///
    /// As [`Self::delay`].
    #[track_caller]
    pub fn delay_at(&mut self, local_delay_ticks: u32, context: impl fmt::Display) {
        or_panic(self.try_delay(local_delay_ticks), Some(&context));
    }

    /// [`Self::Routed`] → [`Self::Delayed`], leaving the phase untouched on an out-of-order call.
    ///
    /// # Errors
    ///
    /// [`PlacementPhaseTransitionError::DelayOnNonRouted`] when the phase is not [`Self::Routed`].
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
    /// # Panics
    ///
    /// If the phase is not [`Self::Delayed`]; [`Self::try_legalize`] returns an error instead.
    #[track_caller]
    pub fn legalize(&mut self, buffer_coords: Vec<BufferCoord>) {
        or_panic(self.try_legalize(buffer_coords), None);
    }

    /// [`Self::legalize`], naming `context` in the panic.
    ///
    /// # Panics
    ///
    /// As [`Self::legalize`].
    #[track_caller]
    pub fn legalize_at(&mut self, buffer_coords: Vec<BufferCoord>, context: impl fmt::Display) {
        or_panic(self.try_legalize(buffer_coords), Some(&context));
    }

    /// [`Self::Delayed`] → [`Self::Legalized`]; an out-of-order call drops `buffer_coords`.
    ///
    /// # Errors
    ///
    /// [`PlacementPhaseTransitionError::LegalizeOnNonDelayed`] when the phase is not
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

#[track_caller]
fn or_panic(result: Result<(), PlacementPhaseTransitionError>, context: Option<&dyn fmt::Display>) {
    if let Err(error) = result {
        transition_panic(&error, context);
    }
}

/// Why a [`PlacementPhase`] transition refused: the transition attempted and the phase it found.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlacementPhaseTransitionError {
    /// [`PlacementPhase::try_route`] on a phase already routed.
    RouteOnNonUnrouted {
        /// The phase found instead of [`PlacementPhase::Unrouted`].
        current: PlacementPhase,
    },
    /// [`PlacementPhase::try_delay`] on a phase not routed, or already delayed.
    DelayOnNonRouted {
        /// The phase found instead of [`PlacementPhase::Routed`].
        current: PlacementPhase,
    },
    /// [`PlacementPhase::try_legalize`] on a phase not delayed, or already legalized.
    LegalizeOnNonDelayed {
        /// The phase found instead of [`PlacementPhase::Delayed`].
        current: PlacementPhase,
    },
}

impl PlacementPhaseTransitionError {
    /// The phase the refused transition found.
    #[must_use]
    pub const fn current(&self) -> &PlacementPhase {
        match self {
            Self::RouteOnNonUnrouted { current }
            | Self::DelayOnNonRouted { current }
            | Self::LegalizeOnNonDelayed { current } => current,
        }
    }

    /// Renders this refusal with `context` where the `*_at` panics put theirs.
    #[must_use]
    pub fn with_context<'a, C: fmt::Display + 'a>(&'a self, context: C) -> impl fmt::Display + 'a {
        TransitionMessage {
            error: self,
            context: Some(context),
        }
    }

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

/// The cardinality clause of every refusal: "exactly", since a skipped stage is a bug too.
pub(super) const TRANSITION_CARDINALITY: &str = "must run exactly once per";

/// The one rendering of a [`PlacementPhaseTransitionError`]; no context drops ` for {context}`.
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

/// Panics with the message a recovering caller's error renders.
#[track_caller]
fn transition_panic(
    error: &PlacementPhaseTransitionError,
    context: Option<&dyn fmt::Display>,
) -> ! {
    panic!("{}", TransitionMessage { error, context });
}

/// Names a placed cell or pad in a transition panic, in the pass diagnostics' wording.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CellIdentity<'a> {
    index: usize,
    coord: CellCoord,
    subject: PlacedSubject,
    scope: &'a ScopedPlacementIrEntry,
}

/// Which list a placed node is an entry of.
#[derive(Debug, Clone, Copy)]
pub(crate) enum PlacedSubject {
    /// An entry of [`PlacementIr::cells`](super::PlacementIr::cells).
    Cell,
    /// An entry of [`PlacementIr::outputs`](super::PlacementIr::outputs).
    Output,
}

impl PlacedSubject {
    const fn label(self) -> &'static str {
        match self {
            Self::Cell => "cell",
            Self::Output => "output",
        }
    }

    /// The label with its indefinite article, as a refusal to read the node names it.
    pub(super) const fn with_article(self) -> &'static str {
        match self {
            Self::Cell => "a cell",
            Self::Output => "an output",
        }
    }
}

impl<'a> CellIdentity<'a> {
    /// Breadcrumb for the cell at `index`, at `coord` in the pass's own clone of the IR.
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

    /// Breadcrumb for the actuator pad at `index`, whose coord is `pad`.
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
