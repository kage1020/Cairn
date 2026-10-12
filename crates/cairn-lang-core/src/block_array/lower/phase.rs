//! The painting phases of `spec/compilation` "Phase evaluation", in order.

/// A phase of `spec/compilation` "Phase evaluation" that paints voxels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Massing,
    Envelope,
    Openings,
    Fixtures,
}

impl Phase {
    /// How many phases paint: one past the highest [`Self::index`].
    pub(super) const COUNT: usize = 4;

    /// This phase's position in `spec/compilation` "Phase evaluation" order, from `0`.
    pub(super) const fn index(self) -> usize {
        match self {
            Self::Massing => 0,
            Self::Envelope => 1,
            Self::Openings => 2,
            Self::Fixtures => 3,
        }
    }
}
