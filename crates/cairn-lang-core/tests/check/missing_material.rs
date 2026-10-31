//! `E_MISSING_MATERIAL`: a `floor` or `walls` with no `mat_slot=` paints nothing, so it is an
//! error at the member; a present but unusable `mat_slot=` is another code's finding.

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::check::{Diagnostic, DiagnosticCode, Severity};
use cairn_lang_core::intent::MemberRole;
use cairn_lang_core::{lower, parse, resolve};

use crate::common::{diagnose, of_code};

const THEME: &str = "theme t:\n  slot m -> @oak_planks\n\n";

fn codes(found: &[Diagnostic]) -> Vec<&'static str> {
    found.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn walls_with_no_mat_slot_is_refused() {
    let src = format!("{THEME}struct s size=3x3\n  walls class=outer height=6\n");
    let found = of_code(&src, DiagnosticCode::MissingMaterial);
    assert_eq!(
        found.len(),
        1,
        "the member paints nothing and nothing else says so; got {:?}",
        codes(&diagnose(&src)),
    );
    assert_eq!(
        found[0].code.severity(),
        Severity::Error,
        "implicit dropping is an error under spec/lint \"Error vs warning\"",
    );
    let at = src.find("walls").expect("the member is in the source");
    assert_eq!(
        found[0].span.start, at,
        "the finding sits on the member that names no material",
    );
}

#[test]
fn floor_with_no_mat_slot_is_refused() {
    let src = format!("{THEME}struct s size=3x3\n  floor id=base\n");
    assert_eq!(
        of_code(&src, DiagnosticCode::MissingMaterial).len(),
        1,
        "a floor takes the same route to a block as a walls; got {:?}",
        codes(&diagnose(&src)),
    );
}

#[test]
fn a_window_with_no_mat_slot_is_an_opening_and_not_a_mistake() {
    // `examples/themed-tower.crn` cuts its arrow slits this way.
    let src = format!(
        "{THEME}struct s size=6x6\n  \
walls mat_slot=m height=4\n  \
window class=arrow_slit side=front repeat=3 step=2 y=2 size=1x2\n"
    );
    assert!(
        of_code(&src, DiagnosticCode::MissingMaterial).is_empty(),
        "a window without a material is a hole, and holes are a feature; got {:?}",
        codes(&diagnose(&src)),
    );
}

#[test]
fn the_roles_that_paint_a_fallback_are_left_alone() {
    // The roof's `overhang=1` gives the eave `stair` somewhere to sit.
    let src = format!(
        "{THEME}struct s size=6x6\n  \
floor mat_slot=m\n  \
walls mat_slot=m height=4\n  \
roof id=r kind=gable overhang=1\n  \
stair id=eave kind=stairs side=front half=top facing=out shape=outer_left\n  \
door id=d side=front at=center\n  \
pressure_plate id=p at=front.outside offset=0 y=0\n"
    );
    assert!(
        of_code(&src, DiagnosticCode::MissingMaterial).is_empty(),
        "each of these puts a block down without a `mat_slot=`; got {:?}",
        codes(&diagnose(&src)),
    );

    // Guard: lowering does not refuse this fixture outright.
    let painted = footprint(&src);
    assert_eq!(
        painted.2, 0,
        "premise: nothing here is deferred, so the ids below were painted \
         rather than skipped",
    );
    for id in [
        "minecraft:spruce_stairs",
        "minecraft:oak_pressure_plate",
        "minecraft:oak_planks",
    ] {
        assert!(
            painted.1.iter().any(|p| p.starts_with(id)),
            "`{id}` must be in what was painted; got {:?}",
            painted.1,
        );
    }
}

#[test]
fn a_slot_no_theme_declares_is_the_resolvers_finding_and_not_this_one() {
    let src = format!("{THEME}struct s size=3x3\n  walls mat_slot=absent height=4\n");
    let found = diagnose(&src);
    assert!(
        found
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedSlot),
        "the key is on the line and unusable, which is the resolver's; got {:?}",
        codes(&found),
    );
    assert!(
        !found
            .iter()
            .any(|d| d.code == DiagnosticCode::MissingMaterial),
        "no input earns both halves of the split; got {:?}",
        codes(&found),
    );
}

/// `intent::lower` hoists only an identifier or a string into `Member::mat_slot`.
#[test]
fn a_mat_slot_whose_value_is_not_a_label_is_the_type_findings_and_not_this_one() {
    for value in ["@oak_planks", "foo.bar"] {
        let src = format!("{THEME}struct s size=3x3\n  walls mat_slot={value} height=4\n");
        let found = diagnose(&src);
        assert!(
            found
                .iter()
                .any(|d| d.code == DiagnosticCode::TypeMismatchLabel),
            "`mat_slot={value}` is a label mismatch; got {:?}",
            codes(&found),
        );
        assert!(
            !found
                .iter()
                .any(|d| d.code == DiagnosticCode::MissingMaterial),
            "`mat_slot={value}` is written, so it is not missing; got {:?}",
            codes(&found),
        );
    }
}

#[test]
fn a_module_with_no_theme_still_names_the_missing_material() {
    let src = "struct s size=3x3\n  walls height=4\n";
    let found = of_code(src, DiagnosticCode::MissingMaterial);
    assert_eq!(found.len(), 1, "got {:?}", codes(&diagnose(src)));
    let note = &found[0].notes[0].message;
    assert!(
        note.contains("declares no `theme`"),
        "the advice must say what has to exist first; got: {note}",
    );
    let with_theme = of_code(
        &format!("{THEME}struct s size=3x3\n  walls height=4\n"),
        DiagnosticCode::MissingMaterial,
    );
    assert!(
        with_theme[0].notes[0].message.contains("applied theme"),
        "and it must point at the slot vocabulary when there is one; got: {}",
        with_theme[0].notes[0].message,
    );
}

#[test]
fn the_rule_reaches_a_def_body_and_a_level_block() {
    let src = format!(
        "{THEME}def hut size=4x4:\n  \
walls height=3\n\n\
struct s size=4x4\n  \
level y=0\n    \
floor id=inner\n"
    );
    assert_eq!(
        of_code(&src, DiagnosticCode::MissingMaterial).len(),
        2,
        "one in the def, one under the level; got {:?}",
        codes(&diagnose(&src)),
    );
}

#[test]
fn a_body_another_pass_has_dropped_earns_nothing_here() {
    let cases = [
        (
            "a geometry row in a site body",
            format!(
                "{THEME}def q size=3x3:\n  walls mat_slot=m height=3\n\n\
site x:\n  floor\n"
            ),
        ),
        (
            "a body indented under a row that groups nothing",
            format!("{THEME}struct s size=3x3\n  walls mat_slot=m height=3\n    floor\n"),
        ),
        (
            "a level inside a level",
            format!("{THEME}struct s size=3x3\n  level y=0\n    level y=1\n      walls height=3\n"),
        ),
        (
            "a body under an unknown keyword",
            format!("{THEME}struct s size=3x3\n  mystery\n    walls height=3\n"),
        ),
        (
            "a body under a place row",
            format!("{THEME}site x:\n  place id=p use=q\n    walls height=3\n"),
        ),
    ];
    for (what, src) in cases {
        assert!(
            of_code(&src, DiagnosticCode::MissingMaterial).is_empty(),
            "{what}: the owning pass already covers the whole subtree; got {:?}",
            codes(&diagnose(&src)),
        );
    }
}

/// One row per role that can paint in a `struct` body, without `mat_slot=`, plus the support it
/// needs not to be deferred.
const PAINTERS: &[(&str, &str, &str)] = &[
    ("floor", "floor id=extra", ""),
    ("walls", "walls class=inner height=2", ""),
    ("door", "door id=d side=front at=center", ""),
    (
        "window",
        "window class=slit side=front y=1 offset=1 size=1x2",
        "",
    ),
    ("roof", "roof kind=gable", ""),
    (
        "stair",
        "stair id=eave kind=stairs side=front half=top facing=out shape=outer_left",
        // An eave band needs a roof drawing an overhang of at least 1.
        "roof id=r kind=gable overhang=1",
    ),
    (
        "pressure_plate",
        "pressure_plate id=p at=inside.front offset=1 y=0",
        "",
    ),
];

/// The lowered structure's extents, its sorted painted block ids, and its finding count.
fn footprint(src: &str) -> (String, Vec<String>, usize) {
    let module = parse(src).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    let lowered = lower_to_block_array(&ir, &resolution, None);
    let array = lowered
        .structures
        .values()
        .next()
        .expect("the struct lowers to one array");
    let mut painted: Vec<String> = array
        .voxels
        .iter()
        .filter(|index| index.0 != 0)
        .map(|index| array.palette.entries[index.0 as usize].id.clone())
        .collect();
    painted.sort();
    (
        format!("{:?}", array.dims),
        painted,
        lowered.diagnostics.len(),
    )
}

#[test]
fn check_refuses_exactly_the_bare_members_that_change_nothing() {
    const BASE: &str = "struct s size=6x6\n  walls id=shell mat_slot=m height=4\n";

    for (keyword, member, support) in PAINTERS {
        let support_line = if support.is_empty() {
            String::new()
        } else {
            format!("  {support}\n")
        };
        let without_src = format!("{THEME}{BASE}{support_line}");
        let with_src = format!("{THEME}{BASE}{support_line}  {member}\n");
        let without = footprint(&without_src);
        let with = footprint(&with_src);

        assert_eq!(
            with.2, without.2,
            "`{keyword}`: lowering deferred the member instead of reaching its \
             painter, so this row measures nothing. Repair the fixture rather \
             than the assertion.",
        );

        let changed_the_build = (&with.0, &with.1) != (&without.0, &without.1);
        let refused = !of_code(&with_src, DiagnosticCode::MissingMaterial).is_empty();
        assert_eq!(
            refused,
            !changed_the_build,
            "`{keyword}` without a `mat_slot=`: check refuses it = {refused}, \
             but it {} the build. The two have to agree — a role that grows a \
             fallback material stops belonging in the refusal set, and a role \
             that loses one starts. with={with:?} without={without:?}",
            if changed_the_build {
                "changes"
            } else {
                "does not change"
            },
        );
    }
}

/// The match is exhaustive, so a new role has to be put on one side.
#[test]
fn every_role_is_either_measured_here_or_paints_nothing() {
    fn measured(role: &MemberRole) -> bool {
        match role {
            MemberRole::Floor
            | MemberRole::Walls
            | MemberRole::Door
            | MemberRole::Window
            | MemberRole::Roof
            | MemberRole::Stair
            | MemberRole::PressurePlate => true,
            // None of these lays a block of its own material.
            MemberRole::Level
            | MemberRole::Circuit
            | MemberRole::Place
            | MemberRole::Connect
            | MemberRole::Other(_) => false,
        }
    }

    let roles = [
        MemberRole::Floor,
        MemberRole::Walls,
        MemberRole::Door,
        MemberRole::Window,
        MemberRole::Roof,
        MemberRole::Stair,
        MemberRole::Level,
        MemberRole::PressurePlate,
        MemberRole::Circuit,
        MemberRole::Place,
        MemberRole::Connect,
        MemberRole::Other("mystery".to_owned()),
    ];
    let painters: Vec<&str> = PAINTERS.iter().map(|(keyword, _, _)| *keyword).collect();
    for role in &roles {
        let keyword = role.keyword();
        assert_eq!(
            measured(role),
            painters.contains(&keyword),
            "`{keyword}` must either have a row in PAINTERS or a reason above",
        );
    }
}
