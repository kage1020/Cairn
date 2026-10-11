//! The phase a placed cell or actuator pad is in as the place-and-route
//! passes write to it, the `stage` tag that names that phase on the wire,
//! and the transitions between phases, each of which refuses a pass run
//! out of order.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::{BufferCoord, CellCoord, ScopedPlacementIrEntry};

/// Which place-and-route pass last wrote to a [`PlacedCellNode`], emitted as the `stage` key of
/// the JSON form.
///
/// Without it a consumer would infer the stage from which optional keys are present, and
/// [`PlacementPhase::Delayed`] and a [`PlacementPhase::Legalized`] with no buffers carry the same
/// keys, because an empty `buffer_coords` is skipped.
///
/// The words are the values `cairn synth --stage <s>` accepts, so a dump names the flag that
/// produced it; keep them in step with the CLI's `SynthStage`, which its tests check.
/// `#[non_exhaustive]` because edition legalization, the fifth stage, has no variant.
///
/// [`PlacedCellNode`]: super::PlacedCellNode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

/// Progressive state of a [`PlacedCellNode`] through the first four of the five stages of
/// `spec/redstone` "Place-and-route"; edition legalization has no variant.
///
/// Every legal `(wire_length, local_delay_ticks, buffer_coords)` combination is one variant:
///
/// | Producer                                             | Variant                                                                     | `stage` tag   |
/// |------------------------------------------------------|-----------------------------------------------------------------------------|---------------|
/// | [`crate::placement::compile_placement`] (Stage 1)    | [`Self::Unrouted`]                                                          | `placement`   |
/// | [`crate::routing::compile_routing`]     (Stage 2)    | [`Self::Routed`] `{ wire_length }`                                          | `route`       |
/// | [`crate::delay::compile_delay`]         (Stage 3)    | [`Self::Delayed`] `{ wire_length, local_delay_ticks }`                      | `delay`       |
/// | [`crate::crossing::compile_crossing`]   (Stage 4)    | [`Self::Legalized`] `{ wire_length, local_delay_ticks, buffer_coords }`     | `crossing`    |
///
/// The rightmost column is [`Self::stage`].
///
/// Illegal shapes, such as `local_delay_ticks` without `wire_length`, cannot be represented.
/// [`Self::route`], [`Self::delay`] and [`Self::legalize`] are the transitions, and panic when
/// called out of order; [`Self::try_route`], [`Self::try_delay`] and [`Self::try_legalize`] return
/// a [`PlacementPhaseTransitionError`] instead, for a caller to whom that is recoverable input. A
/// [`Self::Legalized`] phase may have no buffers.
///
/// `#[non_exhaustive]` so an edition-legalization variant is additive.
///
/// [`PlacedCellNode`]: super::PlacedCellNode
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
        /// The buffer repeaters the delay pass counted, empty when it counted none. See
        /// [`BufferCoord`] for the shape and order of the entries.
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

    /// Buffer coordinates once crossing legalization has run, empty otherwise; [`Self::stage`]
    /// tells a legalized phase without buffers from an earlier one.
    ///
    /// No wildcard arm, so a new variant is a compile error here rather than an empty slice that
    /// drops its data from the dump.
    #[must_use]
    pub fn buffer_coords(&self) -> &[BufferCoord] {
        match self {
            Self::Unrouted | Self::Routed { .. } | Self::Delayed { .. } => &[],
            Self::Legalized { buffer_coords, .. } => buffer_coords,
        }
    }

    /// Which pass last wrote to this cell, as the [`PlacementStage`] the JSON form carries.
    ///
    /// No wildcard arm, so a new variant cannot be labelled as an existing stage.
    #[must_use]
    pub const fn stage(&self) -> PlacementStage {
        match self {
            Self::Unrouted => PlacementStage::Placement,
            Self::Routed { .. } => PlacementStage::Route,
            Self::Delayed { .. } => PlacementStage::Delay,
            Self::Legalized { .. } => PlacementStage::Crossing,
        }
    }

    /// [`Self::Unrouted`] → [`Self::Routed`]. A pipeline pass uses [`Self::route_at`], whose panic
    /// also names the cell.
    ///
    /// # Panics
    ///
    /// Panics if the phase is not [`Self::Unrouted`]: routing runs exactly once per placement.
    /// [`Self::try_route`] returns an error instead.
    #[track_caller]
    pub fn route(&mut self, wire_length: u32) {
        or_panic(self.try_route(wire_length), None);
    }

    /// [`Self::Unrouted`] → [`Self::Routed`], naming `context` in the panic an out-of-order call
    /// raises, since `#[track_caller]` gives only the calling line.
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
    /// For a caller to whom an out-of-order call is recoverable input, such as one reading a dump;
    /// in a fresh compile it is a bug, which [`Self::route`] surfaces by panicking. A refused call
    /// leaves the phase untouched. No wildcard arm, so a new variant forces a decision on whether
    /// it can be routed.
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

    /// [`Self::Routed`] → [`Self::Delayed`]. A pipeline pass uses [`Self::delay_at`].
    ///
    /// # Panics
    ///
    /// Panics if the phase is not [`Self::Routed`]: delay insertion runs exactly once per routed
    /// IR. [`Self::try_delay`] returns an error instead.
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

    /// [`Self::Delayed`] → [`Self::Legalized`]. A pipeline pass uses [`Self::legalize_at`].
    ///
    /// # Panics
    ///
    /// Panics if the phase is not [`Self::Delayed`]: crossing legalization runs exactly once per
    /// delayed IR, so running it twice, or before delay insertion, panics rather than producing a
    /// plausible stale IR. [`Self::try_legalize`] returns an error instead.
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
    /// A refused call drops `buffer_coords` and leaves the phase as it was; clone the vector first
    /// to keep it.
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

/// Why a [`PlacementPhase`] transition refused: the transition attempted and the phase it found.
///
/// Returned by the `try_*` transitions and rendered by the panicking ones, so a recovered refusal
/// and a panic read alike. Carries the whole phase rather than its name, because a consumer
/// diagnosing a bad dump wants the values the phase got as far as. `#[non_exhaustive]` like
/// [`PlacementPhase`].
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
    /// The phase the refused transition found, readable without a match that
    /// `#[non_exhaustive]` would make need a `_` arm.
    #[must_use]
    pub const fn current(&self) -> &PlacementPhase {
        match self {
            Self::RouteOnNonUnrouted { current }
            | Self::DelayOnNonRouted { current }
            | Self::LegalizeOnNonDelayed { current } => current,
        }
    }

    /// Renders this refusal with `context` spliced in where [`PlacementPhase::route_at`] and the
    /// other `*_at` forms put theirs, so a recovering caller names what it was working on in the
    /// panics' own wording.
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

/// The cardinality clause of every refused transition's message, spelled once so every transition
/// states the same strength.
///
/// "Exactly" rather than "at most", because a transition refuses a phase that never reached its
/// source as well as a re-run: a skipped stage is as much a bug as a repeated one.
pub(super) const TRANSITION_CARDINALITY: &str = "must run exactly once per";

/// The one rendering of a [`PlacementPhaseTransitionError`], so the fallible and panicking forms
/// cannot drift apart. An absent context drops the whole ` for {context}` clause.
///
/// The context is a type parameter so [`PlacementPhaseTransitionError::with_context`] can own its
/// caller's value; the panic path uses `&dyn fmt::Display`.
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

/// Panics for an out-of-order [`PlacementPhase`] transition, with the message a recovering
/// caller's error renders. `#[track_caller]` on every layer down to here keeps the reported
/// location the pass's own.
#[track_caller]
fn transition_panic(
    error: &PlacementPhaseTransitionError,
    context: Option<&dyn fmt::Display>,
) -> ! {
    panic!("{}", TransitionMessage { error, context });
}

/// Names a placed cell or pad in the panic of an out-of-order transition, for
/// [`PlacementPhase::route_at`] and the other `*_at` forms.
///
/// A [`PlacedCellNode`] has no source-level name, so it is named by its index, coord and scope,
/// written as the pass diagnostics write them (`cell #{index}`, `({x},{y},{z})`,
/// ``{kind} `{name}` ``) so a panic and a diagnostic about one cell read alike. The layer is shown
/// only off [`RouteLayer::Plane`], where a cell never is; shown rather than asserted, because a
/// breadcrumb built for every cell of every pass must not panic itself.
///
/// [`PlacedCellNode`]: super::PlacedCellNode
/// [`RouteLayer::Plane`]: super::RouteLayer::Plane
#[derive(Debug, Clone, Copy)]
pub(crate) struct CellIdentity<'a> {
    index: usize,
    coord: CellCoord,
    subject: PlacedSubject,
    scope: &'a ScopedPlacementIrEntry,
}

/// Which list the index in a [`CellIdentity`] indexes, since both start at zero and "cell #0"
/// about an actuator pad would send the reader to the wrong line.
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
}

impl<'a> CellIdentity<'a> {
    /// Breadcrumb for the cell at `index`, whose coord is `coord`.
    ///
    /// The coord is passed in rather than read from `scope.ir.cells[index]` because a pass
    /// transitions its own clone of the IR, which `scope.ir` need not match.
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
