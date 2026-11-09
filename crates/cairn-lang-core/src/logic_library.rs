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
    /// `a xor b`.
    Xor,
    /// `a nand b`.
    Nand,
    /// `a nor b`.
    Nor,
    /// `mux(sel=s, a=x, b=y)`: `a` while `sel` is off, `b` while it is on.
    Mux,
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
    /// Called with every operand named once, in any order: `mux(sel=s, a=x, b=y)`.
    Call(CallSignature),
}

/// The name a called [`LogicPrimitive`] is called by and the operands a call names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallSignature {
    callee: &'static str,
    operands: &'static [&'static str],
}

impl CallSignature {
    /// The word the call starts with.
    #[must_use]
    pub const fn callee(self) -> &'static str {
        self.callee
    }

    /// The operands' names, in the order an [`Expr::Call`](crate::ast::Expr::Call) holds them.
    #[must_use]
    pub const fn operands(self) -> &'static [&'static str] {
        self.operands
    }

    /// The call written naming every operand, such as `mux(sel=…, a=…, b=…)`.
    #[must_use]
    pub fn template(self) -> String {
        let operands: Vec<String> = self
            .operands
            .iter()
            .map(|operand| format!("{operand}=…"))
            .collect();
        format!("{}({})", self.callee, operands.join(", "))
    }
}

impl LogicPrimitive {
    /// Every primitive the library holds.
    pub const ALL: &'static [Self] = &[
        Self::And,
        Self::Or,
        Self::Not,
        Self::Xor,
        Self::Nand,
        Self::Nor,
        Self::Mux,
    ];

    /// The word that names the primitive in source.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::And => "and",
            Self::Or => "or",
            Self::Not => "not",
            Self::Xor => "xor",
            Self::Nand => "nand",
            Self::Nor => "nor",
            Self::Mux => "mux",
        }
    }

    /// How the primitive is written.
    #[must_use]
    pub const fn notation(self) -> Notation {
        match self {
            Self::Or | Self::Nor => Notation::Infix { rank: 0 },
            Self::Xor => Notation::Infix { rank: 1 },
            Self::And | Self::Nand => Notation::Infix { rank: 2 },
            Self::Not => Notation::Prefix,
            Self::Mux => Notation::Call(CallSignature {
                callee: self.name(),
                operands: &["sel", "a", "b"],
            }),
        }
    }

    /// The primitive `name` names, or `None` when the library holds none.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|primitive| primitive.name() == name)
    }

    /// The rank of an infix primitive, or `None` for one written otherwise.
    #[must_use]
    pub const fn infix_rank(self) -> Option<u8> {
        match self.notation() {
            Notation::Infix { rank } => Some(rank),
            Notation::Prefix | Notation::Call(_) => None,
        }
    }

    /// The signature of a called primitive, or `None` for one written otherwise.
    #[must_use]
    pub const fn call_signature(self) -> Option<CallSignature> {
        match self.notation() {
            Notation::Call(signature) => Some(signature),
            Notation::Prefix | Notation::Infix { .. } => None,
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
        .iter()
        .copied()
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
    fn only_a_called_primitive_has_a_call_signature() {
        assert_eq!(
            LogicPrimitive::iter()
                .filter_map(|primitive| {
                    let signature = primitive.call_signature()?;
                    Some((primitive, signature.callee(), signature.template()))
                })
                .collect::<Vec<_>>(),
            [(
                LogicPrimitive::Mux,
                "mux",
                "mux(sel=…, a=…, b=…)".to_owned()
            )],
        );
    }

    #[test]
    fn a_call_signature_names_some_operands_each_once() {
        for signature in LogicPrimitive::iter().filter_map(LogicPrimitive::call_signature) {
            let operands = signature.operands();
            assert!(!operands.is_empty(), "{signature:?}");
            for (index, operand) in operands.iter().enumerate() {
                assert!(!operands[..index].contains(operand), "{signature:?}");
            }
        }
    }

    #[test]
    fn the_ranks_climb_through_the_ones_the_library_holds() {
        assert_eq!(
            [
                infix_rank_above(None),
                infix_rank_above(Some(0)),
                infix_rank_above(Some(1)),
                infix_rank_above(Some(2)),
            ],
            [Some(0), Some(1), Some(2), None],
        );
    }
}
