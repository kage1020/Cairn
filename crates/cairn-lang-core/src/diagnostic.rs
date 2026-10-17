//! [`Diagnostic`] payload used by every `check` pass.

use serde::{Serialize, Serializer};

use crate::error::{Position, Span};
use crate::lines::{LineStarts, position_at};

/// Severity of a single [`Diagnostic`] (`spec/lint` "Error vs warning").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Makes `cairn check` exit non-zero.
    Error,
    /// Advisory; leaves the exit code alone.
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

/// Stable identifier for a kind of [`Diagnostic`]; its string form is what tooling matches on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
// `cfg(test)` keeps the proc macro out of shipped builds.
#[cfg_attr(test, derive(strum::EnumIter))]
#[non_exhaustive]
pub enum DiagnosticCode {
    /// The source did not parse; one code for every way it fails.
    Parse,
    /// More than one `size=` argument in a struct or def header.
    DuplicateSize,
    /// Repeated `slot NAME` line in a single `theme` block.
    DuplicateSlot,
    /// Two selector rows in one `theme` block select the same members and bind the same key.
    DuplicateSelector,
    /// Repeated `key=` in one argument list.
    DuplicateArg,
    /// Members of one body share an `id=`; two `place` rows are [`Self::DuplicatePlaceId`].
    DuplicateId,
    /// Two or more top-level items of the same kind share a name.
    DuplicateItem,
    /// A single-valued `@directive` header appears more than once.
    DuplicateHeader,
    /// A `struct` or `def` body holds more than one `circuit` line.
    DuplicateCircuit,
    /// A member carries an indented body that nothing reads.
    UnsupportedNesting,
    /// A known statement keyword whose enclosing body has no reader for it, or a `circuit` line
    /// under a `level`.
    MisplacedMember,
    /// A statement keyword not in the known-keyword table.
    UnknownKeyword,
    /// A key outside the vocabulary of the member's role, or of a `struct` / `def` header.
    UnknownArgument,
    /// A `-> value` tail on a member that cannot emit a signal (`spec/redstone` "Signal binding").
    MisplacedBinding,
    /// Bare positional values in a form that takes none (`spec/syntax` "Lexical").
    UnexpectedPositional,
    /// An `@requires` expression that is not a version floor.
    InvalidRequires,
    /// The `@cairn` value is not a `YYYY.M[.PATCH]` language version.
    InvalidCairnVersion,
    /// The `@cairn` header names a language version later than the build reading it.
    FutureCairnVersion,
    /// A label-typed key (`id=`, `class=`, `mat_slot=`, `use=`, `theme=`) whose value is no label.
    TypeMismatchLabel,
    /// `size=` whose value is not a `WxH` literal.
    TypeMismatchSize,
    /// A member whose only route to a block is `mat_slot=` was written without one.
    MissingMaterial,
    /// `mat_slot=NAME` references a slot the applied theme does not declare.
    UnresolvedSlot,
    /// `slot NAME -> VALUE` whose VALUE is neither a canonical nor an abstract material token.
    UnknownSlotTarget,
    /// A `theme` selector rule that matches no member; a warning despite its `E_` prefix.
    ThemeSelectorUnmatched,
    /// A member whose role the block-array pass does not lower.
    DeferredMember,
    /// A `key=` no pass read on the line it was written on.
    IgnoredArgument,
    /// A body with no theme in which a lowered member reads a `mat_slot=`.
    NoThemeBound,
    /// A `mat_slot=` resolved to an abstract token with no registry pack catalog to lift it.
    AbstractTokenDeferred,
    /// A `mat_slot=` resolved to an abstract token the registry pack's catalog does not declare.
    UnknownAbstractToken,
    /// A state literal (`@oak_log[axis=x]`) taken as written, unchecked against the target.
    StateLiteralUnchecked,
    /// A `mat_slot=` resolved to a block id the pinned target does not declare.
    UnknownId,
    /// A member whose geometry attaches blockstates is bound to a material that cannot carry them.
    IncompatibleMaterial,
    /// A `struct` has no `size=WxH` header, so block-array lowering skips it.
    StructNoSize,
    /// A placed `def` has no `size=WxH` header, so the placement is skipped.
    DefNoSize,
    /// A `place use=`, `east_of=` / `north_of=` or `connect` endpoint names no such def or place.
    UnresolvedPlaceRef,
    /// A `place theme=NAME` references a theme the module does not declare.
    UnresolvedThemeRef,
    /// No per-edition variant of a declared theme can bind under the pinned edition.
    ThemeVariantMissing,
    /// A `place theme=` named one edition's variant and the pinned edition bound another.
    ThemeVariantRebound,
    /// A scope's extent exceeds [`crate::block_array::MAX_STRUCTURE_VOLUME`], so it is skipped.
    StructureTooLarge,
    /// A scope paints more states than [`crate::block_array::PALETTE_CAPACITY`], so it is skipped.
    PaletteTooLarge,
    /// A `place` row omits `id=`, `use=` or `theme=`.
    IncompletePlace,
    /// A `place id=` that [`crate::ids::PlaceId`] refuses.
    InvalidPlaceId,
    /// Two `place` rows in the same site share an `id=`; the first wins.
    DuplicatePlaceId,
    /// Two scopes a build writes share a file name, ignoring case.
    OutputNameCollision,
    /// A `place` has an `at=` other than `origin`, or `at=` beside `east_of=` / `north_of=`.
    InvalidPlaceOrigin,
    /// A `def NAME` that no `place use=NAME` references.
    UnusedDef,
    /// A `connect` row names a port no member of the referenced def's own body carries.
    UnresolvedPort,
    /// A `connect` port id matches more than one member of the referenced def; the first wins.
    AmbiguousPort,
    /// A `connect` row carries no `path=` argument.
    MissingPathMaterial,
    /// Walkway voxelisation hit a building cell between two ports; the cell is skipped.
    WalkwayBlocked,
    /// A `connect` row repeats a `(from, to)` port pair an earlier row in the site laid.
    DuplicateWalkway,
    /// A `connect` row targets a `place` whose row was refused.
    DeferredConnect,
    /// A `place` positioned against an earlier row refused for its id, so not placed itself.
    DeferredPlace,
    /// A `connect` row whose ids would make its walkway scope key ambiguous; the row is dropped.
    InvalidWalkwayIdent,
    /// A `connect` row whose positional shape is not `FROM.PORT to TO.PORT`.
    ConnectArity,
    /// Two members in one phase wrote one voxel to different blocks; the later line wins.
    PhaseConflict,
    /// An `assert truth(...)` with no rows.
    TruthTableEmpty,
    /// An `assert truth(...)` lists one signal as two or more of its inputs.
    TruthTableDuplicateInput,
    /// Two rows of one `assert truth(...)` assign the same inputs different outputs.
    TruthTableConflict,
    /// Two rows of one `assert truth(...)` assign the same inputs the same output.
    TruthTableDuplicateRow,
    /// An `assert truth(...)` whose rows leave input combinations unassigned.
    TruthTablePartial,
    /// Every version `@intended_targets` names is below a floor the same file declares.
    IntendedTargetCap,
    /// Some, not all, of the versions `@intended_targets` names are below a declared floor.
    IntendedTargetCapPartial,
    /// `@intended_targets` names a version no `--target` of the pinned edition can build.
    IntendedTargetUnsupported,
}

impl DiagnosticCode {
    /// Stable string form for text and JSON output; changing one is a breaking change.
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

    /// Severity of this code, decided here alone (`spec/lint` "Error vs warning").
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
        serializer.serialize_str(self.as_str())
    }
}

/// Machine-readable payload attached to a [`Diagnostic`], tagged by `kind` (`spec/lint`
/// "Machine-readable payload").
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum DiagnosticData {
    /// Companion payload for [`DiagnosticCode::WalkwayBlocked`].
    WalkwayBlocked {
        /// Cells skipped for overlapping a structure; at least 1.
        skipped: u64,
    },
    /// Companion payload for [`DiagnosticCode::IncompletePlace`].
    IncompletePlace {
        /// Missing key names (`id`, `use`, `theme`), in message order; never empty.
        missing: Vec<String>,
    },
    /// Companion payload for [`DiagnosticCode::InvalidRequires`].
    InvalidRequires {
        /// The failure's [`crate::resolve::RequirementError::kind`].
        reason: String,
        /// The fragment the reason is about, as written; empty when it names none.
        found: String,
    },
    /// Companion payload for [`DiagnosticCode::InvalidCairnVersion`].
    InvalidCairnVersion {
        /// The failure's [`crate::calver::LanguageVersionError::kind`].
        reason: String,
        /// The component the reason is about, as written; empty when it names none.
        found: String,
    },
    /// Companion payload for [`DiagnosticCode::FutureCairnVersion`].
    FutureCairnVersion {
        /// The `@cairn` value, as written.
        declared: String,
        /// [`crate::CAIRN_VERSION`], the build that reported this.
        compiler: String,
    },
    /// Companion payload for [`DiagnosticCode::DuplicateSelector`].
    DuplicateSelector {
        /// Keys this row rebinds from an earlier one (`frame`, `sill`); never empty.
        rebound: Vec<String>,
    },
    /// Companion payload for [`DiagnosticCode::UnknownId`].
    UnknownId {
        /// The fully namespaced id the target does not declare.
        id: String,
        /// The target it was checked against, as `"<edition> <version>"`.
        registry: String,
        /// Who chose the id, as [`crate::block_array::IdOrigin::kind`] names it.
        origin: String,
        /// The catalog token involved; absent for `authored`.
        #[serde(skip_serializing_if = "Option::is_none")]
        token: Option<String>,
        /// Nearest id the target does declare, when one is within the suggestion cap.
        #[serde(skip_serializing_if = "Option::is_none")]
        suggestion: Option<String>,
        /// Ids the pack's alias table declares for the same block: facts, unlike `suggestion`.
        #[serde(skip_serializing_if = "Vec::is_empty")]
        aliases: Vec<String>,
    },
    /// Companion payload for [`DiagnosticCode::IncompatibleMaterial`].
    IncompatibleMaterial {
        /// The fully namespaced id the member was bound to.
        id: String,
        /// Family the member's geometry needs, as a bare noun (`stair`).
        required: String,
        /// `mat_slot=` name the member read, when it has one.
        #[serde(skip_serializing_if = "Option::is_none")]
        slot: Option<String>,
        /// The theme's slot value as written; a dotted one makes the pack's mapping the fix.
        #[serde(skip_serializing_if = "Option::is_none")]
        token: Option<String>,
    },
    /// Companion payload for [`DiagnosticCode::TruthTablePartial`].
    TruthTablePartial {
        /// Signals left of the `->`; the table has `2^inputs` combinations.
        inputs: u32,
        /// Distinct combinations the rows assign.
        covered: u64,
        /// The lowest few unassigned patterns, ascending; a sample, not the set.
        missing: Vec<String>,
    },
    /// Companion payload for the three `@intended_targets` codes.
    IntendedTargets {
        /// Edition that weighed the header, as [`crate::edition::Edition::as_str`] spells it.
        edition: String,
        /// The versions this finding is about, in the header's order. Never empty.
        targets: Vec<String>,
        /// The floor that refuses the first of them, as written, when a floor is involved.
        #[serde(skip_serializing_if = "Option::is_none")]
        floor: Option<String>,
    },
}

/// A note on a [`Diagnostic`], such as the "first declared here" of a duplicate.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct DiagnosticNote {
    /// Byte range the note refers to; `None` for a note with no location of its own.
    #[serde(skip)]
    pub span: Option<Span>,
    /// Human-readable note text.
    pub message: String,
}

/// One finding emitted by a `check` pass.
///
/// Passes deduplicate findings through `Eq + Hash`, so every field must implement both.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
#[non_exhaustive]
pub struct Diagnostic {
    /// Stable code identifying the kind of finding.
    pub code: DiagnosticCode,
    /// Byte range the primary message points at.
    #[serde(skip)]
    pub span: Span,
    /// Primary message, after the code on the text output's first line.
    pub primary: String,
    /// Further notes, rendered as indented `note:` lines.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<DiagnosticNote>,
    /// Structured payload for machine-readable consumers, when the code has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<DiagnosticData>,
}

impl Diagnostic {
    /// Severity of this finding, read from [`DiagnosticCode::severity`].
    #[must_use]
    pub fn severity(&self) -> Severity {
        self.code.severity()
    }

    /// The primary span's start as a 1-based [`Position`]; use one [`LineStarts`] for many.
    #[must_use]
    pub fn position(&self, source: &str) -> Position {
        position_at(source, self.span.start)
    }

    /// Render against `source`, with 1-based positions for the primary span and each located note.
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

/// Sort `diagnostics` stably into source order by primary span.
pub(crate) fn sort_by_position(diagnostics: &mut [Diagnostic]) {
    diagnostics.sort_by_key(|d| (d.span.start, d.span.end));
}

/// JSON-friendly rendering of a [`Diagnostic`] with line/col populated.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RenderedDiagnostic {
    /// Stable code identifying the kind of finding.
    pub code: DiagnosticCode,
    /// Severity of the finding.
    pub severity: Severity,
    /// 1-based line of the primary span's first byte.
    pub line: u32,
    /// 1-based column of the primary span's first byte, in Unicode scalar values.
    pub col: u32,
    /// 1-based line of the primary span's end.
    pub end_line: u32,
    /// 1-based column of the primary span's end.
    pub end_col: u32,
    /// Primary message string.
    pub primary: String,
    /// Notes, each with line/col when it has a location.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<RenderedNote>,
    /// [`Diagnostic::data`], carried over.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<DiagnosticData>,
}

/// JSON-friendly rendering of a [`DiagnosticNote`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RenderedNote {
    /// 1-based line of the note's location, when it has one.
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
