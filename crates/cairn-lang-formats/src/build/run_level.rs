//! What a run reports beside its findings: the run-level refusals and warnings that name no
//! span, which `spec/lint` "Machine-readable payload" keeps out of the findings array.

use cairn_lang_core::Edition;
use cairn_lang_core::Severity;
use cairn_lang_core::check::{LineStarts, RenderedNote};
use cairn_lang_core::resolve::{FloorOrigin, VersionFloor, VersionOrder, versions_satisfying};
use serde::Serialize;

use super::floors::{DeclaredFloors, FloorRefusal, floor_origin_note, floor_refusing_target};
use crate::bedrock_structure::ParityNote;
use crate::data_version::UnsupportedTarget;
use crate::portability::InvalidPalette;
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
    /// The lines under the first, without their indent.
    pub notes: Vec<String>,
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
        out.extend(self.notes.iter().map(|note| format!("  {note}")));
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
                .map(|note| RenderedNote {
                    line: None,
                    col: None,
                    message: note.clone(),
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
            .map(|scope| format!("note: `{scope}` produced no voxels"))
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

/// The refusal of one edition's portability row, over a palette a registry pack should refuse.
#[must_use]
pub fn invalid_palette(edition: Edition, invalid: &InvalidPalette) -> RunLevelMessage {
    let mut notes: Vec<String> = invalid
        .leaks()
        .iter()
        .map(|leak| format!("error: {leak}"))
        .collect();
    notes.push(
        "note: a validated pack cannot produce these, so each is either a state literal in the \
         source — a property the Java block does not have, or a value outside its domain, which \
         `E_STATE_DOMAIN` refuses at a pinned target but these figures' range-wide lowering does \
         not check — or a leak in the pack or this compiler. The figure is withheld rather than \
         counting a validation gap as ordinary portability"
            .to_owned(),
    );
    RunLevelMessage {
        severity: Severity::Error,
        code: None,
        at: RunLevelAt::Nothing,
        primary: format!(
            "the {} palette carries blockstates a registry pack is expected to refuse, so this \
             edition gets no portability figure:",
            edition.as_str(),
        ),
        notes,
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
    let mut notes: Vec<String> = floor_origin_note(floor).into_iter().collect();
    // `spec/versioning-editions` "Fail-loud and minimum-version inference": name the valid targets.
    let buildable = supported_versions(builtin_pack(edition));
    let usable = versions_satisfying(order, floors, &buildable);
    if usable.is_empty() {
        notes.push(format!(
            "no supported {} target satisfies it: {}",
            edition.as_str(),
            builtin_pack(edition).supported_list(),
        ));
        notes.push(format!(
            "fix: lower the {} floor, or build against another edition",
            floor_keyword(floor),
        ));
    } else {
        notes.push(format!(
            "valid {} targets: {}",
            edition.as_str(),
            usable.join(", ")
        ));
        notes.push(format!(
            "fix: --target {}, or lower the {} floor",
            usable[0],
            floor_keyword(floor),
        ));
    }
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

fn floor_keyword(floor: &VersionFloor) -> &'static str {
    match floor.origin {
        FloorOrigin::Module => "`@requires`",
        FloorOrigin::Def(_) | FloorOrigin::Theme(_) => "`requires`",
    }
}

/// `E_REQUIRES_UNORDERABLE`: a floor the target edition's table cannot place.
fn unplaceable_floor(
    floor: &VersionFloor,
    edition: Edition,
    order: &VersionOrder,
) -> RunLevelMessage {
    let mut notes: Vec<String> = floor_origin_note(floor).into_iter().collect();
    notes.push(match order.neighbours(&floor.version) {
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
    });
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
        notes.push(format!(
            "`{}` is a {} release; if that is the numbering this floor is written in, say so",
            floor.version,
            other.as_str(),
        ));
        notes.push(format!(
            "fix: `@requires {} version>={}`, or name a {} release",
            other.as_str(),
            floor.version,
            edition.as_str(),
        ));
    } else {
        notes.push(format!("fix: name a {} release", edition.as_str()));
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

#[cfg(test)]
mod tests;
