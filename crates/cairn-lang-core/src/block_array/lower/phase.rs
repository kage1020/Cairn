//! The painting phases of `spec/compilation` "Phase evaluation", in order.

/// A phase of `spec/compilation` "Phase evaluation" that paints voxels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    Massing,
    Envelope,
    Openings,
    Fixtures,
    Fluids,
    Raw,
}

impl Phase {
    /// How many phases paint: one past the highest [`Self::index`].
    pub(super) const COUNT: usize = 6;

    /// Every phase, in `spec/compilation` "Phase evaluation" order.
    pub(super) const ALL: [Self; Self::COUNT] = [
        Self::Massing,
        Self::Envelope,
        Self::Openings,
        Self::Fixtures,
        Self::Fluids,
        Self::Raw,
    ];

    /// This phase's position in `spec/compilation` "Phase evaluation" order, from `0`.
    pub(super) const fn index(self) -> usize {
        match self {
            Self::Massing => 0,
            Self::Envelope => 1,
            Self::Openings => 2,
            Self::Fixtures => 3,
            Self::Fluids => 4,
            Self::Raw => 5,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Phase;

    #[test]
    fn every_phase_stands_at_its_own_index() {
        for (at, phase) in Phase::ALL.into_iter().enumerate() {
            assert_eq!(phase.index(), at, "{phase:?}");
        }
    }
}
