//! Surface command keywords, the [`MemberRole`] each classifies to, and each role's arguments.

use serde::Serialize;

/// All keywords known to the role table; the order breaks ties between equally near suggestions.
pub const KNOWN_KEYWORDS: &[&str] = &[
    "floor",
    "walls",
    "door",
    "window",
    "roof",
    "stair",
    "level",
    "pressure_plate",
    "circuit",
    "place",
    "connect",
];

/// Argument keys every member accepts, whatever its role.
pub const UNIVERSAL_ARGUMENTS: &[&str] = &["id", "class", "mat_slot"];

/// Keywords that may carry a `-> value` sensor tail (`spec/redstone` "Signal binding").
pub const SENSOR_HOSTS: &[&str] = &["pressure_plate"];

/// Each actuator argument key of `spec/redstone` "Signal binding", paired with the component
/// keyword that carries it.
pub const ACTUATOR_BINDINGS: &[(&str, &str)] = &[
    ("opened_by", "door"),
    ("powered_by", "piston"),
    ("lit_by", "lamp"),
    ("fired_by", "dispenser"),
];

/// Coarse classification of a member, derived from its source keyword.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value")]
pub enum MemberRole {
    /// `floor` command.
    Floor,
    /// `walls` command.
    Walls,
    /// `door` command.
    Door,
    /// `window` command.
    Window,
    /// `roof` command.
    Roof,
    /// `stair` command.
    Stair,
    /// `level` grouping block.
    Level,
    /// `pressure_plate` fixture.
    PressurePlate,
    /// `circuit` redstone region marker.
    Circuit,
    /// `place` site placement.
    Place,
    /// `connect` site topology edge.
    Connect,
    /// Any keyword the role table does not know.
    Other(String),
}

/// An argument whose value picks a lowering rule, and which conditional keys each rule reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorAxis {
    /// The argument that selects the lowering rule.
    pub selector: &'static str,
    /// One arm per selector value that names a lowering rule; leave out a value naming none, whose
    /// line already gets a refusal (`E_INVALID_MEMBER`, or `E_INVALID_PLACE_ORIGIN` on a `place`).
    pub arms: &'static [SelectorArm],
}

impl SelectorAxis {
    /// The arm a selector written as `value` (`None`: not written) names, if any.
    #[must_use]
    pub fn arm(&self, value: Option<&str>) -> Option<&'static SelectorArm> {
        self.arms.iter().find(|arm| arm.value.matches(value))
    }

    /// Whether some arm reads `key`, so some other arm may not.
    #[must_use]
    pub fn is_conditional(&self, key: &str) -> bool {
        self.arms.iter().any(|arm| arm.reads.contains(&key))
    }

    /// The ways of writing the selector whose rule reads `key`, in table order.
    #[must_use]
    pub fn read_when(&self, key: &str) -> Vec<SelectorValue> {
        self.arms
            .iter()
            .filter(|arm| arm.reads.contains(&key))
            .map(|arm| arm.value)
            .collect()
    }
}

/// One selector value of a [`SelectorAxis`], and the conditional arguments its rule reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorArm {
    /// How the selector is written to reach this rule.
    pub value: SelectorValue,
    /// Keys this rule reads after the dispatch; ones every rule reads before it are left out.
    pub reads: &'static [&'static str],
}

/// How a [`SelectorArm`]'s selector is written on the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorValue {
    /// The selector carries this identifier — `kind=shed`.
    Ident(&'static str),
    /// The selector is not written, and that selects a rule of its own (`place` with no `at=`).
    Absent,
}

impl SelectorValue {
    /// Whether a member that writes `written` (`None`: nothing) reaches this arm.
    #[must_use]
    pub fn matches(self, written: Option<&str>) -> bool {
        match (self, written) {
            (Self::Ident(value), Some(name)) => value == name,
            (Self::Absent, None) => true,
            _ => false,
        }
    }

    /// The identifier this arm answers to, or `None` for [`Self::Absent`].
    #[must_use]
    pub fn ident(self) -> Option<&'static str> {
        match self {
            Self::Ident(value) => Some(value),
            Self::Absent => None,
        }
    }
}

/// Why a key a role accepts is read by nothing on that role, which words the finding's note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    /// The spec defines the key on this role and no lowering rule reads it yet.
    Unreached,
    /// A [`UNIVERSAL_ARGUMENTS`] key that nothing the role puts down would read.
    Inapplicable,
}

/// One entry of [`MemberRole::unread_arguments`]: a key, and why nothing on the role reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnreadArgument {
    /// The key, as written before its `=`.
    pub key: &'static str,
    /// Which of the two kinds of unread key it is.
    pub why: Unread,
}

impl MemberRole {
    /// The `key=` arguments this role's vocabulary holds, in spec order, including spec'd keys
    /// nothing reads yet; `None` for an unknown keyword.
    #[must_use]
    pub fn arguments(&self) -> Option<&'static [&'static str]> {
        Some(match self {
            Self::Floor => &[],
            Self::Walls => &["height"],
            Self::Door => &["side", "at", "opened_by"],
            Self::Window => &[
                "side", "y", "offset", "size", "sym", "repeat", "step", "shape", "anchor",
            ],
            Self::Roof => &["kind", "overhang", "slope_to", "footprint", "bounds"],
            Self::Stair => &["kind", "side", "half", "facing", "shape", "y"],
            Self::Level => &["y"],
            Self::PressurePlate => &["at", "offset", "y"],
            Self::Circuit => &["region", "void"],
            // `spec/components-editing-sites` "Origin selectors" and "Cross-scope references"
            // fix this set.
            Self::Place => &["use", "theme", "at", "east_of", "north_of", "gap"],
            Self::Connect => &["path"],
            Self::Other(_) => return None,
        })
    }

    /// Keys in [`Self::accepted_arguments`] that no pass reads on this role, with the reason; a
    /// `mat_slot` entry also stops [`super::Member::read_slot`] reading the slot.
    #[must_use]
    pub fn unread_arguments(&self) -> &'static [UnreadArgument] {
        match self {
            // Both from `spec/entities` "Anchor conventions".
            Self::Window => &[
                UnreadArgument {
                    key: "shape",
                    why: Unread::Unreached,
                },
                UnreadArgument {
                    key: "anchor",
                    why: Unread::Unreached,
                },
            ],
            // Both from `spec/entities` "Anchor conventions".
            Self::Roof => &[
                UnreadArgument {
                    key: "footprint",
                    why: Unread::Unreached,
                },
                UnreadArgument {
                    key: "bounds",
                    why: Unread::Unreached,
                },
            ],
            // The door block itself is not placed yet; this entry goes when it is.
            Self::Door => &[UnreadArgument {
                key: "mat_slot",
                why: Unread::Unreached,
            }],
            Self::Level | Self::Circuit | Self::Place | Self::Connect => &[UnreadArgument {
                key: "mat_slot",
                why: Unread::Inapplicable,
            }],
            Self::Floor | Self::Walls | Self::Stair | Self::PressurePlate | Self::Other(_) => &[],
        }
    }

    /// Why nothing on this role reads `key`, or `None` if something does or it is not accepted.
    #[must_use]
    pub fn unread_argument(&self, key: &str) -> Option<Unread> {
        self.unread_arguments()
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.why)
    }

    /// Axes on which one rule reads an argument of this role and another rule does not.
    #[must_use]
    pub fn conditional_arguments(&self) -> &'static [SelectorAxis] {
        match self {
            Self::Roof => &[SelectorAxis {
                selector: "kind",
                arms: &[
                    SelectorArm {
                        value: SelectorValue::Ident("gable"),
                        reads: &[],
                    },
                    SelectorArm {
                        value: SelectorValue::Ident("shed"),
                        reads: &["slope_to"],
                    },
                    SelectorArm {
                        value: SelectorValue::Ident("hip"),
                        reads: &[],
                    },
                    SelectorArm {
                        value: SelectorValue::Ident("flat"),
                        reads: &[],
                    },
                ],
            }],
            // Lists every key `fill_stair` reads, so a second stair kind cannot drop one silently.
            Self::Stair => &[SelectorAxis {
                selector: "kind",
                arms: &[SelectorArm {
                    value: SelectorValue::Ident("stairs"),
                    reads: &["side", "half", "facing", "shape", "y"],
                }],
            }],
            // `east_of=` / `north_of=` beside `at=` are already `E_INVALID_PLACE_ORIGIN`.
            Self::Place => &[SelectorAxis {
                selector: "at",
                arms: &[
                    SelectorArm {
                        value: SelectorValue::Ident("origin"),
                        reads: &[],
                    },
                    SelectorArm {
                        value: SelectorValue::Absent,
                        reads: &["gap"],
                    },
                ],
            }],
            Self::Floor
            | Self::Walls
            | Self::Door
            | Self::Window
            | Self::Level
            | Self::PressurePlate
            | Self::Circuit
            | Self::Connect
            | Self::Other(_) => &[],
        }
    }

    /// Every key a member of this role may carry, universal keys last so a typo's tie-break
    /// favours the role's own (`sid` on a `door` is `side`, not `id`).
    #[must_use]
    pub fn accepted_arguments(&self) -> Option<Vec<&'static str>> {
        let mut all = self.arguments()?.to_vec();
        all.extend_from_slice(UNIVERSAL_ARGUMENTS);
        Some(all)
    }
}

/// The known-keyword table.
#[must_use]
pub fn known_keywords() -> &'static [&'static str] {
    KNOWN_KEYWORDS
}

/// The role a command keyword classifies to; an unknown one is [`MemberRole::Other`].
#[must_use]
pub fn role_of(keyword: &str) -> MemberRole {
    match keyword {
        "floor" => MemberRole::Floor,
        "walls" => MemberRole::Walls,
        "door" => MemberRole::Door,
        "window" => MemberRole::Window,
        "roof" => MemberRole::Roof,
        "stair" => MemberRole::Stair,
        "level" => MemberRole::Level,
        "pressure_plate" => MemberRole::PressurePlate,
        "circuit" => MemberRole::Circuit,
        "place" => MemberRole::Place,
        "connect" => MemberRole::Connect,
        other => MemberRole::Other(other.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::{KNOWN_KEYWORDS, MemberRole, role_of};

    #[test]
    fn known_keywords_round_trip_to_concrete_roles() {
        for kw in KNOWN_KEYWORDS {
            let role = role_of(kw);
            assert!(
                !matches!(role, MemberRole::Other(_)),
                "`{kw}` should classify to a concrete MemberRole, got {role:?}",
            );
        }
    }

    #[test]
    fn unknown_keyword_falls_through_to_other() {
        assert_eq!(role_of("mystery"), MemberRole::Other("mystery".to_owned()));
    }

    #[test]
    fn every_known_keyword_survives_the_round_trip_through_its_role() {
        for kw in KNOWN_KEYWORDS {
            assert_eq!(
                role_of(kw).keyword(),
                *kw,
                "`{kw}` should come back out of its role unchanged",
            );
        }
        assert_eq!(role_of("mystery").keyword(), "mystery");
    }
}
