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
/// `id` / `class` / `mat_slot` are hoisted into dedicated [`Member`] fields
/// by `intent::lower` — but only when the value is a plain label. A second
/// occurrence of the key, or a value of any other shape, stays in
/// `intent_state`, where an argument check would otherwise read it as a
/// word nobody knows. `check::type_mismatch` already reports the value; the
/// key is not the mistake.
///
/// [`Member`]: super::Member
pub const UNIVERSAL_ARGUMENTS: &[&str] = &["id", "class", "mat_slot"];

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
    /// Two directions can go wrong, and only one of them is quiet. A key a
    /// reader reads and this table omits is caught the moment any source
    /// uses it — the shipped corpus is held to `cairn check` cleanliness,
    /// and `every_argument_a_pass_reads_is_in_its_role_vocabulary` asks the
    /// question directly. A key listed here that nothing reads is the
    /// silent direction, and it is deliberate: `window shape=` is in
    /// `spec/components-editing-sites` §9.2 and no pass reads it yet, so it
    /// is accepted and reported as ignored rather than refused. The set
    /// that is listed-but-unread is [`Self::unread_arguments`].
    ///
    /// Matched with no wildcard so a new role has to be answered here
    /// rather than silently inheriting an empty vocabulary, which would
    /// report every argument written on it.
    #[must_use]
    pub fn arguments(&self) -> &'static [&'static str] {
        match self {
            // Paints the whole footprint; takes nothing but the universal
            // keys.
            Self::Floor => &[],
            Self::Walls => &["height"],
            Self::Door => &["side", "at", "opened_by"],
            Self::Window => &[
                "side", "y", "offset", "size", "sym", "repeat", "step", "shape",
            ],
            Self::Roof => &["kind", "overhang", "slope_to"],
            Self::Stair => &["kind", "side", "half", "facing", "shape"],
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
            // The keyword itself is unknown, so there is no vocabulary to
            // judge its arguments against. `check::keyword_allowlist` owns
            // the whole line.
            Self::Other(_) => &[],
        }
    }

    /// Arguments in [`Self::arguments`] that no pass reads yet.
    ///
    /// Spelled out rather than derived, because "nothing reads it" is not a
    /// fact any table can compute about itself. Each of these is a key the
    /// spec defines and the implementation has not reached: the value is
    /// carried into the IR and dropped, so the member builds without it and
    /// the author is told so rather than left to notice.
    #[must_use]
    pub fn unread_arguments(&self) -> &'static [&'static str] {
        match self {
            // `spec/components-editing-sites` §9.2 edits a window's
            // `shape=`; `fill_window` reads size, offset, repeat, step and
            // sym, and nothing consults the shape.
            Self::Window => &["shape"],
            Self::Floor
            | Self::Walls
            | Self::Door
            | Self::Roof
            | Self::Stair
            | Self::Level
            | Self::PressurePlate
            | Self::Circuit
            | Self::Place
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
    pub fn accepted_arguments(&self) -> Vec<&'static str> {
        let mut all = self.arguments().to_vec();
        all.extend_from_slice(UNIVERSAL_ARGUMENTS);
        all
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
