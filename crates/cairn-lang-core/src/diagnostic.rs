//! [`Diagnostic`] payload used by every `check` pass.

use serde::{Serialize, Serializer};

use crate::error::{Position, Span};
use crate::lines::{LineStarts, position_at};

/// Severity of a single [`Diagnostic`].
///
/// An error makes `cairn check` exit 1 and a warning does not. `spec/lint` "Error vs warning"
/// draws the line: an error, left alone, causes an unintended result; a warning is advisory.
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
/// The string form (`E_DUPLICATE_SIZE`, `E_UNKNOWN_KEYWORD`, ...) is the contract downstream
/// tooling matches on, rather than the prose `primary`. `#[non_exhaustive]`, so a new code is not
/// a breaking change.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
// Lets the tests walk every variant; `cfg(test)` keeps the proc macro out of shipped builds.
#[cfg_attr(test, derive(strum::EnumIter))]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// The source did not parse.
    ///
    /// One code for every way a parse fails, with the message saying which. A code per
    /// [`crate::error::ParseError`] and [`crate::error::LexError`] variant would make every new
    /// variant of two `#[non_exhaustive]` enums public contract, for a distinction nothing
    /// branches on.
    ///
    /// No check pass produces it, since a file that does not parse reaches none of them; it is
    /// built by [`crate::parse::diagnose_parse_failure`].
    Parse,
    /// More than one `size=` argument in a struct or def header.
    DuplicateSize,
    /// Repeated `slot NAME` line in a single `theme` block.
    DuplicateSlot,
    /// Two selector rows in one `theme` block select the same members and
    /// bind the same key.
    ///
    /// A member's selector bindings merge every row it matches in source order, so the later row
    /// wins. When both rows carry the same keyword and attributes, the earlier binding survives on
    /// no member and is dead text. The finding is about the merge, since no pass reads a merged
    /// binding.
    ///
    /// Rows binding different keys compose. Rows whose attributes only overlap
    /// (`window[class=small]` against `window[class=small,side=front]`) are the cascade, which is
    /// source order by design.
    DuplicateSelector,
    /// Repeated `key=` in the same argument list (struct/def header,
    /// statement args, selector attrs / bindings).
    DuplicateArg,
    /// Two or more members in the same immediate body share an `id=`.
    ///
    /// Two `place` rows of one `site` body are [`Self::DuplicatePlaceId`] instead, because the
    /// resolver compares refused ids too. A `place` indented under another row, or sharing its id
    /// with a row that is not a `place`, is still this code.
    DuplicateId,
    /// Two or more top-level items of the same kind share a name.
    ///
    /// Kinds are separate namespaces, so `theme x` beside `struct x` is no collision. For three
    /// kinds the second declaration binds nothing; two `site` blocks of one name merge into one
    /// place namespace instead, and the note says which.
    DuplicateItem,
    /// A single-valued `@directive` header appears more than once.
    ///
    /// `@cairn` and `@intended_targets` each answer one question once, and nothing would decide
    /// between two answers. `@requires` is not covered: its floors compose, folding to the
    /// strictest.
    DuplicateHeader,
    /// A `struct` or `def` body holds more than one `circuit` line.
    ///
    /// Place-and-route reads only the first usable line, so the others would be dropped silently.
    /// Refused rather than keeping the first, because nothing says which line the author meant.
    DuplicateCircuit,
    /// A member carries an indented body that nothing reads.
    ///
    /// The grammar allows a body under every command, but only `level y=N` in a `struct` or `def`
    /// has a reader; every other body would be dropped before lowering.
    UnsupportedNesting,
    /// A statement keyword that is in the known-keyword table but whose
    /// enclosing body has no reader for it.
    ///
    /// Every keyword parses in every body, so `place` in a `struct` or `floor` in a `site`
    /// classifies, but would be dropped between the IR and the voxels.
    MisplacedMember,
    /// A statement keyword not in the known-keyword table.
    UnknownKeyword,
    /// A key outside the vocabulary of the member's role, written as a
    /// `key=value` argument or inside the member's own `[key=value]` — or a
    /// key other than `size=` / `class=` on a `struct` / `def` header line.
    ///
    /// An error because no pass will read the value, so the member is built without it; a
    /// misspelt argument with a default builds silently at the default. One code for every field
    /// (`clas=outer`, `[clas=outer]`, a header's `siz=7x7`), because it is one defect; the
    /// sentence names the field to edit.
    UnknownArgument,
    /// A `-> value` tail on a member whose keyword cannot emit a signal,
    /// or on a sensor keyword written in the `[selector]` form.
    ///
    /// `spec/redstone` "Signal binding" writes an emitted signal on the component that emits it,
    /// and [`crate::intent::SENSOR_HOSTS`] are the keywords that accept one; anywhere else the
    /// signal is driven by nothing. The host is the fault rather than the value, because no value
    /// makes a `walls` emit. A sensor tail naming no signal is the redstone pipeline's
    /// `E_LOGIC_INVALID_SIGNAL` instead.
    ///
    /// A selector line carries only the door actuator patch, so `pressure_plate[id=p] -> sig.x` is
    /// on no sensor line.
    MisplacedBinding,
    /// A statement carrying bare positional values in a form that takes
    /// none.
    ///
    /// `spec/syntax` "Lexical" requires `key=value` after the command keyword, `connect` aside
    /// (`E_CONNECT_ARITY`). Every other reader ignores positional values, so a dropped `=`
    /// (`height 3`) would change the build silently.
    UnexpectedPositional,
    /// An `@requires` expression that is not a version floor.
    ///
    /// An unreadable expression declares nothing while the line still looks like a constraint.
    /// The grammar is `version>=X` alone, because only a floor composes by taking the strictest
    /// (`spec/syntax` "Headers", `spec/versioning-editions` "Fail-loud and minimum-version
    /// inference"). Not the spec's `E_REQUIRES_CONFLICT`, a floor contradicting the
    /// registry-inferred range, which this compiler does not derive.
    InvalidRequires,
    /// The `@cairn` value is not a `YYYY.M[.PATCH]` language version.
    ///
    /// A warning, because the header is provenance and the build is unaffected; but provenance a
    /// later compiler cannot read is useless, and nothing else looks at the string.
    InvalidCairnVersion,
    /// The `@cairn` header names a language version later than the build reading it.
    ///
    /// A warning for the reason [`Self::InvalidCairnVersion`] is. It tells the author that an
    /// unknown keyword or argument elsewhere may be the version gap; for a file that does not
    /// parse, a note on [`Self::Parse`] says so instead.
    FutureCairnVersion,
    /// A label-typed key whose value is not a label (identifier or
    /// string): `id=`, `class=`, `mat_slot=`, `use=`, or `theme=`.
    ///
    /// One code because every reader lifts the five alike, through `Value::as_label_str`. A
    /// `use=` or `theme=` that is present but not a label is this code rather than
    /// `E_INCOMPLETE_PLACE`, so the message does not ask for a key the author already wrote.
    TypeMismatchLabel,
    /// `size=` whose value is not a `WxH` literal.
    TypeMismatchSize,
    /// A member whose only route to a block is `mat_slot=` was written
    /// without one, so it paints nothing.
    ///
    /// The absent half of [`Self::UnresolvedSlot`]; no member earns both. On a role that reads no
    /// `mat_slot=` neither applies, and one written there is [`Self::IgnoredArgument`].
    MissingMaterial,
    /// `mat_slot=NAME` references a slot the applied theme does not
    /// declare, on a member whose role's
    /// `crate::intent::MemberRole::unread_arguments` does not list
    /// `mat_slot`. On one that does, the name is not looked up.
    UnresolvedSlot,
    /// `slot NAME -> VALUE` whose VALUE is neither a canonical nor an
    /// abstract material token (see `spec/materials-themes` "Canonical
    /// vocabulary").
    ///
    /// An error because every `mat_slot=NAME` pointing at the slot lowers to air, the implicit
    /// dropping `spec/lint` "Error vs warning" forbids.
    UnknownSlotTarget,
    /// `theme` selector rule that does not match any member in the file.
    ///
    /// A warning despite its `E_` prefix: a rule that matches nothing overrides nothing, so the
    /// build is what the rest of the source asks for. The finding is about intent, the advisory
    /// half of `spec/lint` "Error vs warning".
    ThemeSelectorUnmatched,
    /// A member whose role the block-array pass does not lower. A warning, so a partial build
    /// stays inspectable rather than failing the whole module.
    DeferredMember,
    /// A `key=` no pass read on the line it was written on.
    ///
    /// Three shapes, in each of which the build differs from the source and says so:
    ///
    /// - Unreadable value: lowering could not read it and used the default.
    /// - Unread key: nothing reads it where it is written (`crate::intent::Unread`). An unreached
    ///   key is one the spec defines and no pass reads, so a lowering rule is owed: an
    ///   `Unreached` entry of `crate::intent::MemberRole::unread_arguments`, a header key in
    ///   `crate::check::arguments::UNREAD_HEADER_ARGUMENTS`, or any binding right of a `theme`
    ///   selector row's arrow, since no pass lowers those. An inapplicable key is a universal one
    ///   on a role that puts down nothing it would be read for (`mat_slot=` on a `level`); no rule
    ///   is owed, and the note asks for its removal.
    /// - Routed past: a sibling argument picked a lowering rule that does not consult it, as
    ///   `crate::intent::MemberRole::conditional_arguments` records for
    ///   `roof kind=gable slope_to=front`. It fires whether or not the member built.
    ///
    /// Distinct from [`Self::DeferredMember`], which says the member did not lower: a roof with an
    /// unusable `overhang=` is in the build. Where the selector names no rule at all, the deferral
    /// carries the whole repair and this code stays quiet; a member that fails to lower for any
    /// other reason gets both, because they are two repairs. [`Self::severity`] argues why this is
    /// a warning.
    IgnoredArgument,
    /// A `struct` body, or a `place`'s instance of a `def`, has no theme while a member in it reads
    /// a `mat_slot=`, so every reader degrades as `spec/lint` "Materials and targets" describes.
    ///
    /// Raised once per body, and only when a member that lowers reads a slot as
    /// `crate::intent::Member::read_slot` defines it; a member under a `level` counts, one the
    /// `level` drops does not. A body that reads no slot, such as one built from fallback
    /// materials or with every `mat_slot=` on a keyword that reads none, is not reported.
    NoThemeBound,
    /// A `mat_slot=` resolved to an abstract material token (`@floor.wood.broadleaf`) with no
    /// registry pack catalog to lift it, so the cell degrades to air. A catalog that lacks the
    /// token is [`Self::UnknownAbstractToken`] instead; this covers callers with no pack at all,
    /// such as the LSP or `cairn check` without one.
    AbstractTokenDeferred,
    /// A `mat_slot=` resolved to an abstract material token that the registry pack's materials
    /// catalog does not declare. An error per `spec/materials-themes` "Canonical vocabulary": with
    /// a pack offered, the cell must not silently lower to air.
    UnknownAbstractToken,
    /// A canonical token's state literal (`@oak_log[axis=x]`) was taken as written, unchecked
    /// against the target.
    ///
    /// `spec/versioning-editions` "Fail-loud and minimum-version inference" makes an out-of-domain
    /// state the error `E_STATE_DOMAIN`, which needs a table of every block's states that this
    /// compiler does not hold. A warning rather than an error, because most literals are right and
    /// refusing all of them would make literals unwritable; it is the announcement `spec/lint`
    /// "Error vs warning" asks for in place of silence. Raised on every state literal lowering
    /// resolves, from a theme slot or a `connect … path=`.
    StateLiteralUnchecked,
    /// A `mat_slot=` resolved to a block id the compile's target does not declare.
    ///
    /// An error per `spec/versioning-editions` "Fail-loud and minimum-version inference" ("unknown
    /// IDs ... are hard errors"), because the game would load the id as air. Unlike
    /// [`Self::UnknownAbstractToken`], one step earlier, the id resolved cleanly, whether the
    /// author or the pack's catalog chose it, and does not exist in that `(edition, version)`.
    ///
    /// Raised only when the run pinned a target, as `cairn compile --target` and
    /// `cairn check --edition E --target V` do; without one there is no version to check against.
    UnknownId,
    /// A member whose geometry attaches blockstates was bound to a material
    /// that cannot carry them — a sloped roof or an eave `stair` bound to
    /// something outside the stair family.
    ///
    /// An error by the first clause of `spec/lint` "Error vs warning": the lowering is complete,
    /// and what it is asked for does not exist. Adopting the id writes a blockstate no game version
    /// has; substituting a fallback builds from a material nobody asked for. Both are the silent
    /// substitution `spec/versioning-editions` "Fail-loud and minimum-version inference" forbids,
    /// and a warning would not make either loud: the lockfile still says `verified: true`, and an
    /// unpinned `cairn check`, the invocation CI writes by default, carries no lowering finding.
    ///
    /// `data` says whose mistake it is (`slot` or `token`), as [`Self::UnknownId`] carries
    /// `origin`.
    IncompatibleMaterial,
    /// A `struct` has no `size=WxH` header, so block-array lowering cannot
    /// derive a voxel extent and skips it.
    StructNoSize,
    /// A `def` that a `place use=NAME` references has no `size=WxH` header, so the placement is
    /// skipped. Apart from [`Self::StructNoSize`] so a filter on the code can tell a struct from a
    /// template.
    DefNoSize,
    /// A `place use=NAME` references an unknown def, an `east_of=ID` / `north_of=ID` an unknown
    /// prior place in the same site, or a `connect a.port to b.port` a missing place id. Carries a
    /// nearest-match suggestion when one fits.
    UnresolvedPlaceRef,
    /// A `place theme=NAME` references a theme the module does not declare. Carries a
    /// nearest-match suggestion when one fits.
    UnresolvedThemeRef,
    /// The module declares a theme, but under the pinned edition none of its
    /// per-edition variants (`spec/versioning-editions` "Java / Bedrock
    /// portability") can bind.
    ///
    /// An error for the reason the same chapter's "Fail-loud and minimum-version inference"
    /// gives: binding the other edition's variant would route its slot values into this edition's
    /// file, and binding nothing would build the extent from air. The theme is what cannot be
    /// honoured, so the finding names it rather than each `mat_slot=` left without a value.
    ThemeVariantMissing,
    /// A `place theme=NAME` named one edition's variant and the pinned
    /// edition bound a different one.
    ///
    /// A warning: the variant the pin selects is almost certainly what the author wants, and
    /// `spec/versioning-editions` "Java / Bedrock portability" keeps the semantic layer
    /// edition-neutral. An explicit name silently becoming another is still worth a line, and the
    /// fix, writing the logical name, is what that section prescribes.
    ThemeVariantRebound,
    /// A scope's derived voxel extent exceeds
    /// [`crate::block_array::MAX_STRUCTURE_VOLUME`], so the pass skips it
    /// rather than allocating for it.
    ///
    /// Each input to the extent is range-checked alone, so only a product out of reach fires
    /// this. A warning like [`Self::StructNoSize`]: the scope is skipped, and `cairn compile`
    /// refuses the partial build separately.
    StructureTooLarge,
    /// A scope paints more distinct block states than one palette can
    /// index ([`crate::block_array::PALETTE_CAPACITY`] entries, air
    /// included), so the pass skips it rather than panicking.
    ///
    /// Every paint counts, including one a later member covers. A vanilla registry has far fewer
    /// states, so only unchecked block ids (no version pinned) or state-literal properties reach
    /// this. A warning like [`Self::StructureTooLarge`]: the scope is skipped, and `cairn compile`
    /// refuses the partial build separately.
    PaletteTooLarge,
    /// A `place` row omits a key it cannot become a placement without:
    /// `id=`, `use=`, or `theme=`.
    ///
    /// One code because the row fails the same way for each: it is skipped and the building is
    /// missing. The message names every missing key, so one edit fixes the line. A key that is
    /// present but not a label (`use=3`) is `E_TYPE_MISMATCH_LABEL` instead.
    IncompletePlace,
    /// A `place id=` that [`crate::ids::PlaceId`] refuses; the [`crate::ids`] module docs say what
    /// and why. `id=` takes a string literal, so nothing before the key constructor looks at its
    /// contents.
    InvalidPlaceId,
    /// Two `place` rows in the same site share an `id=`. The first wins for references; the
    /// duplicate is dropped.
    DuplicatePlaceId,
    /// Two scopes a build would write would be written to one file.
    ///
    /// Artifacts are named by [`crate::ids::artifact_stem`], so `struct hut` and `place id=hut`,
    /// two sites that each `place id=home`, or the walkways `a_b.c to d.e` and `a.b_c to d.e` in
    /// one site collide. Checked before lowering: a sizeless `struct` or a `place` of a sizeless
    /// `def` is left out, since lowering drops it, but a scope past the volume budget or the
    /// router's area cap still counts. Names compare ignoring case, as the default file systems of
    /// macOS and Windows do, so the verdict does not depend on the host. A scope key declared twice
    /// is `E_DUPLICATE_ITEM` or `E_DUPLICATE_PLACE_ID` instead.
    OutputNameCollision,
    /// A `place` line carries either an `at=` value other than `origin` or
    /// combines `at=` with `east_of=` / `north_of=`. Origin selectors are
    /// mutually exclusive per `spec/components-editing-sites` "Multi-building
    /// with `site`" so the placement coordinate is unambiguous.
    InvalidPlaceOrigin,
    /// A `def NAME` that no `place use=NAME` references.
    ///
    /// Every `place` row whose `use=` is a label naming the def counts, refused or not; `use=3`
    /// names no def. Advisory, since a def lowers to nothing on its own, but an unused def is
    /// usually a typo on the `place use=` side.
    UnusedDef,
    /// A `connect A.PORT to B.PORT` row names a port no member of the referenced def's body
    /// carries. Only the body's own members count, not those indented under another member; an
    /// unknown place is `E_UNRESOLVED_PLACE_REF` instead.
    ///
    /// When a member under a `level` carries the id, the primary names that `level` and a note
    /// points at the member. Otherwise the notes are a nearest-match suggestion, when one fits,
    /// then a note to add the `id=`.
    UnresolvedPort,
    /// A `connect A.PORT to B.PORT` row whose port id matches more than one member of the
    /// referenced def. Lowering takes the first match.
    AmbiguousPort,
    /// A `connect` row carries no `path=` argument, so the walkway has no material; laying air
    /// would leave the buildings invisibly unconnected.
    MissingPathMaterial,
    /// Walkway voxelisation hit a building cell along the L-shaped path between two ports. The
    /// cell is skipped and the rest is laid, so the walkway still visibly reaches both ends.
    ///
    /// When every cell is skipped, or the L alone is past the router's area cap, nothing is laid;
    /// the code stays a warning, and `cairn compile` refuses the build with `E_PARTIAL_BUILD`.
    WalkwayBlocked,
    /// A `connect` row repeats a `(from, to)` port pair an earlier row in the same site laid.
    /// Re-laying the strip changes nothing; the finding tells a duplicate from a mistyped new
    /// connection.
    DuplicateWalkway,
    /// A `connect` row targets a `place` whose row was registered but refused: an absent or
    /// mistyped `use=` / `theme=` (`E_INCOMPLETE_PLACE`, `E_TYPE_MISMATCH_LABEL`), a failed origin
    /// selector (`E_INVALID_PLACE_ORIGIN`), an origin naming a row refused for its id
    /// ([`Self::DeferredPlace`]), or a name that resolved to nothing (`E_UNRESOLVED_PLACE_REF`,
    /// `E_UNRESOLVED_THEME_REF`).
    ///
    /// That finding says which row is broken; this one says which walkway went with it, which
    /// would otherwise vanish without a signal.
    DeferredConnect,
    /// A `place` row's `east_of=` / `north_of=` names an earlier row in the
    /// same site refused for its id (`E_INVALID_PLACE_ID`). The row naming
    /// it is not placed, since what it is placed against has no position.
    ///
    /// The same cascade as [`Self::DeferredConnect`], one layer up. `E_UNRESOLVED_PLACE_REF` would
    /// send the author after a typo that is not there, and silence would leave an unplaced row
    /// looking clean. A warning, because the refused row's error, which the note points at,
    /// already fails the build.
    DeferredPlace,
    /// A `connect` row whose site, place or port id contains `__`, or whose place or port id
    /// starts or ends with `_`, so its walkway scope key would be ambiguous (see
    /// [`crate::ids::KeyConstructError::UnderscoreAtEdge`]). Lowering drops the row. A release
    /// build also raises it if a row's key replaces an earlier row's anyway, which only a hole in
    /// that rule allows; a debug build asserts instead.
    InvalidWalkwayIdent,
    /// A `connect` row whose positional shape is not `FROM.PORT to TO.PORT`. Any other shape would
    /// reach the resolver, which skips the row silently.
    ConnectArity,
    /// Two members evaluated in the same phase wrote one voxel to different blocks, so the later
    /// line wins.
    ///
    /// `spec/compilation` "Phase evaluation" grants last-wins to "local overrides within the same
    /// phase". An author restating a member is what that grant is for and two footprints that
    /// happen to intersect are not, but the grid cannot tell them apart, so this warns rather than
    /// refuses: the mandated resolution still happens, visibly.
    PhaseConflict,
    /// An `assert truth(...)` with no rows. It states nothing, yet in a diff reads like a table
    /// that passes, so it is an error, as `E_INVALID_REQUIRES` is for an unreadable `@requires`.
    /// A table whose input list repeats a signal is [`Self::TruthTableDuplicateInput`] instead.
    TruthTableEmpty,
    /// An `assert truth(...)` lists one signal as two or more of its
    /// inputs.
    ///
    /// The rows then describe combinations the circuit does not have. Refused rather than read as
    /// one input, which would give the rows a meaning the author did not write. No other
    /// truth-table finding is raised beside it, since each would read the rows against a wrong
    /// input list.
    TruthTableDuplicateInput,
    /// Two rows of one `assert truth(...)` assign the same inputs different outputs, which no
    /// circuit satisfies. Reported on the later row with a note at the first; which is wrong is
    /// the author's call.
    TruthTableConflict,
    /// Two rows of one `assert truth(...)` assign the same inputs the same output, so deleting
    /// either changes nothing. Apart from [`Self::TruthTableConflict`] because the work differs,
    /// and severity belongs to the code.
    TruthTableDuplicateRow,
    /// An `assert truth(...)` whose rows leave input combinations unassigned. Every row present
    /// is still a real constraint, so this is about coverage: an author part way through a
    /// sixteen-row table is writing something true.
    TruthTablePartial,
    /// Every version `@intended_targets` names is below a version floor
    /// the same file declares, so the file can be built for nothing it
    /// says it is for.
    ///
    /// An error rather than [`Self::IntendedTargetCapPartial`]'s warning, because the declaration
    /// and the floor agree on nothing: `cairn compile --target` at any listed version is
    /// `E_VERSION_CAP`. A version the edition cannot build at all is
    /// [`Self::IntendedTargetUnsupported`] and is not counted here.
    IntendedTargetCap,
    /// Some, not all, of the versions `@intended_targets` names are below
    /// a version floor the same file declares.
    ///
    /// A warning because the header is "a hint, not a verification record" (`spec/syntax`
    /// "Headers") and the versions above the floor still build. The same contradiction as
    /// [`Self::IntendedTargetCap`], with part of the intent surviving it.
    IntendedTargetCapPartial,
    /// `@intended_targets` names a version no `--target` of the edition
    /// can build.
    ///
    /// A release the registry pack has no block data for (`1.19` on Java), or a label the
    /// edition's table cannot place, as a version in the other edition's numbering is (`1.21.40` on
    /// Java). One code because the repair is the same; the note says which.
    ///
    /// A warning, and only under a pinned edition: unpinned, the version may be meant for the other
    /// edition.
    IntendedTargetUnsupported,
}

impl DiagnosticCode {
    /// Stable string form for text and JSON output. External matchers depend on it, so a change is
    /// breaking.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Parse => "E_PARSE",
            Self::DuplicateSize => "E_DUPLICATE_SIZE",
            Self::DuplicateSlot => "E_DUPLICATE_SLOT",
            Self::DuplicateSelector => "E_DUPLICATE_SELECTOR",
            Self::DuplicateArg => "E_DUPLICATE_ARG",
            Self::DuplicateId => "E_DUPLICATE_ID",
            Self::DuplicateItem => "E_DUPLICATE_ITEM",
            Self::DuplicateHeader => "E_DUPLICATE_HEADER",
            Self::DuplicateCircuit => "E_DUPLICATE_CIRCUIT",
            Self::UnsupportedNesting => "E_UNSUPPORTED_NESTING",
            Self::MisplacedMember => "E_MISPLACED_MEMBER",
            Self::UnknownKeyword => "E_UNKNOWN_KEYWORD",
            Self::UnknownArgument => "E_UNKNOWN_ARGUMENT",
            Self::MisplacedBinding => "E_MISPLACED_BINDING",
            Self::UnexpectedPositional => "E_UNEXPECTED_POSITIONAL",
            Self::InvalidRequires => "E_INVALID_REQUIRES",
            Self::InvalidCairnVersion => "W_INVALID_CAIRN_VERSION",
            Self::FutureCairnVersion => "W_FUTURE_CAIRN_VERSION",
            Self::TypeMismatchLabel => "E_TYPE_MISMATCH_LABEL",
            Self::TypeMismatchSize => "E_TYPE_MISMATCH_SIZE",
            Self::MissingMaterial => "E_MISSING_MATERIAL",
            Self::UnresolvedSlot => "E_UNRESOLVED_SLOT",
            Self::UnknownSlotTarget => "E_UNKNOWN_SLOT_TARGET",
            Self::ThemeSelectorUnmatched => "E_THEME_SELECTOR_UNMATCHED",
            Self::DeferredMember => "W_DEFERRED_MEMBER",
            Self::IgnoredArgument => "W_IGNORED_ARGUMENT",
            Self::NoThemeBound => "W_NO_THEME_BOUND",
            Self::AbstractTokenDeferred => "W_ABSTRACT_TOKEN_DEFERRED",
            Self::UnknownAbstractToken => "E_UNKNOWN_ABSTRACT_TOKEN",
            Self::StateLiteralUnchecked => "W_STATE_LITERAL_UNCHECKED",
            Self::UnknownId => "E_UNKNOWN_ID",
            Self::IncompatibleMaterial => "E_INCOMPATIBLE_MATERIAL",
            Self::StructNoSize => "W_STRUCT_NO_SIZE",
            Self::DefNoSize => "W_DEF_NO_SIZE",
            Self::UnresolvedPlaceRef => "E_UNRESOLVED_PLACE_REF",
            Self::UnresolvedThemeRef => "E_UNRESOLVED_THEME_REF",
            Self::ThemeVariantMissing => "E_THEME_VARIANT_MISSING",
            Self::ThemeVariantRebound => "W_THEME_VARIANT_REBOUND",
            Self::StructureTooLarge => "W_STRUCTURE_TOO_LARGE",
            Self::PaletteTooLarge => "W_PALETTE_TOO_LARGE",
            Self::IncompletePlace => "E_INCOMPLETE_PLACE",
            Self::InvalidPlaceId => "E_INVALID_PLACE_ID",
            Self::DuplicatePlaceId => "E_DUPLICATE_PLACE_ID",
            Self::OutputNameCollision => "E_OUTPUT_NAME_COLLISION",
            Self::InvalidPlaceOrigin => "E_INVALID_PLACE_ORIGIN",
            Self::UnusedDef => "W_UNUSED_DEF",
            Self::UnresolvedPort => "E_UNRESOLVED_PORT",
            Self::AmbiguousPort => "E_AMBIGUOUS_PORT",
            Self::MissingPathMaterial => "E_MISSING_PATH_MATERIAL",
            Self::WalkwayBlocked => "W_WALKWAY_BLOCKED",
            Self::DuplicateWalkway => "W_DUPLICATE_WALKWAY",
            Self::DeferredConnect => "W_DEFERRED_CONNECT",
            Self::DeferredPlace => "W_DEFERRED_PLACE",
            Self::InvalidWalkwayIdent => "W_INVALID_WALKWAY_IDENT",
            Self::ConnectArity => "E_CONNECT_ARITY",
            Self::PhaseConflict => "W_PHASE_CONFLICT",
            Self::TruthTableEmpty => "E_TRUTH_TABLE_EMPTY",
            Self::TruthTableDuplicateInput => "E_TRUTH_TABLE_DUPLICATE_INPUT",
            Self::TruthTableConflict => "E_TRUTH_TABLE_CONFLICT",
            Self::TruthTableDuplicateRow => "W_TRUTH_TABLE_DUPLICATE_ROW",
            Self::TruthTablePartial => "W_TRUTH_TABLE_PARTIAL",
            Self::IntendedTargetCap => "E_INTENDED_TARGET_CAP",
            Self::IntendedTargetCapPartial => "W_INTENDED_TARGET_CAP",
            Self::IntendedTargetUnsupported => "W_INTENDED_TARGET_UNSUPPORTED",
        }
    }

    /// Severity assigned to this code, the one place it is decided.
    ///
    /// `spec/lint` "Error vs warning" draws the line at the build: a finding is an error when
    /// leaving it alone yields something other than what the source asked for (an absent concept,
    /// an id that resolves to nothing, a value outside its domain), because silent substitution
    /// and implicit dropping are forbidden. Everything else is a warning: version or edition
    /// drift, redstone timing, and the partial-build degradations the block-array pass reports
    /// (`W_DEFERRED_MEMBER`, `W_NO_THEME_BOUND`, `W_ABSTRACT_TOKEN_DEFERRED`, `W_STRUCT_NO_SIZE`),
    /// where the compiler is the incomplete side and `cairn compile` refuses separately.
    ///
    /// `W_IGNORED_ARGUMENT` sits on the line. A dropped value with a default substituted is the
    /// build differing from the source, but the rule forbids silent substitution, and this code
    /// is the announcement. Its routed-past shape has two repair sites, the rule the author meant
    /// or the leftover key, and refusing would pick one. Its inapplicable-key shape changes
    /// nothing in the build, and refusing it would make a universal key one some roles reject.
    ///
    /// `E_UNKNOWN_SLOT_TARGET` and `E_THEME_SELECTOR_UNMATCHED` sit close to the line and are
    /// decided in their own docs. `E_UNKNOWN_ABSTRACT_TOKEN` is the one lowering error: with a
    /// registry pack offered, falling back to air would hide a typo the pack author must fix
    /// (`spec/materials-themes` "Canonical vocabulary").
    #[must_use]
    pub fn severity(self) -> Severity {
        match self {
            Self::Parse
            | Self::DuplicateSize
            | Self::DuplicateSlot
            | Self::DuplicateSelector
            | Self::DuplicateArg
            | Self::DuplicateId
            | Self::DuplicateCircuit
            | Self::MisplacedMember
            | Self::UnknownKeyword
            | Self::UnknownArgument
            | Self::MisplacedBinding
            | Self::UnexpectedPositional
            | Self::InvalidRequires
            | Self::TypeMismatchLabel
            | Self::TypeMismatchSize
            | Self::MissingMaterial
            | Self::UnresolvedSlot
            | Self::UnknownSlotTarget
            | Self::UnknownAbstractToken
            | Self::UnknownId
            | Self::IncompatibleMaterial
            | Self::UnresolvedPlaceRef
            | Self::UnresolvedThemeRef
            | Self::ThemeVariantMissing
            | Self::DuplicatePlaceId
            | Self::OutputNameCollision
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
            | Self::TruthTableDuplicateInput
            | Self::IntendedTargetCap
            | Self::TruthTableConflict => Severity::Error,
            Self::InvalidCairnVersion
            | Self::FutureCairnVersion
            | Self::StructureTooLarge
            | Self::PaletteTooLarge
            | Self::ThemeSelectorUnmatched
            | Self::ThemeVariantRebound
            | Self::DeferredMember
            | Self::NoThemeBound
            | Self::IgnoredArgument
            | Self::AbstractTokenDeferred
            | Self::StateLiteralUnchecked
            | Self::StructNoSize
            | Self::DefNoSize
            | Self::UnusedDef
            | Self::WalkwayBlocked
            | Self::DuplicateWalkway
            | Self::DeferredConnect
            | Self::DeferredPlace
            | Self::InvalidWalkwayIdent
            | Self::PhaseConflict
            | Self::TruthTableDuplicateRow
            | Self::IntendedTargetCapPartial
            | Self::IntendedTargetUnsupported
            | Self::TruthTablePartial => Severity::Warning,
        }
    }
}

impl Serialize for DiagnosticCode {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // The same string as the text format, whichever `--format` was chosen.
        serializer.serialize_str(self.as_str())
    }
}

/// Machine-readable payload attached to a [`Diagnostic`].
///
/// Gives a quick-fix the fields it needs without parsing the prose `primary`, which `spec/lint`
/// "Machine-readable payload" tells consumers to avoid. Serializes with a stable `kind` tag:
/// `{"kind":"walkway_blocked","skipped":3}`.
///
/// `#[non_exhaustive]` covers new variants only; a field added to an existing variant is still a
/// breaking change.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiagnosticData {
    /// Companion payload for [`DiagnosticCode::WalkwayBlocked`].
    WalkwayBlocked {
        /// Cells along the path that overlapped a structure and were skipped; at least 1. The
        /// area-cap refusal carries no payload, having skipped and laid nothing. `u64` so a
        /// `usize` converts without loss.
        skipped: u64,
    },
    /// Companion payload for [`DiagnosticCode::IncompletePlace`]: the keys the `place` row lacks,
    /// without the `=`, in the order the message lists them. Never empty.
    IncompletePlace {
        /// Missing key names (`id`, `use`, `theme`).
        missing: Vec<String>,
    },
    /// Companion payload for [`DiagnosticCode::InvalidRequires`]: how the expression failed, and
    /// the text that failed, since the repair differs by failure (`<` to `>=` is one character).
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
    /// Companion payload for [`DiagnosticCode::InvalidCairnVersion`]: how the value failed, and the
    /// text that failed. Shaped like [`Self::InvalidRequires`], so a consumer handles both version
    /// headers alike.
    InvalidCairnVersion {
        /// Stable name of the failure, from
        /// [`crate::calver::LanguageVersionError::kind`]:
        /// `component_count`, `component_not_a_number`,
        /// `year_not_four_digits`, `month_out_of_range`,
        /// `patch_too_large`, or `trailing_tokens`.
        reason: String,
        /// The component the reason is about, as written. Empty when the
        /// failure names no fragment of its own: `component_count` is
        /// about the whole value, and `component_not_a_number` reports an
        /// empty component when that is what the value has (`2026.`).
        found: String,
    },
    /// Companion payload for [`DiagnosticCode::FutureCairnVersion`]: the two versions, each
    /// verbatim, because `declared` is the string the author edits and `compiler` the build a bug
    /// report names.
    FutureCairnVersion {
        /// The `@cairn` value, as written.
        declared: String,
        /// [`crate::CAIRN_VERSION`], the build that reported this.
        compiler: String,
    },
    /// Companion payload for [`DiagnosticCode::DuplicateSelector`]: the binding keys this row takes
    /// over from an earlier one, without the `=`, in the order the message lists them. Never empty.
    DuplicateSelector {
        /// Rebound key names (`frame`, `sill`, ...).
        rebound: Vec<String>,
    },
    /// Companion payload for [`DiagnosticCode::UnknownId`]: the id, the target it was checked
    /// against, and the way back. `origin` says where the edit belongs: the source, the pack's
    /// mapping, or a pack row still to be written.
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
        /// Every id the target declares for the same block, from the registry pack's alias table:
        /// the candidates a rename has and a typo search cannot find. Absent from the JSON when
        /// the pack names none, as a pack without an alias table cannot.
        ///
        /// Apart from `suggestion` because an alias is the pack stating two names are one block,
        /// which a quick-fix may apply unasked, while a suggestion is a guess.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        aliases: Vec<String>,
    },
    /// Companion payload for [`DiagnosticCode::IncompatibleMaterial`].
    ///
    /// Carries where the material came from, because that decides who fixes it: a `slot` the
    /// source binds directly is the author's line, a dotted `token` the registry pack's mapping.
    IncompatibleMaterial {
        /// The fully namespaced id the member was bound to.
        id: String,
        /// Family the member's geometry needs, as a bare noun (`stair`). Named so another family
        /// is a new value rather than a new code.
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
    /// Companion payload for [`DiagnosticCode::TruthTablePartial`]: unassigned combinations, and
    /// the two numbers the count derives from.
    ///
    /// `missing` is a sample, not the set, unlike the other payloads: a twenty-input table has a
    /// million combinations. A consumer that needs the count derives it from the other two fields.
    TruthTablePartial {
        /// Signals left of the `->`; the table has `2^inputs` combinations. Not the total itself,
        /// because the grammar sets no ceiling on the input list and `2^130` fits no integer the
        /// compiler holds. At least 1.
        inputs: u32,
        /// Distinct combinations the rows assign; a repeated pattern counts once, so one table
        /// can be both repeating and partial. At least 1 and below `2^inputs`.
        covered: u64,
        /// The lowest few unassigned patterns, ascending, spelled as a row spells them. Never
        /// empty, and usually shorter than `2^inputs - covered`.
        missing: Vec<String>,
    },
    /// Companion payload for the three `@intended_targets` codes
    /// ([`DiagnosticCode::IntendedTargetCap`],
    /// [`DiagnosticCode::IntendedTargetCapPartial`],
    /// [`DiagnosticCode::IntendedTargetUnsupported`]).
    ///
    /// `edition` is carried because the same header earns different findings from a Java and a
    /// Bedrock build.
    IntendedTargets {
        /// Which edition weighed the header, as
        /// [`crate::edition::Edition::as_str`] spells it.
        edition: String,
        /// The versions this finding is about, in the header's order. Never empty.
        targets: Vec<String>,
        /// The floor that refuses the first of them, as written. Absent for
        /// [`DiagnosticCode::IntendedTargetUnsupported`], which is not about a floor.
        #[serde(skip_serializing_if = "Option::is_none")]
        floor: Option<String>,
    },
}

/// A note on a [`Diagnostic`], such as the "first declared here" of a duplicate.
///
/// `span` is `None` for a note with no location of its own, such as an "expected one of" footer;
/// a renderer prints no `file:L:C:` prefix for it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
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
/// `#[non_exhaustive]`, so a new field is not a breaking change.
///
/// `Eq` and `Hash` let a pass drop a finding it already reported: two findings equal in every
/// field are one finding reported twice, which `block_array::lower_to_block_array` relies on for
/// the copies a def body's per-placement walk produces. A new field must therefore be
/// `Eq + Hash` (no `f64`, no `serde_json::Value`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
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
    /// Structured payload for machine-readable consumers; `None` for codes without one, and then
    /// absent from the JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<DiagnosticData>,
}

impl Diagnostic {
    /// Severity of this finding, read from [`DiagnosticCode::severity`].
    ///
    /// A method rather than a field, so a pass cannot give a finding a severity its code does not
    /// have.
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.code.severity()
    }

    /// Convert this diagnostic's primary byte span into a 1-based
    /// `line:column` [`Position`] against the given source string.
    ///
    /// Lines split as [`crate::lines`] says, as the lexer's do. `O(source.len())` per call; for
    /// many diagnostics, build one [`LineStarts`] and call [`LineStarts::position`].
    #[must_use]
    pub fn position(&self, source: &str) -> Position {
        position_at(source, self.span.start)
    }

    /// Build a [`RenderedDiagnostic`] against the given source, with 1-based positions for the
    /// primary span and every note that has one.
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
                .map(|n| {
                    let at = n.span.as_ref().map(|s| lines.position(source, s.start));
                    RenderedNote {
                        line: at.map(|p| p.line.get()),
                        col: at.map(|p| p.col.get()),
                        message: n.message.clone(),
                    }
                })
                .collect(),
            data: self.data.clone(),
        }
    }
}

/// Sort `diagnostics` into source order by primary span, `(start, end)`.
///
/// Stable, so two findings on one span keep the order they were raised in.
pub(crate) fn sort_by_position(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by_key(|d| (d.span.start, d.span.end));
}

/// JSON-friendly rendering of a [`Diagnostic`] with line/col populated.
///
/// Built by [`Diagnostic::render`].
#[derive(Debug, Clone, PartialEq, Serialize)]
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
    /// [`Diagnostic::data`], carried over.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<DiagnosticData>,
}

/// JSON-friendly rendering of a [`DiagnosticNote`].
#[derive(Debug, Clone, PartialEq, Serialize)]
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

#[cfg(test)]
mod tests;
