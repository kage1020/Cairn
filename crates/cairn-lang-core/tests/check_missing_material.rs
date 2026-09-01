//! A member that paints nothing because it names no material says so.
//!
//! `floor` and `walls` take their block from `mat_slot=NAME` through the
//! applied theme, and that is the only route they have — a selector's
//! bindings never reach the painter, and neither role has a fallback
//! material. A member written without one therefore paints nothing, and
//! before `E_MISSING_MATERIAL` nothing anywhere said so: `cairn check`
//! exited 0 and the lowered structure came out empty.
//!
//! The rule is about the *absent* key only. A `mat_slot=` that names a slot
//! no theme declares is `E_UNRESOLVED_SLOT`'s, the same split
//! `E_INCOMPLETE_PLACE` and `E_TYPE_MISMATCH_LABEL` already draw on a
//! `place` row: absent is one finding, present-but-unusable is another, and
//! no input earns both.
//!
//! Coverage:
//!
//! 1. `walls` and `floor` without one — refused, at the member;
//! 2. a `window` without one — **not** refused. It is an opening carved to
//!    air, which is what `examples/themed-tower.crn`'s `class=arrow_slit`
//!    row is; the report this fixes named `window` alongside the other two
//!    and was wrong about it;
//! 3. the roles that paint a fallback — untouched;
//! 4. `mat_slot=` present but unresolvable — the resolver's finding alone;
//! 5. no theme in the file at all — still refused, because the member names
//!    no material whatever the module declares;
//! 6. inside a `def` body and inside a `level` block — the walk reaches
//!    both;
//! 7. **role parity** — among the roles that paint, `check` refuses exactly
//!    those whose bare form leaves the lowered structure untouched and says
//!    nothing else. That is the property `member_will_paint`'s
//!    documentation warns can drift, and it is measured here rather than
//!    asserted from a list.

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::check::{Diagnostic, DiagnosticCode, Severity};
use cairn_lang_core::intent::MemberRole;
use cairn_lang_core::{check, lower, parse, resolve};

const THEME: &str = "theme t:\n  slot m -> @oak_planks\n\n";

/// Every finding `cairn check` reports for `src`.
fn findings(src: &str) -> Vec<Diagnostic> {
    let module = parse(src).expect("parse");
    let ir = lower(&module);
    check(&module, &ir, None)
}

/// The `E_MISSING_MATERIAL` findings in `src`.
fn missing(src: &str) -> Vec<Diagnostic> {
    findings(src)
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::MissingMaterial)
        .collect()
}

fn codes(found: &[Diagnostic]) -> Vec<&'static str> {
    found.iter().map(|d| d.code.as_str()).collect()
}

#[test]
fn walls_with_no_mat_slot_is_refused() {
    let src = format!("{THEME}struct s size=3x3\n  walls class=outer height=6\n");
    let found = missing(&src);
    assert_eq!(
        found.len(),
        1,
        "the member paints nothing and nothing else says so; got {:?}",
        codes(&findings(&src)),
    );
    assert_eq!(
        found[0].code.severity(),
        Severity::Error,
        "implicit dropping is what spec/lint 11.3 calls an error",
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
        missing(&src).len(),
        1,
        "a floor takes the same route to a block as a walls; got {:?}",
        codes(&findings(&src)),
    );
}

#[test]
fn a_window_with_no_mat_slot_is_an_opening_and_not_a_mistake() {
    // `examples/themed-tower.crn` writes exactly this to punch arrow slits
    // through a stone wall without picking a species for them.
    let src = format!(
        "{THEME}struct s size=6x6\n  \
walls mat_slot=m height=4\n  \
window class=arrow_slit side=front repeat=3 step=2 y=2 size=1x2\n"
    );
    assert!(
        missing(&src).is_empty(),
        "a window without a material is a hole, and holes are a feature; got {:?}",
        codes(&findings(&src)),
    );
}

#[test]
fn the_roles_that_paint_a_fallback_are_left_alone() {
    let src = format!(
        "{THEME}struct s size=6x6\n  \
floor mat_slot=m\n  \
walls mat_slot=m height=4\n  \
roof kind=gable\n  \
door id=d side=front at=center\n  \
pressure_plate id=p at=front.inside offset=0 y=0\n"
    );
    assert!(
        missing(&src).is_empty(),
        "each of these puts a block down without a `mat_slot=`; got {:?}",
        codes(&findings(&src)),
    );
}

#[test]
fn a_slot_no_theme_declares_is_the_resolvers_finding_and_not_this_one() {
    let src = format!("{THEME}struct s size=3x3\n  walls mat_slot=absent height=4\n");
    let found = findings(&src);
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

#[test]
fn a_module_with_no_theme_still_names_the_missing_material() {
    // Nothing here depends on a theme: the member names no material at
    // all, which is true of a module that declares none and of one that
    // declares ten.
    let src = "struct s size=3x3\n  walls height=4\n";
    assert_eq!(missing(src).len(), 1, "got {:?}", codes(&findings(src)));
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
        missing(&src).len(),
        2,
        "one in the def, one under the level; got {:?}",
        codes(&findings(&src)),
    );
}

/// One row per role that can paint inside a `struct` body, written without
/// a `mat_slot=`.
///
/// The roles that are absent from this table are absent because they never
/// reach a painter, and [`every_role_is_either_measured_here_or_paints_nothing`]
/// is what stops one being forgotten.
const PAINTERS: &[(&str, &str)] = &[
    ("floor", "floor id=extra"),
    ("walls", "walls class=inner height=2"),
    ("door", "door id=d side=front at=center"),
    (
        "window",
        "window class=slit side=front y=1 offset=1 size=1x2",
    ),
    ("roof", "roof kind=gable"),
    (
        "stair",
        "stair id=eave kind=stairs side=front half=top facing=out shape=outer_left",
    ),
    (
        "pressure_plate",
        "pressure_plate id=p at=front.inside offset=0 y=0",
    ),
];

/// What the lowered structure looks like: its extents, the block ids it
/// painted, and how much lowering had to say about it.
///
/// The ids are sorted rather than positional because a member that carves
/// removes blocks and a member that paints adds them; either way the
/// multiset moves, and a member that does nothing leaves it alone.
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
    let without = footprint(&format!("{THEME}{BASE}"));

    for (keyword, member) in PAINTERS {
        let src = format!("{THEME}{BASE}  {member}\n");
        let with = footprint(&src);
        let changed_the_build = with != without;
        let refused = !missing(&src).is_empty();
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

/// Every [`MemberRole`] is either measured by [`PAINTERS`] or named here
/// with the reason it cannot paint a material.
///
/// The match is exhaustive so a new role stops this file compiling until
/// someone decides which side it is on — the same device
/// `check::tests`'s `RaisedBy` uses for a new diagnostic code.
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
            // `level` groups its children and paints nothing of its own;
            // `circuit` reserves a volume for the redstone passes and
            // never a material; `place` and `connect` are site rows,
            // refused inside a struct body by `member_scope`; and a
            // keyword the role table does not know is
            // `E_UNKNOWN_KEYWORD`'s, with no painter reached.
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
    let painters: Vec<&str> = PAINTERS.iter().map(|(keyword, _)| *keyword).collect();
    for role in &roles {
        let keyword = MemberRole::keyword(role);
        assert_eq!(
            measured(role),
            painters.contains(&keyword),
            "`{keyword}` must either have a row in PAINTERS or a reason above",
        );
    }
}

/// The shipped examples carry no bare `floor` or `walls`, so the refusal
/// costs the corpus nothing.
#[test]
fn no_shipped_example_names_a_member_with_no_material() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
    let mut checked = 0;
    for entry in std::fs::read_dir(root).expect("the examples directory is there") {
        let path = entry.expect("read the entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("crn") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("read the example");
        let Ok(module) = parse(&source) else {
            continue;
        };
        checked += 1;
        let ir = lower(&module);
        let found: Vec<String> = check(&module, &ir, None)
            .iter()
            .filter(|d| d.code == DiagnosticCode::MissingMaterial)
            .map(|d| d.primary.clone())
            .collect();
        assert!(
            found.is_empty(),
            "{}: {found:?}",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("?"),
        );
    }
    assert!(checked >= 5, "premise: the corpus was actually read");
}
