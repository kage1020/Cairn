//! The range of versions an edition's packs imply for a build, and the `@requires` floors that
//! contradict it (`spec/versioning-editions` "The `inferred range` row", "A floor is weighed
//! against the registry").

use std::path::Path;

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::check::{DiagnosticData, LineStarts};
use cairn_lang_core::intent::IntentModule;
use cairn_lang_core::resolve::{
    FloorVerdict, InferredRange, LastDeclaredId, PaintedId, RequiresConflict, VersionFloor,
    VersionOrder,
};
use cairn_lang_core::{Diagnostic, Edition, Resolution, Span};
use cairn_lang_formats::registry::{RegistryPack, builtin_pack};

use crate::floors::{declared_floors, floor_keyword, floor_origin_note};
use crate::report::Reporting;

/// An id a build paints, at the first place it paints it.
#[derive(Debug, Clone)]
struct Painted {
    id: String,
    span: Span,
}

/// A painted id with the carried version that bounds it.
#[derive(Debug, Clone)]
pub(crate) struct Bound {
    painted: Painted,
    version: String,
    key: i64,
}

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

/// A floor no version declaring every painted id clears, with the ids last declared below it.
pub(crate) struct Conflict {
    floor: VersionFloor,
    /// The earliest `version` first, then in source order; never empty.
    ids: Vec<Bound>,
}

/// One id a pinned lowering found its version lacks.
struct Lacked {
    painted: Painted,
    since: Option<String>,
    until: Option<String>,
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
    fn report_notes(&self, file: &Path, source: &str, lines: &LineStarts) {
        let edition = self.edition.as_str();
        let at = |painted: &Painted| {
            format!(
                "{}:{}",
                file.display(),
                lines.position(source, painted.span.start)
            )
        };
        if let Some((version, ids)) = self.min_edge() {
            for b in ids {
                eprintln!(
                    "note: {}: {edition}'s inferred range starts at {version}, the first {edition} \
                     target that declares `{}`",
                    at(&b.painted),
                    b.painted.id,
                );
            }
        }
        if let Some((version, ids)) = self.max_edge() {
            for b in ids {
                eprintln!(
                    "note: {}: {edition}'s inferred range ends at {version}, the last {edition} \
                     target that declares `{}`",
                    at(&b.painted),
                    b.painted.id,
                );
            }
        }
        if let (true, Some((min, _)), Some((max, _))) =
            (self.crossed(), self.min_edge(), self.max_edge())
        {
            eprintln!(
                "note: {edition} has no inferred range: it would start at {min} and end at {max}"
            );
        }
        for painted in &self.undeclared {
            eprintln!(
                "note: {}: {edition} has no inferred range: no {edition} target declares `{}`",
                at(painted),
                painted.id,
            );
        }
    }
}

/// One edition's `inferred range` row for `cairn info`, its notes and conflicts printed on stderr.
pub(crate) fn report_inferred_range(
    reporting: Reporting<'_>,
    ir: &IntentModule,
    resolution: &Resolution,
    edition: Edition,
    considered: &[String],
    floors: &[VersionFloor],
) -> InferredRange {
    let Reporting {
        file,
        source,
        lines,
        ..
    } = reporting;
    let pack = builtin_pack(edition);
    let inference = Inference::of(ir, resolution, pack, edition, considered);
    inference.report_notes(file, source, lines);
    let conflicts = inference.conflicts(floors, &pack.version_order());
    // Reported, not refused: the build is the command that refuses it.
    report_conflicts(file, source, lines, edition, &conflicts);
    inference.range(source, lines, &conflicts)
}

/// The floors among `floors` above the last version declaring an id the newest of `considered`
/// lacks.
pub(crate) fn requires_conflicts(
    ir: &IntentModule,
    resolution: &Resolution,
    pack: &RegistryPack,
    considered: &[String],
    floors: &[VersionFloor],
    order: &VersionOrder,
) -> Vec<Conflict> {
    let Some(newest) = considered.last() else {
        return Vec::new();
    };
    let (last_declared, _) = bounds_at(ir, resolution, pack, newest, order, |l| l.until.clone());
    conflicts(&last_declared, floors, order)
}

fn conflicts(
    last_declared: &[Bound],
    floors: &[VersionFloor],
    order: &VersionOrder,
) -> Vec<Conflict> {
    floors
        .iter()
        .filter_map(|floor| {
            // An unplaceable floor is `E_REQUIRES_UNORDERABLE`'s, and its verdict is never `Below`.
            let mut ids: Vec<Bound> = last_declared
                .iter()
                .filter(|b| order.verdict(&floor.version, b.key) == FloorVerdict::Below)
                .cloned()
                .collect();
            ids.sort_by_key(|b| (b.key, b.painted.span.start));
            (!ids.is_empty()).then(|| Conflict {
                floor: floor.clone(),
                ids,
            })
        })
        .collect()
}

/// Report each conflict as `E_REQUIRES_CONFLICT`; whether there was one.
pub(crate) fn report_conflicts(
    file: &Path,
    source: &str,
    lines: &LineStarts,
    edition: Edition,
    conflicts: &[Conflict],
) -> bool {
    let edition = edition.as_str();
    for Conflict { floor, ids } in conflicts {
        let bounding = &ids[0];
        eprintln!(
            "error[E_REQUIRES_CONFLICT]: {}:{}: {} requires {}, but no {edition} target it \
             admits declares `{}`, which this build paints.",
            file.display(),
            lines.position(source, floor.span.start),
            floor.declarer(),
            floor.rendered(),
            bounding.painted.id,
        );
        if let Some(note) = floor_origin_note(floor) {
            eprintln!("{note}");
        }
        for b in ids {
            eprintln!(
                "  note: {}:{}: `{}` is last declared at {edition} {}",
                file.display(),
                lines.position(source, b.painted.span.start),
                b.painted.id,
                b.version,
            );
        }
        let replaced: Vec<String> = ids.iter().map(|b| format!("`{}`", b.painted.id)).collect();
        eprintln!(
            "  fix: lower the {} floor to {} or below, or replace {} with {} a {edition} target at \
             or above {} declares",
            floor_keyword(floor),
            bounding.version,
            replaced.join(", "),
            if ids.len() == 1 { "a block" } else { "blocks" },
            floor.version,
        );
    }
    !conflicts.is_empty()
}

/// The ids a lowering pinned at `version` refuses as undeclared: those `side` finds another
/// version declaring, each bounded there and once, then those no version declares.
fn bounds_at(
    ir: &IntentModule,
    resolution: &Resolution,
    pack: &RegistryPack,
    version: &str,
    order: &VersionOrder,
    side: impl Fn(&Lacked) -> Option<String>,
) -> (Vec<Bound>, Vec<Painted>) {
    let pinned = lower_to_block_array(ir, resolution, Some(&pack.view(Some(version))));
    let mut bounds = Vec::new();
    let mut undeclared = Vec::new();
    for lacked in pinned.diagnostics.iter().filter_map(lacked) {
        match side(&lacked) {
            Some(declaring) => bounds.push(bound(order, lacked.painted, declaring)),
            None => undeclared.push(lacked.painted),
        }
    }
    (first_of_each(bounds, |b| &b.painted), undeclared)
}

/// The id `d` refuses as undeclared at its pin, with the versions nearest it that declare it.
fn lacked(d: &Diagnostic) -> Option<Lacked> {
    let (id, since, until) = match d.data.as_ref()? {
        DiagnosticData::UnknownId {
            id, since, until, ..
        } => (id, since.clone(), until.clone()),
        DiagnosticData::ParityUnsupported { id, .. } => (id, None, None),
        _ => return None,
    };
    Some(Lacked {
        painted: Painted {
            id: id.clone(),
            span: d.span.clone(),
        },
        since,
        until,
    })
}

fn bound(order: &VersionOrder, painted: Painted, version: String) -> Bound {
    let key = order
        .key_of(&version)
        .expect("a declaring version is one the pack carries");
    Bound {
        painted,
        version,
        key,
    }
}

/// Each id once, at the first place the build paints it, in source order.
fn first_of_each<T>(mut items: Vec<T>, painted: impl Fn(&T) -> &Painted) -> Vec<T> {
    items.sort_by_key(|item| painted(item).span.start);
    let mut seen: Vec<String> = Vec::new();
    items.retain(|item| {
        let id = &painted(item).id;
        if seen.contains(id) {
            return false;
        }
        seen.push(id.clone());
        true
    });
    items
}

/// The version at `key` and the bounds that sit there.
fn edge(bounds: &[Bound], key: i64) -> (&str, Vec<&Bound>) {
    let at: Vec<&Bound> = bounds.iter().filter(|b| b.key == key).collect();
    (at[0].version.as_str(), at)
}
