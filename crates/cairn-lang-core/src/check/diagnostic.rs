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
#[non_exhaustive]
pub enum DiagnosticCode {
    /// More than one `size=` argument in a struct or def header.
    DuplicateSize,
    /// Repeated `slot NAME` line in a single `theme` block.
    DuplicateSlot,
    /// Repeated `key=` in the same argument list (struct/def header,
    /// statement args, selector attrs / bindings).
    DuplicateArg,
    /// Two or more members in the same immediate body share an `id=`.
    DuplicateId,
    /// A statement keyword not in the known-keyword table.
    UnknownKeyword,
    /// `id=`, `class=`, or `mat_slot=` whose value is not a label
    /// (identifier or string).
    TypeMismatchLabel,
    /// `size=` whose value is not a `WxH` literal.
    TypeMismatchSize,
    /// `mat_slot=NAME` references a slot the applied theme does not declare.
    UnresolvedSlot,
    /// `slot NAME -> VALUE` whose VALUE is neither a canonical nor an
    /// abstract material token (see `spec/materials-themes.md` §7.2).
    UnknownSlotTarget,
    /// `theme` selector rule that does not match any member in the file.
    ThemeSelectorUnmatched,
    /// A member role the block-array lowering pass does not yet handle
    /// (door/window/roof/...). Surfaces during `cairn lower` so a partial
    /// build is still inspectable, rather than failing the whole module.
    DeferredMember,
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
    /// `seen_place_ids` but never finished lifting into `place_def` (the
    /// `place` row was silently skipped by `resolve_site_placements` for a
    /// missing `use=` or `theme=`, neither of which currently has its own
    /// upstream diagnostic). Without this cascade warning the walkway would
    /// vanish from the build and the user would see no signal that the
    /// `connect` did nothing; mirrors the `W_DEFERRED_MEMBER` pattern used
    /// by walkway endpoint cascades in `block_array::lower`.
    DeferredConnect,
    /// A `connect` row whose site / place / port identifier contains the
    /// `__` substring. The surface lexer permits `_` in identifiers, but
    /// the canonical walkway scope key uses `__` as the `from`/`to`
    /// separator — so `(home, b__c, home2, entry)` and
    /// `(home, b, c__home2, entry)` would otherwise encode to the same
    /// wire string. Lowering drops the row and asks the user to rename
    /// the offending segment so the encoding stays unambiguous.
    InvalidWalkwayIdent,
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
            Self::DuplicateArg => "E_DUPLICATE_ARG",
            Self::DuplicateId => "E_DUPLICATE_ID",
            Self::UnknownKeyword => "E_UNKNOWN_KEYWORD",
            Self::TypeMismatchLabel => "E_TYPE_MISMATCH_LABEL",
            Self::TypeMismatchSize => "E_TYPE_MISMATCH_SIZE",
            Self::UnresolvedSlot => "E_UNRESOLVED_SLOT",
            Self::UnknownSlotTarget => "E_UNKNOWN_SLOT_TARGET",
            Self::ThemeSelectorUnmatched => "E_THEME_SELECTOR_UNMATCHED",
            Self::DeferredMember => "W_DEFERRED_MEMBER",
            Self::NoThemeBound => "W_NO_THEME_BOUND",
            Self::AbstractTokenDeferred => "W_ABSTRACT_TOKEN_DEFERRED",
            Self::UnknownAbstractToken => "E_UNKNOWN_ABSTRACT_TOKEN",
            Self::StructNoSize => "W_STRUCT_NO_SIZE",
            Self::DefNoSize => "W_DEF_NO_SIZE",
            Self::UnresolvedPlaceRef => "E_UNRESOLVED_PLACE_REF",
            Self::UnresolvedThemeRef => "E_UNRESOLVED_THEME_REF",
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
        }
    }

    /// Severity assigned to this code.
    ///
    /// Errors are silent-substitution-style problems that would otherwise
    /// feed bad data into later passes; warnings are advisory drift that
    /// does not block a build. See `spec/lint.md` §11.3 for the rule.
    /// The block-array lowering warnings (`W_DEFERRED_MEMBER`,
    /// `W_NO_THEME_BOUND`, `W_ABSTRACT_TOKEN_DEFERRED`, `W_STRUCT_NO_SIZE`)
    /// each mark a partial-build degradation rather than an unsalvageable
    /// input. `E_UNKNOWN_ABSTRACT_TOKEN` is the one lowering code that is
    /// an `Error`: when a registry pack *was* offered but does not declare
    /// the bound token, silently falling back to air would hide a typo
    /// the pack author needs to fix (spec §7.2's fail-loud rule).
    #[must_use]
    pub fn severity(self) -> Severity {
        match self {
            Self::DuplicateSize
            | Self::DuplicateSlot
            | Self::DuplicateArg
            | Self::DuplicateId
            | Self::UnknownKeyword
            | Self::TypeMismatchLabel
            | Self::TypeMismatchSize
            | Self::UnresolvedSlot
            | Self::UnknownAbstractToken
            | Self::UnresolvedPlaceRef
            | Self::UnresolvedThemeRef
            | Self::DuplicatePlaceId
            | Self::InvalidPlaceOrigin
            | Self::UnresolvedPort
            | Self::AmbiguousPort
            | Self::MissingPathMaterial => Severity::Error,
            Self::UnknownSlotTarget
            | Self::ThemeSelectorUnmatched
            | Self::DeferredMember
            | Self::NoThemeBound
            | Self::AbstractTokenDeferred
            | Self::StructNoSize
            | Self::DefNoSize
            | Self::UnusedDef
            | Self::WalkwayBlocked
            | Self::DuplicateWalkway
            | Self::DeferredConnect
            | Self::InvalidWalkwayIdent => Severity::Warning,
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
    /// Severity of the finding.
    pub severity: Severity,
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
    /// Convert this diagnostic's primary byte span into a 1-based
    /// `line:column` [`Position`] against the given source string.
    ///
    /// Lines are split on `\n` (covering both LF and CRLF — the trailing
    /// `\r` adds one column the user sees an extra column for, but the
    /// behaviour matches what `cairn parse` reports today). Column counts
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
            severity: self.severity,
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
    /// offset 0; subsequent entries are the byte after each `\n`).
    starts: Vec<usize>,
}

impl LineStarts {
    /// Build the index by walking the source exactly once.
    #[must_use]
    pub fn new(source: &str) -> Self {
        let mut starts = vec![0];
        for (i, byte) in source.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(i + 1);
            }
        }
        Self { starts }
    }

    /// Resolve a byte offset into a 1-based `line:column` [`Position`].
    ///
    /// Identical semantics to [`position_at`]; this method does the
    /// expensive part (counting newlines) up front, then runs a binary
    /// search per query.
    #[must_use]
    pub fn position(&self, source: &str, byte_offset: usize) -> Position {
        let clamped = byte_offset.min(source.len());
        // partition_point returns the first index whose start > clamped;
        // line numbers are 1-based and the starts vector is 1-aligned with
        // them, so the returned index *is* the line number.
        let line_idx = self.starts.partition_point(|&s| s <= clamped);
        let line_number = line_idx.max(1);
        let line_start = self.starts[line_idx - 1];
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
/// `usize → u32` overflow saturates to `u32::MAX` rather than wrapping: any
/// source large enough to exceed 4 billion lines is also large enough that
/// "we lost track of the exact column" is the user's last concern.
///
/// O(`source.len()`) per call. Prefer [`LineStarts`] when converting many
/// offsets from the same source.
#[must_use]
pub fn position_at(source: &str, byte_offset: usize) -> Position {
    let clamped = byte_offset.min(source.len());
    let prefix = &source[..clamped];
    let line_count = prefix.bytes().filter(|b| *b == b'\n').count() + 1;
    let last_line_start = prefix.rfind('\n').map_or(0, |i| i + 1);
    let column_chars = source[last_line_start..clamped].chars().count() + 1;
    let line =
        NonZeroU32::new(u32::try_from(line_count).unwrap_or(u32::MAX)).unwrap_or(NonZeroU32::MIN);
    let col =
        NonZeroU32::new(u32::try_from(column_chars).unwrap_or(u32::MAX)).unwrap_or(NonZeroU32::MIN);
    Position { line, col }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_as_str_round_trips_for_every_variant() {
        // Every code's string form starts with `E_`; the prefix is part of
        // the diagnostic contract surface and downstream matchers depend on
        // it. The block-array lowering codes use the `W_` prefix instead
        // and are covered by `block_array_codes_use_w_prefix`. Severity
        // expectations live in `code_severity_matches_spec`.
        for code in [
            DiagnosticCode::DuplicateSize,
            DiagnosticCode::DuplicateSlot,
            DiagnosticCode::DuplicateArg,
            DiagnosticCode::DuplicateId,
            DiagnosticCode::UnknownKeyword,
            DiagnosticCode::TypeMismatchLabel,
            DiagnosticCode::TypeMismatchSize,
            DiagnosticCode::UnresolvedSlot,
            DiagnosticCode::UnknownSlotTarget,
            DiagnosticCode::ThemeSelectorUnmatched,
            DiagnosticCode::UnknownAbstractToken,
            DiagnosticCode::UnresolvedPlaceRef,
            DiagnosticCode::UnresolvedThemeRef,
            DiagnosticCode::DuplicatePlaceId,
            DiagnosticCode::InvalidPlaceOrigin,
            DiagnosticCode::UnresolvedPort,
            DiagnosticCode::AmbiguousPort,
            DiagnosticCode::MissingPathMaterial,
        ] {
            let s = code.as_str();
            assert!(
                s.starts_with("E_"),
                "code {code:?} should render to an E_-prefixed string, got {s}",
            );
        }
    }

    #[test]
    fn block_array_codes_use_w_prefix() {
        // The block-array lowering warnings opt into a distinct `W_` prefix
        // so LSP quick-fixes and CI annotators can tell partial-build
        // degradations apart from the older `E_` warnings without having to
        // re-decide severity. Locking the stable string form here makes the
        // next addition fail loud if it lands with the wrong name.
        for (code, expected) in [
            (DiagnosticCode::DeferredMember, "W_DEFERRED_MEMBER"),
            (DiagnosticCode::NoThemeBound, "W_NO_THEME_BOUND"),
            (
                DiagnosticCode::AbstractTokenDeferred,
                "W_ABSTRACT_TOKEN_DEFERRED",
            ),
            (DiagnosticCode::StructNoSize, "W_STRUCT_NO_SIZE"),
            (DiagnosticCode::DefNoSize, "W_DEF_NO_SIZE"),
            (DiagnosticCode::UnusedDef, "W_UNUSED_DEF"),
            (DiagnosticCode::WalkwayBlocked, "W_WALKWAY_BLOCKED"),
            (DiagnosticCode::DuplicateWalkway, "W_DUPLICATE_WALKWAY"),
            (DiagnosticCode::DeferredConnect, "W_DEFERRED_CONNECT"),
            (
                DiagnosticCode::InvalidWalkwayIdent,
                "W_INVALID_WALKWAY_IDENT",
            ),
        ] {
            assert_eq!(code.as_str(), expected, "{code:?}");
        }
    }

    #[test]
    fn code_severity_matches_spec() {
        // Errors block a build; warnings are advisory. The split here mirrors
        // `spec/lint.md` §11.3.
        for code in [
            DiagnosticCode::DuplicateSize,
            DiagnosticCode::DuplicateSlot,
            DiagnosticCode::DuplicateArg,
            DiagnosticCode::DuplicateId,
            DiagnosticCode::UnknownKeyword,
            DiagnosticCode::TypeMismatchLabel,
            DiagnosticCode::TypeMismatchSize,
            DiagnosticCode::UnresolvedSlot,
            DiagnosticCode::UnknownAbstractToken,
            DiagnosticCode::UnresolvedPlaceRef,
            DiagnosticCode::UnresolvedThemeRef,
            DiagnosticCode::DuplicatePlaceId,
            DiagnosticCode::InvalidPlaceOrigin,
            DiagnosticCode::UnresolvedPort,
            DiagnosticCode::AmbiguousPort,
            DiagnosticCode::MissingPathMaterial,
        ] {
            assert_eq!(code.severity(), Severity::Error, "{code:?}");
        }
        for code in [
            DiagnosticCode::UnknownSlotTarget,
            DiagnosticCode::ThemeSelectorUnmatched,
            DiagnosticCode::DeferredMember,
            DiagnosticCode::NoThemeBound,
            DiagnosticCode::AbstractTokenDeferred,
            DiagnosticCode::StructNoSize,
            DiagnosticCode::DefNoSize,
            DiagnosticCode::UnusedDef,
            DiagnosticCode::WalkwayBlocked,
            DiagnosticCode::DuplicateWalkway,
            DiagnosticCode::DeferredConnect,
            DiagnosticCode::InvalidWalkwayIdent,
        ] {
            assert_eq!(code.severity(), Severity::Warning, "{code:?}");
        }
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
        // AC1 from issue #40: the structured payload must surface as
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
    fn rendered_diagnostic_omits_data_key_when_payload_absent() {
        // AC3: `data: None` must serialise to *no key at all* so existing
        // JSON consumers that did not opt into the new field keep working.
        let lines = LineStarts::new("abc\n");
        let diag = Diagnostic {
            code: DiagnosticCode::DuplicateSize,
            severity: Severity::Error,
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
        // AC2 boundary at the render layer: a `Diagnostic` carrying a
        // payload must lift it into `RenderedDiagnostic` so the JSON
        // formatter (and any other consumer of `render`) sees the same
        // structured data the in-memory finding holds.
        let lines = LineStarts::new("abc\n");
        let diag = Diagnostic {
            code: DiagnosticCode::WalkwayBlocked,
            severity: Severity::Warning,
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
    fn line_starts_returns_identical_positions_to_position_at() {
        // Soak the equivalence — for any char-boundary offset, the cached
        // and linear implementations agree. Locks the optimisation in step
        // with the (already-tested) reference.
        let source = "α\nfoo\nbar\nβaz\n";
        let lines = LineStarts::new(source);
        // Only char-boundary offsets: byte 0 (start of α), 2 (after α =
        // start of \n), 3 (start of 'f'), 6 (start of \n), 7 (start of 'b'),
        // 10 (start of \n), 11 (start of β), 13 (after β), and EOF.
        for offset in [0_usize, 2, 3, 6, 7, 10, 11, 13, source.len()] {
            assert!(
                source.is_char_boundary(offset),
                "test bug: offset {offset} is not a char boundary",
            );
            let cached = lines.position(source, offset);
            let linear = position_at(source, offset);
            assert_eq!(
                cached, linear,
                "offset {offset} disagrees: cached={cached:?} linear={linear:?}",
            );
        }
    }
}
