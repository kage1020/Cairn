//! Acceptance tests for `E_OUTPUT_NAME_COLLISION`.
//!
//! Every artifact of one build is written into the same directory, named by
//! `artifact_stem`: a `struct` by its name, a `place` by its `id=` alone, a
//! walkway by its site and endpoints. Two scopes can therefore name one
//! file. The resolver compares the names before lowering, so `cairn check`
//! reports the pair without a `--target`.

use cairn_lang_core::{
    Diagnostic, DiagnosticCode, PlaceId, PortId, Severity, SiteName, WalkwayEndpoint,
    WalkwayScopeKey, artifact_stem,
};

mod common;
use common::{PRELUDE, codes, diagnose, exactly_one, lowered, notes, slice};

fn collisions(source: &str) -> Vec<Diagnostic> {
    diagnose(source)
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::OutputNameCollision)
        .collect()
}

/// The simplest shape: a `struct` and a `place id=` of one name both
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
        "places are compared after structs, so the finding anchors on the place",
    );
    assert_eq!(
        found.primary,
        "`place id=hut` in site `s` would be written to the same file as `struct hut`",
    );
    let first = found.notes[0].span.clone().expect("the first note points");
    assert!(
        source[first].starts_with("struct hut"),
        "the first note points at the struct",
    );
    assert!(
        notes(&found)[1].contains("both would be written to `hut`"),
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
        "`place id=home` in site `b` would be written to the same file as `place id=home` in \
         site `a`",
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
        "`struct hut` would be written to the same file as `struct Hut`",
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
        "the walkway `x.y_entry ↔ z.entry` in site `s` would be written to the same file as the \
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

/// Structs are compared before places whatever the source order, so with
/// the site written above the struct the finding still anchors on the
/// `place`, and its note points down the file at the struct.
#[test]
fn the_finding_anchors_by_kind_not_by_source_order() {
    let source = format!(
        "{PRELUDE}site s:\n  place id=hut use=hut theme=plain at=origin\n\n\
         struct hut size=3x3\n  floor mat_slot=floor\n"
    );
    let found = exactly_one(collisions(&source));
    assert_eq!(
        slice(&source, &found),
        "place id=hut use=hut theme=plain at=origin"
    );
    let first = found.notes[0].span.clone().expect("the first note points");
    assert!(
        first.start > found.span.start && source[first].starts_with("struct hut"),
        "the note points forward, at the struct",
    );
}

/// One scope declared twice, with a third scope sharing its name, is still
/// one finding: the repeat is skipped whichever artifact first held the
/// name, so the duplicate codes are left to report it.
#[test]
fn a_scope_declared_twice_beside_a_third_is_reported_once() {
    let structs = format!(
        "{PRELUDE}struct Hut size=3x3\n  floor mat_slot=floor\n\n\
         struct hut size=3x3\n  floor mat_slot=floor\n\n\
         struct hut size=3x3\n  floor mat_slot=floor\n"
    );
    let struct_then_places = format!(
        "{PRELUDE}struct hut size=3x3\n  floor mat_slot=floor\n\n\
         site s:\n  place id=hut use=hut theme=plain at=origin\n  \
         place id=hut use=hut theme=plain at=origin\n"
    );
    let two_sites = format!(
        "{PRELUDE}site a:\n  place id=home use=hut theme=plain at=origin\n\n\
         site b:\n  place id=home use=hut theme=plain at=origin\n  \
         place id=home use=hut theme=plain at=origin\n"
    );
    let shapes = [
        (&structs, "E_DUPLICATE_ITEM"),
        (&struct_then_places, "E_DUPLICATE_PLACE_ID"),
        (&two_sites, "E_DUPLICATE_PLACE_ID"),
    ];
    for (source, duplicate) in shapes {
        assert!(codes(source).contains(&duplicate), "{source}");
    }
    let counts: Vec<usize> = shapes
        .iter()
        .map(|(source, _)| collisions(source).len())
        .collect();
    assert_eq!(counts, [1, 1, 1], "one finding per shape");
}

/// `a.entry to b.entry` and `b.entry to a.entry` ask for one walkway:
/// lowering lays the first and drops the second as `W_DUPLICATE_WALKWAY`,
/// so the name the second row would have had is never written, and a
/// struct spelled like it is not a collision.
#[test]
fn a_reversed_duplicate_connect_is_one_walkway() {
    let source = format!(
        "{PRELUDE}struct s_walkway_b_entry__a_entry size=3x3\n  floor mat_slot=floor\n\n\
         site s:\n  place id=a use=hut theme=plain at=origin\n  \
         place id=b use=hut theme=plain east_of=a gap=4\n  \
         connect a.entry to b.entry path=@gravel\n  \
         connect b.entry to a.entry path=@gravel\n"
    );
    assert_eq!(collisions(&source), []);
    let ir = lowered(&source);
    let written: Vec<&str> = ir.structures.keys().map(String::as_str).collect();
    assert_eq!(
        written,
        [
            "struct::s_walkway_b_entry__a_entry",
            "site::s::a",
            "site::s::b",
            "walkway::s::a.entry__b.entry",
        ],
        "lowering writes one walkway, named after the first row",
    );
}

/// A `place` of a sizeless `def` is dropped by lowering with
/// `W_DEF_NO_SIZE`, so it writes no file and cannot meet a struct.
#[test]
fn a_place_of_a_sizeless_def_does_not_collide() {
    let source = format!(
        "{PRELUDE}def bare:\n  floor mat_slot=floor\n\n\
         struct hut size=3x3\n  floor mat_slot=floor\n\n\
         site s:\n  place id=hut use=bare theme=plain at=origin\n"
    );
    assert_eq!(collisions(&source), []);
    let ir = lowered(&source);
    let written: Vec<&str> = ir.structures.keys().map(String::as_str).collect();
    assert_eq!(written, ["struct::hut"]);
}

/// A `struct` with no `size=` is dropped by lowering with
/// `W_STRUCT_NO_SIZE`, so it writes no file and cannot meet a place.
#[test]
fn a_sizeless_struct_does_not_collide() {
    let source = format!(
        "{PRELUDE}struct hut\n  floor mat_slot=floor\n\n\
         site s:\n  place id=hut use=hut theme=plain at=origin\n"
    );
    assert_eq!(collisions(&source), []);
    let ir = lowered(&source);
    let written: Vec<&str> = ir.structures.keys().map(String::as_str).collect();
    assert_eq!(written, ["site::s::hut"]);
}

/// A walkway with an endpoint on a placement lowering drops is not laid,
/// so its name is not a collision either.
#[test]
fn a_walkway_to_a_dropped_placement_does_not_collide() {
    let source = format!(
        "{PRELUDE}def bare:\n  floor mat_slot=floor\n  door id=entry side=front at=center\n\n\
         struct s_walkway_a_entry__b_entry size=3x3\n  floor mat_slot=floor\n\n\
         site s:\n  place id=b use=hut theme=plain at=origin\n  \
         place id=a use=bare theme=plain east_of=b gap=4\n  \
         connect a.entry to b.entry path=@gravel\n"
    );
    assert_eq!(collisions(&source), []);
    let ir = lowered(&source);
    let written: Vec<&str> = ir.structures.keys().map(String::as_str).collect();
    assert_eq!(
        written,
        ["struct::s_walkway_a_entry__b_entry", "site::s::b"]
    );
}

/// A `connect` row whose scope key cannot be built is skipped here because
/// lowering builds the same key, reports `W_INVALID_WALKWAY_IDENT` and lays
/// nothing. If lowering ever laid such a row, this test fails on the
/// structures it writes.
#[test]
fn a_walkway_whose_key_is_refused_writes_no_file() {
    let source = format!(
        "{PRELUDE}site s:\n  place id=a__b use=hut theme=plain at=origin\n  \
         place id=b_ use=hut theme=plain east_of=a__b gap=4\n  \
         connect a__b.entry to b_.entry path=@gravel\n"
    );
    let site = SiteName::new("s").expect("a site name");
    let end = |place: &str| WalkwayEndpoint {
        place: PlaceId::new(place).expect("a place id"),
        port: PortId::new("entry").expect("a port id"),
    };
    assert!(
        WalkwayScopeKey::from_parts(&site, &end("a__b"), &end("b_")).is_err(),
        "the row's key must be refused for this test to mean anything",
    );
    assert_eq!(collisions(&source), []);
    let ir = lowered(&source);
    let written: Vec<&str> = ir.structures.keys().map(String::as_str).collect();
    assert_eq!(written, ["site::s::a__b", "site::s::b_"]);
    assert!(
        ir.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::InvalidWalkwayIdent),
        "{:#?}",
        ir.diagnostics,
    );
}

/// A walkway's name keeps its site, so two sites with the same rows give
/// two walkway files. Their placements still meet, one `id=` in two sites,
/// and those are the only findings.
#[test]
fn walkways_in_two_sites_do_not_collide() {
    let site = |name: &str| {
        format!(
            "site {name}:\n  place id=a use=hut theme=plain at=origin\n  \
             place id=b use=hut theme=plain east_of=a gap=4\n  \
             connect a.entry to b.entry path=@gravel\n\n"
        )
    };
    let source = format!("{PRELUDE}{}{}", site("x"), site("y"));
    let found = collisions(&source);
    let anchored: Vec<&str> = found.iter().map(|d| slice(&source, d)).collect();
    assert_eq!(
        anchored,
        [
            "place id=a use=hut theme=plain at=origin",
            "place id=b use=hut theme=plain east_of=a gap=4",
        ],
        "only the placements collide, not the walkways",
    );
}

/// `struct` names and `place id=` values may carry `_`, so either can be
/// spelled exactly like a walkway's name.
#[test]
fn a_struct_or_a_place_spelled_like_a_walkway_collides_with_it() {
    let sites = "site s:\n  place id=a use=hut theme=plain at=origin\n  \
                 place id=b use=hut theme=plain east_of=a gap=4\n  \
                 connect a.entry to b.entry path=@gravel\n";
    let with_struct = format!(
        "{PRELUDE}struct s_walkway_a_entry__b_entry size=3x3\n  floor mat_slot=floor\n\n{sites}"
    );
    let found = exactly_one(collisions(&with_struct));
    assert_eq!(
        found.primary,
        "the walkway `a.entry ↔ b.entry` in site `s` would be written to the same file as \
         `struct s_walkway_a_entry__b_entry`",
    );

    let with_place = format!(
        "{PRELUDE}{sites}  place id=s_walkway_a_entry__b_entry use=hut theme=plain north_of=a \
         gap=4\n"
    );
    let found = exactly_one(collisions(&with_place));
    assert_eq!(
        found.primary,
        "the walkway `a.entry ↔ b.entry` in site `s` would be written to the same file as \
         `place id=s_walkway_a_entry__b_entry` in site `s`",
    );
}
