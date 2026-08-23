//! [`Diagnostic`] payload used by every `check` pass.

use std::num::NonZeroU32;

use serde::{Serialize, Serializer};

use crate::error::{Position, Span};

/// Severity of a single [`Diagnostic`].
///
/// `Error` participates in the `cairn check` exit code (any error → exit 1);
/// `Warning` does not. Stable per `spec/lint.md` §11.3: errors are things
/// that, left alone, cause unintended results; warnings are advisory drift.
/// Both variants ship in the public enum so a new `Warning` code can land
/// without changing the discriminant a downstream matcher already pinned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// User-impacting problem — `cairn check` exits non-zero.
    Error,
    /// Advisory finding — emitted but does not change the exit code.
    Warning,
}

impl Severity {
    /// Lowercase rendering used in the gcc-style text format.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Warning => "warning",
        }
    }
}

/// Stable identifier for a kind of [`Diagnostic`].
///
/// The string form (`E_DUPLICATE_SIZE`, `E_UNKNOWN_KEYWORD`, ...) is the
/// contract surface: downstream tooling matches on it without inspecting
/// the prose `primary` message. Marked `#[non_exhaustive]` so adding new
/// codes while the diagnostic surface is still **Evolving** does not break
/// callers' exhaust matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
// `EnumIter` exists so the in-crate tests can walk every variant instead of
// re-listing them by hand — a hand-written list silently omits whatever was
// added last, which is exactly the case the tests are there to catch. It is
// `cfg(test)` so no proc-macro dependency reaches a shipped build.
#[cfg_attr(test, derive(strum::EnumIter))]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// More than one `size=` argument in a struct or def header.
    DuplicateSize,
    /// Repeated `slot NAME` line in a single `theme` block.
    DuplicateSlot,
    /// Two selector rows in one `theme` block select the same members and
    /// bind the same key.
    ///
    /// A member's selector bindings are the merge of every row it matches,
    /// taken in source order, so a key two rows bind keeps the later
    /// value. When the rows carry the same keyword and the same attributes
    /// they match member for member, which leaves no member anywhere that
    /// reads the earlier binding.
    ///
    /// Two shapes are not covered. Rows binding *different* keys compose,
    /// the way `@requires` floors do — every binding reaches every member
    /// both rows select. And rows whose attributes merely overlap
    /// (`window[class=small]` against `window[class=small,side=front]`) do
    /// not coincide: a member the wider row selects alone still reads its
    /// binding. Which of two overlapping rows wins is the cascade, and the
    /// cascade is source order by design.
    DuplicateSelector,
    /// Repeated `key=` in the same argument list (struct/def header,
    /// statement args, selector attrs / bindings).
    DuplicateArg,
    /// Two or more members in the same immediate body share an `id=`.
    DuplicateId,
    /// Two or more top-level items of the same kind share a name.
    ///
    /// The four kinds occupy separate namespaces (the resolver keys
    /// them `struct::NAME`, `def::NAME`, `site::NAME::PLACE_ID`, and a
    /// themes map of its own), so `theme x` alongside `struct x` is not
    /// a collision — only a repeat within one kind is. The message names
    /// the keyword, and the note differs by kind: for the three whose
    /// name is the binding key the second declaration binds nothing,
    /// while two `site` blocks of one name merge into a shared place
    /// namespace instead of shadowing.
    DuplicateItem,
    /// A single-valued `@directive` header appears more than once.
    ///
    /// `@cairn` and `@intended_targets` each answer a question with one
    /// answer — which language version the file was written against,
    /// which targets it was designed for. Neither has a consumer in the
    /// compiler yet, so nothing would decide between two of them.
    /// `@requires` is not covered: its floors compose, folding to the
    /// strictest across every line, so a second one adds a constraint
    /// rather than displacing the first.
    DuplicateHeader,
    /// A member carries an indented body that nothing reads.
    ///
    /// The surface grammar hangs a body off every command, but only
    /// `level y=N` inside a `struct` or `def` has a reader — the
    /// block-array pass unwraps it so the children join the phase
    /// buckets. Every other body is dropped before lowering, and a
    /// `site` body has no grouping construct at all.
    UnsupportedNesting,
    /// A statement keyword that is in the known-keyword table but whose
    /// enclosing body has no reader for it.
    ///
    /// The surface grammar accepts every keyword in every body, and the
    /// role table is global, so `place` in a `struct` body and `floor` in
    /// a `site` body both parse and classify. Neither reaches the build:
    /// the geometry passes bucket by role and the site passes match only
    /// `place` / `connect`, so the member is dropped between the IR and
    /// the voxels.
    MisplacedMember,
    /// A statement keyword not in the known-keyword table.
    UnknownKeyword,
    /// A statement carrying bare positional values in a form that takes
    /// none.
    ///
    /// Spec §5.1 requires `key=value` for everything after the command
    /// keyword; `connect FROM.PORT to TO.PORT` is the single exception and
    /// is checked by `E_CONNECT_ARITY` instead. The line-based parser
    /// collects any bare token into `positional` and every reader but
    /// `connect`'s ignores that list, so a dropped `=` (`height 3`) or a
    /// spec-forbidden positional form (`window front G 2 2 2x2`) changes
    /// the build without a word.
    UnexpectedPositional,
    /// An `@requires` expression that is not a version floor.
    ///
    /// The directive declares one constraint and nothing else, so an
    /// expression the compiler cannot read declares nothing — and the
    /// author cannot tell, because the line is still in the file. The
    /// grammar is `version>=X` and only that: a floor composes with other
    /// floors by taking the strictest, which no other operator does
    /// (spec syntax §5.3, versioning-editions §10.4).
    ///
    /// Not to be confused with `E_REQUIRES_CONFLICT`, which the spec
    /// reserves for a declared floor contradicting the registry-*inferred*
    /// range. No such range is derived yet, so that code has nothing to
    /// compare against and is not defined here.
    InvalidRequires,
    /// A label-typed key whose value is not a label (identifier or
    /// string): `id=`, `class=`, `mat_slot=`, `use=`, or `theme=`.
    ///
    /// The five are one code because every reader lifts them the same
    /// way, through `Value::as_label_str`, and answers `None` the same
    /// way. For `use=` / `theme=` that `None` is indistinguishable at the
    /// resolver from the key being absent — and both are errors, reported
    /// by different codes: an absent key is `E_INCOMPLETE_PLACE`, a key
    /// that is on the line but not usable is this one. Telling them apart
    /// is what keeps the message from asking the author to add a key they
    /// already wrote.
    TypeMismatchLabel,
    /// `size=` whose value is not a `WxH` literal.
    TypeMismatchSize,
    /// `mat_slot=NAME` references a slot the applied theme does not declare.
    UnresolvedSlot,
    /// `slot NAME -> VALUE` whose VALUE is neither a canonical nor an
    /// abstract material token (see `spec/materials-themes.md` §7.2).
    ///
    /// Error rather than advisory: the slot binds to nothing, so every
    /// `mat_slot=NAME` pointing at it lowers to air. A theme whose slots
    /// are all mistyped builds a hollow shell of the requested extent —
    /// the "implicit dropping" `spec/lint.md` §11.3 forbids, and not
    /// something the author can see in an exit code that stayed 0.
    UnknownSlotTarget,
    /// `theme` selector rule that does not match any member in the file.
    ///
    /// Warning, unlike its `E_`-prefixed neighbour above: a rule that
    /// matches nothing overrides nothing, so every member keeps the
    /// material it would have had with the rule deleted. Nothing is
    /// dropped and the build is exactly what the rest of the source
    /// asked for — the finding is about the author's intent, which is
    /// the advisory half of `spec/lint.md` §11.3.
    ThemeSelectorUnmatched,
    /// A member role the block-array lowering pass does not yet handle
    /// (door/window/roof/...). Surfaces during `cairn lower` so a partial
    /// build is still inspectable, rather than failing the whole module.
    DeferredMember,
    /// A `key=` the lowering pass could not read, on a member it drew
    /// anyway with the default in place of the value.
    ///
    /// Distinct from [`Self::DeferredMember`], which says the member did
    /// not lower. A roof whose `overhang=` is unusable is in the build,
    /// flush with the wall line, and reporting that as a deferral tells
    /// the author to look for a member that is not missing.
    ///
    /// Where it sits against `spec/lint.md` §11.3, and why it is a
    /// warning, is argued once on [`Self::severity`] rather than twice.
    IgnoredArgument,
    /// A struct/def scope has no theme bound to it, so every `mat_slot=`
    /// member silently degrades to air during block-array lowering.
    NoThemeBound,
    /// A `mat_slot=` resolved to an abstract material token
    /// (`@floor.wood.broadleaf`) and no registry pack materials catalog was
    /// available to lift it. The block-array lowering needs a canonical id;
    /// the cell degrades to air. Distinct from `UnknownAbstractToken`, which
    /// fires when a catalog *is* present but does not declare the token —
    /// this variant covers the older "no pack at all" path that survives for
    /// library callers (LSP highlighting, `cairn check` without a pack).
    AbstractTokenDeferred,
    /// A `mat_slot=` resolved to an abstract material token that the registry
    /// pack's materials catalog does not declare. Fail-loud per spec §7.2:
    /// the cell cannot lower silently to air when a pack was offered, so the
    /// build stops with a structured suggestion towards the closest known
    /// token.
    UnknownAbstractToken,
    /// A `mat_slot=` resolved to a block id the compile's target does not
    /// declare. Fail-loud per spec versioning-editions §10.4 ("unknown IDs
    /// ... are hard errors"): the id would otherwise be written into a
    /// structure file the game loads as air, with nothing to explain the
    /// hole. Distinct from `UnknownAbstractToken`, which fires one step
    /// earlier when the abstract token itself is undeclared — this variant
    /// fires on an id that resolved cleanly and simply does not exist in
    /// that `(edition, version)`, whether the author or the pack's catalog
    /// chose it.
    ///
    /// Only raised when the run pinned a target (`cairn compile --target`).
    /// `cairn check` / `info` / `lower` have no version to check against
    /// and skip the comparison rather than guess one.
    UnknownId,
    /// A member whose geometry attaches blockstates was bound to a material
    /// that cannot carry them — a sloped roof or an eave `stair` bound to
    /// something outside the stair family.
    ///
    /// Error, and by the first clause of §11.3 rather than the last. The
    /// pass is not the incomplete side: `gable` / `shed` / `hip` lowering
    /// is finished, and what it is being asked for does not exist. Adopting
    /// the id writes a blockstate no version of the game has; substituting
    /// the fallback species builds a roof out of a material nobody asked
    /// for. Both are the silent substitution §10.4 forbids, and a warning
    /// does not make either loud: no machine-readable surface carries a
    /// lowering warning — `cairn check` does not lower, the lockfile still
    /// says `verified: true`, and there is no `--deny-warnings`.
    ///
    /// Whose mistake it is rides in `data` (`slot` and `token`), the way
    /// [`Self::UnknownId`] carries `origin` — a pack that maps a token onto
    /// the wrong material is not the author's to fix, but it is not less of
    /// an error for that.
    IncompatibleMaterial,
    /// A `struct` has no `size=WxH` header, so block-array lowering cannot
    /// derive a voxel extent and skips it.
    StructNoSize,
    /// A `def` (referenced by a `place use=NAME`) has no `size=WxH` header.
    /// Without an interior footprint the place cannot lower into a voxel
    /// volume, so the placement is skipped. Distinct from `StructNoSize`
    /// so a CI / LSP filter that matches on `code` can tell whether the
    /// missing size sits on a struct or on a template the user meant to
    /// instantiate.
    DefNoSize,
    /// A `place use=NAME` references an unknown def, an `east_of=ID` /
    /// `north_of=ID` references an unknown prior place in the same site, or
    /// a `connect a.port to b.port` refers to a missing place id. Carries a
    /// nearest-match suggestion when one fits within the spell cap. The
    /// referenced symbol cannot be substituted silently, so the build stops.
    UnresolvedPlaceRef,
    /// A `place theme=NAME` references a theme the module does not declare.
    /// Fail-loud because the per-place colour scheme would otherwise vanish
    /// silently; carries a nearest-match suggestion when one fits.
    UnresolvedThemeRef,
    /// The module declares a theme, but under the pinned edition none of
    /// its per-edition variants (spec versioning-editions §10.7) can bind.
    ///
    /// Error, and for the reason §10.4 gives: the alternative to stopping
    /// is binding the other edition's variant — which routes, say,
    /// Bedrock-only slot values into a Java `.nbt` — or binding nothing,
    /// which builds the requested extent out of air. Both are silent
    /// substitution. The theme is the thing that cannot be honoured, so
    /// this names the theme rather than reporting each `mat_slot=` that
    /// found no value.
    ThemeVariantMissing,
    /// A `place theme=NAME` named one edition's variant and the pinned
    /// edition bound a different one.
    ///
    /// Warning, not an error: binding whichever variant the pin selects is
    /// what the author almost certainly wants — that is the pinned edition's
    /// variant when the module has one, and the unsuffixed theme when it
    /// does not — and §10.7 asks the semantic layer to stay edition-neutral
    /// anyway. But an explicit name silently becoming a different name is
    /// worth one line, and the fix — write the logical name — is the
    /// spelling §10.7 prescribes.
    ThemeVariantRebound,
    /// A scope's derived voxel extent exceeds
    /// [`crate::block_array::MAX_STRUCTURE_VOLUME`], so the pass skips it
    /// rather than allocating for it.
    ///
    /// Every input that feeds the extent — `size=`, `walls height=`,
    /// `roof overhang=`, `level y=` — is range-checked on its own, so this
    /// only fires on a combination whose *product* is out of reach. Warning
    /// severity, matching `StructNoSize`: the scope is skipped, and
    /// `cairn compile` refuses separately rather than certifying a build
    /// missing one of the scopes its source asked for.
    StructureTooLarge,
    /// A `place` row omits a key it cannot become a placement without:
    /// `id=`, `use=`, or `theme=`.
    ///
    /// The three are one code because the row fails the same way for each
    /// — `resolve_site_placements` skips it, no `.nbt` is written, and the
    /// build is missing a building the source asked for. The message names
    /// every key the row is short of, so the author fixes one line once
    /// rather than re-running the compiler per key.
    ///
    /// Only a key that is *absent* counts. A key that is present but not
    /// label-shaped (`use=3`) reaches the same skip, but calling it missing
    /// would be a lie and `E_TYPE_MISMATCH_LABEL` already names it.
    IncompletePlace,
    /// A `place id=` breaks an invariant [`crate::ids::PlaceId`] relies on:
    /// it is empty, or contains `.`, `:`, or whitespace.
    ///
    /// Those characters are the structural separators the scope key
    /// `site::SITE::PLACE` and every walkway key parsed back out of it are
    /// built from, so an id carrying one cannot round-trip. `id=` accepts a
    /// string literal, which is what let the value through — nothing between
    /// the lexer and the key constructor looked at its contents.
    InvalidPlaceId,
    /// Two `place` rows in the same site share an `id=`. The first definition
    /// wins for downstream references; the duplicate is dropped and the
    /// error names both spans.
    DuplicatePlaceId,
    /// A `place` line carries either an `at=` value other than `origin` or
    /// combines `at=` with `east_of=` / `north_of=`. Origin selectors are
    /// mutually exclusive per spec §9.3 so the placement coordinate is
    /// unambiguous.
    InvalidPlaceOrigin,
    /// A `def NAME` is never referenced by any `place use=NAME`. The def
    /// itself lowers to no voxels (defs are templates), so this is advisory
    /// rather than fatal — but worth surfacing because an unused def is
    /// usually a typo on the `place use=` side.
    UnusedDef,
    /// A `connect A.PORT to B.PORT` row names a port id (`PORT`) that the
    /// referenced def does not expose. The place id sides are reported by
    /// `E_UNRESOLVED_PLACE_REF` instead — this code is specifically for the
    /// port half of the `place.port` shape. Carries a nearest-match
    /// suggestion when one fits the standard spell cap.
    UnresolvedPort,
    /// A `connect A.PORT to B.PORT` row whose port id matches more than one
    /// member of the referenced def. The first match is taken for downstream
    /// lowering; the duplicate is flagged so the author can disambiguate by
    /// renaming the colliding member.
    AmbiguousPort,
    /// A `connect` row carries no `path=` argument. Walkway lowering has no
    /// material to lay the path with — silently degrading to air would leave
    /// the buildings invisibly unconnected, so the build fails.
    MissingPathMaterial,
    /// Walkway voxelisation hit an existing building cell along the L-shaped
    /// path between two ports. The blocked cell is skipped (the rest of the
    /// walkway still lays), so the connection still reaches both ends visibly
    /// even when an obstacle steals one or two cells in between.
    WalkwayBlocked,
    /// A `connect` row repeats a `(from, to)` port pair already laid by an
    /// earlier row in the same site. The second walkway is dropped silently
    /// at the voxel level — re-laying the same gravel strip is a no-op — and
    /// the row is flagged so the author can tell the duplicate from a missed
    /// new connection.
    DuplicateWalkway,
    /// A `connect` row targets a `place` that the resolver registered in
    /// `seen_place_ids` but never finished lifting into `place_def` — the
    /// `place` row was skipped by `resolve_site_placements` for an absent
    /// `use=` / `theme=` (`E_INCOMPLETE_PLACE`), a mistyped one
    /// (`E_TYPE_MISMATCH_LABEL`), a failed origin selector
    /// (`E_INVALID_PLACE_ORIGIN`, whose row registers before it is
    /// validated), or a name that resolved to nothing
    /// (`E_UNRESOLVED_PLACE_REF` / `E_UNRESOLVED_THEME_REF`).
    ///
    /// The root cause is therefore reported elsewhere, and this is not a
    /// duplicate of it: the other finding says which row is broken, this
    /// one says which walkway went with it. Without it the walkway would
    /// vanish from the build with no signal that the `connect` did nothing.
    /// Mirrors the `W_DEFERRED_MEMBER` pattern used by walkway endpoint
    /// cascades in `block_array::lower`.
    DeferredConnect,
    /// A `connect` row whose site / place / port identifier contains the
    /// `__` substring. The surface lexer permits `_` in identifiers, but
    /// the canonical walkway scope key uses `__` as the `from`/`to`
    /// separator — so `(home, b__c, home2, entry)` and
    /// `(home, b, c__home2, entry)` would otherwise encode to the same
    /// wire string. Lowering drops the row and asks the user to rename
    /// the offending segment so the encoding stays unambiguous.
    InvalidWalkwayIdent,
    /// A `connect` row whose positional shape is not
    /// `FROM.PORT to TO.PORT`. The line-based parser accepts any number
    /// of positionals up to the next newline and `intent::lower` carries
    /// them through verbatim, so `connect a.entry` / `connect a.entry to`
    /// / `connect a.entry xxx b.entry` would otherwise reach the
    /// resolver, which silently short-circuits the row. Fail-loud here
    /// so the user gets a position-anchored signal at the offending span
    /// instead of a vanished walkway.
    ConnectArity,
    /// Two members evaluated in the same phase wrote one voxel to different
    /// blocks, so the block the build keeps is the one whose line comes
    /// last. `spec/compilation.md` §4.1 opens by promising that "order
    /// accidents are eliminated" and then grants last-wins to "local
    /// overrides within the same phase" — an author restating a member is
    /// the case that grant is for, and two footprints that happen to
    /// intersect is not, yet the grid cannot tell them apart. Warned rather
    /// than refused for that reason: the resolution the spec mandates still
    /// happens, it just stops being invisible.
    PhaseConflict,
    /// An `assert truth(...)` with no rows at all. The construct exists to
    /// state something about the circuit, and a table with no rows states
    /// nothing — no context around it and no pass written later makes it
    /// verify anything. That is the argument `E_INVALID_REQUIRES` makes
    /// for a `@requires` the compiler cannot read, and the reason this is
    /// an error rather than a note: in a diff an empty table reads exactly
    /// like one that passes.
    TruthTableEmpty,
    /// Two rows of one `assert truth(...)` assign the same inputs
    /// different outputs. No circuit satisfies both, so whatever the table
    /// was written to verify, it cannot. Reported on the later row with a
    /// note at the first row carrying that pattern — which of the two is
    /// wrong is the author's to decide, and no reading of the pair is
    /// worth less than the other.
    TruthTableConflict,
    /// Two rows of one `assert truth(...)` assign the same inputs the same
    /// output. The table still says exactly what it says, so the repair is
    /// to delete either row and nothing else changes. Kept apart from
    /// [`Self::TruthTableConflict`] because the two ask for different
    /// work — one line to drop against two readings to choose between —
    /// and because severity is a property of the code.
    TruthTableDuplicateRow,
    /// An `assert truth(...)` whose rows leave input combinations
    /// unassigned. Every row present is still a real constraint, so the
    /// finding is about coverage rather than about the statement being
    /// void: a four-input table is sixteen rows, and an author part way
    /// through writing one is writing something true. `data` carries the
    /// combinations to write.
    TruthTablePartial,
}

impl DiagnosticCode {
    /// Stable string form for the gcc-style text format and JSON output.
    /// The same string is used by external matchers (LSP quick-fix etc.) so
    /// changes here are breaking for consumers.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DuplicateSize => "E_DUPLICATE_SIZE",
            Self::DuplicateSlot => "E_DUPLICATE_SLOT",
            Self::DuplicateSelector => "E_DUPLICATE_SELECTOR",
            Self::DuplicateArg => "E_DUPLICATE_ARG",
            Self::DuplicateId => "E_DUPLICATE_ID",
            Self::DuplicateItem => "E_DUPLICATE_ITEM",
            Self::DuplicateHeader => "E_DUPLICATE_HEADER",
            Self::UnsupportedNesting => "E_UNSUPPORTED_NESTING",
            Self::MisplacedMember => "E_MISPLACED_MEMBER",
            Self::UnknownKeyword => "E_UNKNOWN_KEYWORD",
            Self::UnexpectedPositional => "E_UNEXPECTED_POSITIONAL",
            Self::InvalidRequires => "E_INVALID_REQUIRES",
            Self::TypeMismatchLabel => "E_TYPE_MISMATCH_LABEL",
            Self::TypeMismatchSize => "E_TYPE_MISMATCH_SIZE",
            Self::UnresolvedSlot => "E_UNRESOLVED_SLOT",
            Self::UnknownSlotTarget => "E_UNKNOWN_SLOT_TARGET",
            Self::ThemeSelectorUnmatched => "E_THEME_SELECTOR_UNMATCHED",
            Self::DeferredMember => "W_DEFERRED_MEMBER",
            Self::IgnoredArgument => "W_IGNORED_ARGUMENT",
            Self::NoThemeBound => "W_NO_THEME_BOUND",
            Self::AbstractTokenDeferred => "W_ABSTRACT_TOKEN_DEFERRED",
            Self::UnknownAbstractToken => "E_UNKNOWN_ABSTRACT_TOKEN",
            Self::UnknownId => "E_UNKNOWN_ID",
            Self::IncompatibleMaterial => "E_INCOMPATIBLE_MATERIAL",
            Self::StructNoSize => "W_STRUCT_NO_SIZE",
            Self::DefNoSize => "W_DEF_NO_SIZE",
            Self::UnresolvedPlaceRef => "E_UNRESOLVED_PLACE_REF",
            Self::UnresolvedThemeRef => "E_UNRESOLVED_THEME_REF",
            Self::ThemeVariantMissing => "E_THEME_VARIANT_MISSING",
            Self::ThemeVariantRebound => "W_THEME_VARIANT_REBOUND",
            Self::StructureTooLarge => "W_STRUCTURE_TOO_LARGE",
            Self::IncompletePlace => "E_INCOMPLETE_PLACE",
            Self::InvalidPlaceId => "E_INVALID_PLACE_ID",
            Self::DuplicatePlaceId => "E_DUPLICATE_PLACE_ID",
            Self::InvalidPlaceOrigin => "E_INVALID_PLACE_ORIGIN",
            Self::UnusedDef => "W_UNUSED_DEF",
            Self::UnresolvedPort => "E_UNRESOLVED_PORT",
            Self::AmbiguousPort => "E_AMBIGUOUS_PORT",
            Self::MissingPathMaterial => "E_MISSING_PATH_MATERIAL",
            Self::WalkwayBlocked => "W_WALKWAY_BLOCKED",
            Self::DuplicateWalkway => "W_DUPLICATE_WALKWAY",
            Self::DeferredConnect => "W_DEFERRED_CONNECT",
            Self::InvalidWalkwayIdent => "W_INVALID_WALKWAY_IDENT",
            Self::ConnectArity => "E_CONNECT_ARITY",
            Self::PhaseConflict => "W_PHASE_CONFLICT",
            Self::TruthTableEmpty => "E_TRUTH_TABLE_EMPTY",
            Self::TruthTableConflict => "E_TRUTH_TABLE_CONFLICT",
            Self::TruthTableDuplicateRow => "W_TRUTH_TABLE_DUPLICATE_ROW",
            Self::TruthTablePartial => "W_TRUTH_TABLE_PARTIAL",
        }
    }

    /// Severity assigned to this code.
    ///
    /// **The** severity for the code: every emission site reads it from
    /// here rather than writing a literal, so reclassifying a code is one
    /// edit and cannot leave a pass disagreeing with the ledger. Pinned by
    /// `tests/diagnostic_severity.rs`, which walks a broad fixture corpus
    /// and compares each finding against this function.
    ///
    /// `spec/lint.md` §11.3 draws the line at the *build*: a finding is an
    /// error when leaving it alone yields something other than what the
    /// source asked for — a concept that is absent, an id that resolves to
    /// nothing, a value outside its domain — because silent substitution
    /// and implicit dropping are forbidden. Everything else is a warning:
    /// version / edition drift, the non-guarantee of redstone timing, and
    /// the partial-build degradations the block-array pass reports
    /// (`W_DEFERRED_MEMBER`, `W_NO_THEME_BOUND`, `W_ABSTRACT_TOKEN_DEFERRED`,
    /// `W_STRUCT_NO_SIZE`), where the compiler — not the source — is the
    /// incomplete side and `cairn compile` refuses separately rather than
    /// certifying a partial build.
    ///
    /// `W_IGNORED_ARGUMENT` sits on the line. §11.3's error clause covers
    /// a value dropped and a default substituted, which is the build
    /// differing from the source — but the clause forbids *silent*
    /// substitution, and this code is the announcement, so the letter
    /// cuts both ways. It is a warning because it replaces a
    /// `W_DEFERRED_MEMBER` on the same shapes and moves what the finding
    /// says without moving any exit code. Promoting it is the same call
    /// an argument key the compiler does not recognise waits on — those
    /// are unreported outside the actuator-patch keys, and every source
    /// carrying one builds today — and the two want one decision rather
    /// than two. §11.3 records the same.
    ///
    /// Two codes sit close to the line and are decided in their variant
    /// docs: `E_UNKNOWN_SLOT_TARGET` is an error because the members
    /// bound to the slot lower to air, and `E_THEME_SELECTOR_UNMATCHED`
    /// is a warning because a rule matching nothing changes nothing.
    /// `E_UNKNOWN_ABSTRACT_TOKEN` is the one lowering code that is an
    /// error: when a registry pack *was* offered but does not declare the
    /// bound token, falling back to air would hide a typo the pack author
    /// needs to fix (spec §7.2's fail-loud rule).
    #[must_use]
    pub fn severity(self) -> Severity {
        match self {
            Self::DuplicateSize
            | Self::DuplicateSlot
            | Self::DuplicateSelector
            | Self::DuplicateArg
            | Self::DuplicateId
            | Self::MisplacedMember
            | Self::UnknownKeyword
            | Self::UnexpectedPositional
            | Self::InvalidRequires
            | Self::TypeMismatchLabel
            | Self::TypeMismatchSize
            | Self::UnresolvedSlot
            | Self::UnknownSlotTarget
            | Self::UnknownAbstractToken
            | Self::UnknownId
            | Self::IncompatibleMaterial
            | Self::UnresolvedPlaceRef
            | Self::UnresolvedThemeRef
            | Self::ThemeVariantMissing
            | Self::DuplicatePlaceId
            | Self::IncompletePlace
            | Self::InvalidPlaceId
            | Self::InvalidPlaceOrigin
            | Self::UnresolvedPort
            | Self::AmbiguousPort
            | Self::MissingPathMaterial
            | Self::ConnectArity
            | Self::DuplicateItem
            | Self::DuplicateHeader
            | Self::UnsupportedNesting
            | Self::TruthTableEmpty
            | Self::TruthTableConflict => Severity::Error,
            Self::StructureTooLarge
            | Self::ThemeSelectorUnmatched
            | Self::ThemeVariantRebound
            | Self::DeferredMember
            | Self::NoThemeBound
            | Self::IgnoredArgument
            | Self::AbstractTokenDeferred
            | Self::StructNoSize
            | Self::DefNoSize
            | Self::UnusedDef
            | Self::WalkwayBlocked
            | Self::DuplicateWalkway
            | Self::DeferredConnect
            | Self::InvalidWalkwayIdent
            | Self::PhaseConflict
            | Self::TruthTableDuplicateRow
            | Self::TruthTablePartial => Severity::Warning,
        }
    }
}

impl Serialize for DiagnosticCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // JSON output uses the same `E_*` string as the text format so
        // downstream tooling can match on a single contract surface
        // regardless of which `--format` was selected.
        serializer.serialize_str(self.as_str())
    }
}

/// Machine-readable payload attached to a [`Diagnostic`].
///
/// Lets downstream tooling (LSP quick-fix, CI annotator, test asserts)
/// inspect structured numeric / categorical fields without re-parsing the
/// human-readable `primary` string. `tag = "kind"` is used so the JSON
/// form (`{"kind":"walkway_blocked","skipped":3}`) carries a stable
/// discriminator that downstream matchers pin on.
///
/// `#[non_exhaustive]` on the enum protects consumer exhaust matches
/// against **new variants** landing for additional codes as the
/// diagnostic surface is still **Evolving**. Adding a new field to an
/// existing variant is still breaking by itself; per-variant
/// `#[non_exhaustive]` is added on a per-case basis when a follow-up
/// expansion is anticipated.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiagnosticData {
    /// Companion payload for [`DiagnosticCode::WalkwayBlocked`]. `skipped`
    /// is the number of cells along the L-shaped path that overlapped an
    /// existing structure and were dropped from the walkway lay.
    WalkwayBlocked {
        /// Count of cells the walkway lowering had to skip. Invariant:
        /// `>= 1` — `lower_connects` only emits `W_WALKWAY_BLOCKED` when
        /// the underlying `skipped > 0`. Typed as `u64` so `usize` lifts
        /// without lossy truncation on any platform Cairn supports.
        skipped: u64,
    },
    /// Companion payload for [`DiagnosticCode::IncompletePlace`]. The keys
    /// the `place` row does not declare, without the trailing `=`, in the
    /// order the message lists them.
    ///
    /// Carried because "insert the missing keys" is the obvious quick-fix
    /// for this code, and recovering the set from the rendered sentence is
    /// exactly the prose-parsing `spec/lint.md` §11.2 tells consumers to
    /// avoid. Invariant: non-empty — a row that declares all three keys
    /// produces no finding at all.
    IncompletePlace {
        /// Missing key names (`id`, `use`, `theme`).
        missing: Vec<String>,
    },
    /// Companion payload for [`DiagnosticCode::InvalidRequires`]. Which
    /// way the expression failed, and the text that failed.
    ///
    /// Carried for the reason [`Self::IncompletePlace`] is: the code
    /// covers several distinct mistakes and the obvious quick-fix differs
    /// between them — replacing `<` with `>=` is a one-character edit a
    /// tool can offer, while a snapshot label is not repairable at all
    /// today. Telling them apart from the rendered sentence is the
    /// prose-parsing `spec/lint.md` §11.2 tells consumers to avoid.
    InvalidRequires {
        /// Stable name of the failure, from
        /// [`crate::resolve::RequirementError::kind`]:
        /// `not_a_version_requirement`, `unsupported_operator`,
        /// `empty_version`, `component_not_a_number`,
        /// `component_too_large`, or `trailing_tokens`.
        reason: String,
        /// The part of the expression the reason is about: the operator as
        /// written, the offending component, or the trailing text. Empty
        /// when the failure names no fragment of its own.
        found: String,
    },
    /// Companion payload for [`DiagnosticCode::DuplicateSelector`]. The
    /// binding keys this selector row takes over from an earlier row,
    /// without the trailing `=`, in the order the message lists them.
    ///
    /// Carried for the reason [`Self::IncompletePlace`] is: "merge these
    /// rows" is the obvious quick-fix and it needs the key set, not a
    /// sentence to take apart. Invariant: non-empty — a row sharing no
    /// binding key with an earlier one produces no finding at all.
    DuplicateSelector {
        /// Rebound key names (`frame`, `sill`, ...).
        rebound: Vec<String>,
    },
    /// Companion payload for [`DiagnosticCode::UnknownId`]. The id that does
    /// not exist, the target it was checked against, and the way back.
    ///
    /// Carried for the reason [`Self::IncompletePlace`] is: the obvious
    /// quick-fix is "replace this id with that one", and a tool should not
    /// have to recover either id from the sentence. `origin` additionally
    /// tells a consumer *where* the edit belongs, which differs by case —
    /// in the source, in the pack's mapping, or in a pack row that does
    /// not exist yet.
    UnknownId {
        /// The fully namespaced id the target does not declare.
        id: String,
        /// The target it was checked against, as `"<edition> <version>"`.
        registry: String,
        /// Who chose the id, from [`crate::block_array::IdOrigin::kind`]:
        /// `authored` (the source names it), `catalog` (the pack maps a
        /// token onto it), or `builtin` (the pack declares no row for a
        /// member default, so the compiler's own id was used).
        origin: String,
        /// The catalog token involved, for the two origins that have one.
        /// Absent from the JSON for `authored`, rather than `null`.
        #[serde(skip_serializing_if = "Option::is_none")]
        token: Option<String>,
        /// Nearest id the target does declare, when one is within the
        /// suggestion cap. Absent from the JSON when there is none.
        #[serde(skip_serializing_if = "Option::is_none")]
        suggestion: Option<String>,
    },
    /// Companion payload for [`DiagnosticCode::IncompatibleMaterial`].
    ///
    /// Carries where the material came from, because that decides who fixes
    /// it: a `slot` the source binds directly is the author's line to edit,
    /// while a dotted `token` is the registry pack's mapping. Same reason
    /// [`Self::UnknownId`] carries `origin` — the severity does not move,
    /// only the address does.
    IncompatibleMaterial {
        /// The fully namespaced id the member was bound to.
        id: String,
        /// Family the member's geometry needs, as a bare noun (`stair`).
        /// Named rather than implied so a second family added later is a
        /// value here and not a second code.
        required: String,
        /// `mat_slot=` name the member read, when it has one. Absent when
        /// the member carries no binding at all.
        #[serde(skip_serializing_if = "Option::is_none")]
        slot: Option<String>,
        /// The theme's slot value as written (`@cobblestone`,
        /// `@roof.dark_wood`). A dotted one is a catalog token, which makes
        /// the pack's mapping the thing to correct.
        #[serde(skip_serializing_if = "Option::is_none")]
        token: Option<String>,
    },
    /// Companion payload for [`DiagnosticCode::TruthTablePartial`]. The
    /// combinations the table leaves unassigned, and the two numbers the
    /// rest of the count is derived from.
    ///
    /// Carried for the reason [`Self::IncompletePlace`] is: "write the
    /// missing rows" is the obvious quick-fix and it needs the patterns
    /// themselves, not a sentence to take apart.
    ///
    /// `missing` is a **sample rather than the set**, which breaks the
    /// habit the other payloads here establish and is why it is said
    /// twice: a twenty-input table has a million combinations, and
    /// building that list to describe a one-row table would cost more
    /// than the finding is worth. A consumer that needs the count derives
    /// it from the other two fields.
    TruthTablePartial {
        /// How many signals sit left of the `->`, so the table has
        /// `2^inputs` combinations in all. Carried instead of that total
        /// because no integer the compiler holds fits `2^130`, and the
        /// grammar puts no ceiling on the input list. At least 1: a
        /// zero-input table does not parse.
        inputs: u32,
        /// How many distinct combinations the rows do assign. A pattern
        /// written twice counts once, which is why one table can be
        /// reported as both repeating and partial. Invariant: `>= 1` and
        /// fewer than `2^inputs` — a table with no rows is
        /// `E_TRUTH_TABLE_EMPTY` instead, and a complete one raises
        /// nothing.
        covered: u64,
        /// The lowest few unassigned patterns, in ascending order, spelled
        /// as a row spells them: one character per input, leading zeros
        /// kept. Non-empty, and usually shorter than
        /// `2^inputs - covered`.
        missing: Vec<String>,
    },
}

/// Secondary location for a [`Diagnostic`] (the "first declared here"
/// pointer attached to a duplicate-key error, etc.).
///
/// `span` is optional because some notes are *informational* rather than
/// locational — the "expected one of: ..." footer on
/// `E_UNKNOWN_KEYWORD`, for example, has no byte range distinct from the
/// primary finding's span. Renderers should suppress the `file:L:C:`
/// prefix for `span == None`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DiagnosticNote {
    /// Byte range the note refers to, when the note points at a distinct
    /// secondary location.
    #[serde(skip)]
    pub span: Option<Span>,
    /// Human-readable note text.
    pub message: String,
}

/// One finding emitted by a `check` pass.
///
/// `#[non_exhaustive]` so external crates cannot construct a
/// [`Diagnostic`] by struct literal — when a future field arrives
/// (another structured payload slot, a `source` pointer, etc.) the
/// addition is no longer a breaking change for downstream callers.
/// In-crate sites still build the struct directly and update in step
/// when new fields land; cross-crate consumers must route through a
/// future builder rather than depending on the field set being frozen.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[non_exhaustive]
pub struct Diagnostic {
    /// Stable code identifying the kind of finding.
    pub code: DiagnosticCode,
    /// Byte range the primary message points at.
    #[serde(skip)]
    pub span: Span,
    /// Primary message rendered after the code on the first line of the
    /// gcc-style text output.
    pub primary: String,
    /// Additional locations relevant to this finding. Emitted as indented
    /// `note: ...` lines in the text format.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<DiagnosticNote>,
    /// Optional structured payload for machine-readable consumers. `None`
    /// for codes that have no companion data yet. Serialised as a `data`
    /// key only when present, keeping the JSON contract additive for
    /// existing downstream tooling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<DiagnosticData>,
}

impl Diagnostic {
    /// Severity of this finding, read from [`DiagnosticCode::severity`].
    ///
    /// A method rather than a field so the two cannot disagree. Every pass
    /// builds its findings as struct literals, and twenty-eight sites used
    /// to write a `severity:` value of their own — which made
    /// [`DiagnosticCode::severity`] documentation rather than the source of
    /// truth, and made reclassifying a code an edit that compiled while
    /// changing no exit code. A literal written back in now fails to
    /// compile, which is a stronger guarantee than any corpus-driven test
    /// could give: a test only covers the codes its fixtures reach.
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.code.severity()
    }

    /// Convert this diagnostic's primary byte span into a 1-based
    /// `line:column` [`Position`] against the given source string.
    ///
    /// Lines are split by [`crate::lines`], the same rule the lexer walks
    /// by, so this and a parse error name the same row. Column counts
    /// Unicode scalar values, mirroring the `Position` documentation.
    ///
    /// O(`source.len()`) per call. When converting many diagnostics from
    /// the same source, build a [`LineStarts`] index once and call
    /// [`LineStarts::position`] instead.
    #[must_use]
    pub fn position(&self, source: &str) -> Position {
        position_at(source, self.span.start)
    }

    /// Build a [`RenderedDiagnostic`] suitable for JSON/structured output
    /// against the given source. Populates 1-based `line` / `col` /
    /// `end_line` / `end_col` for both the primary span and each note that
    /// carries a span — without these the JSON form would carry zero
    /// position information, defeating the `--format json` contract for
    /// downstream tooling.
    #[must_use]
    pub fn render(&self, source: &str, lines: &LineStarts) -> RenderedDiagnostic {
        let start = lines.position(source, self.span.start);
        let end = lines.position(source, self.span.end);
        RenderedDiagnostic {
            code: self.code,
            severity: self.severity(),
            line: start.line.get(),
            col: start.col.get(),
            end_line: end.line.get(),
            end_col: end.col.get(),
            primary: self.primary.clone(),
            notes: self
                .notes
                .iter()
                .map(|n| RenderedNote {
                    line: n
                        .span
                        .as_ref()
                        .map(|s| lines.position(source, s.start).line.get()),
                    col: n
                        .span
                        .as_ref()
                        .map(|s| lines.position(source, s.start).col.get()),
                    message: n.message.clone(),
                })
                .collect(),
            data: self.data.clone(),
        }
    }
}

/// JSON-friendly rendering of a [`Diagnostic`] with line/col populated.
///
/// Built via [`Diagnostic::render`]. The `code` field serialises to the same
/// `E_*` string as the text format (see [`DiagnosticCode::as_str`]) so
/// downstream tooling matches a single contract regardless of `--format`.
#[derive(Debug, Clone, Serialize)]
pub struct RenderedDiagnostic {
    /// Stable code identifying the kind of finding.
    pub code: DiagnosticCode,
    /// Severity of the finding.
    pub severity: Severity,
    /// 1-based line of the primary span's first byte.
    pub line: u32,
    /// 1-based column of the primary span's first byte, in Unicode scalar
    /// values.
    pub col: u32,
    /// 1-based line of the primary span's last-byte-exclusive boundary.
    pub end_line: u32,
    /// 1-based column of the primary span's last-byte-exclusive boundary.
    pub end_col: u32,
    /// Primary message string.
    pub primary: String,
    /// Notes, each with optional line/col if they pointed at a distinct
    /// secondary location.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<RenderedNote>,
    /// Mirror of [`Diagnostic::data`] — see that field for the full
    /// contract. Carried separately so the rendered form can be
    /// serialised without re-walking the source `Diagnostic`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<DiagnosticData>,
}

/// JSON-friendly rendering of a [`DiagnosticNote`].
#[derive(Debug, Clone, Serialize)]
pub struct RenderedNote {
    /// 1-based line of the note's source position, when the note has a
    /// distinct secondary location. Omitted for informational notes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// 1-based column of the note's source position.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub col: Option<u32>,
    /// Note message text.
    pub message: String,
}

/// Precomputed line-start byte offsets for a source string.
///
/// Construct once with [`LineStarts::new`], then look up many byte offsets
/// via [`LineStarts::position`]. Reduces an N-diagnostic conversion from
/// `O(N * file_len)` (re-walking the source per call) to
/// `O(file_len + N * log L)` where `L` is the line count.
#[derive(Debug, Clone)]
pub struct LineStarts {
    /// Byte offset of the first character of each line (line 1 starts at
    /// offset 0; subsequent entries are the byte after each line break).
    starts: Vec<usize>,
}

impl LineStarts {
    /// Build the index by walking the source exactly once.
    #[must_use]
    pub fn new(source: &str) -> Self {
        Self {
            starts: crate::lines::starts(source),
        }
    }

    /// Resolve a byte offset into a 1-based `line:column` [`Position`].
    ///
    /// `byte_offset` must be a character boundary — every offset in this
    /// crate comes from a [`Span`], which is built from one. Offsets past
    /// the end of `source` clamp to the final position; an offset inside a
    /// character does not, and panics on the slice.
    ///
    /// Line and column saturate at `u32::MAX` rather than wrapping. Each
    /// needs its own four billion — of lines in the file, of characters on
    /// the line — and a source that reaches either has worse problems than
    /// a truncated position.
    #[must_use]
    pub fn position(&self, source: &str, byte_offset: usize) -> Position {
        let clamped = byte_offset.min(source.len());
        // partition_point returns the first index whose start > clamped;
        // line numbers are 1-based and the starts vector is 1-aligned with
        // them, so the returned index *is* the line number. It is never 0:
        // `starts[0]` is 0, which is `<= clamped` for every offset.
        let line_number = self.starts.partition_point(|&s| s <= clamped);
        let line_start = self.starts[line_number - 1];
        let column_chars = source[line_start..clamped].chars().count() + 1;
        let line = NonZeroU32::new(u32::try_from(line_number).unwrap_or(u32::MAX))
            .unwrap_or(NonZeroU32::MIN);
        let col = NonZeroU32::new(u32::try_from(column_chars).unwrap_or(u32::MAX))
            .unwrap_or(NonZeroU32::MIN);
        Position { line, col }
    }
}

/// Compute a 1-based `line:column` for a byte offset into `source`.
///
/// The one-shot convenience over [`LineStarts`], not a second answer to the
/// same question — computing the line here independently is how the two
/// came to disagree about a lone `\r`. It builds and discards an index per
/// call, so prefer holding one when converting many offsets from the same
/// source; the O(`source.len()`) cost per call is unchanged.
#[must_use]
pub fn position_at(source: &str, byte_offset: usize) -> Position {
    LineStarts::new(source).position(source, byte_offset)
}

#[cfg(test)]
mod tests {
    use strum::IntoEnumIterator;

    use super::*;

    /// Stable strings of every code the predicate accepts, sorted.
    ///
    /// The left side comes from `EnumIter`, so the assertions below are
    /// exhaustive: a new variant lands in one of the expected lists or
    /// fails the comparison. The `for code in [...]` loops this replaced
    /// tolerated an omission silently, which is the one mistake a
    /// contract-surface test exists to catch.
    fn codes_where(predicate: impl Fn(DiagnosticCode) -> bool) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = DiagnosticCode::iter()
            .filter(|c| predicate(*c))
            .map(DiagnosticCode::as_str)
            .collect();
        names.sort_unstable();
        names
    }

    #[test]
    fn every_code_renders_its_documented_string() {
        // The string form is the contract surface downstream matchers pin
        // on, so it is spelled out here rather than derived — a rename
        // that breaks an LSP quick-fix should be visible in the diff as a
        // changed literal, not as a passing test.
        assert_eq!(
            codes_where(|_| true),
            [
                "E_AMBIGUOUS_PORT",
                "E_CONNECT_ARITY",
                "E_DUPLICATE_ARG",
                "E_DUPLICATE_HEADER",
                "E_DUPLICATE_ID",
                "E_DUPLICATE_ITEM",
                "E_DUPLICATE_PLACE_ID",
                "E_DUPLICATE_SELECTOR",
                "E_DUPLICATE_SIZE",
                "E_DUPLICATE_SLOT",
                "E_INCOMPATIBLE_MATERIAL",
                "E_INCOMPLETE_PLACE",
                "E_INVALID_PLACE_ID",
                "E_INVALID_PLACE_ORIGIN",
                "E_INVALID_REQUIRES",
                "E_MISPLACED_MEMBER",
                "E_MISSING_PATH_MATERIAL",
                "E_THEME_SELECTOR_UNMATCHED",
                "E_THEME_VARIANT_MISSING",
                "E_TRUTH_TABLE_CONFLICT",
                "E_TRUTH_TABLE_EMPTY",
                "E_TYPE_MISMATCH_LABEL",
                "E_TYPE_MISMATCH_SIZE",
                "E_UNEXPECTED_POSITIONAL",
                "E_UNKNOWN_ABSTRACT_TOKEN",
                "E_UNKNOWN_ID",
                "E_UNKNOWN_KEYWORD",
                "E_UNKNOWN_SLOT_TARGET",
                "E_UNRESOLVED_PLACE_REF",
                "E_UNRESOLVED_PORT",
                "E_UNRESOLVED_SLOT",
                "E_UNRESOLVED_THEME_REF",
                "E_UNSUPPORTED_NESTING",
                "W_ABSTRACT_TOKEN_DEFERRED",
                "W_DEFERRED_CONNECT",
                "W_DEFERRED_MEMBER",
                "W_DEF_NO_SIZE",
                "W_DUPLICATE_WALKWAY",
                "W_IGNORED_ARGUMENT",
                "W_INVALID_WALKWAY_IDENT",
                "W_NO_THEME_BOUND",
                "W_PHASE_CONFLICT",
                "W_STRUCTURE_TOO_LARGE",
                "W_STRUCT_NO_SIZE",
                "W_THEME_VARIANT_REBOUND",
                "W_TRUTH_TABLE_DUPLICATE_ROW",
                "W_TRUTH_TABLE_PARTIAL",
                "W_UNUSED_DEF",
                "W_WALKWAY_BLOCKED",
            ],
        );
    }

    #[test]
    fn every_code_is_classified_against_spec_11_3() {
        // Errors block a build; warnings are advisory. `severity` carries
        // the rule and the two borderline calls; this pins the resulting
        // partition so a reclassification is a deliberate edit here rather
        // than a silent change in exit codes.
        assert_eq!(
            codes_where(|c| c.severity() == Severity::Error),
            [
                "E_AMBIGUOUS_PORT",
                "E_CONNECT_ARITY",
                "E_DUPLICATE_ARG",
                "E_DUPLICATE_HEADER",
                "E_DUPLICATE_ID",
                "E_DUPLICATE_ITEM",
                "E_DUPLICATE_PLACE_ID",
                "E_DUPLICATE_SELECTOR",
                "E_DUPLICATE_SIZE",
                "E_DUPLICATE_SLOT",
                "E_INCOMPATIBLE_MATERIAL",
                "E_INCOMPLETE_PLACE",
                "E_INVALID_PLACE_ID",
                "E_INVALID_PLACE_ORIGIN",
                "E_INVALID_REQUIRES",
                "E_MISPLACED_MEMBER",
                "E_MISSING_PATH_MATERIAL",
                "E_THEME_VARIANT_MISSING",
                "E_TRUTH_TABLE_CONFLICT",
                "E_TRUTH_TABLE_EMPTY",
                "E_TYPE_MISMATCH_LABEL",
                "E_TYPE_MISMATCH_SIZE",
                "E_UNEXPECTED_POSITIONAL",
                "E_UNKNOWN_ABSTRACT_TOKEN",
                "E_UNKNOWN_ID",
                "E_UNKNOWN_KEYWORD",
                "E_UNKNOWN_SLOT_TARGET",
                "E_UNRESOLVED_PLACE_REF",
                "E_UNRESOLVED_PORT",
                "E_UNRESOLVED_SLOT",
                "E_UNRESOLVED_THEME_REF",
                "E_UNSUPPORTED_NESTING",
            ],
        );
        assert_eq!(
            codes_where(|c| c.severity() == Severity::Warning),
            [
                "E_THEME_SELECTOR_UNMATCHED",
                "W_ABSTRACT_TOKEN_DEFERRED",
                "W_DEFERRED_CONNECT",
                "W_DEFERRED_MEMBER",
                "W_DEF_NO_SIZE",
                "W_DUPLICATE_WALKWAY",
                "W_IGNORED_ARGUMENT",
                "W_INVALID_WALKWAY_IDENT",
                "W_NO_THEME_BOUND",
                "W_PHASE_CONFLICT",
                "W_STRUCTURE_TOO_LARGE",
                "W_STRUCT_NO_SIZE",
                "W_THEME_VARIANT_REBOUND",
                "W_TRUTH_TABLE_DUPLICATE_ROW",
                "W_TRUTH_TABLE_PARTIAL",
                "W_UNUSED_DEF",
                "W_WALKWAY_BLOCKED",
            ],
            "the `W_` prefix marks a partial-build degradation, which is a \
             claim about what the compiler did rather than about severity, so \
             an `E_`-prefixed warning is not by itself a misclassification — \
             but it is the shape worth re-reading against §11.3 whenever one \
             lands",
        );
    }

    #[test]
    fn position_at_handles_unicode_columns() {
        // Two-byte UTF-8 character: the column count must advance by 1
        // (one Unicode scalar value), not by the byte length.
        let source = "α\nβ\n";
        let pos_after_alpha = position_at(source, 2); // byte 2 = start of '\n'
        assert_eq!(pos_after_alpha.line.get(), 1);
        assert_eq!(pos_after_alpha.col.get(), 2);

        let pos_on_beta = position_at(source, 3); // byte 3 = start of 'β'
        assert_eq!(pos_on_beta.line.get(), 2);
        assert_eq!(pos_on_beta.col.get(), 1);
    }

    #[test]
    fn position_at_for_offset_past_end_clamps_to_eof() {
        let source = "abc\n";
        let pos = position_at(source, 99);
        assert_eq!(pos.line.get(), 2);
        assert_eq!(pos.col.get(), 1);
    }

    #[test]
    fn diagnostic_data_walkway_blocked_serialises_with_kind_tag() {
        // The structured payload must surface as
        // `{"kind":"walkway_blocked","skipped":N}` so downstream tooling
        // can match on a stable discriminator instead of re-parsing the
        // human-readable `primary` string.
        let value = serde_json::to_value(DiagnosticData::WalkwayBlocked { skipped: 3 })
            .expect("serialise payload");
        assert_eq!(
            value,
            serde_json::json!({"kind": "walkway_blocked", "skipped": 3}),
        );
    }

    #[test]
    fn unknown_id_payload_omits_the_halves_it_does_not_have() {
        // `spec/lint.md` §11.2 documents both optional fields as absent
        // rather than `null`, and the two absences mean distinct things: no
        // `token` says the author wrote the id, no `suggestion` says the
        // target has nothing near it. A `null` would read as "unknown" for
        // both. `origin` is never optional — it is what separates the three
        // cases, so a consumer always has it.
        let authored = serde_json::to_value(DiagnosticData::UnknownId {
            id: "minecraft:light".to_owned(),
            registry: "bedrock 1.21.60".to_owned(),
            origin: "authored".to_owned(),
            token: None,
            suggestion: None,
        })
        .expect("serialise payload");
        assert_eq!(
            authored,
            serde_json::json!({
                "kind": "unknown_id",
                "id": "minecraft:light",
                "registry": "bedrock 1.21.60",
                "origin": "authored",
            }),
        );

        let from_catalog = serde_json::to_value(DiagnosticData::UnknownId {
            id: "minecraft:stone_bricks".to_owned(),
            registry: "bedrock 1.21.0".to_owned(),
            origin: "catalog".to_owned(),
            token: Some("floor.stone.smooth".to_owned()),
            suggestion: Some("minecraft:stonebrick".to_owned()),
        })
        .expect("serialise payload");
        assert_eq!(
            from_catalog,
            serde_json::json!({
                "kind": "unknown_id",
                "id": "minecraft:stone_bricks",
                "registry": "bedrock 1.21.0",
                "origin": "catalog",
                "token": "floor.stone.smooth",
                "suggestion": "minecraft:stonebrick",
            }),
        );
    }

    /// `origin` and `token` are two fields with one job between them, so
    /// the pairs a consumer may rely on are pinned here rather than left
    /// to the two call sites to agree on by hand.
    #[test]
    fn every_id_origin_has_a_wire_name_and_the_token_that_goes_with_it() {
        use crate::block_array::IdOrigin;

        let cases = [
            (IdOrigin::Authored, "authored", None),
            (
                IdOrigin::Catalog {
                    token: "floor.stone.smooth".to_owned(),
                },
                "catalog",
                Some("floor.stone.smooth"),
            ),
            (
                IdOrigin::Builtin {
                    token: "pressure_plate.default",
                },
                "builtin",
                Some("pressure_plate.default"),
            ),
        ];
        for (origin, kind, token) in cases {
            assert_eq!(origin.kind(), kind);
            assert_eq!(origin.token(), token);
        }
    }

    #[test]
    fn rendered_diagnostic_omits_data_key_when_payload_absent() {
        // `data: None` must serialise to *no key at all* so existing
        // JSON consumers that did not opt into the new field keep working.
        let lines = LineStarts::new("abc\n");
        let diag = Diagnostic {
            code: DiagnosticCode::DuplicateSize,
            span: Span { start: 0, end: 3 },
            primary: "duplicate size".to_owned(),
            notes: vec![],
            data: None,
        };
        let rendered = diag.render("abc\n", &lines);
        let value = serde_json::to_value(&rendered).expect("serialise rendered");
        let object = value.as_object().expect("rendered as object");
        assert!(
            !object.contains_key("data"),
            "data key should be omitted when payload is None, got {value}",
        );
    }

    #[test]
    fn rendered_diagnostic_propagates_data_payload_when_present() {
        // At the render layer: a `Diagnostic` carrying a
        // payload must lift it into `RenderedDiagnostic` so the JSON
        // formatter (and any other consumer of `render`) sees the same
        // structured data the in-memory finding holds.
        let lines = LineStarts::new("abc\n");
        let diag = Diagnostic {
            code: DiagnosticCode::WalkwayBlocked,
            span: Span { start: 0, end: 3 },
            primary: "walkway skipped 3 cells".to_owned(),
            notes: vec![],
            data: Some(DiagnosticData::WalkwayBlocked { skipped: 3 }),
        };
        let rendered = diag.render("abc\n", &lines);
        assert_eq!(
            rendered.data,
            Some(DiagnosticData::WalkwayBlocked { skipped: 3 }),
        );
    }

    #[test]
    fn position_agrees_with_a_direct_walk_of_the_source() {
        // The index exists so N diagnostics do not re-walk the source N
        // times, and the walk is what it has to reproduce: step one
        // character at a time, bumping the line at each break and the
        // column otherwise. Comparing against `position_at` instead would
        // now be comparing the index with itself — there is one
        // implementation, which is the point of having one rule.
        for source in ["α\nfoo\nbar\nβaz\n", "a\r\nb\rc", "\r\r\n", "", "no break"] {
            let lines = LineStarts::new(source);
            let (mut line, mut col, mut offset) = (1_u32, 1_u32, 0_usize);
            loop {
                let expected = Position {
                    line: NonZeroU32::new(line).expect("1-based"),
                    col: NonZeroU32::new(col).expect("1-based"),
                };
                assert_eq!(
                    lines.position(source, offset),
                    expected,
                    "{source:?} at byte {offset}",
                );
                if offset == source.len() {
                    break;
                }
                if let Some(len) = crate::lines::terminator_len(source, offset) {
                    offset += len;
                    line += 1;
                    col = 1;
                } else {
                    offset += source[offset..]
                        .chars()
                        .next()
                        .expect("offset is inside the source")
                        .len_utf8();
                    col += 1;
                }
            }
        }
    }
}
