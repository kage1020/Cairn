//! Lowering records each member's anchor, declared bbox, actual bbox and host face beside the grid
//! it builds (`spec/entities` "Anchor conventions").

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::block_array::{Bbox, BlockArrayIr, Face, MemberGeometry};
use serde_json::json;

use crate::common::{findings, lowered};

const PRELUDE: &str = "theme t:\n  \
                       slot wall -> @cobblestone\n  \
                       slot roof -> @spruce_stairs\n  \
                       slot glass -> @glass_pane\n  \
                       slot eave -> @oak_stairs\n\n";

/// Each member recorded under `struct::s`, as `(role, geometry)`.
fn geometry(ir: &BlockArrayIr) -> Vec<(&str, MemberGeometry)> {
    ir.members["struct::s"]
        .iter()
        .map(|member| (member.role.as_str(), member.geometry))
        .collect()
}

/// `body` under `struct s size=SIZE`, lowered with no finding.
fn clean(size: &str, body: &str) -> BlockArrayIr {
    let source = format!("{PRELUDE}struct s size={size}\n{body}");
    let ir = lowered(&source);
    assert_eq!(
        ir.diagnostics
            .iter()
            .map(|d| (d.code.as_str(), d.primary.as_str()))
            .collect::<Vec<_>>(),
        Vec::<(&str, &str)>::new(),
        "source:\n{source}",
    );
    ir
}

fn boxed(min: (u32, u32, u32), max: (u32, u32, u32)) -> Bbox {
    Bbox::spanning(min, max)
}

/// The geometry of a member that holds every cell it declares.
fn whole(anchor: (u32, u32, u32), bbox: Bbox, host_face: Option<Face>) -> MemberGeometry {
    MemberGeometry {
        anchor,
        declared_bbox: bbox,
        actual_bbox: bbox,
        host_face,
    }
}

#[test]
fn the_shell_declares_its_footprint_and_an_overhanging_roof_holds_past_it() {
    let ir = clean(
        "5x3",
        "  floor mat_slot=wall\n  \
         walls mat_slot=wall height=3\n  \
         roof kind=gable mat_slot=roof overhang=1\n",
    );
    assert_eq!(
        geometry(&ir),
        [
            ("floor", whole((1, 0, 1), boxed((1, 0, 1), (5, 0, 3)), None)),
            ("walls", whole((1, 1, 1), boxed((1, 1, 1), (5, 3, 3)), None)),
            (
                "roof",
                MemberGeometry {
                    anchor: (1, 4, 1),
                    declared_bbox: boxed((1, 4, 1), (5, 6, 3)),
                    actual_bbox: boxed((0, 4, 0), (6, 6, 4)),
                    host_face: Some(Face::Up),
                },
            ),
        ],
    );
}

#[test]
fn an_opening_declares_its_cut_against_the_outer_face_of_its_wall() {
    let ir = clean(
        "7x3",
        "  walls mat_slot=wall height=4\n  \
         door side=front at=center\n  \
         window side=back anchor=top_center offset=3 y=3 size=3x2 mat_slot=glass\n",
    );
    assert_eq!(
        geometry(&ir),
        [
            ("walls", whole((0, 1, 0), boxed((0, 1, 0), (6, 4, 2)), None)),
            (
                "door",
                whole((3, 1, 2), boxed((3, 1, 2), (3, 2, 2)), Some(Face::South)),
            ),
            (
                "window",
                whole((3, 3, 0), boxed((2, 2, 0), (4, 3, 0)), Some(Face::North)),
            ),
        ],
    );
}

#[test]
fn an_opening_on_a_side_wall_declares_its_cut_against_that_wall_s_outer_face() {
    let ir = clean(
        "7x5",
        "  walls mat_slot=wall height=4\n  \
         door side=left at=center\n  \
         window side=right offset=1 y=2 size=2x1 mat_slot=glass\n",
    );
    assert_eq!(
        geometry(&ir)[1..],
        [
            (
                "door",
                whole((0, 1, 2), boxed((0, 1, 2), (0, 2, 2)), Some(Face::West)),
            ),
            (
                "window",
                whole((6, 2, 3), boxed((6, 2, 2), (6, 2, 3)), Some(Face::East)),
            ),
        ],
    );
}

#[test]
fn a_window_anchor_moves_the_cell_offset_and_y_place_and_nothing_else() {
    let body =
        |window: &str| format!("  walls mat_slot=wall height=4\n  {window} mat_slot=glass\n");
    // The rows each third of a 2-high and a 3-high window lies above its lowest.
    for (size, rows) in [("3x2", [0, 1, 1]), ("3x3", [0, 1, 2])] {
        let top = 1 + rows[2];
        let unanchored = clean(
            "7x3",
            &body(&format!("window side=front offset=2 y=1 size={size}")),
        );
        for (anchor, along, up) in [
            ("bottom_left", 0, 0),
            ("bottom_center", 1, 0),
            ("bottom_right", 2, 0),
            ("middle_left", 0, 1),
            ("center", 1, 1),
            ("middle_right", 2, 1),
            ("top_left", 0, 2),
            ("top_center", 1, 2),
            ("top_right", 2, 2),
        ] {
            let (offset, y) = (2 + along, 1 + rows[up]);
            let window =
                format!("window side=front anchor={anchor} offset={offset} y={y} size={size}");
            let anchored = clean("7x3", &body(&window));
            assert_eq!(anchored.structures, unanchored.structures, "{window}");
            assert_eq!(
                geometry(&anchored)[1],
                (
                    "window",
                    whole(
                        (offset, y, 2),
                        boxed((2, 1, 2), (4, top, 2)),
                        Some(Face::South)
                    ),
                ),
                "{window}",
            );
        }
    }
}

#[test]
fn a_window_anchor_names_its_cell_as_seen_from_outside_its_wall() {
    // From outside the back wall its right end is its low `x`.
    let ir = clean(
        "7x3",
        "  walls mat_slot=wall height=3\n  \
         window side=back anchor=bottom_right offset=4 y=1 size=3x1 mat_slot=glass\n",
    );
    assert_eq!(
        geometry(&ir)[1],
        (
            "window",
            whole((2, 1, 0), boxed((2, 1, 0), (4, 1, 0)), Some(Face::North)),
        ),
    );
}

#[test]
fn a_window_series_and_its_mirror_declare_every_rectangle_the_line_places() {
    let ir = clean(
        "9x3",
        "  walls mat_slot=wall height=3\n  \
         window side=front offset=0 y=1 size=1x1 repeat=2 step=2 mat_slot=glass\n  \
         window side=back offset=1 y=2 size=2x1 sym=true mat_slot=glass\n",
    );
    assert_eq!(
        geometry(&ir)[1..],
        [
            (
                "window",
                whole((0, 1, 2), boxed((0, 1, 2), (2, 1, 2)), Some(Face::South)),
            ),
            (
                "window",
                whole((7, 2, 0), boxed((1, 2, 0), (7, 2, 0)), Some(Face::North)),
            ),
        ],
    );
}

#[test]
fn a_skipped_mirror_stays_in_the_declared_bbox_and_out_of_the_actual_one() {
    let ir = lowered(&format!(
        "{PRELUDE}struct s size=6x3\n  \
         walls mat_slot=wall height=3\n  \
         window side=front offset=2 y=1 size=3x1 sym=true mat_slot=glass\n"
    ));
    assert_eq!(
        ir.diagnostics
            .iter()
            .map(|d| d.code.as_str())
            .collect::<Vec<_>>(),
        ["W_DEFERRED_MEMBER"],
    );
    assert_eq!(
        geometry(&ir)[1],
        (
            "window",
            MemberGeometry {
                anchor: (2, 1, 2),
                declared_bbox: boxed((1, 1, 2), (4, 1, 2)),
                actual_bbox: boxed((2, 1, 2), (4, 1, 2)),
                host_face: Some(Face::South),
            },
        ),
    );
}

#[test]
fn an_eave_stair_declares_its_band_against_the_outer_face() {
    let ir = clean(
        "3x1",
        "  walls mat_slot=wall height=1\n  \
         roof kind=flat mat_slot=wall overhang=1\n  \
         stair kind=stairs mat_slot=eave side=front half=top facing=out shape=outer_left\n",
    );
    assert_eq!(
        geometry(&ir)[0],
        (
            "stair",
            whole((1, 0, 2), boxed((1, 0, 2), (3, 0, 2)), Some(Face::South)),
        ),
    );
}

#[test]
fn an_eave_stair_is_anchored_at_its_wall_s_left_end_seen_from_outside() {
    let ir = clean(
        "3x1",
        "  walls mat_slot=wall height=1\n  \
         roof kind=flat mat_slot=wall overhang=1\n  \
         stair kind=stairs mat_slot=eave side=back half=top facing=out shape=outer_left\n",
    );
    assert_eq!(
        geometry(&ir)[0],
        (
            "stair",
            whole((3, 0, 0), boxed((1, 0, 0), (3, 0, 0)), Some(Face::North)),
        ),
    );
}

#[test]
fn a_fixture_declares_its_one_cell_against_the_face_it_stands_by() {
    let ir = clean(
        "7x5",
        "  walls mat_slot=wall height=3\n  \
         roof kind=flat mat_slot=wall overhang=1\n  \
         chest at=front.outside offset=1 y=1\n  \
         pressure_plate at=inside.front offset=3 y=1\n  \
         lever at=inside.back offset=1 y=1\n  \
         repeater at=inside.left offset=1 y=1 facing=in\n  \
         redstone_dust at=3,1,2\n",
    );
    let cell = |at: (u32, u32, u32), host_face: Option<Face>| whole(at, Bbox::cell(at), host_face);
    assert_eq!(
        geometry(&ir)[1..6],
        [
            ("repeater", cell((2, 1, 2), Some(Face::East))),
            ("lever", cell((6, 1, 2), Some(Face::South))),
            ("redstone_dust", cell((4, 1, 3), None)),
            ("pressure_plate", cell((4, 1, 4), Some(Face::North))),
            ("chest", cell((2, 1, 6), Some(Face::South))),
        ],
    );
}

#[test]
fn a_box_member_declares_its_corners_and_holds_what_later_phases_leave_it() {
    let ir = clean(
        "5x5",
        "  walls mat_slot=wall height=2\n  \
         fill mat_slot=wall from=1,1,1 to=3,1,1\n  \
         raw_fill mat=minecraft:stone from=3,1,1 to=3,1,1\n  \
         fill fluid=water kind=source from=1,1,2 to=2,2,3\n  \
         raw_block mat=minecraft:stone at=3,2,3\n",
    );
    assert_eq!(
        geometry(&ir)[1..],
        [
            (
                "fill",
                MemberGeometry {
                    anchor: (1, 1, 1),
                    declared_bbox: boxed((1, 1, 1), (3, 1, 1)),
                    actual_bbox: boxed((1, 1, 1), (2, 1, 1)),
                    host_face: None,
                },
            ),
            (
                "raw_fill",
                whole((3, 1, 1), boxed((3, 1, 1), (3, 1, 1)), None)
            ),
            ("fill", whole((1, 1, 2), boxed((1, 1, 2), (2, 2, 3)), None)),
            (
                "raw_block",
                whole((3, 2, 3), boxed((3, 2, 3), (3, 2, 3)), None)
            ),
        ],
    );
}

#[test]
fn a_member_under_a_level_declares_the_rows_its_level_raises_it_to() {
    let ir = clean(
        "5x3",
        "  walls mat_slot=wall height=3\n  \
         level y=3\n    \
         walls mat_slot=wall height=3\n    \
         window side=front anchor=bottom_center offset=2 y=1 size=3x1 mat_slot=glass\n",
    );
    assert_eq!(
        geometry(&ir)[1..],
        [
            ("walls", whole((0, 4, 0), boxed((0, 4, 0), (4, 6, 2)), None)),
            (
                "window",
                whole((2, 4, 2), boxed((1, 4, 2), (3, 4, 2)), Some(Face::South)),
            ),
        ],
    );
}

#[test]
fn a_roof_footprint_and_bounds_that_name_what_it_builds_change_nothing() {
    let body = |extent: &str| {
        format!(
            "  walls mat_slot=wall height=3\n  roof kind=hip mat_slot=roof overhang=1 {extent}\n"
        )
    };
    let bare = clean("5x3", &body(""));
    for extent in [
        "footprint=struct",
        "bounds=expand",
        "footprint=struct bounds=expand",
    ] {
        let written = clean("5x3", &body(extent));
        assert_eq!(written.structures, bare.structures, "{extent}");
        assert_eq!(geometry(&written), geometry(&bare), "{extent}");
    }
}

/// The note refusing a roof's `footprint=`.
const FOOTPRINT: &str = "a roof covers its struct's `size=` footprint, which `footprint=struct` \
                         names; write `footprint=struct` or drop `footprint=`";

/// The note refusing a roof's `bounds=`.
const BOUNDS: &str = "a roof's `overhang=` widens its body's block array past the footprint on \
                      every side, which `bounds=expand` names; write `bounds=expand` or drop \
                      `bounds=`";

/// Each finding lowering `source` raises, as its code, the text it points at, its primary and
/// its notes.
fn found(source: &str) -> Vec<(DiagnosticCode, String, String, Vec<String>)> {
    let ir = lowered(source);
    findings(source, &ir)
        .into_iter()
        .map(|(code, at, primary, notes)| {
            (
                code,
                at.to_owned(),
                primary.to_owned(),
                notes.into_iter().map(str::to_owned).collect(),
            )
        })
        .collect()
}

/// The `E_INVALID_MEMBER` findings refusing the line `roof`, one per `(primary, notes)`.
fn refusals(
    roof: &str,
    each: &[(&str, &[&str])],
) -> Vec<(DiagnosticCode, String, String, Vec<String>)> {
    each.iter()
        .map(|&(primary, notes)| {
            (
                DiagnosticCode::InvalidMember,
                roof.to_owned(),
                primary.to_owned(),
                notes.iter().map(|&n| n.to_owned()).collect(),
            )
        })
        .collect()
}

#[test]
fn a_roof_footprint_or_bounds_that_names_another_extent_is_refused_and_draws_nothing() {
    let footprint_level = ("roof has `footprint=level`, not `struct`", &[FOOTPRINT][..]);
    for (extent, each) in [
        ("footprint=level", vec![footprint_level]),
        (
            "footprint=3",
            vec![("roof has `footprint=3`, not `struct`", &[FOOTPRINT][..])],
        ),
        (
            "bounds=clip",
            vec![("roof has `bounds=clip`, not `expand`", &[BOUNDS][..])],
        ),
        (
            "bounds=\"expand\"",
            vec![(
                "roof has a `bounds=` that is string `\"expand\"`, not `expand`",
                &[BOUNDS][..],
            )],
        ),
        (
            "footprint=level bounds=clip",
            vec![
                footprint_level,
                ("roof has `bounds=clip`, not `expand`", &[BOUNDS][..]),
            ],
        ),
    ] {
        for level in ["", "  level y=3\n  "] {
            let roof = format!("roof kind=gable mat_slot=roof {extent}");
            let source = format!(
                "{PRELUDE}struct s size=5x3\n  walls mat_slot=wall height=3\n{level}  {roof}\n"
            );
            assert_eq!(found(&source), refusals(&roof, &each), "source:\n{source}");
            let ir = lowered(&source);
            assert_eq!(
                geometry(&ir)
                    .iter()
                    .map(|(role, _)| *role)
                    .collect::<Vec<_>>(),
                ["walls"],
                "source:\n{source}",
            );
            assert_eq!(ir.structures["struct::s"].dims.y, 4, "source:\n{source}");
        }
    }
}

#[test]
fn a_roof_line_is_refused_for_each_of_its_keys_that_keeps_it_from_drawing() {
    let kinds = "unknown roof `kind=bogus` (expected one of gable, shed, hip, flat)";
    let slope = "shed roof requires `slope_to=` (one of front, back, left, right)";
    let footprint = ("roof has `footprint=walls`, not `struct`", &[FOOTPRINT][..]);
    for (roof, each) in [
        (
            "roof kind=bogus mat_slot=roof footprint=walls",
            vec![(kinds, &[][..]), footprint],
        ),
        (
            "roof kind=shed mat_slot=roof footprint=walls",
            vec![(slope, &[][..]), footprint],
        ),
        (
            "roof kind=gable mat_slot=roof footprint=walls bounds=shrink",
            vec![
                footprint,
                ("roof has `bounds=shrink`, not `expand`", &[BOUNDS][..]),
            ],
        ),
    ] {
        let source =
            format!("{PRELUDE}struct s size=5x3\n  walls mat_slot=wall height=3\n  {roof}\n");
        assert_eq!(found(&source), refusals(roof, &each), "source:\n{source}");
    }
}

#[test]
fn a_refused_roof_s_overhang_leaves_the_footprint_alone() {
    let roof = "roof kind=gable mat_slot=roof overhang=1 footprint=level";
    let source = format!("{PRELUDE}struct s size=5x3\n  walls mat_slot=wall height=3\n  {roof}\n");
    assert_eq!(
        found(&source),
        refusals(
            roof,
            &[("roof has `footprint=level`, not `struct`", &[FOOTPRINT])]
        ),
    );
    let dims = lowered(&source).structures["struct::s"].dims;
    assert_eq!((dims.x, dims.z), (5, 3));
}

#[test]
fn a_roof_refused_for_its_extent_still_writes_the_rows_its_shape_rises_to() {
    let roof = "roof kind=flat mat_slot=wall footprint=level";
    let raw = "raw_block mat=minecraft:stone at=2,4,1";
    let source =
        format!("{PRELUDE}struct s size=5x3\n  walls mat_slot=wall height=3\n  {roof}\n  {raw}\n");
    let mut expected = refusals(
        roof,
        &[("roof has `footprint=level`, not `struct`", &[FOOTPRINT])],
    );
    expected.push((
        DiagnosticCode::DeferredMember,
        raw.to_owned(),
        "raw_block `at=2,4,1` does not fit in the struct (dims.y=4)".to_owned(),
        vec![
            "a `roof` that would rise to this row draws nothing; the block is placed once that \
             roof draws"
                .to_owned(),
        ],
    ));
    assert_eq!(found(&source), expected);
}

#[test]
fn the_geometry_serializes_beside_the_resolved_state() {
    let ir = clean(
        "3x1",
        "  walls mat_slot=wall height=2\n  \
         window side=front offset=1 y=1 size=1x1 mat_slot=glass\n",
    );
    let json = serde_json::to_value(&ir).expect("the IR serializes");
    assert_eq!(
        json["members"]["struct::s"][1]["geometry"],
        json!({
            "anchor": [1, 1, 0],
            "declared_bbox": { "min": [1, 1, 0], "max": [1, 1, 0] },
            "actual_bbox": { "min": [1, 1, 0], "max": [1, 1, 0] },
            "host_face": "south",
        }),
    );
}
