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

const WALLS_PAINT_NOTHING: &str = "a `walls` member that would reach this row paints nothing; the \
                                   plate fits once that member's material resolves to a block";

const INTERIOR_NEEDS: &str = "an interior needs a size of at least 3 on both axes";

/// A finding as its code, the text it points at, its primary and its notes.
type Finding<'a> = (DiagnosticCode, &'a str, &'a str, Vec<&'a str>);

/// [`THEME`] and a 5x5 struct with walls up to row 3, `lines` after its walls.
fn walled(lines: &str) -> String {
    walled_at("5x5", lines)
}

/// [`walled`] at `size=<size>`.
fn walled_at(size: &str, lines: &str) -> String {
    format!("{THEME}struct s size={size}\n  walls mat_slot=wall height=3\n  {lines}\n")
}

/// A 5x5 struct with no theme, so its walls up to row 3 paint nothing, `lines` after its walls.
fn unpainted(lines: &str) -> String {
    format!("struct s size=5x5\n  walls mat_slot=wall height=3\n  {lines}\n")
}

/// The note on an `at=` naming no side, giving `example` as one.
fn side_note(example: &str) -> String {
    format!("a side is one of front, back, left, right, as in `at={example}`")
}

/// Each finding of `out` as a [`Finding`], in report order.
fn findings<'a>(src: &'a str, out: &'a BlockArrayIr) -> Vec<Finding<'a>> {
    out.diagnostics
        .iter()
        .map(|d| (d.code, slice(src, d), d.primary.as_str(), notes(d)))
        .collect()
}

/// Each finding of `found` as its code and the text it points at.
fn located<'a>(found: &[Finding<'a>]) -> Vec<(DiagnosticCode, &'a str)> {
    found.iter().map(|f| (f.0, f.1)).collect()
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
    assert_refused_at("5x5", line, primary, expected_notes);
}

/// [`assert_refused`] in [`walled_at`] `size`.
fn assert_refused_at(size: &str, line: &str, primary: &str, expected_notes: &[&str]) {
    let src = walled_at(size, line);
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::UnplaceablePlate,
            line,
            primary,
            expected_notes.to_vec(),
        )],
        "{size} {line}",
    );
    assert_eq!(plate_cells(only_structure(&out)), [], "{size} {line}");
}

#[test]
fn a_plate_with_no_anchor_it_can_read_is_refused_with_the_forms_one_takes() {
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
fn a_bare_side_is_offered_the_anchor_on_either_face_of_its_wall() {
    for (line, primary, suggestion) in [
        (
            "pressure_plate at=front",
            "pressure_plate `at=` must be a two-segment reference, not identifier `front`",
            "did you mean `at=front.outside` or `at=inside.front`?",
        ),
        (
            "pressure_plate at=lfet",
            "pressure_plate `at=` must be a two-segment reference, not identifier `lfet`",
            "did you mean `at=left.outside` or `at=inside.left`?",
        ),
    ] {
        assert_refused(line, primary, &[ANCHOR_FORMS, suggestion]);
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
        "pressure_plate at=inside.front offset=2 y=4",
        "pressure_plate y=4 does not fit in the struct (dims.y=4)",
        &[
            "`y=` runs from 0 to 3, the struct's top row",
            LOWER_OR_RAISE,
        ],
    );
}

#[test]
fn a_plate_past_its_wall_on_a_struct_with_no_interior_names_every_axis_to_grow() {
    let no_interior = |sizes: &str| {
        format!("the struct's {sizes}, so no `offset=` reaches an interior voxel; {INTERIOR_NEEDS}")
    };
    for (size, line, primary, expected_notes) in [
        (
            "5x2",
            "pressure_plate at=inside.front offset=9",
            "pressure_plate `offset=9` runs past the front wall (length 5)",
            [
                no_interior("size.h is 2"),
                "grow `size.h` to at least 3 and use an `offset=` from 1 to 3".to_owned(),
            ],
        ),
        (
            "5x2",
            "pressure_plate at=inside.left offset=2",
            "pressure_plate `offset=2` runs past the left wall (length 2)",
            [
                no_interior("size.h is 2"),
                "grow `size.h` to at least 3 and use an `offset=` away from both ends of the wall"
                    .to_owned(),
            ],
        ),
        (
            "2x5",
            "pressure_plate at=inside.left offset=5",
            "pressure_plate `offset=5` runs past the left wall (length 5)",
            [
                no_interior("size.w is 2"),
                "grow `size.w` to at least 3 and use an `offset=` from 1 to 3".to_owned(),
            ],
        ),
        (
            "2x2",
            "pressure_plate at=inside.front offset=9",
            "pressure_plate `offset=9` runs past the front wall (length 2)",
            [
                no_interior("size.w is 2 and size.h is 2"),
                "grow `size.w` to at least 3 and `size.h` to at least 3 and use an `offset=` away \
                 from both ends of the wall"
                    .to_owned(),
            ],
        ),
    ] {
        let expected: Vec<&str> = expected_notes.iter().map(String::as_str).collect();
        assert_refused_at(size, line, primary, &expected);
    }
}

#[test]
fn a_plate_outside_a_wall_no_roof_overhangs_is_refused_above_the_floor_row() {
    let line = "pressure_plate at=front.outside y=2";
    let primary = "pressure_plate `at=front.outside` at y=2 has no exterior voxel to sit on";
    let fix = "set `y=0`, or give a `roof` an `overhang=` of at least 1";
    assert_refused(line, primary, &[NO_ROOF_OVERHANG, fix]);
    // A roof whose `overhang=` is unreadable draws none, and the same fix applies to it.
    let roof = "roof kind=flat mat_slot=wall overhang=big";
    let src = walled(&format!("{roof}\n  {line}"));
    let out = lowered(&src);
    let found = findings(&src, &out);
    assert_eq!(
        located(&found),
        [
            (DiagnosticCode::IgnoredArgument, "big"),
            (DiagnosticCode::UnplaceablePlate, line),
        ],
    );
    assert_eq!(
        found[1],
        (
            DiagnosticCode::UnplaceablePlate,
            line,
            primary,
            vec![NO_ROOF_OVERHANG, fix],
        ),
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
                "lower the plate's `level`, or raise the `walls` to reach its row",
            ],
        ),
        (
            "level y=1\n    pressure_plate at=front.outside",
            "pressure_plate at=front.outside",
            "pressure_plate `at=front.outside` at y=0 under `level y=1` (row 1) has no exterior \
             voxel to sit on",
            [
                NO_ROOF_OVERHANG,
                "give a `roof` an `overhang=` of at least 1",
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
    for y in [2, 3] {
        let line = format!("pressure_plate at=inside.front offset=2 y={y}");
        let src = unpainted(&line);
        let out = lowered(&src);
        let found = findings(&src, &out);
        assert_eq!(
            located(&found),
            [
                (DiagnosticCode::NoThemeBound, src.trim_end()),
                (DiagnosticCode::DeferredMember, line.as_str()),
            ],
        );
        assert_eq!(
            found[1],
            (
                DiagnosticCode::DeferredMember,
                line.as_str(),
                format!("pressure_plate y={y} does not fit in the struct (dims.y=1)").as_str(),
                vec![WALLS_PAINT_NOTHING],
            ),
        );
    }
}

#[test]
fn a_plate_above_where_walls_that_paint_nothing_would_reach_is_refused_with_their_rows() {
    let line = "pressure_plate at=inside.front offset=2 y=4";
    let src = unpainted(line);
    let out = lowered(&src);
    let found = findings(&src, &out);
    assert_eq!(
        located(&found),
        [
            (DiagnosticCode::NoThemeBound, src.trim_end()),
            (DiagnosticCode::UnplaceablePlate, line),
        ],
    );
    assert_eq!(
        found[1],
        (
            DiagnosticCode::UnplaceablePlate,
            line,
            "pressure_plate y=4 does not fit in the struct (dims.y=1)",
            vec![
                "`y=` runs from 0 to 3, the struct's top row",
                "rows 1 to 3 wait on a `walls` member that paints nothing",
                LOWER_OR_RAISE,
            ],
        ),
    );
}

#[test]
fn a_plate_its_own_line_puts_nowhere_is_refused_though_the_walls_paint_nothing() {
    let placed = "pressure_plate at=inside.left offset=1 y=0";
    for (line, primary, expected_notes) in [
        (
            "pressure_plate at=inside.front offset=0 y=2 -> sig.a",
            "pressure_plate `at=inside.front offset=0` is at a corner of the front wall, so the \
             voxel inside it belongs to the neighbouring wall",
            vec![
                "use an `offset=` from 1 to 3 to reach an interior voxel",
                BINDING_STILL_READ,
            ],
        ),
        (
            "pressure_plate at=front.outside y=2",
            "pressure_plate `at=front.outside` at y=2 has no exterior voxel to sit on",
            vec![
                NO_ROOF_OVERHANG,
                "set `y=0`, or give a `roof` an `overhang=` of at least 1",
            ],
        ),
        (
            "pressure_plate at=inside.front offset=9 y=2",
            "pressure_plate `offset=9` runs past the front wall (length 5)",
            vec![
                "an `at=inside.front` plate takes an `offset=` from 1 to 3, clear of the wall's \
                 corners",
            ],
        ),
    ] {
        let src = unpainted(&format!("{placed}\n  {line}"));
        let out = lowered(&src);
        let found = findings(&src, &out);
        assert_eq!(
            located(&found),
            [
                (DiagnosticCode::NoThemeBound, src.trim_end()),
                (DiagnosticCode::UnplaceablePlate, line),
            ],
            "{line}",
        );
        assert_eq!(
            found[1],
            (
                DiagnosticCode::UnplaceablePlate,
                line,
                primary,
                expected_notes
            ),
            "{line}",
        );
        assert_eq!(plate_cells(only_structure(&out)), [(1, 0, 1)], "{line}");
    }
}

#[test]
fn a_plate_outside_an_overhang_a_level_scoped_roof_does_not_draw_only_defers() {
    let roof = "roof kind=flat mat_slot=wall overhang=1";
    let line = "pressure_plate at=front.outside offset=2 y=1";
    let src = walled(&format!("level y=3\n    {roof}\n  {line}"));
    let out = lowered(&src);
    let found = findings(&src, &out);
    assert_eq!(
        located(&found),
        [
            (DiagnosticCode::DeferredMember, roof),
            (DiagnosticCode::DeferredMember, line),
        ],
    );
    assert_eq!(
        found[1],
        (
            DiagnosticCode::DeferredMember,
            line,
            "pressure_plate `at=front.outside` at y=1 has no exterior voxel to sit on",
            vec![
                "a `roof` written with an `overhang=` draws nothing, so the struct has no \
                 overhang to hold the plate; it fits once that roof draws",
            ],
        ),
    );
}

#[test]
fn a_roof_under_a_level_with_no_offset_gives_a_plate_no_overhang_to_wait_on() {
    let level = "level";
    let line = "pressure_plate at=front.outside offset=2 y=1";
    let src = walled(&format!(
        "{level}\n    roof kind=flat mat_slot=wall overhang=1\n  {line}"
    ));
    let out = lowered(&src);
    let found = findings(&src, &out);
    assert_eq!(
        located(&found),
        [
            (DiagnosticCode::DeferredMember, level),
            (DiagnosticCode::UnplaceablePlate, line),
        ],
    );
    assert_eq!(
        found[1],
        (
            DiagnosticCode::UnplaceablePlate,
            line,
            "pressure_plate `at=front.outside` at y=1 has no exterior voxel to sit on",
            vec![
                NO_ROOF_OVERHANG,
                "set `y=0`, or give a `roof` an `overhang=` of at least 1",
            ],
        ),
    );
}

#[test]
fn every_plate_left_unplaced_says_its_signal_still_reaches_the_netlist() {
    let token = "@wall.stone.cobble";
    let unresolved = format!(
        "theme t:\n  slot wall -> {token}\n\nstruct s size=5x5\n  walls mat_slot=wall height=3\n  \
         PLATE\n"
    );
    let literal = "@oak_pressure_plate[powered=true]";
    let state_literal = format!(
        "theme t:\n  slot wall -> @cobblestone\n  slot p -> {literal}\n\n\
         struct s size=5x5\n  walls mat_slot=wall height=3\n  PLATE\n"
    );
    for (template, line, before, code) in [
        (
            walled("PLATE"),
            "pressure_plate at=front.outside offset=5",
            vec![],
            DiagnosticCode::UnplaceablePlate,
        ),
        (
            walled("PLATE"),
            "pressure_plate at=sideways",
            vec![],
            DiagnosticCode::UnplaceablePlate,
        ),
        (
            unresolved,
            "pressure_plate at=inside.front offset=2 y=2",
            vec![(DiagnosticCode::AbstractTokenDeferred, token)],
            DiagnosticCode::DeferredMember,
        ),
        (
            state_literal,
            "pressure_plate at=front.outside mat_slot=p",
            vec![(DiagnosticCode::StateLiteralUnchecked, literal)],
            DiagnosticCode::DeferredMember,
        ),
    ] {
        let bound_line = format!("{line} -> sig.a");
        let unbound_src = template.replace("PLATE", line);
        let bound_src = template.replace("PLATE", &bound_line);
        let (unbound_out, bound_out) = (lowered(&unbound_src), lowered(&bound_src));
        let unbound = findings(&unbound_src, &unbound_out);
        let bound = findings(&bound_src, &bound_out);
        let mut expected = before.clone();
        expected.push((code, line));
        assert_eq!(located(&unbound), expected, "{line}");
        expected.pop();
        expected.push((code, bound_line.as_str()));
        assert_eq!(located(&bound), expected, "{line}");
        let (unplaced, others) = unbound.split_last().expect("the plate is reported");
        let (bound_plate, bound_others) = bound.split_last().expect("the plate is reported");
        assert_eq!(bound_others, others, "{line}");
        assert_eq!(bound_plate.2, unplaced.2, "{line}");
        assert!(!unplaced.3.contains(&BINDING_STILL_READ), "{line}");
        let mut with_note = unplaced.3.clone();
        with_note.push(BINDING_STILL_READ);
        assert_eq!(bound_plate.3, with_note, "{line}");
    }
}
