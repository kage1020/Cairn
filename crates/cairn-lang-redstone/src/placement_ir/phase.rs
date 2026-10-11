//! The phase a placed cell or actuator pad is in as the place-and-route
//! passes write to it, the `stage` tag that names that phase on the wire,
//! and the transitions between phases, each of which refuses a pass run
//! out of order.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::{BufferCoord, CellCoord, ScopedPlacementIrEntry};

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
/// equality per stage. Both directions are derived from the variant
/// names in snake case, which are the words [`Self::as_str`] spells; a
/// test walks every variant through both.
///
/// `#[non_exhaustive]` for the same reason [`PlacementPhase`] is: the
/// fifth stage (Edition legalization) has no variant yet.
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
    /// Stable lowercase string form used in the JSON wire format and
    /// accepted by `cairn synth --stage <s>`. Mirrors
    /// [`RouteLayer::as_str`], which fixes the other wire-level enum in
    /// the Placement IR to one authoritative spelling.
    ///
    /// [`RouteLayer::as_str`]: super::RouteLayer::as_str
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
pub(super) const TRANSITION_CARDINALITY: &str = "must run exactly once per";

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
///
/// [`PlacedCellNode`]: super::PlacedCellNode
/// [`PlacementIr::cells`]: super::PlacementIr::cells
/// [`RouteLayer::Plane`]: super::RouteLayer::Plane
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
