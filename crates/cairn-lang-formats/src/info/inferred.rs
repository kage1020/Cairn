//! The range of versions an edition's packs imply for a build, with the notes on its edges and the
//! `@requires` floors that contradict it (`spec/versioning-editions` "The `inferred range` row",
//! "A floor is weighed against the registry").

use cairn_lang_core::check::LineStarts;
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::resolve::{
    InferredRange, LastDeclaredId, PaintedId, RequiresConflict, VersionFloor, VersionOrder,
};
use cairn_lang_core::{Edition, Resolution};

use super::Reporter;
use super::floors::declared_floors;
use crate::build::conflicts::{Bound, Conflict, Painted, bounds_at, conflicts, first_of_each};
use crate::build::run_level::requires_conflict;
use crate::registry::{RegistryPack, builtin_pack};

/// What one edition's oldest and newest carried versions say about the ids a build paints.
struct Inference {
    edition: Edition,
    /// Ids the oldest version lacks, each with the first version that declares it.
    first_declared: Vec<Bound>,
    /// Ids the newest version lacks, each with the last version that declares it.
    last_declared: Vec<Bound>,
    /// Ids no carried version declares.
    undeclared: Vec<Painted>,
}

impl Inference {
    /// Lower `ir` at the oldest and the newest of `considered`, which ascends.
    fn of(
        ir: &IntentModule,
        resolution: &Resolution,
        pack: &RegistryPack,
        edition: Edition,
        considered: &[String],
    ) -> Self {
        let order = pack.version_order();
        let mut inference = Self {
            edition,
            first_declared: Vec::new(),
            last_declared: Vec::new(),
            undeclared: Vec::new(),
        };
        let (Some(oldest), Some(newest)) = (considered.first(), considered.last()) else {
            return inference;
        };
        let (first_declared, mut undeclared) =
            bounds_at(ir, resolution, pack, oldest, &order, |l| l.since.clone());
        inference.first_declared = first_declared;
        if newest != oldest {
            let (last_declared, more) =
                bounds_at(ir, resolution, pack, newest, &order, |l| l.until.clone());
            inference.last_declared = last_declared;
            undeclared.extend(more);
        }
        inference.undeclared = first_of_each(undeclared, |painted| painted);
        inference
    }

    /// The lower edge and the ids that set it, or `None` when no id bounds it.
    fn min_edge(&self) -> Option<(&str, Vec<&Bound>)> {
        let key = self.first_declared.iter().map(|b| b.key).max()?;
        Some(edge(&self.first_declared, key))
    }

    /// The upper edge and the ids that set it, or `None` when no id bounds it.
    fn max_edge(&self) -> Option<(&str, Vec<&Bound>)> {
        let key = self.last_declared.iter().map(|b| b.key).min()?;
        Some(edge(&self.last_declared, key))
    }

    /// Whether the edges cross: one id first declared after another is last declared.
    fn crossed(&self) -> bool {
        let first = self.first_declared.iter().map(|b| b.key).max();
        let last = self.last_declared.iter().map(|b| b.key).min();
        matches!((first, last), (Some(first), Some(last)) if first > last)
    }

    /// The row as `--format json` carries it, with the floors above it.
    fn range(&self, source: &str, lines: &LineStarts, conflicts: &[Conflict]) -> InferredRange {
        let painted = |painted: &Painted| PaintedId {
            id: painted.id.clone(),
            position: lines.position(source, painted.span.start),
        };
        let edge_ids = |edge: Option<&(&str, Vec<&Bound>)>| {
            edge.map_or_else(Vec::new, |(_, ids)| {
                ids.iter().map(|b| painted(&b.painted)).collect()
            })
        };
        let (min, max) = (self.min_edge(), self.max_edge());
        InferredRange {
            edition: self.edition,
            min: min
                .as_ref()
                .map_or_else(|| "0.0".to_owned(), |(v, _)| (*v).to_owned()),
            max: max
                .as_ref()
                .map_or_else(|| "latest".to_owned(), |(v, _)| (*v).to_owned()),
            empty: !self.undeclared.is_empty() || self.crossed(),
            min_ids: edge_ids(min.as_ref()),
            max_ids: edge_ids(max.as_ref()),
            undeclared: self.undeclared.iter().map(painted).collect(),
            conflicts: conflicts
                .iter()
                .map(|conflict| RequiresConflict {
                    floor: declared_floors(source, lines, &[&conflict.floor])
                        .pop()
                        .expect("one floor in, one out"),
                    ids: conflict
                        .ids
                        .iter()
                        .map(|b| LastDeclaredId {
                            id: b.painted.id.clone(),
                            last_declared: b.version.clone(),
                            position: lines.position(source, b.painted.span.start),
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    /// The floors among `floors` that no carried version declaring every painted id clears.
    fn conflicts(&self, floors: &[VersionFloor], order: &VersionOrder) -> Vec<Conflict> {
        conflicts(&self.last_declared, floors, order)
    }

    /// The `note:` lines naming each id that sets an edge, and each no version declares.
    fn report_notes(&self, out: &mut Reporter<'_>) {
        let edition = self.edition.as_str();
        if let Some((version, ids)) = self.min_edge() {
            for b in ids {
                let line = format!(
                    "note: {}: {edition}'s inferred range starts at {version}, the first {edition} \
                     target that declares `{}`",
                    out.at(b.painted.span.start),
                    b.painted.id,
                );
                out.line(&line);
            }
        }
        if let Some((version, ids)) = self.max_edge() {
            for b in ids {
                let line = format!(
                    "note: {}: {edition}'s inferred range ends at {version}, the last {edition} \
                     target that declares `{}`",
                    out.at(b.painted.span.start),
                    b.painted.id,
                );
                out.line(&line);
            }
        }
        if let (true, Some((min, _)), Some((max, _))) =
            (self.crossed(), self.min_edge(), self.max_edge())
        {
            out.line(&format!(
                "note: {edition} has no inferred range: it would start at {min} and end at {max}"
            ));
        }
        for painted in &self.undeclared {
            let line = format!(
                "note: {}: {edition} has no inferred range: no {edition} target declares `{}`",
                out.at(painted.span.start),
                painted.id,
            );
            out.line(&line);
        }
    }
}

/// One edition's `inferred range` row for `cairn info`, its notes and conflicts sent to `out`.
pub(super) fn report_inferred_range(
    out: &mut Reporter<'_>,
    ir: &IntentModule,
    resolution: &Resolution,
    edition: Edition,
    considered: &[String],
    floors: &[VersionFloor],
) -> InferredRange {
    let pack = builtin_pack(edition);
    let inference = Inference::of(ir, resolution, pack, edition, considered);
    inference.report_notes(out);
    let conflicts = inference.conflicts(floors, &pack.version_order());
    // Reported, not refused: the build is the command that refuses it.
    report_conflicts(out, edition, &conflicts);
    inference.range(out.source, out.lines, &conflicts)
}

/// Report each conflict as `E_REQUIRES_CONFLICT`.
fn report_conflicts(out: &mut Reporter<'_>, edition: Edition, conflicts: &[Conflict]) {
    for conflict in conflicts {
        out.message(&requires_conflict(edition, conflict));
    }
}

/// The version at `key` and the bounds that sit there.
fn edge(bounds: &[Bound], key: i64) -> (&str, Vec<&Bound>) {
    let at: Vec<&Bound> = bounds.iter().filter(|b| b.key == key).collect();
    (at[0].version.as_str(), at)
}

#[cfg(test)]
mod tests;
