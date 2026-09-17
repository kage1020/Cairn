//! Mapping from surface command keywords to [`MemberRole`] classifications.
//!
//! Kept as a single source of truth so the `keyword_allowlist` validation
//! pass in `crate::check` can share the same table without drifting.
//!
//! The roster is drawn from the surface keywords used by the four shipped
//! examples (`cottage`, `themed-tower`, `village`, `redstone-door`) and from
//! the phase-ordered evaluation table in `spec/compilation.md` §4.1
//! (massing → envelope → openings → fixtures → logic). Any keyword outside
//! this table is intentionally surfaced as [`MemberRole::Other`] so the
//! lowering step never has to reject input.

use super::member::MemberRole;

/// All keywords known to the role table, in their declaration order.
///
/// Used by the `keyword_allowlist` diagnostic pass to render the
/// "expected one of ..." note attached to `E_UNKNOWN_KEYWORD`. Kept in
/// lock-step with [`role_of`] — the unit test below trips if the two
/// drift apart.
///
/// The declaration order doubles as the tie-break order for
/// `cairn-lang-core::suggest`'s `did you mean ...?` note: when two keywords
/// sit at the same Damerau-Levenshtein distance from the user's typo, the
/// one appearing earlier in this array wins. Re-sorting the array changes
/// which keyword surfaces on ambiguous near-misses.
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
/// Two reasons, and they answer different questions.
///
/// They are in the **candidate** set because `intent::lower` dispatches on
/// the key — `match arg.key { "id" | "class" | "mat_slot" => ... }` — so a
/// misspelled `clas=` is not that key at all and never reaches the
/// dedicated field. The repair is a word the role's own arguments do not
/// contain, and a suggestion drawn from them alone could not offer it.
///
/// They are in the **accepted** set because the hoist also requires the
/// value to be a plain label. A second occurrence of the key, or a value of
/// any other shape, stays in `intent_state`, where an argument check would
/// otherwise read it as a word nobody knows. `check::type_mismatch` already
/// reports the value; the key is not the mistake.
///
/// [`Member`]: super::Member
pub const UNIVERSAL_ARGUMENTS: &[&str] = &["id", "class", "mat_slot"];

/// A `key=` one lowering rule reads and another does not, together with the
/// sibling argument whose value picks between them.
///
/// The vocabulary's second axis. [`MemberRole::arguments`] is a flat list
/// per keyword, and it answers the question a misspelled key asks: is this
/// a word any member of this role may carry. One level down, a key that
/// *is* in the list can still be read by nothing on the line it is written
/// on, because the pass that would read it only runs for some values of
/// another argument. `roof slope_to=` is the instance: `fill_roof`
/// dispatches on `kind=` and only the `shed` arm consults the direction, so
/// `kind=gable slope_to=front` carries it into the IR and drops it — the
/// same silence a key outside the vocabulary used to build in.
///
/// A relation between two keys does not fit in a flat list, which is why
/// this is a table of its own rather than a flag on the vocabulary.
///
/// Where the boundary runs: the selector is an argument that picks a
/// *lowering rule*, and its arms are the ways of writing it that name one.
/// A key another key makes inert without selecting a rule — `window step=`,
/// which the stamp loop consults only from the second instance on, so
/// `repeat=1 step=3` drops the spacing — is a condition on a count rather
/// than on a rule, and is not this table's shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelectorAxis {
    /// The argument that selects the lowering rule.
    pub selector: &'static str,
    /// One arm per way of writing [`Self::selector`] that a lowering rule
    /// exists for, in the order the dispatch writes them.
    ///
    /// A selector value *outside* this list is deliberately absent rather
    /// than listed with an empty [`SelectorArm::reads`]: it names no rule,
    /// so the member lowers to nothing and already earns a
    /// `W_DEFERRED_MEMBER` from the pass that tried to draw it, and a
    /// second finding about the argument that rule would not have read
    /// would be a second bill for one repair.
    ///
    /// Absence is not automatically that case, which is why
    /// [`SelectorValue::Absent`] is an arm rather than the lack of one. On
    /// a `roof`, no `kind=` means no rule and the deferral owns the line;
    /// on a `place`, no `at=` is the relative-placement rule, which is
    /// where `gap=` is read.
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

    /// Whether `key` is conditional on this axis at all — some arm reads
    /// it, so some other arm may not.
    ///
    /// Derived from the arms rather than listed beside them, so a key every
    /// rule reads before the dispatch (`roof overhang=`) is simply absent
    /// from all of them. Writing it under each arm to say "not conditional"
    /// would put that claim in as many places as there are rules.
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
    /// The keys this rule reads inside itself: everything it consults once
    /// the dispatch has picked it, whether or not a sibling arm exists.
    /// Arguments read *before* the dispatch, by every rule alike, are not
    /// listed — see [`SelectorAxis::is_conditional`].
    ///
    /// On a one-arm axis that makes every key the rule reads conditional
    /// on a selector nothing else answers to, so `is_conditional` says
    /// `true` and the finding can still never fire: any other selector
    /// value names no rule, and the deferral carries that line. The row is
    /// written anyway, because the arm that lands beside it is where a key
    /// this one reads would otherwise go missing in silence.
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

impl MemberRole {
    /// The `key=` arguments this role's vocabulary contains, in the order
    /// the spec introduces them.
    ///
    /// Not the grammar's — the surface parser accepts any `key=value` on
    /// any line — and not any one pass's either. This is the closed set a
    /// member of this role may be written with, so a key outside it is a
    /// word that will be read by nothing however the passes grow, which is
    /// what makes `E_UNKNOWN_ARGUMENT` an error rather than a note.
    ///
    /// Two directions can go wrong. A key a reader reads and this table
    /// omits refuses a source the compiler is built to accept, which is
    /// what `stair y=` did between two commits of the branch that added
    /// this table; `the_table_and_the_sweep_agree_key_for_key` in
    /// `tests/check_arguments.rs` walks every keyword and compares this
    /// list against a line that writes it, in both directions. A key
    /// listed here that nothing reads is the other direction, and it is
    /// deliberate: the specification defines arguments the implementation
    /// has not reached, and refusing them would make a future lowering
    /// rule a change from error to legal. They are accepted and reported
    /// as ignored — see [`Self::unread_arguments`].
    ///
    /// Membership here is per keyword and says nothing about the line the
    /// key is written on: a key this list contains can still be read by
    /// nothing on one particular member, because a sibling argument's value
    /// chose a lowering rule that does not consult it. That is
    /// [`Self::conditional_arguments`], and it is reported as ignored too.
    ///
    /// `None` for a keyword the role table does not know — not an empty
    /// vocabulary but the absence of one, which is a different answer and
    /// the reason the two are not the same arm. A `floor` takes no
    /// arguments of its own and writing one on it is a mistake; a
    /// `torch` has no vocabulary for anything to be a mistake against,
    /// and `check::keyword_allowlist` owns the whole line.
    ///
    /// Matched with no wildcard so a new role has to be answered here
    /// rather than silently inheriting an empty vocabulary, which would
    /// report every argument written on it.
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
            // `spec/components-editing-sites` §9.3.2 and §9.3.3 fix this
            // set: a name, what to instantiate, what to resolve materials
            // against, and exactly one origin selector. §9.1 reserves
            // parameterisation, which nothing forwards today; the day it
            // lands this is the arm that opens.
            Self::Place => &["use", "theme", "at", "east_of", "north_of", "gap"],
            Self::Connect => &["path"],
            Self::Other(_) => return None,
        })
    }

    /// Arguments in [`Self::arguments`] that no pass reads yet.
    ///
    /// Spelled out rather than derived, because "nothing reads it" is not a
    /// fact any table can compute about itself. Each of these is a key the
    /// specification defines and the implementation has not reached: the
    /// value is carried into the IR and dropped, so the member builds
    /// without it and the author is told so rather than left to notice.
    ///
    /// Where the boundary runs: a spec'd key on a keyword the role table
    /// knows belongs here. A spec'd keyword the table does *not* know —
    /// `painting`, in the same worked example the three below come from —
    /// has no row for its arguments to sit in, and `E_UNKNOWN_KEYWORD`
    /// owns the whole line, the way it does for any other unknown word.
    #[must_use]
    pub fn unread_arguments(&self) -> &'static [&'static str] {
        match self {
            // `spec/entities` §8.2 writes both on a `window` member line;
            // `spec/components-editing-sites` §9.2 also sets `shape=`
            // through the edit DSL. `fill_window` reads side, y, offset,
            // size, sym, repeat and step, and consults neither of these.
            Self::Window => &["shape", "anchor"],
            // `spec/entities` §8.2, on the same `roof` line. `fill_roof`
            // reads kind, overhang and slope_to.
            Self::Roof => &["footprint", "bounds"],
            Self::Floor
            | Self::Walls
            | Self::Door
            | Self::Stair
            | Self::Level
            | Self::PressurePlate
            | Self::Circuit
            | Self::Place
            | Self::Connect
            | Self::Other(_) => &[],
        }
    }

    /// The axes on which one of this role's arguments is read by one
    /// lowering rule and not by another.
    ///
    /// Spelled out rather than derived, for the reason
    /// [`Self::unread_arguments`] is: which pass reads what is not a fact
    /// any table can compute about itself. What holds it to the dispatch it
    /// describes is `tests/conditional_arguments.rs`, which writes each
    /// pair at a value a reader would notice and lowers the source twice,
    /// with the argument and without it. An arm this table says reads the
    /// key has to build something different; an arm it says does not has to
    /// build the same voxels. Both directions fail there rather than in a
    /// silent build.
    ///
    /// A slice, because nothing says a role has only one axis: a second
    /// selector on the same keyword is another entry here rather than a
    /// second shape of table. Empty for a role whose vocabulary has no
    /// conditional key —
    /// [`Self::Other`] among them, whose whole line belongs to
    /// `check::keyword_allowlist` and whose arguments are judged against no
    /// vocabulary at all.
    #[must_use]
    pub fn conditional_arguments(&self) -> &'static [SelectorAxis] {
        match self {
            // `fill_roof` dispatches on `kind=` and hands each kind its own
            // generator. Three of the four take the inflated footprint and
            // the wall top and nothing else; `shed` is the one with a
            // direction to be told. `overhang=` is read before the
            // dispatch, by every kind, which is why it is in none of the
            // arms — the axis is not "everything after `kind=`".
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
            // `fill_stair` checks `kind=stairs` before it reads anything
            // else, so every argument it goes on to read is read under that
            // one value — which is why the arm lists all five rather than
            // the three that shape the blockstate. Nothing is reported
            // today, because a `kind=` outside the arm lowers to nothing
            // and its own `W_DEFERRED_MEMBER` carries the repair. The row
            // is the shape of the answer for the day a second stair kind
            // lands: whichever of these it does not read is asked about
            // here rather than dropped in silence.
            Self::Stair => &[SelectorAxis {
                selector: "kind",
                arms: &[SelectorArm {
                    value: SelectorValue::Ident("stairs"),
                    reads: &["side", "half", "facing", "shape", "y"],
                }],
            }],
            // `resolve_place_origin` answers `at=origin` with the world
            // origin and returns before it reads anything else, so `gap=`
            // belongs to the other rule: the relative placement a row with
            // no `at=` asks for. The two origin selectors it reads there,
            // `east_of=` and `north_of=`, are not listed — writing one
            // beside `at=` is two origin selectors, which
            // `E_INVALID_PLACE_ORIGIN` already refuses, and this would be
            // the second finding for that one repair. `gap=` is not an
            // origin selector, so nothing else has anything to say about
            // it.
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

/// Return the known-keyword table.
///
/// Public-facing helper so external passes can render the same list this
/// module uses for classification, without duplicating the constant.
#[must_use]
pub fn known_keywords() -> &'static [&'static str] {
    KNOWN_KEYWORDS
}

/// Look up a command keyword in the known-keyword table.
///
/// Returns the corresponding [`MemberRole`] for known keywords and
/// [`MemberRole::Other`] (wrapping the original keyword string) for
/// everything else. The fallback keeps the AST → IR lowering total: an
/// unknown keyword surfaces as data rather than an error and is reported by
/// the `keyword_allowlist` pass in `crate::check`.
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

    /// `MemberRole::keyword` is the inverse of [`role_of`], and
    /// diagnostics quote it back to the author. A role added here
    /// without its keyword arm would render as some other member's
    /// word, which is worse than rendering nothing.
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
