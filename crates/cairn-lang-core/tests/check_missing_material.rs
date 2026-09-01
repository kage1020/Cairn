//! A member that paints nothing because it names no material says so.
//!
//! `floor` and `walls` take their block from `mat_slot=NAME` through the
//! applied theme, and that is the only route they have — a selector's
//! bindings never reach the painter, and neither role has a fallback
//! material. A member written without one therefore paints nothing, and
//! before `E_MISSING_MATERIAL` nothing anywhere said so: `cairn check`
//! exited 0 and the lowered structure came out empty.
//!
//! The rule is about the *absent* key only, and "absent" means the author
//! did not write it — not that no `mat_slot` field survived lowering.
//! `mat_slot=@oak_planks` is a key that is there and unusable, which is
//! `E_TYPE_MISMATCH_LABEL`'s; `mat_slot=absent` is a key that is there and
//! unresolvable, which is `E_UNRESOLVED_SLOT`'s. No member earns two of
//! them, the same split `E_INCOMPLETE_PLACE` and `E_TYPE_MISMATCH_LABEL`
//! already draw on a `place` row.
//!
//! Coverage:
//!
//!  1. `walls` and `floor` without one — refused, at the member;
//!  2. a `window` without one — **not** refused: it is an opening carved
//!     to air, which is what `examples/themed-tower.crn`'s
//!     `class=arrow_slit` row is. Grouping `window` with `floor` and
//!     `walls` is wrong for that reason;
//!  3. the roles that paint a fallback — untouched, with the fallback
//!     shown painting rather than assumed;
//!  4. `mat_slot=` present but unusable, in both of its shapes — the
//!     other code's finding alone;
//!  5. no theme in the file at all — still refused, because the member
//!     names no material whatever the module declares, and the advice
//!     says what has to exist first;
//!  6. inside a `def` body and inside a `level` block — the walk reaches
//!     both;
//!  7. a body another pass has already dropped — silent, because one
//!     mistake is one finding and the two repairs disagree;
//!  8. **role parity** — among the roles that paint, `check` refuses
//!     exactly those whose bare form leaves the lowered structure
//!     untouched. That is the property `member_will_paint`'s
//!     documentation warns can drift, and it is measured here rather than
//!     asserted from a list.

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
    // The `roof` carries no `mat_slot=` either, and its `overhang=1` is
    // what lets the eave `stair` band sit outside the wall — without it
    // lowering defers the stair, and this test would assert nothing about
    // a fallback.
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
        missing(&src).is_empty(),
        "each of these puts a block down without a `mat_slot=`; got {:?}",
        codes(&findings(&src)),
    );

    // And they really do put one down. Without this the assertion above
    // holds just as well for a fixture lowering refuses outright.
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

/// The other present-but-unusable shape, and the one the `mat_slot` field
/// cannot see.
///
/// `intent::lower` hoists a value into `Member::mat_slot` only when it is
/// an identifier or a string, so these two leave the field empty while the
/// key is plainly on the line. A rule reading the field alone would say
/// "has no `mat_slot=`" about a source that has one, and tell the author
/// to add a key they already wrote.
#[test]
fn a_mat_slot_whose_value_is_not_a_label_is_the_type_findings_and_not_this_one() {
    for value in ["@oak_planks", "foo.bar"] {
        let src = format!("{THEME}struct s size=3x3\n  walls mat_slot={value} height=4\n");
        let found = findings(&src);
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
    // Nothing here depends on a theme: the member names no material at
    // all, which is true of a module that declares none and of one that
    // declares ten.
    let src = "struct s size=3x3\n  walls height=4\n";
    let found = missing(src);
    assert_eq!(found.len(), 1, "got {:?}", codes(&findings(src)));
    // The advice has to be followable. "Name a slot the applied theme
    // declares" is not, in a file with no theme to declare one.
    let note = &found[0].notes[0].message;
    assert!(
        note.contains("declares no `theme`"),
        "the advice must say what has to exist first; got: {note}",
    );
    let with_theme = missing(&format!("{THEME}struct s size=3x3\n  walls height=4\n"));
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
        missing(&src).len(),
        2,
        "one in the def, one under the level; got {:?}",
        codes(&findings(&src)),
    );
}

/// A body another pass has already dropped is not walked.
///
/// Each of these earns one finding, from the pass that owns the row:
/// `check::member_scope` for a geometry keyword in a `site` and for
/// anything under a row its body cannot read, `check::nesting` for an
/// indented body nothing groups, `check::keyword_allowlist` for a keyword
/// with no vocabulary. Adding this code on top would bill one mistake
/// twice — and the repairs disagree, which is the sharper half: adding a
/// `mat_slot=` to a `floor` written into a `site` body silences this code
/// and leaves `E_MISPLACED_MEMBER` saying the row still produces no
/// blocks.
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
            missing(&src).is_empty(),
            "{what}: the owning pass already covers the whole subtree; got {:?}",
            codes(&findings(&src)),
        );
    }
}

/// One row per role that can paint inside a `struct` body, written without
/// a `mat_slot=`, plus whatever that role needs present to reach its
/// painter at all.
///
/// The support goes into both sides of the comparison, so it never shows
/// up as a change. It exists because a member lowering *defers* is
/// indistinguishable, in the array, from a member that painted nothing —
/// so a deferred fixture would let a role escape the refusal set for a
/// reason that has nothing to do with materials.
///
/// The roles absent from this table are absent because they lay no block
/// of their own, and [`every_role_is_either_measured_here_or_paints_nothing`]
/// is what stops one being forgotten.
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
        // An eave band sits outside the wall, so it needs a roof drawing
        // an overhang of at least 1 or lowering defers it.
        "roof id=r kind=gable overhang=1",
    ),
    (
        "pressure_plate",
        // `<side>.outside` and `inside.<side>` are the recognised anchors;
        // `front.inside` is neither, and is deferred.
        "pressure_plate id=p at=front.outside offset=0 y=0",
        "",
    ),
];

/// What the lowered structure looks like: its extents, the block ids it
/// painted, and how much lowering had to say about it.
///
/// The ids are sorted rather than positional because a member that carves
/// removes blocks and a member that paints adds them; either way the
/// multiset moves, and a member that does nothing leaves it alone. What
/// sorting costs is position: a member that only *relocated* blocks whose
/// ids are already in the palette would read as having changed nothing.
/// No role does that today, and one that did would need position back.
///
/// The diagnostic count is here so a caller can assert lowering deferred
/// nothing. It is deliberately *not* part of what "changed the build"
/// means — a deferral is a member that did not paint, and letting it count
/// as a change is how a fixture escapes the comparison it exists for.
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
        let refused = !missing(&with_src).is_empty();
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
/// with the reason it lays no block of its own.
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
            // `level` groups its children and lays nothing of its own;
            // `circuit` reserves a volume the redstone phases paint into
            // later and lays no block itself; `place` instantiates a def
            // whose members carry their own materials; `connect` lays a
            // walkway out of `path=@MAT`, a different key with a code of
            // its own; and a keyword the role table does not know is
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

/// The shipped examples carry no bare `floor` or `walls`, so the refusal
/// costs the corpus nothing.
///
/// Every `.crn` in the directory is read and every one is parsed — a
/// source that stops parsing fails here rather than being skipped past,
/// which `cli_check_parity`'s "every example passes all four commands"
/// requires of them anyway.
#[test]
fn no_shipped_example_names_a_member_with_no_material() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
    let paths: Vec<std::path::PathBuf> = std::fs::read_dir(root)
        .expect("the examples directory is there")
        .map(|entry| entry.expect("read the entry").path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("crn"))
        .collect();
    assert!(!paths.is_empty(), "premise: the corpus was actually read");

    for path in paths {
        let source = std::fs::read_to_string(&path).expect("read the example");
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let module = parse(&source).unwrap_or_else(|e| panic!("{name} must parse: {e}"));
        let ir = lower(&module);
        let found: Vec<String> = check(&module, &ir, None)
            .iter()
            .filter(|d| d.code == DiagnosticCode::MissingMaterial)
            .map(|d| d.primary.clone())
            .collect();
        assert!(found.is_empty(), "{name}: {found:?}");
    }
}
