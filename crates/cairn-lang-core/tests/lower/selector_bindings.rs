//! A theme selector row's `trim=` lays a `walls` member's corner posts and its `frame=` rings each
//! opening a `door` or `window` cuts (`spec/materials-themes` "Selector bindings").

use cairn_lang_core::block_array::{
    BlockArray, BlockArrayIr, BlockIdSet, BlockState, TargetRegistry,
};
use cairn_lang_core::check::DiagnosticCode;

use crate::common::{
    findings, lowered, lowered_with, lowered_with_resolver_diagnostics, only_structure, state_at,
};

/// The theme every fixture binds, with `rows` under its slots.
fn themed(rows: &str, body: &str) -> String {
    format!(
        "theme t:\n  \
         slot floor -> @oak_planks\n  \
         slot wall  -> @cobblestone\n  \
         slot glass -> @glass_pane\n\
         {rows}\n\
         {body}"
    )
}

/// One character per cell: `#` cobblestone, `L` spruce log, `F` spruce wood, `B` birch wood,
/// `g` glass pane, `o` oak planks, `D` a door half, `.` air.
fn glyph(array: &BlockArray, x: u32, y: u32, z: u32) -> char {
    match state_at(array, x, y, z).id.as_str() {
        "minecraft:cobblestone" => '#',
        "minecraft:spruce_log" => 'L',
        "minecraft:spruce_wood" => 'F',
        "minecraft:birch_wood" => 'B',
        "minecraft:glass_pane" => 'g',
        "minecraft:oak_planks" => 'o',
        "minecraft:oak_door" => 'D',
        "minecraft:air" => '.',
        other => panic!("unexpected block {other} at ({x}, {y}, {z})"),
    }
}

/// Row `y` of the `front` wall (`+z`), from low `x`.
fn front_row(array: &BlockArray, y: u32) -> String {
    (0..array.dims.x)
        .map(|x| glyph(array, x, y, array.dims.z - 1))
        .collect()
}

/// The `front` wall, top row first.
fn front_wall(array: &BlockArray) -> Vec<String> {
    (0..array.dims.y)
        .rev()
        .map(|y| front_row(array, y))
        .collect()
}

/// Layer `y` of `array`, one string per `z` from low `z`, each from low `x`.
fn layer(array: &BlockArray, y: u32) -> Vec<String> {
    (0..array.dims.z)
        .map(|z| (0..array.dims.x).map(|x| glyph(array, x, y, z)).collect())
        .collect()
}

/// The ids of the blocks the member at `span_text` of `source` built.
fn built_by<'a>(source: &str, out: &'a BlockArrayIr, span_text: &str) -> Vec<&'a str> {
    let start = source
        .find(span_text)
        .unwrap_or_else(|| panic!("`{span_text}` is not in the source"));
    out.members
        .values()
        .flatten()
        .find(|member| member.span.start == start)
        .unwrap_or_else(|| panic!("no member at `{span_text}` owns a cell"))
        .resolved_state
        .iter()
        .map(|block| block.state.id.as_str())
        .collect()
}

#[test]
fn trim_lays_a_post_at_each_corner_of_every_row_the_walls_paint() {
    let src = themed(
        "  walls[class=outer] -> trim=@spruce_log\n",
        "struct s size=5x4\n  walls class=outer mat_slot=wall height=2\n",
    );
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), []);
    let ba = only_structure(&out);
    for y in [1, 2] {
        assert_eq!(
            layer(ba, y),
            ["L###L", "#...#", "#...#", "L###L"],
            "row {y}"
        );
    }
    assert_eq!(
        built_by(&src, &out, "walls class=outer"),
        ["minecraft:spruce_log", "minecraft:cobblestone"],
        "the posts are the walls member's own cells",
    );
}

#[test]
fn trim_reaches_only_the_walls_its_row_selects() {
    let src = themed(
        "  walls[class=ground] -> trim=@spruce_log\n",
        "struct s size=3x3\n  \
         level y=0\n    walls class=ground mat_slot=wall height=2\n  \
         level y=2\n    walls class=upper mat_slot=wall height=2\n",
    );
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), []);
    let ba = only_structure(&out);
    assert_eq!(front_wall(ba), ["###", "###", "L#L", "L#L", "..."]);
}

#[test]
fn walls_whose_every_cell_is_a_corner_are_all_trim() {
    let src = themed(
        "  walls[class=outer] -> trim=@spruce_log\n",
        "struct s size=2x2\n  walls class=outer mat_slot=wall height=1\n",
    );
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), []);
    let ba = only_structure(&out);
    assert_eq!(layer(ba, 1), ["LL", "LL"]);
    assert!(
        ba.palette
            .entries
            .iter()
            .all(|state| state.id != "minecraft:cobblestone"),
        "the walls' own block is painted nowhere, so it claims no palette entry: {:?}",
        ba.palette.entries,
    );
}

#[test]
fn a_frame_rings_the_window_on_the_wall_cells_round_it() {
    let src = themed(
        "  window[class=small] -> frame=@spruce_wood\n",
        "struct s size=7x5\n  walls mat_slot=wall height=4\n  \
         window class=small side=front offset=2 y=2 size=2x1 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), []);
    let ba = only_structure(&out);
    assert_eq!(
        front_wall(ba),
        ["#######", "#FFFF##", "#FggF##", "#FFFF##", "......."],
    );
    assert_eq!(
        built_by(&src, &out, "window class=small"),
        ["minecraft:spruce_wood", "minecraft:glass_pane"],
        "the frame is the window member's own cells",
    );
}

#[test]
fn a_frame_on_an_opening_cut_to_air_rings_it_all_the_same() {
    let src = themed(
        "  window[class=small] -> frame=@spruce_wood\n",
        "struct s size=5x5\n  walls mat_slot=wall height=3\n  \
         window class=small side=front offset=2 y=2 size=1x1\n",
    );
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), []);
    assert_eq!(
        front_wall(only_structure(&out)),
        ["#FFF#", "#F.F#", "#FFF#", "....."],
    );
}

#[test]
fn a_frame_stops_at_the_ends_of_its_wall_and_of_its_course() {
    // The window fills the course's rows and starts at the wall's first column.
    let src = themed(
        "  window[class=tall] -> frame=@spruce_wood\n",
        "struct s size=4x4\n  floor mat_slot=floor\n  walls mat_slot=wall height=3\n  \
         window class=tall side=front offset=0 y=1 size=1x3 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), []);
    assert_eq!(
        front_wall(only_structure(&out)),
        ["gF##", "gF##", "gF##", "oooo"],
    );
}

#[test]
fn a_frame_round_a_door_leaves_its_hinge_to_the_wall_beside_it() {
    let framed = themed(
        "  door[class=main] -> frame=@spruce_wood\n",
        "struct s size=7x5\n  walls mat_slot=wall height=4\n  \
         door class=main side=front at=center\n",
    );
    let out = lowered(&framed);
    assert_eq!(findings(&framed, &out), []);
    let ba = only_structure(&out);
    assert_eq!(
        front_wall(ba),
        ["#######", "##FFF##", "##FDF##", "##FDF##", "......."],
    );
    let bare = framed.replace("  door[class=main] -> frame=@spruce_wood\n", "");
    let unframed = lowered(&bare);
    let z = ba.dims.z - 1;
    assert_eq!(
        [1, 2].map(|y| state_at(ba, 3, y, z).clone()),
        [1, 2].map(|y| state_at(only_structure(&unframed), 3, y, z).clone()),
        "the hinge is derived from the wall as the openings left it",
    );
}

#[test]
fn a_frame_never_turns_a_corner_or_covers_a_corner_post() {
    let src = themed(
        "  walls[class=outer] -> trim=@spruce_log\n  window[class=small] -> frame=@spruce_wood\n",
        "struct s size=5x5\n  walls class=outer mat_slot=wall height=3\n  \
         window class=small side=front offset=1 y=2 size=1x1 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), []);
    let ba = only_structure(&out);
    assert_eq!(front_wall(ba), ["LFF#L", "LgF#L", "LFF#L", "....."]);
    assert_eq!(
        (1..=3).map(|y| glyph(ba, 0, y, 3)).collect::<String>(),
        "###",
        "the left wall beside the corner keeps its own block",
    );
}

#[test]
fn a_frame_leaves_a_cell_another_member_took_from_the_walls() {
    let src = themed(
        "  window[class=small] -> frame=@spruce_wood\n",
        "struct s size=5x5\n  walls mat_slot=wall height=3\n  \
         fill mat_slot=floor from=1,3,4 to=1,3,4\n  \
         window class=small side=front offset=2 y=2 size=1x1 mat_slot=glass\n",
    );
    let out = lowered(&src);
    let found: Vec<(DiagnosticCode, &str)> = findings(&src, &out)
        .into_iter()
        .map(|(code, at, ..)| (code, at))
        .collect();
    assert_eq!(
        found,
        [(
            DiagnosticCode::PhaseConflict,
            "fill mat_slot=floor from=1,3,4 to=1,3,4"
        )],
        "the fill takes the cell from the walls in their own phase",
    );
    assert_eq!(
        front_wall(only_structure(&out)),
        ["#oFF#", "#FgF#", "#FFF#", "....."],
    );
}

#[test]
fn every_rectangle_a_window_cuts_is_framed() {
    let series = themed(
        "  window[class=small] -> frame=@spruce_wood\n",
        "struct s size=9x5\n  walls mat_slot=wall height=3\n  \
         window class=small side=front offset=1 y=2 size=1x1 repeat=2 step=4 mat_slot=glass\n",
    );
    let out = lowered(&series);
    assert_eq!(findings(&series, &out), []);
    assert_eq!(front_row(only_structure(&out), 2), "#gF#FgF##");

    let mirrored = themed(
        "  window[class=small] -> frame=@spruce_wood\n",
        "struct s size=7x5\n  walls mat_slot=wall height=3\n  \
         window class=small side=front offset=1 y=2 size=1x1 sym=true mat_slot=glass\n",
    );
    let out = lowered(&mirrored);
    assert_eq!(findings(&mirrored, &out), []);
    assert_eq!(front_row(only_structure(&out), 2), "#gF#Fg#");
}

#[test]
fn a_frame_never_closes_another_opening_whichever_is_written_first() {
    let framed = "window class=framed side=front offset=1 y=2 size=1x1 mat_slot=glass";
    let plain = "window class=plain side=front offset=2 y=2 size=1x1";
    let built: Vec<Vec<String>> = [[framed, plain], [plain, framed]]
        .into_iter()
        .map(|[first, second]| {
            let src = themed(
                "  window[class=framed] -> frame=@spruce_wood\n",
                &format!(
                    "struct s size=5x5\n  walls mat_slot=wall height=3\n  {first}\n  {second}\n"
                ),
            );
            let out = lowered(&src);
            assert_eq!(findings(&src, &out), []);
            front_wall(only_structure(&out))
        })
        .collect();
    assert_eq!(built[0], ["#FF##", "#g.##", "#FF##", "....."]);
    assert_eq!(built[1], built[0]);
}

#[test]
fn two_frames_of_different_blocks_that_meet_are_a_phase_conflict() {
    let src = themed(
        "  window[class=a] -> frame=@spruce_wood\n  window[class=b] -> frame=@birch_wood\n",
        "struct s size=7x5\n  walls mat_slot=wall height=3\n  \
         window class=a side=front offset=1 y=2 size=1x1 mat_slot=glass\n  \
         window class=b side=front offset=3 y=2 size=1x1 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::PhaseConflict,
            "window class=b side=front offset=3 y=2 size=1x1 mat_slot=glass",
            "the `frame=` of this `window` overwrites 3 voxels of the `frame=` of a `window`, so \
             which block the build keeps is decided by the order the two lines are written in",
            vec![
                "overwritten frame's member declared here",
                "if the later frame is not meant to win, bind both `frame=` to one block, or move \
                 one opening so the frames do not meet",
            ],
        )],
    );
    assert_eq!(
        &src[out.diagnostics[0].notes[0].span.clone().expect("anchored")],
        "window class=a side=front offset=1 y=2 size=1x1 mat_slot=glass",
    );
    assert_eq!(front_row(only_structure(&out), 2), "#gBgB##");

    let same = src.replace("@birch_wood", "@spruce_wood");
    let out = lowered(&same);
    assert_eq!(
        findings(&same, &out),
        [],
        "a shared cell of one block is no contest"
    );
}

#[test]
fn a_frame_bound_to_an_abstract_token_leaves_the_opening_cut_and_the_wall_round_it() {
    let src = themed(
        "  window[class=small] -> frame=@wood.frame\n",
        "struct s size=5x5\n  walls mat_slot=wall height=3\n  \
         window class=small side=front offset=2 y=2 size=1x1 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::AbstractTokenDeferred,
            "@wood.frame",
            "abstract token `@wood.frame` cannot be lowered without the registry pack; no frame \
             is laid",
            vec![
                "use a canonical block token (e.g. `frame=@spruce_wood`) until the registry pack \
                 ships"
            ],
        )],
    );
    assert_eq!(front_row(only_structure(&out), 2), "##g##");
}

#[test]
fn trim_bound_to_an_abstract_token_leaves_the_corners_to_the_walls() {
    let src = themed(
        "  walls[class=outer] -> trim=@wood.post\n",
        "struct s size=3x3\n  walls class=outer mat_slot=wall height=1\n",
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::AbstractTokenDeferred,
            "@wood.post",
            "abstract token `@wood.post` cannot be lowered without the registry pack; no corner \
             posts are laid",
            vec![
                "use a canonical block token (e.g. `trim=@spruce_log`) until the registry pack \
                 ships"
            ],
        )],
    );
    assert_eq!(layer(only_structure(&out), 1), ["###", "#.#", "###"]);
}

#[test]
fn a_binding_is_judged_even_when_its_member_is_refused() {
    let frame_deferred = (
        DiagnosticCode::AbstractTokenDeferred,
        "@wood.frame",
        "abstract token `@wood.frame` cannot be lowered without the registry pack; no frame is \
         laid",
        vec![
            "use a canonical block token (e.g. `frame=@spruce_wood`) until the registry pack ships",
        ],
    );
    let src = themed(
        "  window[class=small] -> frame=@wood.frame\n",
        "struct s size=5x5\n  walls mat_slot=wall height=3\n  \
         window class=small side=front offset=9 y=2 size=1x1 mat_slot=glass\n",
    );
    assert_eq!(
        findings(&src, &lowered(&src)),
        [
            frame_deferred.clone(),
            (
                DiagnosticCode::InvalidMember,
                "window class=small side=front offset=9 y=2 size=1x1 mat_slot=glass",
                "window extends beyond the `front` wall (offset=9 size=1x1 repeat=1 step=0, wall \
                 length=5)",
                vec![
                    "the window ends at `offset + size.w`, which must be at most 5; lower \
                     `offset=` or narrow `size=`"
                ],
            ),
        ],
    );

    let src = themed(
        "  door[class=main] -> frame=@wood.frame\n",
        "struct s size=5x5\n  walls mat_slot=wall height=3\n  door class=main side=front at=99\n",
    );
    assert_eq!(
        findings(&src, &lowered(&src)),
        [
            frame_deferred,
            (
                DiagnosticCode::InvalidMember,
                "door class=main side=front at=99",
                "door has `at=99`, which is not one of center, left, right",
                vec![
                    "`at=` takes `center`, `left` or `right`; `offset=` takes a column between the \
                     wall's corners, from 1 to 3"
                ],
            ),
        ],
    );

    let src = themed(
        "  walls[class=outer] -> trim=@wood.post\n",
        "struct s size=3x3\n  walls class=outer mat_slot=wall height=zz\n",
    );
    assert_eq!(
        findings(&src, &lowered(&src)),
        [
            (
                DiagnosticCode::AbstractTokenDeferred,
                "@wood.post",
                "abstract token `@wood.post` cannot be lowered without the registry pack; no \
                 corner posts are laid",
                vec![
                    "use a canonical block token (e.g. `trim=@spruce_log`) until the registry pack \
                     ships"
                ],
            ),
            (
                DiagnosticCode::InvalidMember,
                "walls class=outer mat_slot=wall height=zz",
                "walls `height=` must be a positive integer that fits in u32, not identifier `zz`",
                vec![
                    "write `height=N`, where `N` is a positive integer: the walls fill the `N` rows \
                     above the base row"
                ],
            ),
        ],
    );
}

#[test]
fn a_member_that_builds_nothing_lays_no_part() {
    let window = "window class=small side=front offset=2 y=2 size=1x1";
    for (slots, mat_slot) in [("", "nope"), ("  slot pane -> @glass.clear\n", "pane")] {
        let src = themed(
            &format!("{slots}  window[class=small] -> frame=@spruce_wood\n"),
            &format!(
                "struct s size=5x5\n  walls mat_slot=wall height=3\n  {window} mat_slot={mat_slot}\n"
            ),
        );
        let out = lowered_with_resolver_diagnostics(&src);
        assert_eq!(
            front_wall(only_structure(&out)),
            ["#####", "#####", "#####", "....."],
            "a window whose `mat_slot={mat_slot}` builds nothing is not cut or framed",
        );
    }

    let src = themed(
        "  walls[class=outer] -> trim=@spruce_log\n",
        "struct s size=3x3\n  walls class=outer mat_slot=nope height=1\n",
    );
    let out = lowered_with_resolver_diagnostics(&src);
    assert_eq!(
        layer(only_structure(&out), 0),
        ["...", "...", "..."],
        "walls that build nothing lay no posts",
    );

    let src = themed(
        "  window[class=small] -> frame=@spruce_wood\n",
        "struct s size=9x5\n  walls mat_slot=wall height=3\n  \
         window class=small side=front offset=1 y=2 size=1x1 repeat=2 step=4 sym=maybe \
         mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out)
            .into_iter()
            .map(|(code, at, ..)| (code, at))
            .collect::<Vec<_>>(),
        [
            (
                DiagnosticCode::DeferredMember,
                "window class=small side=front offset=1 y=2 size=1x1 repeat=2 step=4 sym=maybe \
                 mat_slot=glass",
            ),
            (DiagnosticCode::IgnoredArgument, "maybe"),
        ],
    );
    assert_eq!(
        front_row(only_structure(&out), 2),
        "#########",
        "a window refused for its unreadable `sym=` is not framed",
    );
}

#[test]
fn a_later_row_that_also_selects_a_member_rebinds_its_frame() {
    let src = themed(
        "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small,side=front] -> frame=@birch_wood\n",
        "struct s size=7x5\n  walls mat_slot=wall height=3\n  \
         window class=small side=front offset=2 y=2 size=1x1 mat_slot=glass\n  \
         window class=small side=back offset=2 y=2 size=1x1 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), []);
    let ba = only_structure(&out);
    assert_eq!(
        front_row(ba, 2),
        "#BgB###",
        "both rows select it, and the later wins"
    );
    let back: String = (0..ba.dims.x).map(|x| glyph(ba, x, 2, 0)).collect();
    assert_eq!(back, "###FgF#", "only the wider row selects it");
}

#[test]
fn frames_are_laid_after_the_hinge_is_derived() {
    // The window cut beside the door leaves one wall cell on its left and two on its right, which
    // a frame laid first would turn to the door's own.
    let src = themed(
        "  door[class=main] -> frame=@spruce_wood\n",
        "struct s size=7x5\n  walls mat_slot=wall height=4\n  \
         window side=front offset=2 y=1 size=1x2\n  door class=main side=front at=center\n",
    );
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), []);
    let ba = only_structure(&out);
    assert_eq!(
        front_wall(ba),
        ["#######", "##FFF##", "##.DF##", "##.DF##", "......."],
    );
    let z = ba.dims.z - 1;
    assert_eq!(
        [1, 2].map(|y| state_at(ba, 3, y, z).properties["hinge"].clone()),
        ["right", "right"],
    );
}

#[test]
fn a_frame_on_a_side_wall_runs_along_its_length() {
    for (side, x) in [("left", 0), ("right", 4)] {
        let src = themed(
            "  window[class=small] -> frame=@spruce_wood\n",
            &format!(
                "struct s size=5x9\n  walls mat_slot=wall height=3\n  \
                 window class=small side={side} offset=4 y=2 size=1x1 mat_slot=glass\n"
            ),
        );
        let out = lowered(&src);
        assert_eq!(findings(&src, &out), []);
        let ba = only_structure(&out);
        let wall: Vec<String> = (1..=3)
            .rev()
            .map(|y| (0..ba.dims.z).map(|z| glyph(ba, x, y, z)).collect())
            .collect();
        assert_eq!(wall, ["###FFF###", "###FgF###", "###FFF###"], "side={side}");
    }
}

#[test]
fn a_frame_on_an_upper_level_rings_its_opening_there() {
    let src = themed(
        "  window[class=small] -> frame=@spruce_wood\n",
        "struct s size=5x5\n  level y=0\n    walls mat_slot=wall height=3\n  \
         level y=3\n    walls mat_slot=wall height=3\n    \
         window class=small side=front offset=2 y=2 size=1x1 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), []);
    assert_eq!(
        front_wall(only_structure(&out)),
        [
            "#FFF#", "#FgF#", "#FFF#", "#####", "#####", "#####", "....."
        ],
    );
}

#[test]
fn a_ring_that_falls_on_no_wall_cell_lays_nothing() {
    // The course is one row high and the opening's columns sit between the wall's two ends.
    let src = themed(
        "  window[class=small] -> frame=@spruce_wood\n",
        "struct s size=3x3\n  walls mat_slot=wall height=1\n  \
         window class=small side=front offset=1 y=1 size=1x1\n",
    );
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), []);
    assert_eq!(front_wall(only_structure(&out)), ["#.#", "..."]);
}

#[test]
fn a_corner_post_is_judged_with_its_phase_and_built_over_by_a_later_one() {
    let rows = "  walls[class=outer] -> trim=@spruce_log\n";
    let walls = "struct s size=3x3\n  walls class=outer mat_slot=wall height=2\n";
    let src = themed(
        rows,
        &format!("{walls}  fill mat_slot=floor from=0,1,2 to=0,1,2\n"),
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out)
            .into_iter()
            .map(|(code, at, primary, _)| (code, at, primary))
            .collect::<Vec<_>>(),
        [(
            DiagnosticCode::PhaseConflict,
            "fill mat_slot=floor from=0,1,2 to=0,1,2",
            "`fill` overwrites 1 voxel that `walls` painted in the same phase, so which block the \
             build keeps is decided by the order the two lines are written in",
        )],
        "a `fill` paints in `massing`, the posts' phase",
    );
    assert_eq!(front_wall(only_structure(&out)), ["L#L", "o#L", "..."]);

    let src = themed(
        rows,
        &format!("{walls}  raw_block mat=minecraft:oak_planks at=0,1,2\n"),
    );
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [],
        "a raw block builds over the post in its own phase"
    );
    assert_eq!(front_wall(only_structure(&out)), ["L#L", "o#L", "..."]);
}

#[test]
fn a_binding_that_is_no_material_token_is_the_resolvers_to_report() {
    let src = themed(
        "  window[class=small] -> frame=spruce_wood\n",
        "struct s size=5x5\n  walls mat_slot=wall height=3\n  \
         window class=small side=front offset=2 y=2 size=1x1 mat_slot=glass\n",
    );
    let out = lowered_with_resolver_diagnostics(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::UnknownSlotTarget,
            "spruce_wood",
            "`frame=` on `window[class=small]` in theme `t` does not bind to a canonical or \
             abstract material token",
            vec!["expected a `@canonical_block` or `@abstract.material` value"],
        )],
    );
    assert_eq!(front_row(only_structure(&out), 2), "##g##");
}

#[test]
fn a_key_the_keyword_does_not_bind_builds_nothing() {
    let body = "struct s size=5x5\n  walls mat_slot=wall height=3\n  \
                window class=small side=front offset=2 y=2 size=1x1 mat_slot=glass\n";
    let with = lowered(&themed("  window[class=small] -> trim=@spruce_log\n", body));
    let without = lowered(&themed("", body));
    assert_eq!(with.diagnostics, []);
    assert_eq!(only_structure(&with), only_structure(&without));
}

/// A pinned `java 2.0` that declares the fixtures' blocks and no other.
struct Pinned {
    ids: Vec<String>,
}

impl Pinned {
    fn new() -> Self {
        let mut ids: Vec<String> = [
            "cobblestone",
            "glass_pane",
            "oak_door",
            "oak_planks",
            "spruce_log",
            "spruce_wood",
        ]
        .iter()
        .map(|id| format!("minecraft:{id}"))
        .collect();
        ids.sort();
        Self { ids }
    }
}

impl TargetRegistry for Pinned {
    fn lookup(&self, _token: &str) -> Option<BlockState> {
        None
    }

    fn known_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        Some(BlockIdSet::new("java 2.0", &self.ids))
    }
}

#[test]
fn a_binding_the_pinned_target_does_not_declare_is_refused_at_the_binding() {
    let src = themed(
        "  door[class=main] -> frame=@spruce_wod\n",
        "struct s size=5x5\n  walls mat_slot=wall height=3\n  door class=main side=front at=center\n",
    );
    let out = lowered_with(&src, &Pinned::new());
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::UnknownId,
            "@spruce_wod",
            "`minecraft:spruce_wod` is not a block in `java 2.0`",
            vec!["`java 2.0` spells the nearest block `minecraft:spruce_wood`"],
        )],
    );
    assert_eq!(
        front_row(only_structure(&out), 2),
        "##D##",
        "the door stands without a frame"
    );
}
