//! A body's `place use=` names a def of the file or of the furniture library, and counts as a use
//! of the file's (`spec/components-editing-sites` "A `def` placed in a body").

use cairn_lang_core::check::DiagnosticCode;
use cairn_lang_core::resolve::resolve;
use cairn_lang_core::{IntentModule, lower, parse};

fn ir(source: &str) -> IntentModule {
    lower(&parse(source).expect("parse"))
}

/// Each finding of `source` as its code, the text it points at, its primary and its notes.
fn found(source: &str) -> Vec<(DiagnosticCode, &str, String, Vec<String>)> {
    resolve(&ir(source), None)
        .diagnostics
        .into_iter()
        .map(|d| {
            (
                d.code,
                &source[d.span.clone()],
                d.primary,
                d.notes.into_iter().map(|n| n.message).collect(),
            )
        })
        .collect()
}

const STOOL: &str = "theme t:\n  slot post -> @spruce_log\n\n\
                     def stool size=1x1:\n  fill mat_slot=post from=0,0,0 to=0,0,0\n\n";

#[test]
fn a_use_naming_no_def_is_unresolved_with_the_nearest_name() {
    let src = format!("{STOOL}struct s size=5x5\n  place use=tabel at=1,0,1\n");
    assert_eq!(
        found(&src),
        [
            (
                DiagnosticCode::UnresolvedPlaceRef,
                "place use=tabel at=1,0,1",
                "`use=tabel` names no def of this file and none of the furniture library"
                    .to_owned(),
                vec![
                    "did you mean `table`?".to_owned(),
                    "declare a `def` of that name, or name one the furniture library ships: \
                     table, chair, fireplace, bookcase, crafting_table, furnace, barrel, \
                     lectern, wall_torch, lantern"
                        .to_owned(),
                ],
            ),
            (
                DiagnosticCode::UnusedDef,
                "def stool size=1x1:\n  fill mat_slot=post from=0,0,0 to=0,0,0",
                "def `stool` is never referenced by a `place use=stool`".to_owned(),
                vec![
                    "remove the def, or place an instance via `site ... place use=...`, or \
                     build one inside a body with `place use=...` there"
                        .to_owned(),
                ],
            ),
        ],
    );
}

#[test]
fn a_def_a_body_places_is_used() {
    let src = format!("{STOOL}struct s size=5x5\n  place use=stool at=1,0,1\n");
    assert_eq!(found(&src), []);
}

#[test]
fn a_library_def_resolves_its_slots_once_per_place_line() {
    let src = "theme t:\n  slot chair -> @spruce_stairs[facing=north]\n\n\
               struct s size=5x5\n  place use=chair at=1,0,1\n  place use=table at=3,0,1\n";
    let resolution = resolve(&ir(src), None);
    assert_eq!(resolution.diagnostics, []);
    let scope = &resolution.scopes["struct::s"];
    let tokens: Vec<Vec<String>> = scope
        .placed
        .values()
        .map(|placed| {
            placed
                .members
                .values()
                .map(|binding| {
                    let value = binding.slot_value.as_ref().expect("a library slot binds");
                    format!(
                        "{} at {:?}",
                        value.value.describe(),
                        &src[value.span.clone()]
                    )
                })
                .collect()
        })
        .collect();
    assert_eq!(
        scope.placed.keys().copied().collect::<Vec<_>>(),
        [
            src.find("place use=chair").expect("the chair line"),
            src.find("place use=table").expect("the table line"),
        ],
    );
    // The body's theme binds `chair`; the library's binds the table's, standing on its line.
    assert_eq!(
        tokens,
        [
            vec!["token `@spruce_stairs[facing=north]` at \"@spruce_stairs[facing=north]\""],
            vec![
                "token `@oak_fence` at \"place use=table at=3,0,1\"",
                "token `@oak_pressure_plate` at \"place use=table at=3,0,1\"",
            ],
        ],
    );
}
