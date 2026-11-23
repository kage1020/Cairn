use std::collections::BTreeSet;

use crate::check;
use crate::intent::{MemberRole, lower};
use crate::parse::parse;

use super::{SOURCE, defs, theme};

#[test]
fn the_library_checks_and_resolves_clean() {
    let module = parse(SOURCE).expect("parses");
    let ir = lower(&module);
    let found: Vec<_> = check(&module, &ir, None)
        .into_iter()
        .map(|d| d.code)
        .collect();
    // Nothing in the library places its defs, which is what `W_UNUSED_DEF` says of each.
    assert!(
        found
            .iter()
            .all(|code| *code == crate::DiagnosticCode::UnusedDef),
        "{found:?}",
    );
    assert_eq!(found.len(), defs().len());
}

#[test]
fn every_def_is_sized_and_built_of_slotted_fills() {
    for def in defs() {
        assert!(def.size.is_some(), "`{}` declares no size", def.name);
        for member in &def.members {
            assert!(
                matches!(member.role, MemberRole::Fill)
                    && member.read_slot().is_some()
                    && !member.pours_fluid(),
                "`{}` holds a line a body cannot build: {:?}",
                def.name,
                member.role,
            );
        }
    }
}

#[test]
fn the_theme_binds_exactly_the_slots_the_defs_read() {
    let read: BTreeSet<&str> = defs()
        .iter()
        .flat_map(|def| def.members.iter().filter_map(|m| m.read_slot()))
        .collect();
    let bound: BTreeSet<&str> = theme().slots.keys().map(String::as_str).collect();
    assert_eq!(read, bound);
}

#[test]
fn the_library_ships_these_defs() {
    let names: Vec<&str> = defs().iter().map(|def| def.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "table",
            "chair",
            "fireplace",
            "bookcase",
            "crafting_table",
            "furnace",
            "barrel",
            "lectern",
            "wall_torch",
            "lantern",
        ],
    );
}
