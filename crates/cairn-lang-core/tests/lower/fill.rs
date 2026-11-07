//! A `fill` paints its box with a material in `massing`, or pours water into it once every other
//! member is painted and waterlogs the blocks there that hold water (`spec/blockstate`
//! "`waterlogged`").

use cairn_lang_core::DiagnosticCode;

use crate::common::{
    BIN_THEME, Finding, Wet, bin, door_at, findings, layer, lowered, lowered_with, oak_door,
    only_structure, state_at,
};

#[test]
fn a_material_fill_paints_every_cell_of_its_box_with_its_corners_in_either_order() {
    let out = lowered(&bin("fill mat_slot=stone from=3,2,1 to=1,1,2"));
    assert_eq!(out.diagnostics, []);
    let array = only_structure(&out);
    for y in [1, 2] {
        assert_eq!(
            layer(array, y),
            [
                ".......", ".#####.", ".#===#.", ".#===#.", ".#####.", ".......",
            ],
            "y={y}",
        );
    }
}

#[test]
fn a_water_fill_pours_a_source_into_each_cell_that_holds_no_block() {
    let out = lowered(&bin("fill fluid=water kind=source from=0,1,0 to=4,1,3"));
    assert_eq!(out.diagnostics, []);
    let array = only_structure(&out);
    assert_eq!(
        layer(array, 1),
        [
            ".......", ".#####.", ".#~~~#.", ".#~~~#.", ".#####.", ".......",
        ],
    );
    assert_eq!(
        state_at(array, 2, 1, 2).properties_text(),
        "level=0",
        "a still source",
    );
    assert_eq!(layer(array, 2)[2], ".#...#.", "the box ends at its top row");
}

#[test]
fn water_waterlogs_the_blocks_that_hold_it_and_each_stays_its_own_members() {
    let src = bin("window side=front offset=1 y=1 size=3x1 mat_slot=pane\n  \
         chest at=inside.back offset=1 y=1\n  \
         chest at=inside.back offset=3 y=1 waterlogged=false\n  \
         fill fluid=water from=0,1,0 to=4,1,3");
    let out = lowered_with(&src, &Wet);
    assert_eq!(out.diagnostics, []);
    let array = only_structure(&out);
    assert_eq!(
        layer(array, 1),
        [
            ".......", ".#####.", ".#c~C#.", ".#~~~#.", ".#PPP#.", ".......",
        ],
    );
    let members = &out.members["struct::s"];
    let cells_of = |role: &str| -> Vec<(String, usize)> {
        members
            .iter()
            .filter(|member| member.role == role)
            .flat_map(|member| &member.resolved_state)
            .map(|block| {
                let state = &block.state;
                (
                    format!("{}[{}]", state.id, state.properties_text()),
                    block.cells().count(),
                )
            })
            .collect()
    };
    assert_eq!(
        cells_of("window"),
        [("minecraft:glass_pane[waterlogged=true]".to_owned(), 3)],
    );
    assert_eq!(
        cells_of("fill"),
        [("minecraft:water[level=0]".to_owned(), 4)]
    );
    assert_eq!(
        cells_of("chest"),
        [
            (
                "minecraft:chest[facing=south,waterlogged=false]".to_owned(),
                1
            ),
            (
                "minecraft:chest[facing=south,waterlogged=true]".to_owned(),
                1
            ),
        ],
    );
}

#[test]
fn without_a_target_that_says_no_block_is_waterlogged() {
    let out = lowered(&bin(
        "window side=front offset=1 y=1 size=3x1 mat_slot=pane\n  \
         fill fluid=water from=0,1,0 to=4,1,3",
    ));
    assert_eq!(out.diagnostics, []);
    assert_eq!(layer(only_structure(&out), 1)[4], ".#ppp#.");
}

#[test]
fn a_stair_or_chest_keeps_the_waterlogged_its_line_writes() {
    let eave = |written: &str| {
        let out = lowered(&bin(&format!(
            "stair kind=stairs side=front y=1 mat_slot=step{written}"
        )));
        assert_eq!(out.diagnostics, [], "{written}");
        state_at(only_structure(&out), 1, 1, 5).properties_text()
    };
    assert_eq!(eave(""), "facing=south,half=top,shape=straight");
    assert_eq!(
        eave(" waterlogged=auto"),
        "facing=south,half=top,shape=straight"
    );
    assert_eq!(
        eave(" waterlogged=true"),
        "facing=south,half=top,shape=straight,waterlogged=true",
    );
    let chest = lowered(&bin("chest at=inside.back offset=2 y=1 waterlogged=true"));
    assert_eq!(chest.diagnostics, []);
    assert_eq!(layer(only_structure(&chest), 1)[2], ".#.C.#.");
}

#[test]
fn a_second_water_fill_pours_only_where_the_first_left_no_block() {
    let src = bin("fill fluid=water from=0,1,0 to=2,1,3\n  \
         fill fluid=water from=2,1,0 to=4,1,3\n  \
         fill mat_slot=stone from=3,1,1 to=3,1,1");
    let out = lowered_with(&src, &Wet);
    assert_eq!(out.diagnostics, []);
    assert_eq!(
        layer(only_structure(&out), 1),
        [
            ".......", ".#####.", ".#~~=#.", ".#~~~#.", ".#####.", ".......",
        ],
    );
    let poured: Vec<Vec<(u32, u32, u32)>> = out.members["struct::s"]
        .iter()
        .filter(|member| member.role == "fill")
        .filter(|member| member.resolved_state[0].state.id == "minecraft:water")
        .map(|member| member.resolved_state[0].cells().collect())
        .collect();
    assert_eq!(
        poured,
        [
            vec![(2, 1, 2), (3, 1, 2), (2, 1, 3), (3, 1, 3)],
            vec![(4, 1, 3)],
        ],
    );
}

#[test]
fn a_fill_under_a_level_is_measured_from_the_level() {
    let out = lowered(&bin(
        "level y=1\n    fill mat_slot=stone from=1,0,1 to=3,0,2",
    ));
    assert_eq!(out.diagnostics, []);
    let array = only_structure(&out);
    assert_eq!(layer(array, 1)[2], ".#===#.");
    assert_eq!(layer(array, 2)[2], ".#...#.");
}

#[test]
fn a_material_fill_over_the_walls_contests_them_in_their_phase() {
    let src = bin("fill mat_slot=stone from=0,1,0 to=0,1,3");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::PhaseConflict,
            "fill mat_slot=stone from=0,1,0 to=0,1,3",
            "`fill` overwrites 4 voxels that `walls` painted in the same phase, so which block \
             the build keeps is decided by the order the two lines are written in",
            vec![
                "overwritten member declared here",
                "If the override is deliberate this is the local-override rule of \
                 spec/compilation \"Phase evaluation\" and the later line wins as written. If it \
                 is not, move one of the two so their footprints do not meet.",
                "Members in different phases may overlap freely — a `door` cut through a `walls` \
                 is the evaluation model working — so this is only reported for two members the \
                 same phase evaluates.",
            ],
        )],
    );
}

#[test]
fn walls_written_after_a_material_fill_paint_over_it_and_say_so() {
    let fill = "fill mat_slot=stone from=0,1,0 to=0,1,3";
    let walls = "walls mat_slot=wall height=2";
    let src = format!("{BIN_THEME}struct s size=5x4\n  {fill}\n  {walls}\n");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::PhaseConflict,
            walls,
            "`walls` overwrites 4 voxels that `fill` painted in the same phase, so which block \
             the build keeps is decided by the order the two lines are written in",
            vec![
                "overwritten member declared here",
                "If the override is deliberate this is the local-override rule of \
                 spec/compilation \"Phase evaluation\" and the later line wins as written. If it \
                 is not, move one of the two so their footprints do not meet.",
                "Members in different phases may overlap freely — a `door` cut through a `walls` \
                 is the evaluation model working — so this is only reported for two members the \
                 same phase evaluates.",
            ],
        )],
    );
    let array = only_structure(&out);
    let west: Vec<&str> = (0..4)
        .map(|z| state_at(array, 0, 1, z).id.as_str())
        .collect();
    assert_eq!(west, ["minecraft:cobblestone"; 4]);
}

#[test]
fn a_fill_painted_over_the_wall_beside_a_door_is_no_wall_to_its_hinge() {
    // The door is hinged toward the side with more `walls` blocks beside it.
    let walled = |fill: &str| {
        format!(
            "{BIN_THEME}struct s size=5x3\n  walls mat_slot=wall height=3\n  \
             door side=front at=center{fill}\n"
        )
    };
    let src = walled("");
    let out = lowered(&src);
    assert_eq!(out.diagnostics, []);
    assert_eq!(
        door_at(only_structure(&out), 2, 1, 2),
        oak_door("north", "left")
    );
    let fill = "fill mat_slot=stone from=1,1,2 to=1,2,2";
    let src = walled(&format!("\n  {fill}"));
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::PhaseConflict,
            fill,
            "`fill` overwrites 2 voxels that `walls` painted in the same phase, so which block \
             the build keeps is decided by the order the two lines are written in",
            vec![
                "overwritten member declared here",
                "If the override is deliberate this is the local-override rule of \
                 spec/compilation \"Phase evaluation\" and the later line wins as written. If it \
                 is not, move one of the two so their footprints do not meet.",
                "Members in different phases may overlap freely — a `door` cut through a `walls` \
                 is the evaluation model working — so this is only reported for two members the \
                 same phase evaluates.",
            ],
        )],
    );
    assert_eq!(
        door_at(only_structure(&out), 2, 1, 2),
        oak_door("north", "right")
    );
}

#[test]
fn a_fill_whose_line_names_no_box_or_no_fluid_it_can_pour_is_refused() {
    const CORNERS: &str = "write `from=` and `to=` as two opposite corners of the box, each a \
                           coordinate triple `x,y,z`, as in `from=1,1,1 to=3,2,3`";
    for (line, primary, note) in [
        (
            "fill fluid=water to=1,1,1",
            "fill has no `from=`, so nothing says which cells its box holds",
            CORNERS,
        ),
        (
            "fill mat_slot=stone from=1,1,1 to=1",
            "fill `to=` must be a coordinate triple, not integer `1`",
            CORNERS,
        ),
        (
            "fill fluid=lava from=1,1,1 to=1,1,1",
            "unknown fill `fluid=lava`",
            "`fluid=` takes `water`, the one fluid a fill pours",
        ),
        (
            "fill fluid=\"water\" from=1,1,1 to=1,1,1",
            "fill `fluid=` must be an identifier, not string `\"water\"`",
            "`fluid=` takes `water`, the one fluid a fill pours",
        ),
        (
            "fill fluid=water kind=flowing from=1,1,1 to=1,1,1",
            "unknown fill `kind=flowing`",
            "`kind=` takes `source`, a still water source in each cell the fill reaches; flowing \
             water is not built yet",
        ),
        (
            "fill fluid=water mat_slot=stone from=1,1,1 to=1,1,1",
            "fill writes both `mat_slot=` and `fluid=`, so nothing says whether it paints a \
             material or pours water",
            "keep `mat_slot=` to paint the box with a material, or `fluid=` to pour water into it",
        ),
        (
            "fill fluid=water from=1,1,1 to=5,1,1",
            "fill `to=5,1,1` lies outside the struct's `size=5x4` footprint",
            "a corner's `x` runs from 0 to 4 and its `z` from 0 to 3",
        ),
        (
            "fill fluid=water from=1,1,4 to=1,1,1",
            "fill `from=1,1,4` lies outside the struct's `size=5x4` footprint",
            "a corner's `x` runs from 0 to 4 and its `z` from 0 to 3",
        ),
        (
            "fill fluid=water mat_slot=2 from=1,1,1 to=1,1,1",
            "fill writes both `mat_slot=` and `fluid=`, so nothing says whether it paints a \
             material or pours water",
            "keep `mat_slot=` to paint the box with a material, or `fluid=` to pour water into it",
        ),
    ] {
        let src = bin(line);
        let out = lowered(&src);
        let expected: Vec<Finding<'_>> =
            vec![(DiagnosticCode::InvalidMember, line, primary, vec![note])];
        assert_eq!(findings(&src, &out), expected, "{line}");
        assert_eq!(
            layer(only_structure(&out), 1)[2],
            ".#...#.",
            "{line} builds nothing"
        );
    }
}

#[test]
fn a_fill_above_the_struct_is_refused_and_one_a_roof_may_reach_is_deferred() {
    let src = bin("fill fluid=water from=1,1,1 to=1,9,1");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::InvalidMember,
            "fill fluid=water from=1,1,1 to=1,9,1",
            "fill `to=1,9,1` does not fit in the struct (dims.y=4)",
            vec![
                "a corner's `y` runs from 0 to 3, the struct's top row",
                "lower the fill's box, or raise the `walls` to reach its rows",
            ],
        )],
    );

    let src = format!(
        "{BIN_THEME}struct s size=5x4\n  walls mat_slot=wall height=2\n  roof mat_slot=wall\n  \
         level y=3\n    fill fluid=water from=1,0,1 to=1,0,1\n"
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::InvalidMember,
                "roof mat_slot=wall",
                "missing `kind=` (expected one of gable, shed, hip, flat)",
                vec![],
            ),
            (
                DiagnosticCode::DeferredMember,
                "fill fluid=water from=1,0,1 to=1,0,1",
                "fill `to=1,0,1` under `level y=3` (row 3) does not fit in the struct (dims.y=3)",
                vec![
                    "a `roof` whose `kind=` or `slope_to=` does not read may rise to these rows; the \
                 box is filled once that roof draws",
                ],
            ),
        ],
    );
}

#[test]
fn a_fill_above_the_struct_under_a_raised_level_names_the_rows_its_level_leaves() {
    let within = "fill fluid=water from=1,0,1 to=1,9,1";
    let src = bin(&format!("level y=1\n    {within}"));
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::InvalidMember,
            within,
            "fill `to=1,9,1` under `level y=1` (row 10) does not fit in the struct (dims.y=4)",
            vec![
                "a corner's `y` runs from 0 to 2 under `level y=1`, the struct's top row being 3",
                "lower the fill's box, or raise the `walls` to reach its rows",
            ],
        )],
    );

    let above = "fill fluid=water from=1,0,1 to=1,0,1";
    let src = bin(&format!("level y=5\n    {above}"));
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::InvalidMember,
            above,
            "fill `to=1,0,1` under `level y=5` (row 5) does not fit in the struct (dims.y=4)",
            vec![
                "the struct's top row is 3, below this fill's `level y=5`",
                "lower the fill's `level`, or raise the `walls` to reach its rows",
            ],
        )],
    );
}

#[test]
fn an_unreadable_kind_pours_a_source_or_says_the_fill_is_refused_anyway() {
    let src = bin("fill fluid=water kind=2 from=1,1,1 to=1,1,1");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::IgnoredArgument,
            "2",
            "`kind=` must be `source`, not integer `2`; the value was ignored",
            vec!["the fill pours source water, as `kind=source` does"],
        )],
    );
    assert_eq!(layer(only_structure(&out), 1)[2], ".#~..#.");

    let src = bin("fill fluid=lava kind=2 from=1,1,1 to=1,1,1");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [
            (
                DiagnosticCode::InvalidMember,
                "fill fluid=lava kind=2 from=1,1,1 to=1,1,1",
                "unknown fill `fluid=lava`",
                vec!["`fluid=` takes `water`, the one fluid a fill pours"],
            ),
            (
                DiagnosticCode::IgnoredArgument,
                "2",
                "`kind=` must be `source`, not integer `2`; the value was ignored",
                vec!["this fill is not built either way — see the finding on the same line"],
            ),
        ],
    );
}

#[test]
fn an_unreadable_waterlogged_is_ignored_and_leaves_the_block_to_the_water() {
    let src = bin("chest at=inside.back offset=2 y=1 waterlogged=2\n  \
         fill fluid=water from=0,1,0 to=4,1,3");
    let out = lowered_with(&src, &Wet);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::IgnoredArgument,
            "2",
            "`waterlogged=` must be `auto`, `true` or `false`, not integer `2`; the value was \
             ignored",
            vec![
                "the chest holds water where a water `fill` overlaps it, as with `waterlogged=auto`"
            ],
        )],
    );
    assert_eq!(layer(only_structure(&out), 1)[2], ".#~C~#.");

    let src = bin("stair kind=stairs side=front y=1 mat_slot=step waterlogged=2");
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::IgnoredArgument,
            "2",
            "`waterlogged=` must be `auto`, `true` or `false`, not integer `2`; the value was \
             ignored",
            vec!["the stair is built with the default `waterlogged=auto`"],
        )],
    );
}

#[test]
fn an_unknown_waterlogged_refuses_the_stair_or_chest() {
    const TAKES: &str = "`waterlogged=` takes `auto`, which lets a water `fill` set it, or `true` \
                         or `false`, which keep it";
    for (line, primary) in [
        (
            "stair kind=stairs side=front y=1 mat_slot=step waterlogged=yes",
            "unknown stair `waterlogged=yes`",
        ),
        (
            "chest at=inside.back offset=2 y=1 waterlogged=yes",
            "unknown chest `waterlogged=yes`",
        ),
    ] {
        let src = bin(line);
        let out = lowered(&src);
        let expected: Vec<Finding<'_>> =
            vec![(DiagnosticCode::InvalidMember, line, primary, vec![TAKES])];
        assert_eq!(findings(&src, &out), expected);
    }
}
