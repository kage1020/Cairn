//! Mapping from surface command keywords to [`MemberRole`] classifications.
//!
//! One table, so `check`'s `keyword_allowlist` pass shares it without drifting. The roster follows
//! the phase table in `spec/compilation` "Phase evaluation" (massing → envelope → openings →
//! fixtures → logic) and the keywords the shipped examples use. A keyword outside it is
//! [`MemberRole::Other`], so lowering never rejects input.

use super::member::MemberRole;

/// All keywords known to the role table, in declaration order.
///
/// `keyword_allowlist` renders the "expected one of ..." note of `E_UNKNOWN_KEYWORD` from it, and
/// the tests below keep it in step with [`role_of`].
///
/// The order is also the tie-break for the `did you mean ...?` note of `cairn-lang-core::suggest`:
/// of two keywords at the same Damerau-Levenshtein distance from a typo, the earlier wins, so
/// re-sorting changes which keyword an ambiguous near-miss suggests.
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
///
/// They are **candidates** because `intent::lower` dispatches on the key, so a misspelled `clas=`
/// never reaches the dedicated field, and its repair is a word the role's own arguments do not
/// contain.
///
/// They are **accepted** because the hoist also requires a plain label: a repeat, or a value of
/// another shape, stays in `intent_state`, where an argument check would read it as an unknown
/// word. `check::type_mismatch` reports the shape and `check::duplicate` the repeat; the key is not
/// the mistake. For the same reason one left in `intent_state` is not reported as unread on a role
/// whose [`MemberRole::unread_arguments`] lists it: the hoisted occurrence, if any, is reported
/// from its field.
pub const UNIVERSAL_ARGUMENTS: &[&str] = &["id", "class", "mat_slot"];

/// The component keywords that may carry a `-> value` sensor tail.
///
/// `spec/redstone` "Signal binding" writes an emitted signal on the component that emits it, and
/// lists `lever` / `button` / `daylight` / `observer` as sensors, none of which the surface accepts
/// yet. `pressure_plate` is the one sensor [`known_keywords()`] returns, so it is the only member a
/// tail may sit on, and recognising each of the other four costs one entry here.
///
/// Beside the argument tables rather than in `cairn-lang-redstone`, because it is a fact about the
/// member line and not the Logic IR: `cairn synth` sits behind `--experimental-logic-synth`, so the
/// rule there would leave `walls ... -> sig.w` silent through `check` and `compile`.
pub const SENSOR_HOSTS: &[&str] = &["pressure_plate"];

/// A `key=` one lowering rule reads and another does not, together with the sibling argument whose
/// value picks between them.
///
/// The vocabulary's second axis. [`MemberRole::arguments`] is a flat list per keyword, answering
/// whether a key is a word any member of the role may carry. A listed key can still be read by
/// nothing on its line, because the pass that reads it runs only for some values of another
/// argument: `fill_roof` dispatches on `kind=` and only the `shed` arm reads `slope_to=`, so `roof
/// kind=gable slope_to=front` carries the key into the IR and drops it. A relation between two keys
/// does not fit a flat list, so this is a table of its own.
///
/// The selector is an argument that picks a *lowering rule*, and its arms are the ways of writing
/// it that name one. A key another key makes inert without selecting a rule is not this table's
/// shape: `window step=` is read only from the second instance on, so `repeat=1 step=3` drops the
/// spacing on a count, not a rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorAxis {
    /// The argument that selects the lowering rule.
    pub selector: &'static str,
    /// One arm per way of writing [`Self::selector`] that a lowering rule exists for, in the order
    /// the dispatch writes them.
    ///
    /// A selector value outside this list is absent rather than listed with an empty
    /// [`SelectorArm::reads`]: it names no rule, so the member lowers to nothing and already earns
    /// a `W_DEFERRED_MEMBER`, and a finding about an argument that rule would not have read would
    /// bill one repair twice.
    ///
    /// A missing selector is not automatically that case, so [`SelectorValue::Absent`] is an arm:
    /// on a `roof`, no `kind=` means no rule and the deferral owns the line; on a `place`, no `at=`
    /// is the relative-placement rule, which reads `gap=`.
    pub arms: &'static [SelectorArm],
}

impl SelectorAxis {
    /// The arm a written selector names, or `None` when no lowering rule
    /// answers to it.
    ///
    /// `None` for the value means the selector is not on the line at all,
    /// which some axes have a rule for and others do not.
    #[must_use]
    pub fn arm(&self, value: Option<&str>) -> Option<&'static SelectorArm> {
        self.arms.iter().find(|arm| arm.value.matches(value))
    }

    /// Whether `key` is conditional on this axis at all: some arm reads it, so some other arm may
    /// not.
    ///
    /// Derived from the arms, so a key every rule reads before the dispatch (`roof overhang=`) is
    /// absent from all of them rather than written under each to say it is not conditional.
    #[must_use]
    pub fn is_conditional(&self, key: &str) -> bool {
        self.arms.iter().any(|arm| arm.reads.contains(&key))
    }

    /// The ways of writing the selector whose rule reads `key`, in table
    /// order.
    #[must_use]
    pub fn read_when(&self, key: &str) -> Vec<SelectorValue> {
        self.arms
            .iter()
            .filter(|arm| arm.reads.contains(&key))
            .map(|arm| arm.value)
            .collect()
    }
}

/// One way of writing a [`SelectorAxis`]'s selector, and the conditional
/// arguments the rule it selects reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorArm {
    /// How the selector is written to reach this rule.
    pub value: SelectorValue,
    /// The keys this rule reads once the dispatch has picked it, whether or not a sibling arm
    /// exists. Keys every rule reads before the dispatch are not listed; see
    /// [`SelectorAxis::is_conditional`].
    ///
    /// On a one-arm axis every key the rule reads is conditional on a selector nothing else answers
    /// to, so `is_conditional` says `true` and the finding still never fires: any other value names
    /// no rule, and the deferral carries that line. The row is written so that a sibling arm, once
    /// added, cannot drop a key this one reads silently.
    pub reads: &'static [&'static str],
}

/// How a [`SelectorArm`]'s selector is written on the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectorValue {
    /// The selector carries this identifier — `kind=shed`.
    Ident(&'static str),
    /// The selector is not written at all, and that is itself a rule the
    /// lowering has: a `place` with no `at=` is placed relative to another,
    /// which is the only rule that reads `gap=`.
    Absent,
}

impl SelectorValue {
    /// Whether a member that writes `written` (or nothing, for `None`)
    /// reaches this arm.
    #[must_use]
    pub fn matches(self, written: Option<&str>) -> bool {
        match (self, written) {
            (Self::Ident(value), Some(name)) => value == name,
            (Self::Absent, None) => true,
            _ => false,
        }
    }

    /// The identifier this arm answers to, or `None` for
    /// [`Self::Absent`].
    #[must_use]
    pub fn ident(self) -> Option<&'static str> {
        match self {
            Self::Ident(value) => Some(value),
            Self::Absent => None,
        }
    }
}

/// Why a key a role accepts is read by nothing on that role.
///
/// Both kinds build the member as though the key were not written, and
/// both are `W_IGNORED_ARGUMENT` on the value. They differ in what is owed
/// and so in what the author is told, which is why the reason is a value on
/// each [`UnreadArgument`] rather than a second list beside
/// [`MemberRole::unread_arguments`] that could drift from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    /// The specification defines the key on this role, and no lowering
    /// rule reads it yet. The gap is the compiler's: the key is accepted so
    /// that the rule landing is not a change from error to legal, and an
    /// author may keep it and expect no effect until then.
    Unreached,
    /// The key is one of [`UNIVERSAL_ARGUMENTS`], accepted on this role
    /// only because every role accepts it, and nothing the role puts down
    /// would read it. No lowering rule is owed, so the repair is to remove
    /// the argument.
    Inapplicable,
}

/// One entry of [`MemberRole::unread_arguments`]: a key, and why nothing on
/// the role reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnreadArgument {
    /// The key, as written before its `=`.
    pub key: &'static str,
    /// Which of the two kinds of unread key it is.
    pub why: Unread,
}

impl MemberRole {
    /// The `key=` arguments this role's vocabulary contains, in the order the spec introduces them.
    ///
    /// Not the grammar's, which accepts any `key=value` on any line, and not any one pass's: the
    /// closed set a member of this role may be written with, so a key outside it will be read by
    /// nothing however the passes grow, which is what makes `E_UNKNOWN_ARGUMENT` an error rather
    /// than a note.
    ///
    /// A key a reader reads and this table omits refuses a source the compiler is built to accept;
    /// `the_table_and_the_sweep_agree_key_for_key` in `tests/check/arguments.rs` compares this list
    /// against a line writing each keyword, in both directions. A listed key that nothing reads is
    /// deliberate: either the spec defines it and the implementation has not reached it, and
    /// refusing it would make a future lowering rule a change from error to legal, or it is one of
    /// the [`UNIVERSAL_ARGUMENTS`]. Both are accepted and reported as ignored; see
    /// [`Self::unread_arguments`].
    ///
    /// Membership is per keyword: a listed key can still be read by nothing on one member, because
    /// a sibling argument chose a rule that does not consult it. That is
    /// [`Self::conditional_arguments`], also reported as ignored.
    ///
    /// `None` for a keyword the role table does not know: not an empty vocabulary but none at all.
    /// An argument on a `floor` is a mistake; a `torch` has no vocabulary to be mistaken against,
    /// and `check::keyword_allowlist` owns its whole line.
    ///
    /// Matched with no wildcard so a new role is answered here rather than inheriting an empty
    /// vocabulary that would report every argument written on it.
    #[must_use]
    pub fn arguments(&self) -> Option<&'static [&'static str]> {
        Some(match self {
            // Paints the whole footprint; takes nothing but the universal
            // keys.
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
            // `spec/components-editing-sites` "Origin selectors" and "Cross-scope references" fix
            // this set: a name, what to instantiate, what to resolve materials against, and exactly
            // one origin selector. Parameterisation, which `spec/components-editing-sites` "`def`,
            // the component construct" reserves, would open this arm.
            Self::Place => &["use", "theme", "at", "east_of", "north_of", "gap"],
            Self::Connect => &["path"],
            Self::Other(_) => return None,
        })
    }

    /// Keys in [`Self::accepted_arguments`] that no pass reads on this role, each with the
    /// [`Unread`] reason why.
    ///
    /// Spelled out, because "nothing reads it" is not a fact a table can compute about itself. The
    /// value is carried into the IR and dropped, so the member builds without it and the author is
    /// told. An [`Unread::Unreached`] key is one the spec defines and the implementation has not
    /// reached, so a lowering rule is owed; an [`Unread::Inapplicable`] key is a universal one on a
    /// role that puts down nothing it would be read for, so none is. The finding's note is worded
    /// from which.
    ///
    /// Every `mat_slot` entry is also a slot no member of the role reads:
    /// [`super::Member::read_slot`] defines reading one from this table. The resolver does not look
    /// the name up, so a name the theme lacks refuses nothing, and the member alone does not earn a
    /// scope with no theme bound a `W_NO_THEME_BOUND`. `tests/lower/unread_mat_slot.rs` holds these
    /// entries to the lowering by building each role under two slot names that resolve to different
    /// blocks and comparing the builds.
    ///
    /// A spec'd key on a keyword the role table knows belongs here. A spec'd keyword the table does
    /// not know (`painting`, in the worked example the window and roof keys below come from) has no
    /// row, and `E_UNKNOWN_KEYWORD` owns its whole line.
    #[must_use]
    pub fn unread_arguments(&self) -> &'static [UnreadArgument] {
        match self {
            // `spec/entities` "Anchor conventions" writes both on a `window`
            // member line; `spec/components-editing-sites` "Editing model"
            // also sets `shape=` through the edit DSL. `fill_window` reads
            // side, y, offset, size, sym, repeat and step, and consults
            // neither of these.
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
            // `spec/entities` "Anchor conventions", on the same `roof` line.
            // `fill_roof` reads kind, overhang and slope_to.
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
            // A door's material is the material of the door block, whose `hinge` / `open` states
            // `spec/blockstate` "Derive by default, promote on override" keeps overridable, and
            // that block is not placed: `block_array::lower`'s `carve_door` cuts the opening and
            // records the block, with per-theme door materials, as deferred. This entry goes when
            // the block is placed.
            Self::Door => &[UnreadArgument {
                key: "mat_slot",
                why: Unread::Unreached,
            }],
            // None of these puts down a block a slot would choose; `check::material`'s
            // `without_a_material` says why for each.
            Self::Level | Self::Circuit | Self::Place | Self::Connect => &[UnreadArgument {
                key: "mat_slot",
                why: Unread::Inapplicable,
            }],
            Self::Floor | Self::Walls | Self::Stair | Self::PressurePlate | Self::Other(_) => &[],
        }
    }

    /// Why nothing on this role reads `key`, or `None` when something does
    /// — or when the role does not accept `key` at all, which
    /// [`Self::accepted_arguments`] answers.
    #[must_use]
    pub fn unread_argument(&self, key: &str) -> Option<Unread> {
        self.unread_arguments()
            .iter()
            .find(|entry| entry.key == key)
            .map(|entry| entry.why)
    }

    /// The axes on which one of this role's arguments is read by one lowering rule and not by
    /// another.
    ///
    /// Spelled out for the reason [`Self::unread_arguments`] is.
    /// `tests/lower/conditional_arguments.rs` holds it to the dispatch: it writes each pair at a
    /// value a reader would notice and lowers with and without the argument; an arm said to read
    /// the key must build something different, and one said not to must build the same voxels.
    ///
    /// A slice, since a second selector on the same keyword is another entry rather than a second
    /// table shape. Empty for a role with no conditional key, [`Self::Other`] among them, whose
    /// whole line belongs to `check::keyword_allowlist`.
    #[must_use]
    pub fn conditional_arguments(&self) -> &'static [SelectorAxis] {
        match self {
            // `fill_roof` hands each `kind=` its own generator, and only `shed` has a direction to
            // be told. `overhang=` is read before the dispatch, by every kind, so it is in no arm.
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
            // `fill_stair` checks `kind=stairs` before reading anything else, so every argument it
            // reads is read under that one value, and the arm lists all five rather than the three
            // that shape the blockstate. Nothing fires while this is the only arm, since any other
            // `kind=` lowers to nothing and carries its own `W_DEFERRED_MEMBER`; a second stair
            // kind is asked about each key it does not read rather than dropping it silently.
            Self::Stair => &[SelectorAxis {
                selector: "kind",
                arms: &[SelectorArm {
                    value: SelectorValue::Ident("stairs"),
                    reads: &["side", "half", "facing", "shape", "y"],
                }],
            }],
            // `resolve_place_anchor` answers `at=origin` with the world origin before reading
            // anything else, so `gap=` belongs to the other rule, the relative placement a row with
            // no `at=` asks for. `east_of=` and `north_of=` are not listed: beside `at=` they make
            // two origin selectors, which `E_INVALID_PLACE_ORIGIN` already refuses. `gap=` is no
            // origin selector, so this is its only finding.
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

    /// Every key a member of this role may carry, for the closed-set note
    /// and the `did you mean` candidates.
    ///
    /// The universal keys come last so the tie-break on an ambiguous typo
    /// favours the role's own vocabulary — `sid` on a `door` should answer
    /// `side`, not `id`.
    #[must_use]
    pub fn accepted_arguments(&self) -> Option<Vec<&'static str>> {
        let mut all = self.arguments()?.to_vec();
        all.extend_from_slice(UNIVERSAL_ARGUMENTS);
        Some(all)
    }
}

/// Return the known-keyword table, for passes outside this module that render the same list.
#[must_use]
pub fn known_keywords() -> &'static [&'static str] {
    KNOWN_KEYWORDS
}

/// Look up a command keyword in the known-keyword table.
///
/// Returns the [`MemberRole`] for a known keyword and [`MemberRole::Other`] wrapping it otherwise,
/// which keeps lowering total: an unknown keyword is data, reported by `check`'s
/// `keyword_allowlist` pass.
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

    /// Diagnostics quote `MemberRole::keyword` back to the author, so a role whose arm spells
    /// another member's word would misreport the line.
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
