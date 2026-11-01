//! A `window` is cut only where every one of its rows is a row some `walls` member paints, and a
//! walkway port draws the same line as the openings pass.

use cairn_lang_core::block_array::BlockArray;
use cairn_lang_core::check::DiagnosticCode;

use crate::common::{lowered, only_structure, refusals, state_at, walkway_was_laid};

const THEME: &str = "theme t:\n  \
                     slot floor -> @oak_planks\n  \
                     slot wall  -> @cobblestone\n  \
                     slot glass -> @glass_pane\n  \
                     slot gravel -> @gravel\n\n";

/// Whether any voxel anywhere carries `id`.
fn contains_id(ba: &BlockArray, id: &str) -> bool {
    ba.voxels
        .iter()
        .any(|v| ba.palette.entries[v.0 as usize].id == id)
}

// ------------------------------------------------- the openings pass

#[test]
fn a_window_on_the_floor_plane_is_refused_instead_of_carving_the_slab() {
    // `walls height=3` paints rows 1..=3, so a rectangle from y=0 is half wall and half floor.
    let src = format!(
        "{THEME}struct t size=7x5\n  \
         floor mat_slot=floor\n  \
         walls mat_slot=wall height=3\n  \
         window class=arrow_slit side=front y=0 offset=3 size=1x2 shape=slit\n",
    );
    let out = lowered(&src);
    assert_eq!(
        refusals(&out),
        [
            "window rows y=0..=1 are not all inside one wall course (size=1x2; the walls occupy y=1..=3)"
        ],
    );
    let ba = only_structure(&out);
    for x in 0..ba.dims.x {
        assert_eq!(
            state_at(ba, x, 0, 4).id,
            "minecraft:oak_planks",
            "floor cell x={x} was carved",
        );
    }
}

#[test]
fn a_window_in_the_air_between_two_courses_is_refused_instead_of_hanging_glass() {
    // Walls at 1..=2 and 7..=8; the rectangle wants 3..=5, which is all sky.
    let src = format!(
        "{THEME}struct t size=7x5\n  \
         walls mat_slot=wall height=2\n  \
         level id=up y=6\n    \
         walls mat_slot=wall height=2\n  \
         window side=front y=3 offset=3 size=1x3 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(
        refusals(&out),
        [
            "window rows y=3..=5 are not all inside one wall course (size=1x3; the walls occupy y=1..=2, y=7..=8)"
        ],
    );
    assert!(
        !contains_id(only_structure(&out), "minecraft:glass_pane"),
        "no pane should exist anywhere",
    );
}

#[test]
fn a_window_that_fills_the_wall_end_to_end_is_carved() {
    // Rows 1..=3 of a wall that occupies 1..=3: the acceptance edge on both sides.
    let src = format!(
        "{THEME}struct t size=7x5\n  \
         walls mat_slot=wall height=3\n  \
         window side=front y=1 offset=3 size=1x3 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    let ba = only_structure(&out);
    for y in 1..=3 {
        assert_eq!(state_at(ba, 3, y, 4).id, "minecraft:glass_pane");
    }
}

#[test]
fn a_window_flush_with_the_top_course_is_carved() {
    let src = format!(
        "{THEME}struct t size=7x5\n  \
         walls mat_slot=wall height=3\n  \
         window side=front y=3 offset=3 size=1x1 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(
        state_at(only_structure(&out), 3, 3, 4).id,
        "minecraft:glass_pane",
    );
}

#[test]
fn a_window_one_row_past_the_top_course_is_refused() {
    let src = format!(
        "{THEME}struct t size=7x5\n  \
         walls mat_slot=wall height=3\n  \
         window side=front y=3 offset=3 size=1x2 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(
        refusals(&out),
        [
            "window rows y=3..=4 are not all inside one wall course (size=1x2; the walls occupy y=1..=3)"
        ],
    );
    assert!(!contains_id(only_structure(&out), "minecraft:glass_pane"));
}

#[test]
fn a_window_across_the_seam_between_two_stacked_courses_is_carved() {
    // The courses abut at 1..=5 and 6..=9, so a rectangle at 4..=6 is in masonry throughout.
    let src = format!(
        "{THEME}struct t size=7x5\n  \
         walls mat_slot=wall height=5\n  \
         level id=up y=5\n    \
         walls mat_slot=wall height=4\n  \
         window side=front y=4 offset=3 size=1x3 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    let ba = only_structure(&out);
    for y in 4..=6 {
        assert_eq!(
            state_at(ba, 3, y, 4).id,
            "minecraft:glass_pane",
            "row y={y}"
        );
    }
}

#[test]
fn a_window_on_a_struct_with_no_walls_says_there_is_no_wall() {
    let src = format!(
        "{THEME}struct t size=7x5\n  \
         floor mat_slot=floor\n  \
         window side=front y=0 offset=3 size=1x1 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(
        refusals(&out),
        [
            "window at y=0 size=1x1 has no wall to cut into: this struct declares no `walls` with a positive `height=`"
        ],
    );
    assert!(!contains_id(only_structure(&out), "minecraft:glass_pane"));
}

#[test]
fn a_window_inside_a_level_is_checked_against_the_rows_it_lands_on() {
    // `level y=5` puts the rectangle at world row 5: the top course in one source, sky in another.
    let inside = format!(
        "{THEME}struct t size=7x5\n  \
         walls mat_slot=wall height=5\n  \
         level id=up y=5\n    \
         window side=front y=0 offset=3 size=1x1 mat_slot=glass\n",
    );
    let out = lowered(&inside);
    assert!(out.diagnostics.is_empty(), "{:?}", out.diagnostics);
    assert_eq!(
        state_at(only_structure(&out), 3, 5, 4).id,
        "minecraft:glass_pane",
    );

    let above = inside.replace(
        "walls mat_slot=wall height=5",
        "walls mat_slot=wall height=4",
    );
    let out = lowered(&above);
    assert_eq!(
        refusals(&out),
        [
            "window rows y=5..=5 are not all inside one wall course (size=1x1; the walls occupy y=1..=4)"
        ],
    );
}

#[test]
fn a_height_on_a_member_that_is_not_a_wall_builds_no_wall() {
    // `height=` is not a `floor` argument, and only the roles make a row a wall row.
    let src = format!(
        "{THEME}struct t size=5x5\n  \
         floor mat_slot=floor height=3\n  \
         window side=front y=1 offset=2 size=1x2 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(
        refusals(&out),
        [
            "window at y=1 size=1x2 has no wall to cut into: this struct declares no `walls` with a positive `height=`"
        ],
    );
    assert!(!contains_id(only_structure(&out), "minecraft:glass_pane"));
}

#[test]
fn a_window_in_a_course_that_paints_nothing_defers_to_its_walls() {
    // The upper walls' material is an abstract token no registry pack lifts, so only 1..=3 paints.
    let src = "theme t:\n  \
               slot wall  -> @cobblestone\n  \
               slot upper -> @wall.stone.cobble\n  \
               slot glass -> @glass_pane\n\n\
               struct t size=7x5\n  \
               walls mat_slot=wall height=3\n  \
               level id=up y=3\n    \
               walls mat_slot=upper height=3\n  \
               window side=front y=5 offset=3 size=1x1 mat_slot=glass\n";
    let out = lowered(src);
    assert_eq!(refusals(&out), Vec::<&str>::new());
    let deferred: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::DeferredMember)
        .map(|d| {
            (
                d.primary.as_str(),
                d.notes
                    .iter()
                    .map(|n| n.message.as_str())
                    .collect::<Vec<_>>(),
            )
        })
        .collect();
    assert_eq!(
        deferred,
        [(
            "window rows y=5..=5 lie in a course the walls are written to (y=1..=6) but not in one \
             they paint (y=1..=3)",
            vec![
                "a `walls` member written to hold these rows paints nothing; the opening is cut once \
                 that member's material resolves to a block"
            ],
        )],
    );
}

// --------------------------------------------- the walkway port agrees

/// A two-hut site whose walkway anchors on the window `a.top`.
fn site_with_window_port(y: u32, height: u32, wall_height: u32) -> String {
    format!(
        "{THEME}def hut size=5x5:\n  \
         walls id=w class=outer mat_slot=wall height={wall_height}\n  \
         window id=top side=front offset=1 y={y} size=1x{height} mat_slot=glass\n  \
         door id=e side=front at=center\n\n\
         site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=4\n  \
         connect a.top to b.e path=@gravel\n",
    )
}

#[test]
fn a_window_flush_with_the_top_course_anchors_a_walkway() {
    // `walls height=3` paints rows 1..=3, and the window occupies row 3.
    assert!(walkway_was_laid(&site_with_window_port(3, 1, 3)));
}

#[test]
fn a_window_on_the_floor_plane_does_not_anchor_a_walkway() {
    assert!(!walkway_was_laid(&site_with_window_port(0, 1, 3)));
}

#[test]
fn the_port_accepts_exactly_the_rectangles_the_openings_pass_carves() {
    // `spec/components-editing-sites` "Ports and `connect`", as an equivalence of the two limits.
    const WALL_HEIGHT: u32 = 3;
    for (y, height) in [
        (0, 1),
        (0, 4),
        (1, 1),
        (1, 3),
        (2, 2),
        (3, 1),
        (3, 2),
        (4, 1),
    ] {
        let carved = {
            let src = format!(
                "{THEME}struct t size=5x5\n  \
                 walls mat_slot=wall height={WALL_HEIGHT}\n  \
                 window side=front offset=1 y={y} size=1x{height} mat_slot=glass\n",
            );
            lowered(&src).diagnostics.is_empty()
        };
        let anchored = walkway_was_laid(&site_with_window_port(y, height, WALL_HEIGHT));
        assert_eq!(
            carved, anchored,
            "y={y} size=1x{height} on walls height={WALL_HEIGHT}: \
             the openings pass carves it = {carved}, the port anchors it = {anchored}",
        );
    }
}

#[test]
fn walls_under_a_level_are_carved_into_and_anchor_a_port() {
    let src = format!(
        "{THEME}def hut size=5x5:\n  \
         level id=up y=0\n    \
         walls id=w class=outer mat_slot=wall height=3\n  \
         window id=top side=front offset=1 y=2 size=1x1 mat_slot=glass\n  \
         door id=e side=front at=center\n\n\
         site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=4\n  \
         connect a.top to b.e path=@gravel\n",
    );
    let out = lowered(&src);
    let carved = out
        .structures
        .values()
        .any(|ba| contains_id(ba, "minecraft:glass_pane"));
    assert!(carved, "the openings pass cuts the window");
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.primary.contains("was skipped because port")),
        "the port resolves against the same masonry: {:?}",
        out.diagnostics,
    );
    assert_eq!(out.walkways.len(), 1, "the strip is laid");
}

#[test]
fn the_walkway_defer_states_the_rule_the_window_broke() {
    let src = site_with_window_port(0, 1, 3);
    let out = lowered(&src);
    let notes: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.primary.contains("was skipped because port"))
        .flat_map(|d| &d.notes)
        .collect();
    assert_eq!(
        notes.len(),
        1,
        "one note for the one refused port: {notes:?}"
    );
    assert_eq!(
        notes[0].message,
        "`a.top` is a window whose rows y=0..=0 are not all inside one wall course \
         (the walls occupy y=1..=3); the member says so on its own line too",
    );
    let at = notes[0]
        .span
        .clone()
        .expect("the note points at the window");
    assert!(
        src[at].starts_with("window id=top"),
        "the note points at the window's own line",
    );
}

#[test]
fn walls_that_paint_nothing_are_masonry_to_neither_pass() {
    // The wall's material is an abstract token and no registry pack lifts it, so it paints nothing.
    let src = "theme t:\n  \
               slot floor -> @oak_planks\n  \
               slot wall  -> @wall.stone.cobble\n  \
               slot glass -> @glass_pane\n  \
               slot gravel -> @gravel\n\n\
               def hut size=5x5:\n  \
               walls id=w class=outer mat_slot=wall height=3\n  \
               window id=top side=front offset=1 y=2 size=1x1 mat_slot=glass\n  \
               door id=e side=front at=center\n\n\
               site s:\n  \
               place id=a use=hut theme=t at=origin\n  \
               place id=b use=hut theme=t east_of=a gap=4\n  \
               connect a.top to b.e path=@gravel\n";
    let out = lowered(src);
    assert!(
        !out.structures
            .values()
            .any(|ba| contains_id(ba, "minecraft:glass_pane")),
        "the openings pass cuts nothing into a wall that paints nothing",
    );
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.primary.contains("was skipped because port")),
        "the port refuses it too: {:?}",
        out.diagnostics,
    );
    assert!(out.walkways.is_empty(), "and no strip is laid");
}
