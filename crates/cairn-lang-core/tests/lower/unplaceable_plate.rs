//! A `pressure_plate` its own line puts on no voxel is refused, one that a member painting nothing
//! displaces only defers, and either way a bound plate says its signal still reaches the netlist
//! (`spec/redstone` "Signal binding").

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::block_array::{BlockArray, BlockArrayIr};

use crate::common::{lowered, notes, only_structure, slice, state_at};

const THEME: &str = "theme t:\n  slot wall -> @cobblestone\n\n";

const ANCHOR_FORMS: &str = "write `at=<side>.outside` to sit outside a wall or `at=inside.<side>` \
                            to sit inside it, where `<side>` is front, back, left or right";

const BINDING_STILL_READ: &str =
    "its signal binding still reaches the netlist, with no plate placed to drive it";

const NO_ROOF_OVERHANG: &str = "no roof on this struct draws an overhang, and only at y=0 does \
                                the plate fall back to the wall's foundation";

const LOWER_OR_RAISE: &str = "lower the plate, or raise the `walls` to reach its row";

/// A finding as its code, the text it points at, its primary and its notes.
type Finding<'a> = (DiagnosticCode, &'a str, &'a str, Vec<&'a str>);

/// [`THEME`] and a 5x5 struct with walls up to row 3, `lines` after its walls.
fn walled(lines: &str) -> String {
    format!("{THEME}struct s size=5x5\n  walls mat_slot=wall height=3\n  {lines}\n")
}

/// Each finding of `out` as a [`Finding`], in report order.
fn findings<'a>(src: &'a str, out: &'a BlockArrayIr) -> Vec<Finding<'a>> {
    out.diagnostics
        .iter()
        .map(|d| (d.code, slice(src, d), d.primary.as_str(), notes(d)))
        .collect()
}

/// The cells of `ba` that hold a pressure plate.
fn plate_cells(ba: &BlockArray) -> Vec<(u32, u32, u32)> {
    let mut cells = Vec::new();
    for y in 0..ba.dims.y {
        for z in 0..ba.dims.z {
            for x in 0..ba.dims.x {
                if state_at(ba, x, y, z).id == "minecraft:oak_pressure_plate" {
                    cells.push((x, y, z));
                }
            }
        }
    }
    cells
}

/// `line` in [`walled`] raises exactly one `E_UNPLACEABLE_PLATE` with `primary` and `notes`
/// over the line, and paints no plate.
fn assert_refused(line: &str, primary: &str, expected_notes: &[&str]) {
    let src = walled(line);
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::UnplaceablePlate,
            line,
            primary,
            expected_notes.to_vec(),
        )],
        "{line}",
    );
    assert_eq!(plate_cells(only_structure(&out)), [], "{line}");
}

#[test]
fn a_plate_with_no_anchor_it_can_read_is_refused_with_the_forms_one_takes() {
    let side_note =
        |example: &str| format!("a side is one of front, back, left, right, as in `at={example}`");
    for (line, primary, expected_notes) in [
        (
            "pressure_plate",
            "pressure_plate has no `at=`, so nothing says which wall it sits by",
            vec![ANCHOR_FORMS.to_owned()],
        ),
        (
            "pressure_plate at=center",
            "pressure_plate `at=` must be a two-segment reference, not identifier `center`",
            vec![ANCHOR_FORMS.to_owned()],
        ),
        (
            "pressure_plate at=\"front.outside\"",
            "pressure_plate `at=` must be a two-segment reference, not string `\"front.outside\"`",
            vec![ANCHOR_FORMS.to_owned()],
        ),
        (
            "pressure_plate at=front.outside.left",
            "pressure_plate `at=front.outside.left` has 3 segments, and an anchor has two",
            vec![ANCHOR_FORMS.to_owned()],
        ),
        (
            "pressure_plate at=sideways.up",
            "pressure_plate `at=sideways.up` is not an anchor",
            vec![ANCHOR_FORMS.to_owned()],
        ),
        (
            "pressure_plate at=up.outside",
            "pressure_plate `at=up.outside`: `up` is not a wall side",
            vec![side_note("front.outside")],
        ),
        (
            "pressure_plate at=inside.up",
            "pressure_plate `at=inside.up`: `up` is not a wall side",
            vec![side_note("inside.front")],
        ),
    ] {
        let expected: Vec<&str> = expected_notes.iter().map(String::as_str).collect();
        assert_refused(line, primary, &expected);
    }
}

#[test]
fn an_anchor_one_typo_or_swap_from_a_valid_one_is_suggested() {
    let side_note =
        |example: &str| format!("a side is one of front, back, left, right, as in `at={example}`");
    for (line, primary, expected_notes) in [
        (
            "pressure_plate at=front.inside",
            "pressure_plate `at=front.inside` is not an anchor",
            [
                ANCHOR_FORMS.to_owned(),
                "did you mean `at=inside.front`?".to_owned(),
            ],
        ),
        (
            "pressure_plate at=outside.left",
            "pressure_plate `at=outside.left` is not an anchor",
            [
                ANCHOR_FORMS.to_owned(),
                "did you mean `at=left.outside`?".to_owned(),
            ],
        ),
        (
            "pressure_plate at=back.outsde",
            "pressure_plate `at=back.outsde` is not an anchor",
            [
                ANCHOR_FORMS.to_owned(),
                "did you mean `at=back.outside`?".to_owned(),
            ],
        ),
        (
            "pressure_plate at=fornt.outside",
            "pressure_plate `at=fornt.outside`: `fornt` is not a wall side",
            [
                side_note("front.outside"),
                "did you mean `at=front.outside`?".to_owned(),
            ],
        ),
        (
            "pressure_plate at=inside.rigth",
            "pressure_plate `at=inside.rigth`: `rigth` is not a wall side",
            [
                side_note("inside.front"),
                "did you mean `at=inside.right`?".to_owned(),
            ],
        ),
    ] {
        let expected: Vec<&str> = expected_notes.iter().map(String::as_str).collect();
        assert_refused(line, primary, &expected);
    }
}

#[test]
fn a_plate_coordinate_that_is_no_u32_is_refused_with_what_it_counts() {
    let offset_counts =
        "`offset=` counts cells along the wall from 0, its left end seen from outside";
    let y_counts =
        "`y=` counts rows up from 0, the base row of the struct or of the plate's `level`";
    assert_refused(
        "pressure_plate at=front.outside offset=two",
        "pressure_plate `offset=` must be a non-negative integer that fits in u32, not identifier \
         `two`",
        &[offset_counts],
    );
    assert_refused(
        "pressure_plate at=front.outside y=4294967296",
        "pressure_plate `y=` must be a non-negative integer that fits in u32, not integer \
         `4294967296`",
        &[y_counts],
    );
    assert_refused(
        "pressure_plate at=front.outside y=\"1\"",
        "pressure_plate `y=` must be a non-negative integer that fits in u32, not string `\"1\"`",
        &[y_counts],
    );
}

#[test]
fn a_plate_past_its_wall_or_above_the_struct_is_refused_with_the_range_it_takes() {
    assert_refused(
        "pressure_plate at=front.outside offset=5",
        "pressure_plate `offset=5` runs past the front wall (length 5)",
        &["an `at=front.outside` plate takes an `offset=` from 0 to 4"],
    );
    assert_refused(
        "pressure_plate at=inside.left offset=5",
        "pressure_plate `offset=5` runs past the left wall (length 5)",
        &["an `at=inside.left` plate takes an `offset=` from 1 to 3, clear of the wall's corners"],
    );
    assert_refused(
        "pressure_plate at=inside.front y=4",
        "pressure_plate y=4 does not fit in the struct (dims.y=4)",
        &[
            "`y=` runs from 0 to 3, the struct's top row",
            LOWER_OR_RAISE,
        ],
    );
}

#[test]
fn a_plate_with_no_voxel_beside_or_inside_its_wall_is_refused_with_the_fix() {
    assert_refused(
        "pressure_plate at=front.outside y=2",
        "pressure_plate `at=front.outside` at y=2 has no exterior voxel to sit on",
        &[
            NO_ROOF_OVERHANG,
            "set `y=0`, or give the struct a `roof` with an `overhang=` of at least 1",
        ],
    );
    assert_refused(
        "pressure_plate at=inside.front offset=0 y=1",
        "pressure_plate `at=inside.front offset=0` is at a corner of the front wall, so the voxel \
         inside it belongs to the neighbouring wall",
        &["use an `offset=` from 1 to 3 to reach an interior voxel"],
    );
    let line = "pressure_plate at=inside.front offset=2";
    let src = format!("{THEME}struct s size=5x2\n  walls mat_slot=wall height=3\n  {line}\n");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::UnplaceablePlate,
            line,
            "pressure_plate `at=inside.front offset=2`: the struct's size.h is 2, so it has no \
             interior voxel",
            vec![
                "an interior needs a size of at least 3 on both axes",
                "grow `size.h` to at least 3",
            ],
        )],
    );
}

#[test]
fn a_plate_under_a_level_is_refused_naming_the_level_and_the_row_it_reaches() {
    for (lines, line, primary, expected_notes) in [
        (
            "level y=2\n    pressure_plate at=inside.front offset=2 y=2",
            "pressure_plate at=inside.front offset=2 y=2",
            "pressure_plate y=2 under `level y=2` (row 4) does not fit in the struct (dims.y=4)",
            [
                "`y=` runs from 0 to 1 under `level y=2`, the struct's top row being 3",
                LOWER_OR_RAISE,
            ],
        ),
        (
            "level y=3\n    pressure_plate at=inside.front offset=2 y=1",
            "pressure_plate at=inside.front offset=2 y=1",
            "pressure_plate y=1 under `level y=3` (row 4) does not fit in the struct (dims.y=4)",
            [
                "`y=` takes only 0 under `level y=3`, the struct's top row being 3",
                LOWER_OR_RAISE,
            ],
        ),
        (
            "level y=5\n    pressure_plate at=inside.front offset=2",
            "pressure_plate at=inside.front offset=2",
            "pressure_plate y=0 under `level y=5` (row 5) does not fit in the struct (dims.y=4)",
            [
                "the struct's top row is 3, below this plate's `level y=5`",
                LOWER_OR_RAISE,
            ],
        ),
        (
            "level y=1\n    pressure_plate at=front.outside",
            "pressure_plate at=front.outside",
            "pressure_plate `at=front.outside` at y=0 under `level y=1` (row 1) has no exterior \
             voxel to sit on",
            [
                NO_ROOF_OVERHANG,
                "give the struct a `roof` with an `overhang=` of at least 1",
            ],
        ),
    ] {
        let src = walled(lines);
        let out = lowered(&src);
        assert_eq!(
            findings(&src, &out),
            [(
                DiagnosticCode::UnplaceablePlate,
                line,
                primary,
                expected_notes.to_vec(),
            )],
            "{lines}",
        );
    }
}

#[test]
fn a_plate_on_the_last_row_or_cell_its_wall_offers_is_placed_silently() {
    for (line, cell) in [
        ("pressure_plate at=inside.front offset=3 y=3", (3, 3, 3)),
        ("pressure_plate at=front.outside offset=4 y=0", (4, 0, 4)),
        ("pressure_plate at=inside.left offset=3 y=1", (1, 1, 3)),
    ] {
        let src = walled(line);
        let out = lowered(&src);
        assert_eq!(findings(&src, &out), [], "{line}");
        assert_eq!(plate_cells(only_structure(&out)), [cell], "{line}");
    }
}

#[test]
fn a_plate_walls_that_paint_nothing_would_have_reached_only_defers() {
    let line = "pressure_plate at=inside.front offset=2 y=2";
    let src = format!("struct s size=5x5\n  walls mat_slot=wall height=3\n  {line}\n");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out)[1..],
        [(
            DiagnosticCode::DeferredMember,
            line,
            "pressure_plate y=2 does not fit in the struct (dims.y=1)",
            vec![
                "a `walls` member that would reach this row paints nothing; the plate fits once \
                 that member's material resolves to a block",
            ],
        )],
    );
    assert_eq!(out.diagnostics[0].code, DiagnosticCode::NoThemeBound);

    let past = "pressure_plate at=inside.front offset=2 y=4";
    let src = format!("struct s size=5x5\n  walls mat_slot=wall height=3\n  {past}\n");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out)[1..],
        [(
            DiagnosticCode::UnplaceablePlate,
            past,
            "pressure_plate y=4 does not fit in the struct (dims.y=1)",
            vec!["`y=` takes only 0, the struct's top row", LOWER_OR_RAISE],
        )],
    );
}

#[test]
fn a_plate_outside_an_overhang_a_level_scoped_roof_does_not_draw_only_defers() {
    let roof = "roof kind=flat mat_slot=wall overhang=1";
    let line = "pressure_plate at=front.outside offset=2 y=1";
    let src = walled(&format!("level y=3\n    {roof}\n  {line}"));
    let out = lowered(&src);
    let found = findings(&src, &out);
    assert_eq!(
        found.iter().map(|f| (f.0, f.1)).collect::<Vec<_>>(),
        [
            (DiagnosticCode::DeferredMember, roof),
            (DiagnosticCode::DeferredMember, line),
        ],
    );
    assert_eq!(
        (found[1].2, found[1].3.clone()),
        (
            "pressure_plate `at=front.outside` at y=1 has no exterior voxel to sit on",
            vec![
                "a `roof` written with an `overhang=` draws nothing, so the struct has no \
                 overhang to hold the plate; it fits once that roof draws",
            ],
        ),
    );
}

#[test]
fn every_plate_left_unplaced_says_its_signal_still_reaches_the_netlist() {
    let themeless = "struct s size=5x5\n  walls mat_slot=wall height=3\n  PLATE\n".to_owned();
    let state_literal = "theme t:\n  slot wall -> @cobblestone\n  \
                         slot p -> @oak_pressure_plate[powered=true]\n\n\
                         struct s size=5x5\n  walls mat_slot=wall height=3\n  PLATE\n"
        .to_owned();
    for (template, line, code) in [
        (
            walled("PLATE"),
            "pressure_plate at=front.outside offset=5",
            DiagnosticCode::UnplaceablePlate,
        ),
        (
            walled("PLATE"),
            "pressure_plate at=sideways",
            DiagnosticCode::UnplaceablePlate,
        ),
        (
            themeless,
            "pressure_plate at=inside.front offset=2 y=2",
            DiagnosticCode::DeferredMember,
        ),
        (
            state_literal,
            "pressure_plate at=front.outside mat_slot=p",
            DiagnosticCode::DeferredMember,
        ),
    ] {
        let bound_line = format!("{line} -> sig.a");
        let unbound_src = template.replace("PLATE", line);
        let bound_src = template.replace("PLATE", &bound_line);
        let (unbound, bound) = (lowered(&unbound_src), lowered(&bound_src));
        let plate = |src: &str, out: &BlockArrayIr| {
            out.diagnostics
                .iter()
                .find(|d| slice(src, d).starts_with("pressure_plate"))
                .unwrap_or_else(|| panic!("{line} must be reported: {:#?}", out.diagnostics))
                .clone()
        };
        let (unbound, bound) = (plate(&unbound_src, &unbound), plate(&bound_src, &bound));
        assert_eq!((unbound.code, bound.code), (code, code), "{line}");
        assert_eq!(bound.primary, unbound.primary, "{line}");
        assert_eq!(slice(&bound_src, &bound), bound_line, "{line}");
        assert!(!notes(&unbound).contains(&BINDING_STILL_READ), "{line}");
        let mut expected = notes(&unbound);
        expected.push(BINDING_STILL_READ);
        assert_eq!(notes(&bound), expected, "{line}");
    }
}
