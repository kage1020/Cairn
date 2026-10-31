//! The built-in logic library: each primitive a `logic` expression applies and its notation
//! (`spec/redstone` "Two tiers, and the v1 boundary").

use serde::{Serialize, Serializer};

/// A primitive of the built-in logic library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
// `cfg(test)` keeps the proc macro out of shipped builds.
#[cfg_attr(test, derive(strum::EnumIter))]
pub enum LogicPrimitive {
    /// `a and b`.
    And,
    /// `a or b`.
    Or,
    /// `not a`.
    Not,
}

/// How a [`LogicPrimitive`] is written in an expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Notation {
    /// Before its one operand, binding tighter than any infix primitive.
    Prefix,
    /// Between its two operands.
    Infix {
        /// How tightly it binds: a higher rank groups first, and one rank groups left to right.
        rank: u8,
    },
}

impl LogicPrimitive {
    /// Every primitive the library holds.
    pub const ALL: [Self; 3] = [Self::And, Self::Or, Self::Not];

    /// The word that names the primitive in source.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
            Self::Not => "not",
        }
    }

    /// How the primitive is written.
    #[must_use]
    pub const fn notation(self) -> Notation {
        match self {
            Self::And => Notation::Infix { rank: 1 },
            Self::Or => Notation::Infix { rank: 0 },
            Self::Not => Notation::Prefix,
        }
    }

    /// The primitive `name` names, or `None` when the library holds none.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|primitive| primitive.name() == name)
    }

    /// The rank of an infix primitive, or `None` for a prefix one.
    #[must_use]
    pub const fn infix_rank(self) -> Option<u8> {
        match self.notation() {
            Notation::Infix { rank } => Some(rank),
            Notation::Prefix => None,
        }
    }
}

impl Serialize for LogicPrimitive {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.name())
    }
}

/// The lowest infix rank the library holds above `rank`, or the lowest of all for `None`.
pub(crate) fn infix_rank_above(rank: Option<u8>) -> Option<u8> {
    LogicPrimitive::ALL
        .into_iter()
        .filter_map(LogicPrimitive::infix_rank)
        .filter(|held| rank.is_none_or(|rank| *held > rank))
        .min()
}

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::*;

    #[test]
    fn all_holds_every_primitive_once() {
        assert_eq!(
            LogicPrimitive::iter().collect::<Vec<_>>(),
            LogicPrimitive::ALL
        );
    }

    #[test]
    fn each_primitive_is_found_by_its_name_and_serialised_as_it() {
        for primitive in LogicPrimitive::iter() {
            assert_eq!(LogicPrimitive::from_name(primitive.name()), Some(primitive));
            assert_eq!(
                serde_json::to_value(primitive).expect("serialise"),
                serde_json::Value::from(primitive.name()),
            );
        }
    }

    #[test]
    fn the_ranks_climb_through_the_ones_the_library_holds() {
        assert_eq!(infix_rank_above(None), Some(0));
        assert_eq!(infix_rank_above(Some(0)), Some(1));
        assert_eq!(infix_rank_above(Some(1)), None);
    }
}
