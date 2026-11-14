//! What a run reports beside its findings: the run-level refusals and warnings that name no
//! span, which `spec/lint` "Machine-readable payload" keeps out of the findings array.

use cairn_lang_core::Edition;
use cairn_lang_core::Severity;
use cairn_lang_core::check::{LineStarts, RenderedNote};
use cairn_lang_core::resolve::{VersionFloor, VersionOrder, versions_satisfying};
use serde::Serialize;

use super::conflicts::Conflict;
use super::floors::{
    DeclaredFloors, FloorRefusal, floor_keyword, floor_origin_note, floor_refusing_target,
};
use crate::bedrock_structure::ParityNote;
use crate::data_version::UnsupportedTarget;
use crate::registry::builtin_pack;
use crate::target::{ResolvedTarget, supported_versions};

/// Where a run-level message points, when it points anywhere.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunLevelAt {
    /// Nowhere: the message is about the command line or a structure.
    Nothing,
    /// The source as a whole.
    Source,
    /// The byte offset of the line the message is about.
    Offset(usize),
}

/// One run-level refusal or warning: its first line, then the lines under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunLevelMessage {
    /// Whether the message refuses the run.
    pub severity: Severity,
    /// The `E_*` / `W_*` code, when the message carries one.
    pub code: Option<&'static str>,
    /// Where it points.
    pub at: RunLevelAt,
    /// The first line's text, after its prefix.
    pub primary: String,
    /// The lines under the first.
    pub notes: Vec<RunLevelNote>,
}

/// One line under a run-level message's first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunLevelNote {
    /// A line as written after its indent: `note: ...`, `fix: ...`, or one continuing the message.
    Line(String),
    /// A `note:` about the line holding the byte offset `at`.
    At {
        /// The byte offset the note points at.
        at: usize,
        /// What the note says.
        note: String,
    },
}

impl RunLevelMessage {
    /// The message as gcc-style lines, its location written as `file` and a position in `source`.
    #[must_use]
    pub fn lines(&self, file: &str, source: &str, lines: &LineStarts) -> Vec<String> {
        let code = self
            .code
            .map(|code| format!("[{code}]"))
            .unwrap_or_default();
        let at = match self.at {
            RunLevelAt::Nothing => String::new(),
            RunLevelAt::Source => format!("{file}: "),
            RunLevelAt::Offset(offset) => format!("{file}:{}: ", lines.position(source, offset)),
        };
        let mut out = vec![format!(
            "{}{code}: {at}{}",
            self.severity.as_str(),
            self.primary
        )];
        out.extend(self.notes.iter().map(|note| match note {
            RunLevelNote::Line(line) => format!("  {line}"),
            RunLevelNote::At { at, note } => {
                format!("  note: {file}:{}: {note}", lines.position(source, *at))
            }
        }));
        out
    }

    /// The JSON form, shaped as a rendered finding is, with a position only when it has an offset.
    #[must_use]
    pub fn render(&self, source: &str, lines: &LineStarts) -> RenderedRunLevelMessage {
        let at = match self.at {
            RunLevelAt::Offset(offset) => Some(lines.position(source, offset)),
            RunLevelAt::Nothing | RunLevelAt::Source => None,
        };
        RenderedRunLevelMessage {
            code: self.code,
            severity: self.severity,
            line: at.map(|p| p.line.get()),
            col: at.map(|p| p.col.get()),
            primary: self.primary.clone(),
            notes: self
                .notes
                .iter()
                .map(|note| match note {
                    RunLevelNote::Line(line) => RenderedNote {
                        line: None,
                        col: None,
                        message: line.clone(),
                    },
                    RunLevelNote::At { at, note } => {
                        let position = lines.position(source, *at);
                        RenderedNote {
                            line: Some(position.line.get()),
                            col: Some(position.col.get()),
                            message: format!("note: {note}"),
                        }
                    }
                })
                .collect(),
        }
    }
}

/// The JSON form of a [`RunLevelMessage`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RenderedRunLevelMessage {
    /// The `E_*` / `W_*` code, when the message carries one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
    /// Whether the message refuses the run.
    pub severity: Severity,
    /// 1-based line of the line the message is about, when it is about one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    /// 1-based column of the same byte, in Unicode scalar values.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub col: Option<u32>,
    /// The first line's text.
    pub primary: String,
    /// The lines under it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<RenderedNote>,
}

/// `E_PARTIAL_BUILD`: the scopes a lowering lost, the line ending with `because`.
#[must_use]
pub fn partial_build(dropped: &[String], built: usize, because: &str) -> RunLevelMessage {
    RunLevelMessage {
        severity: Severity::Error,
        code: Some("E_PARTIAL_BUILD"),
        at: RunLevelAt::Source,
        primary: format!(
            "{} of {} requested scopes did not lower; {because}",
            dropped.len(),
            dropped.len() + built,
        ),
        notes: dropped
            .iter()
            .map(|scope| RunLevelNote::Line(format!("note: `{scope}` produced no voxels")))
            .collect(),
    }
}

/// A `--target` the edition's version table does not ship.
#[must_use]
pub fn unsupported_target(err: &UnsupportedTarget) -> RunLevelMessage {
    RunLevelMessage {
        severity: Severity::Error,
        code: None,
        at: RunLevelAt::Nothing,
        primary: err.to_string(),
        notes: Vec::new(),
    }
}

/// A structure `scope` the target's backend refuses, with the backend's reason.
#[must_use]
pub fn backend_refusal(scope: &str, reason: &str) -> RunLevelMessage {
    RunLevelMessage {
        severity: Severity::Error,
        code: None,
        at: RunLevelAt::Nothing,
        primary: format!("checking `{scope}`: {reason}"),
        notes: Vec::new(),
    }
}

/// Two structures, `first` and then `scope`, whose files would both be `file_name`.
#[must_use]
pub fn artifact_collision(file_name: &str, first: &str, scope: &str) -> RunLevelMessage {
    RunLevelMessage {
        severity: Severity::Error,
        code: None,
        at: RunLevelAt::Nothing,
        primary: format!(
            "output filename `{file_name}` collides between scopes `{first}` and `{scope}`"
        ),
        notes: Vec::new(),
    }
}

/// A palette entry's name, `id[key=value,...]`.
#[must_use]
pub fn palette_entry_name(id: &str, states: &str) -> String {
    format!("{id}[{states}]")
}

/// `W_INTENT_DEGRADED` for one intent the structure `scope` dropped.
#[must_use]
pub fn degraded(scope: &str, note: &ParityNote) -> RunLevelMessage {
    RunLevelMessage {
        severity: Severity::Warning,
        code: Some("W_INTENT_DEGRADED"),
        at: RunLevelAt::Nothing,
        primary: format!(
            "{scope}: {}: {}",
            palette_entry_name(&note.id, &note.states),
            note.dropped.reason,
        ),
        notes: Vec::new(),
    }
}

/// The refusal of a `--target` the source's floors rule out: `E_VERSION_CAP` below a floor,
/// `E_REQUIRES_UNORDERABLE` for a floor the edition cannot place.
#[must_use]
pub fn version_floor_refusal(
    declared: &DeclaredFloors,
    edition: Edition,
    target: &ResolvedTarget,
) -> Option<RunLevelMessage> {
    let DeclaredFloors { floors, order } = declared;
    let floor = match floor_refusing_target(floors, order, target)? {
        FloorRefusal::Unplaceable(floor) => return Some(unplaceable_floor(floor, edition, order)),
        FloorRefusal::Below(floor) => floor,
    };
    let mut notes = origin_note(floor);
    // `spec/versioning-editions` "Fail-loud and minimum-version inference": name the valid targets.
    let buildable = supported_versions(builtin_pack(edition));
    let usable = versions_satisfying(order, floors, &buildable);
    let lines = if usable.is_empty() {
        [
            format!(
                "no supported {} target satisfies it: {}",
                edition.as_str(),
                builtin_pack(edition).supported_list(),
            ),
            format!(
                "fix: lower the {} floor, or build against another edition",
                floor_keyword(floor),
            ),
        ]
    } else {
        [
            format!("valid {} targets: {}", edition.as_str(), usable.join(", ")),
            format!(
                "fix: --target {}, or lower the {} floor",
                usable[0],
                floor_keyword(floor),
            ),
        ]
    };
    notes.extend(lines.map(RunLevelNote::Line));
    Some(RunLevelMessage {
        severity: Severity::Error,
        code: Some("E_VERSION_CAP"),
        at: RunLevelAt::Offset(floor.span.start),
        primary: format!(
            "{} requires {} (target {}).",
            floor.declarer(),
            floor.rendered(),
            target.mc_version(),
        ),
        notes,
    })
}

/// `E_REQUIRES_UNORDERABLE`: a floor the target edition's table cannot place.
fn unplaceable_floor(
    floor: &VersionFloor,
    edition: Edition,
    order: &VersionOrder,
) -> RunLevelMessage {
    let mut notes = origin_note(floor);
    notes.push(RunLevelNote::Line(match order.neighbours(&floor.version) {
        (Some(below), Some(above)) => format!(
            "{edition_name} ships {below} and then {above}, and nothing between them; Cairn \
             orders versions by DataVersion and a label that names no release has none",
            edition_name = edition.as_str(),
        ),
        _ => format!(
            "{} releases: {}",
            edition.as_str(),
            order
                .rows()
                .map(|(label, _)| label)
                .collect::<Vec<_>>()
                .join(", "),
        ),
    }));
    // Offer an edition scope only where the other edition names the label; elsewhere the floor
    // would go inert.
    let other = match edition {
        Edition::Java => Edition::Bedrock,
        Edition::Bedrock => Edition::Java,
    };
    let other_edition_names_it = floor.edition.is_none()
        && builtin_pack(other)
            .version_order()
            .place(&floor.version)
            .names_release();
    if other_edition_names_it {
        notes.push(RunLevelNote::Line(format!(
            "`{}` is a {} release; if that is the numbering this floor is written in, say so",
            floor.version,
            other.as_str(),
        )));
        notes.push(RunLevelNote::Line(format!(
            "fix: `@requires {} version>={}`, or name a {} release",
            other.as_str(),
            floor.version,
            edition.as_str(),
        )));
    } else {
        notes.push(RunLevelNote::Line(format!(
            "fix: name a {} release",
            edition.as_str()
        )));
    }
    RunLevelMessage {
        severity: Severity::Error,
        code: Some("E_REQUIRES_UNORDERABLE"),
        at: RunLevelAt::Offset(floor.span.start),
        primary: format!(
            "{} requires {}, which names no {} release.",
            floor.declarer(),
            floor.rendered(),
            edition.as_str(),
        ),
        notes,
    }
}

/// `E_REQUIRES_CONFLICT`: a floor that admits no `edition` target declaring every id the build
/// paints.
#[must_use]
pub fn requires_conflict(edition: Edition, conflict: &Conflict) -> RunLevelMessage {
    let Conflict { floor, ids } = conflict;
    let edition = edition.as_str();
    let bounding = &ids[0];
    let mut notes = origin_note(floor);
    notes.extend(ids.iter().map(|b| RunLevelNote::At {
        at: b.painted.span.start,
        note: format!(
            "`{}` is last declared at {edition} {}",
            b.painted.id, b.version
        ),
    }));
    let replaced: Vec<String> = ids.iter().map(|b| format!("`{}`", b.painted.id)).collect();
    notes.push(RunLevelNote::Line(format!(
        "fix: lower the {} floor to {} or below, or replace {} with {} a {edition} target at or \
         above {} declares",
        floor_keyword(floor),
        bounding.version,
        replaced.join(", "),
        if ids.len() == 1 { "a block" } else { "blocks" },
        floor.version,
    )));
    RunLevelMessage {
        severity: Severity::Error,
        code: Some("E_REQUIRES_CONFLICT"),
        at: RunLevelAt::Offset(floor.span.start),
        primary: format!(
            "{} requires {}, but no {edition} target it admits declares `{}`, which this build \
             paints.",
            floor.declarer(),
            floor.rendered(),
            bounding.painted.id,
        ),
        notes,
    }
}

/// The line naming the part that declared `floor`, when a part did.
fn origin_note(floor: &VersionFloor) -> Vec<RunLevelNote> {
    floor_origin_note(floor)
        .map(RunLevelNote::Line)
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests;
