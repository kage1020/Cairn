//! `W_EDIT_RIPPLE`: a member no `edit` selected whose `resolved_state` the edits change
//! (`spec/lint` "Semantic categories", edit stability).

use std::collections::{BTreeMap, HashSet};

use indexmap::IndexMap;

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::intent::EditIr;

use super::super::{BlockArrayIr, BlockState, ResolvedMember};

/// A cell, `(x, y, z)`, in the edited build's coordinates.
type Cell = (i64, i64, i64);

/// One finding per member that no edit selected and whose cells resolve differently in `edited`
/// than in `written`, the same module built without its edits, naming the first such cell in
/// block-array order.
pub(super) fn edit_ripples(
    edits: &[EditIr],
    edited: &BlockArrayIr,
    written: &BlockArrayIr,
) -> Vec<Diagnostic> {
    let selected: HashSet<usize> = edits
        .iter()
        .flat_map(|edit| &edit.selected)
        .map(|span| span.start)
        .collect();
    let mut out = Vec::new();
    for (key, after) in &edited.members {
        let (Some(before), Some(after_array), Some(before_array)) = (
            written.members.get(key),
            edited.structures.get(key),
            written.structures.get(key),
        ) else {
            continue;
        };
        // An `overhang=` an edit changes widens the box on each side alike.
        let shift = (
            (i64::from(after_array.dims.x) - i64::from(before_array.dims.x)) / 2,
            (i64::from(after_array.dims.z) - i64::from(before_array.dims.z)) / 2,
        );
        let after_cells = cells_by_member(after, (0, 0));
        let before_cells = cells_by_member(before, shift);
        // A cell an edited member holds in either build is the edit's own doing.
        let edited_cells: HashSet<Cell> = after_cells
            .iter()
            .chain(&before_cells)
            .filter(|(line, _)| selected.contains(&line.start))
            .flat_map(|(_, held)| held.keys().copied())
            .collect();
        let touching: Vec<&EditIr> = edits
            .iter()
            .filter(|edit| {
                edit.selected
                    .iter()
                    .any(|span| after_cells.contains_key(span) || before_cells.contains_key(span))
            })
            .collect();
        let empty = BTreeMap::new();
        let mut judged = HashSet::new();
        for record in after.iter().chain(before) {
            if selected.contains(&record.span.start) || !judged.insert(record.span.start) {
                continue;
            }
            let was = before_cells.get(&record.span).unwrap_or(&empty);
            let is = after_cells.get(&record.span).unwrap_or(&empty);
            let mut cells: Vec<Cell> = was.keys().chain(is.keys()).copied().collect();
            cells.sort_unstable_by_key(|&(x, y, z)| (y, z, x));
            cells.dedup();
            let changed: Vec<(Cell, Option<&BlockState>, Option<&BlockState>)> = cells
                .into_iter()
                .filter(|cell| !edited_cells.contains(cell))
                .map(|cell| (cell, was.get(&cell).copied(), is.get(&cell).copied()))
                .filter(|(_, was, is)| was != is)
                .collect();
            if let Some(first) = changed.first() {
                out.push(ripple(record, key, changed.len(), *first, &touching));
            }
        }
    }
    out
}

/// Each member's cells and the state each holds, keyed by its line, shifted by `(dx, dz)`.
fn cells_by_member(
    records: &[ResolvedMember],
    (dx, dz): (i64, i64),
) -> IndexMap<Span, BTreeMap<Cell, &BlockState>> {
    let mut out: IndexMap<Span, BTreeMap<Cell, &BlockState>> = IndexMap::new();
    for record in records {
        let held = out.entry(record.span.clone()).or_default();
        for block in &record.resolved_state {
            for (x, y, z) in block.cells() {
                held.insert(
                    (i64::from(x) + dx, i64::from(y), i64::from(z) + dz),
                    &block.state,
                );
            }
        }
    }
    out
}

fn state_text(state: Option<&BlockState>) -> String {
    match state {
        None => "none of its blocks".to_owned(),
        Some(state) if state.properties.is_empty() => format!("`{}`", state.id),
        Some(state) => format!("`{}[{}]`", state.id, state.properties_text()),
    }
}

fn ripple(
    record: &ResolvedMember,
    key: &str,
    count: usize,
    (cell, was, is): (Cell, Option<&BlockState>, Option<&BlockState>),
    touching: &[&EditIr],
) -> Diagnostic {
    let name = record
        .address
        .as_ref()
        .map_or_else(|| record.role.clone(), ToString::to_string);
    let cells = if count == 1 { "cell" } else { "cells" };
    let (x, y, z) = cell;
    let mut notes = vec![DiagnosticNote {
        span: None,
        message: format!(
            "first at ({x}, {y}, {z}) of `{key}`, which holds {} as written and {} with the edits",
            state_text(was),
            state_text(is),
        ),
    }];
    notes.extend(touching.iter().map(|edit| DiagnosticNote {
        span: Some(edit.span.clone()),
        message: "this edit changes a member of the same body".to_owned(),
    }));
    notes.push(DiagnosticNote {
        span: None,
        message: format!(
            "a member's resolved state follows the members around it, as a roof sits on its \
             walls and a chest pairs with the chest beside it: if the change is meant, the build \
             can stand as it is; if not, change the edit so it leaves what `{name}` follows as it \
             was, or write the state `{name}` should keep on its own line, where its keyword \
             takes it"
        ),
    });
    Diagnostic {
        code: DiagnosticCode::EditRipple,
        span: record.span.clone(),
        primary: format!(
            "no edit selects `{name}`, and the edits change its resolved state at {count} {cells}"
        ),
        notes,
        data: None,
    }
}
