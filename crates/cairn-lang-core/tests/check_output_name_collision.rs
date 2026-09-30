//! Acceptance tests for `E_OUTPUT_NAME_COLLISION`.
//!
//! Every artifact of one build is written into the same directory, named by
//! `artifact_stem`: a `struct` by its name, a `place` by its `id=` alone, a
//! walkway by its site and endpoints. Two scopes can therefore name one
//! file, and before this code only `cairn compile` noticed, after lowering,
//! with no code — `cairn check` passed the source and `cairn info` listed
//! every version as buildable.

use cairn_lang_core::{Diagnostic, DiagnosticCode, Severity, artifact_stem};

mod common;
use common::{PRELUDE, codes, diagnose, exactly_one, notes, slice};

fn collisions(source: &str) -> Vec<Diagnostic> {
    diagnose(source)
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::OutputNameCollision)
        .collect()
}

/// The issue's own shape: a `struct` and a `place id=` of one name both
/// write `hut`.
#[test]
fn a_struct_and_a_place_of_one_name_collide() {
    let source = format!(
        "{PRELUDE}struct hut size=3x3\n  floor mat_slot=floor\n\n\
         site s:\n  place id=hut use=hut theme=plain at=origin\n"
    );
    let found = exactly_one(collisions(&source));
    assert_eq!(found.severity(), Severity::Error);
    assert_eq!(
        slice(&source, &found),
        "place id=hut use=hut theme=plain at=origin",
        "the finding anchors on the later declaration",
    );
    assert_eq!(
        found.primary,
        "`place id=hut` in site `s` is written to the same file as `struct hut`",
    );
    let first = found.notes[0].span.clone().expect("the first note points");
    assert!(
        source[first].starts_with("struct hut"),
        "the first note points at the struct",
    );
    assert!(
        notes(&found)[1].contains("both are written to `hut`"),
        "{:?}",
        notes(&found),
    );
}

/// A placement's file name leaves its site out, so one `id=` in two sites
/// is one file.
#[test]
fn one_place_id_in_two_sites_collides() {
    let source = format!(
        "{PRELUDE}site a:\n  place id=home use=hut theme=plain at=origin\n\n\
         site b:\n  place id=home use=hut theme=plain at=origin\n"
    );
    let found = exactly_one(collisions(&source));
    assert_eq!(
        found.primary,
        "`place id=home` in site `b` is written to the same file as `place id=home` in site `a`",
    );
}

/// `Hut` and `hut` are two scopes but one file wherever case is folded, and
/// the verdict must not depend on which host checks the source.
#[test]
fn names_that_differ_only_in_case_collide() {
    let source = format!(
        "{PRELUDE}struct Hut size=3x3\n  floor mat_slot=floor\n\n\
         struct hut size=3x3\n  floor mat_slot=floor\n"
    );
    let found = exactly_one(collisions(&source));
    assert_eq!(
        found.primary,
        "`struct hut` is written to the same file as `struct Hut`",
    );
    assert!(
        notes(&found)[1].contains("`Hut` and `hut` differ only in case"),
        "{:?}",
        notes(&found),
    );
}

/// A walkway's name joins place and port with `_`, and ids may carry `_`,
/// so two different `connect` rows can flatten to one name.
#[test]
fn two_walkways_whose_names_flatten_alike_collide() {
    let source = "theme plain:\n  slot floor -> @oak_planks\n  slot wall -> @cobblestone\n\n\
         def one size=3x3:\n  floor id=floor mat_slot=floor\n  \
         walls id=walls class=outer mat_slot=wall height=3\n  door id=entry side=front at=center\n\n\
         def two size=3x3:\n  floor id=floor mat_slot=floor\n  \
         walls id=walls class=outer mat_slot=wall height=3\n  door id=y_entry side=front at=center\n\n\
         site s:\n  place id=x_y use=one theme=plain at=origin\n  \
         place id=x use=two theme=plain east_of=x_y gap=10\n  \
         place id=z use=one theme=plain east_of=x gap=10\n\n  \
         connect x_y.entry to z.entry path=@gravel\n  \
         connect x.y_entry to z.entry path=@gravel\n";
    assert_eq!(
        artifact_stem("walkway::s::x_y.entry__z.entry"),
        artifact_stem("walkway::s::x.y_entry__z.entry"),
        "the fixture's two walkways must share a name for this test to mean anything",
    );
    let found = exactly_one(collisions(source));
    assert_eq!(
        slice(source, &found),
        "connect x.y_entry to z.entry path=@gravel"
    );
    assert_eq!(
        found.primary,
        "the walkway `x.y_entry ↔ z.entry` in site `s` is written to the same file as the \
         walkway `x_y.entry ↔ z.entry` in site `s`",
    );
}

/// One scope declared twice is not two artifacts: the second declaration
/// builds nothing, and the finding that says so is someone else's.
#[test]
fn a_scope_declared_twice_is_left_to_the_duplicate_codes() {
    let structs = format!(
        "{PRELUDE}struct hut size=3x3\n  floor mat_slot=floor\n\n\
         struct hut size=3x3\n  floor mat_slot=floor\n"
    );
    assert!(codes(&structs).contains(&"E_DUPLICATE_ITEM"));
    assert_eq!(collisions(&structs), []);

    let places = format!(
        "{PRELUDE}site s:\n  place id=home use=hut theme=plain at=origin\n  \
         place id=home use=hut theme=plain at=origin\n"
    );
    assert!(codes(&places).contains(&"E_DUPLICATE_PLACE_ID"));
    assert_eq!(collisions(&places), []);
}

/// Only what a build writes can collide. A `def` is a template with no
/// file of its own, and a `place` the resolver refused has no scope.
#[test]
fn names_that_write_no_file_do_not_collide() {
    let def_and_struct = format!(
        "{PRELUDE}struct hut size=3x3\n  floor mat_slot=floor\n\n\
         site s:\n  place id=home use=hut theme=plain at=origin\n"
    );
    assert_eq!(collisions(&def_and_struct), []);

    let refused_place = format!(
        "{PRELUDE}struct home size=3x3\n  floor mat_slot=floor\n\n\
         site s:\n  place id=home use=nowhere theme=plain at=origin\n"
    );
    assert!(codes(&refused_place).contains(&"E_UNRESOLVED_PLACE_REF"));
    assert_eq!(collisions(&refused_place), []);
}
