//! Surface command keywords, the [`MemberRole`] each classifies to, and each role's arguments.

use serde::Serialize;

/// All keywords known to the role table; the order breaks ties between equally near suggestions.
pub const KNOWN_KEYWORDS: &[&str] = &[
    "floor",
    "walls",
    "fill",
    "door",
    "window",
    "roof",
    "stair",
    "beam",
    "level",
    "pressure_plate",
    "lever",
    "button",
    "daylight",
    "observer",
    "lamp",
    "piston",
    "dispenser",
    "chest",
    "trapdoor",
    "note_block",
    "repeater",
    "comparator",
    "redstone_torch",
    "redstone_dust",
    "circuit",
    "raw_block",
    "raw_fill",
    "place",
    "connect",
];

/// Argument keys every member accepts, whatever its role.
pub const UNIVERSAL_ARGUMENTS: &[&str] = &["id", "class", "mat_slot"];

/// Keywords that may carry a `-> value` sensor tail (`spec/redstone` "Signal binding").
pub const SENSOR_HOSTS: &[&str] = &["lever", "button", "daylight", "observer", "pressure_plate"];

/// A component a signal drives, read from one key of its line (`spec/redstone` "Signal binding").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Actuator {
    /// `door`, read from `opened_by=`.
    Door,
    /// `piston`, read from `powered_by=`.
    Piston,
    /// `lamp`, read from `lit_by=`.
    Lamp,
    /// `dispenser`, read from `fired_by=`.
    Dispenser,
}

impl Actuator {
    /// Every actuator, in the order `spec/redstone` "Signal binding" lists their keys.
    pub const ALL: &'static [Self] = &[Self::Door, Self::Piston, Self::Lamp, Self::Dispenser];

    /// The keyword its line starts with.
    #[must_use]
    pub const fn keyword(self) -> &'static str {
        match self {
            Self::Door => "door",
            Self::Piston => "piston",
            Self::Lamp => "lamp",
            Self::Dispenser => "dispenser",
        }
    }

    /// The key its line reads the signal from.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Door => "opened_by",
            Self::Piston => "powered_by",
            Self::Lamp => "lit_by",
            Self::Dispenser => "fired_by",
        }
    }

    /// What the signal does to it, as a finding says it: `("opens", "open")` completes both "the
    /// signal that opens the door" and "for it to open".
    #[must_use]
    pub const fn verbs(self) -> (&'static str, &'static str) {
        match self {
            Self::Door => ("opens", "open"),
            Self::Piston => ("powers", "power"),
            Self::Lamp => ("lights", "light"),
            Self::Dispenser => ("fires", "fire"),
        }
    }

    /// The actuator whose line reads `key`, or `None` for a key no actuator reads.
    #[must_use]
    pub fn reading(key: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|actuator| actuator.key() == key)
    }
}

/// Coarse classification of a member, derived from its source keyword.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", content = "value")]
pub enum MemberRole {
    /// `floor` command.
    Floor,
    /// `walls` command.
    Walls,
    /// `fill` command.
    Fill,
    /// `door` command.
    Door,
    /// `window` command.
    Window,
    /// `roof` command.
    Roof,
    /// `stair` command.
    Stair,
    /// `beam` command, a run of pillar blocks laid along a wall top or over an opening.
    Beam,
    /// `level` grouping block.
    Level,
    /// `pressure_plate` sensor.
    PressurePlate,
    /// `lever` sensor.
    Lever,
    /// `button` sensor.
    Button,
    /// `daylight` sensor, a daylight detector.
    Daylight,
    /// `observer` sensor.
    Observer,
    /// `lamp` actuator, a redstone lamp.
    Lamp,
    /// `piston` actuator.
    Piston,
    /// `dispenser` actuator.
    Dispenser,
    /// `chest` fixture.
    Chest,
    /// `trapdoor` fixture.
    Trapdoor,
    /// `note_block` fixture, placed at a coordinate triple.
    NoteBlock,
    /// `repeater` Tier 0 part.
    Repeater,
    /// `comparator` Tier 0 part.
    Comparator,
    /// `redstone_torch` Tier 0 part, a standing or a wall torch.
    RedstoneTorch,
    /// `redstone_dust` Tier 0 part.
    RedstoneDust,
    /// `circuit` redstone region marker.
    Circuit,
    /// `raw_block` escape hatch: one block id in one cell.
    RawBlock,
    /// `raw_fill` escape hatch: one block id in every cell of a box.
    RawFill,
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
    /// The actuator a member of this role is, or `None` for a role no signal drives.
    ///
    /// Every role is spelled out, so a new one fails the compile here.
    #[must_use]
    pub const fn actuator(&self) -> Option<Actuator> {
        match self {
            Self::Door => Some(Actuator::Door),
            Self::Piston => Some(Actuator::Piston),
            Self::Lamp => Some(Actuator::Lamp),
            Self::Dispenser => Some(Actuator::Dispenser),
            Self::Floor
            | Self::Walls
            | Self::Fill
            | Self::Window
            | Self::Roof
            | Self::Stair
            | Self::Beam
            | Self::Level
            | Self::PressurePlate
            | Self::Lever
            | Self::Button
            | Self::Daylight
            | Self::Observer
            | Self::Chest
            | Self::Trapdoor
            | Self::NoteBlock
            | Self::Repeater
            | Self::Comparator
            | Self::RedstoneTorch
            | Self::RedstoneDust
            | Self::Circuit
            | Self::RawBlock
            | Self::RawFill
            | Self::Place
            | Self::Connect
            | Self::Other(_) => None,
        }
    }

    /// Whether this is a role of the escape-hatch layer, a raw member or a Tier 0 part, the only
    /// kind an `@edition` guard holds.
    #[must_use]
    pub fn is_escape_hatch(&self) -> bool {
        matches!(self, Self::RawBlock | Self::RawFill) || self.is_tier0_part()
    }

    /// Whether this is a Tier 0 part, a redstone part placed by hand (`spec/redstone` "Tier 0
    /// parts").
    #[must_use]
    pub fn is_tier0_part(&self) -> bool {
        matches!(
            self,
            Self::Repeater | Self::Comparator | Self::RedstoneTorch | Self::RedstoneDust
        )
    }

    /// The `key=` keys this role's own vocabulary holds, in spec order, its blockstate keys and
    /// spec'd keys nothing reads yet included; `None` for an unknown keyword.
    #[must_use]
    pub fn own_keys(&self) -> Option<&'static [&'static str]> {
        Some(match self {
            Self::Floor => &[],
            Self::Walls => &["height"],
            Self::Fill => &["from", "to", "fluid", "kind"],
            Self::Door => &["side", "at", "opened_by", "hinge", "open"],
            Self::Window => &[
                "side", "y", "offset", "size", "sym", "repeat", "step", "shape", "anchor",
            ],
            Self::Roof => &["kind", "overhang", "slope_to", "footprint", "bounds"],
            Self::Stair => &[
                "kind",
                "side",
                "half",
                "facing",
                "shape",
                "y",
                "waterlogged",
            ],
            Self::Beam => &["kind", "at", "axis"],
            Self::Level => &["y"],
            Self::PressurePlate | Self::Lever | Self::Button | Self::Daylight => {
                &["at", "offset", "y"]
            }
            Self::Observer | Self::RedstoneTorch => &["at", "offset", "y", "facing"],
            Self::Lamp => &["at", "offset", "y", "lit_by"],
            Self::Piston => &["at", "offset", "y", "powered_by", "facing", "sticky"],
            Self::Dispenser => &["at", "offset", "y", "fired_by", "facing"],
            Self::Chest => &["at", "offset", "y", "facing", "size", "waterlogged"],
            Self::Trapdoor => &["at", "offset", "y", "facing", "half", "open", "waterlogged"],
            Self::NoteBlock => &["at", "note", "instrument"],
            Self::Repeater => &["at", "offset", "y", "facing", "delay"],
            Self::Comparator => &["at", "offset", "y", "facing", "mode"],
            Self::RedstoneDust => &["at", "offset", "y", "connect"],
            Self::Circuit => &["region", "void"],
            Self::RawBlock => &["mat", "at"],
            Self::RawFill => &["mat", "from", "to"],
            // `spec/components-editing-sites` "Origin selectors" and "Cross-scope references"
            // fix this set.
            Self::Place => &["use", "theme", "at", "east_of", "north_of", "gap"],
            Self::Connect => &["path"],
            Self::Other(_) => return None,
        })
    }

    /// The keys of [`Self::own_keys`] that name a blockstate of the block this role places
    /// (`spec/blockstate` "Derive by default, promote on override").
    #[must_use]
    pub fn state_arguments(&self) -> &'static [&'static str] {
        match self {
            Self::Stair => &["half", "facing", "shape", "waterlogged"],
            Self::Door => &["hinge", "open"],
            Self::Chest => &["facing", "size", "waterlogged"],
            Self::Beam => &["axis"],
            Self::Trapdoor => &["facing", "half", "open", "waterlogged"],
            Self::NoteBlock => &["note", "instrument"],
            Self::Observer | Self::Piston | Self::Dispenser | Self::RedstoneTorch => &["facing"],
            Self::Repeater => &["facing", "delay"],
            Self::Comparator => &["facing", "mode"],
            Self::RedstoneDust => &["connect"],
            Self::Floor
            | Self::Walls
            | Self::Fill
            | Self::Window
            | Self::Roof
            | Self::Level
            | Self::PressurePlate
            | Self::Lever
            | Self::Button
            | Self::Daylight
            | Self::Lamp
            | Self::Circuit
            | Self::RawBlock
            | Self::RawFill
            | Self::Place
            | Self::Connect
            | Self::Other(_) => &[],
        }
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
            Self::Lever
            | Self::Daylight
            | Self::Observer
            | Self::Lamp
            | Self::Piston
            | Self::Dispenser
            | Self::NoteBlock
            | Self::Repeater
            | Self::Comparator
            | Self::RedstoneTorch
            | Self::RedstoneDust
            | Self::Level
            | Self::Circuit
            | Self::RawBlock
            | Self::RawFill
            | Self::Place
            | Self::Connect => &[UnreadArgument {
                key: "mat_slot",
                why: Unread::Inapplicable,
            }],
            Self::Floor
            | Self::Walls
            | Self::Fill
            | Self::Door
            | Self::Stair
            | Self::Beam
            | Self::PressurePlate
            | Self::Button
            | Self::Chest
            | Self::Trapdoor
            | Self::Other(_) => &[],
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
                    reads: &["side", "half", "facing", "shape", "y", "waterlogged"],
                }],
            }],
            Self::Fill => &[SelectorAxis {
                selector: "fluid",
                arms: &[
                    SelectorArm {
                        value: SelectorValue::Ident("water"),
                        reads: &["kind"],
                    },
                    SelectorArm {
                        value: SelectorValue::Absent,
                        reads: &[],
                    },
                ],
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
            | Self::Beam
            | Self::Level
            | Self::PressurePlate
            | Self::Lever
            | Self::Button
            | Self::Daylight
            | Self::Observer
            | Self::Lamp
            | Self::Piston
            | Self::Dispenser
            | Self::Chest
            | Self::Trapdoor
            | Self::NoteBlock
            | Self::Repeater
            | Self::Comparator
            | Self::RedstoneTorch
            | Self::RedstoneDust
            | Self::Circuit
            | Self::RawBlock
            | Self::RawFill
            | Self::Connect
            | Self::Other(_) => &[],
        }
    }

    /// Every key a member of this role may carry, universal keys last so a typo's tie-break
    /// favours the role's own (`sid` on a `door` is `side`, not `id`).
    #[must_use]
    pub fn accepted_arguments(&self) -> Option<Vec<&'static str>> {
        let mut all = self.own_keys()?.to_vec();
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
        "fill" => MemberRole::Fill,
        "door" => MemberRole::Door,
        "window" => MemberRole::Window,
        "roof" => MemberRole::Roof,
        "stair" => MemberRole::Stair,
        "beam" => MemberRole::Beam,
        "level" => MemberRole::Level,
        "pressure_plate" => MemberRole::PressurePlate,
        "lever" => MemberRole::Lever,
        "button" => MemberRole::Button,
        "daylight" => MemberRole::Daylight,
        "observer" => MemberRole::Observer,
        "lamp" => MemberRole::Lamp,
        "piston" => MemberRole::Piston,
        "dispenser" => MemberRole::Dispenser,
        "chest" => MemberRole::Chest,
        "trapdoor" => MemberRole::Trapdoor,
        "note_block" => MemberRole::NoteBlock,
        "repeater" => MemberRole::Repeater,
        "comparator" => MemberRole::Comparator,
        "redstone_torch" => MemberRole::RedstoneTorch,
        "redstone_dust" => MemberRole::RedstoneDust,
        "circuit" => MemberRole::Circuit,
        "raw_block" => MemberRole::RawBlock,
        "raw_fill" => MemberRole::RawFill,
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
