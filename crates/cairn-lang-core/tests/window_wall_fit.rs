//! A `window` is a rectangle cut into a wall, and the two passes that act
//! on one have to agree about which rectangles are inside a wall.
//!
//! `walls height=H` under `level y=N` paints the world rows
//! `N + 1 ..= N + H` — the floor slab owns the base plane — so a window
//! sits inside masonry exactly when every row of `y ..= y + size.h - 1`
//! is one of those. Read as a single ceiling instead, the rule cannot see
//! the two ways out of the wall that are not "too tall": the rows below
//! the first course, where the rectangle carves through the floor, and
//! the air between two `level` courses, where it hangs glass in the open.
//!
//! `spec/components-editing-sites.md` §9.3.5 requires the walkway port to
//! draw the same line as the openings pass — "a window that the openings
//! pass would defer cannot anchor a walkway either" — so the last group
//! here asserts the two answers case by case rather than each in
//! isolation. Two checks that agree today drift apart the moment one of
//! them is edited alone; a matrix that compares them fails when they do.

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::DiagnosticCode;
use cairn_lang_core::{lower, parse, resolve};

const THEME: &str = "theme t:\n  \
                     slot floor -> @oak_planks\n  \
                     slot wall  -> @cobblestone\n  \
                     slot glass -> @glass_pane\n  \
                     slot gravel -> @gravel\n\n";

fn lowered(source: &str) -> BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, None)
}

fn only_structure(ir: &BlockArrayIr) -> &BlockArray {
    assert_eq!(ir.structures.len(), 1, "these sources declare one struct");
    ir.structures.values().next().expect("one structure")
}

/// Every `W_DEFERRED_MEMBER` message, in the order the pass raised them.
fn defers(ir: &BlockArrayIr) -> Vec<&str> {
    ir.diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::DeferredMember)
        .map(|d| d.primary.as_str())
        .collect()
}

fn block_id(ba: &BlockArray, x: u32, y: u32, z: u32) -> &str {
    let i = ((y * ba.dims.z + z) * ba.dims.x + x) as usize;
    ba.palette.entries[ba.voxels[i].0 as usize].id.as_str()
}

/// Whether any voxel anywhere carries `id`.
fn contains_id(ba: &BlockArray, id: &str) -> bool {
    ba.voxels
        .iter()
        .any(|v| ba.palette.entries[v.0 as usize].id == id)
}

// ------------------------------------------------- the openings pass

#[test]
fn a_window_on_the_floor_plane_is_refused_instead_of_carving_the_slab() {
    // The issue's first repro. `walls height=3` paints rows 1..=3, so a
    // rectangle starting at y=0 is one row of wall and one row of floor —
    // and a `class=arrow_slit` window carries no `mat_slot=`, so the cut
    // is to air and the slab loses a cell without a word.
    let src = format!(
        "{THEME}struct t size=7x5\n  \
         floor mat_slot=floor\n  \
         walls mat_slot=wall height=3\n  \
         window class=arrow_slit side=front y=0 offset=3 size=1x2 shape=slit\n",
    );
    let out = lowered(&src);
    assert_eq!(
        defers(&out),
        [
            "window rows y=0..=1 are not all inside one wall course (size=1x2; the walls occupy y=1..=3)"
        ],
    );
    // The front row of the slab is unbroken: the carve never happened.
    let ba = only_structure(&out);
    for x in 0..ba.dims.x {
        assert_eq!(
            block_id(ba, x, 0, 4),
            "minecraft:oak_planks",
            "floor cell x={x} was carved",
        );
    }
}

#[test]
fn a_window_in_the_air_between_two_courses_is_refused_instead_of_hanging_glass() {
    // The issue's second repro. Walls at 1..=2 and 7..=8 with open air
    // between them; the rectangle wants 3..=5, which is entirely sky. A
    // ceiling-shaped check sees only `5 < 8` and cuts.
    let src = format!(
        "{THEME}struct t size=7x5\n  \
         walls mat_slot=wall height=2\n  \
         level id=up y=6\n    \
         walls mat_slot=wall height=2\n  \
         window side=front y=3 offset=3 size=1x3 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(
        defers(&out),
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
    // The acceptance edge on both sides at once: rows 1..=3 of a wall
    // that occupies 1..=3. A check that tightened either bound by one
    // would refuse this and only this.
    let src = format!(
        "{THEME}struct t size=7x5\n  \
         walls mat_slot=wall height=3\n  \
         window side=front y=1 offset=3 size=1x3 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(defers(&out), [] as [&str; 0]);
    let ba = only_structure(&out);
    for y in 1..=3 {
        assert_eq!(block_id(ba, 3, y, 4), "minecraft:glass_pane");
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
    assert_eq!(defers(&out), [] as [&str; 0]);
    assert_eq!(
        block_id(only_structure(&out), 3, 3, 4),
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
        defers(&out),
        [
            "window rows y=3..=4 are not all inside one wall course (size=1x2; the walls occupy y=1..=3)"
        ],
    );
    assert!(!contains_id(only_structure(&out), "minecraft:glass_pane"));
}

#[test]
fn a_window_across_the_seam_between_two_stacked_courses_is_carved() {
    // `walls height=5` paints 1..=5 and `level y=5 walls height=4` paints
    // 6..=9. They abut, so the tower has one wall and a rectangle at
    // 4..=6 is cut into masonry the whole way. Refusing it — which is
    // what a per-member check rather than a merged column would do —
    // would make a legal window depend on where the author put a `level`
    // line.
    let src = format!(
        "{THEME}struct t size=7x5\n  \
         walls mat_slot=wall height=5\n  \
         level id=up y=5\n    \
         walls mat_slot=wall height=4\n  \
         window side=front y=4 offset=3 size=1x3 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(defers(&out), [] as [&str; 0]);
    let ba = only_structure(&out);
    for y in 4..=6 {
        assert_eq!(block_id(ba, 3, y, 4), "minecraft:glass_pane", "row y={y}");
    }
}

#[test]
fn a_window_on_a_struct_with_no_walls_says_there_is_no_wall() {
    // `wall_top` is 0 here, so the old bound accepted any rectangle
    // ending at row 1 — including `y=0 size=1x1`, which painted a pane on
    // the floor plane of a struct that has no wall to put a window in.
    let src = format!(
        "{THEME}struct t size=7x5\n  \
         floor mat_slot=floor\n  \
         window side=front y=0 offset=3 size=1x1 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(
        defers(&out),
        [
            "window at y=0 size=1x1 has no wall to cut into (this struct declares no `walls` that paints — one with a positive `height=` and a `mat_slot=` that resolves)"
        ],
    );
    assert!(!contains_id(only_structure(&out), "minecraft:glass_pane"));
}

#[test]
fn a_window_inside_a_level_is_checked_against_the_rows_it_lands_on() {
    // `level y=5` shifts the rectangle to world row 5. Over a wall that
    // reaches row 5 it is the top course and is cut; over one that stops
    // at row 4 it is a row of sky. The level-local `y=0` is identical in
    // both sources, so a check reading the authored number rather than
    // the world row cannot tell them apart.
    let inside = format!(
        "{THEME}struct t size=7x5\n  \
         walls mat_slot=wall height=5\n  \
         level id=up y=5\n    \
         window side=front y=0 offset=3 size=1x1 mat_slot=glass\n",
    );
    let out = lowered(&inside);
    assert_eq!(defers(&out), [] as [&str; 0]);
    assert_eq!(
        block_id(only_structure(&out), 3, 5, 4),
        "minecraft:glass_pane",
    );

    let above = inside.replace(
        "walls mat_slot=wall height=5",
        "walls mat_slot=wall height=4",
    );
    let out = lowered(&above);
    assert_eq!(
        defers(&out),
        [
            "window rows y=5..=5 are not all inside one wall course (size=1x1; the walls occupy y=1..=4)"
        ],
    );
}

#[test]
fn a_height_on_a_member_that_is_not_a_wall_builds_no_wall() {
    // `height=` is not a `floor` argument, and the openings pass reads
    // arguments by name — so a column built from every member that
    // happens to carry one would let `floor height=3` stand in for the
    // masonry that is not there and carve a window into open air. The
    // roles are what make a row a wall row.
    let src = format!(
        "{THEME}struct t size=5x5\n  \
         floor mat_slot=floor height=3\n  \
         window side=front y=1 offset=2 size=1x2 mat_slot=glass\n",
    );
    let out = lowered(&src);
    assert_eq!(
        defers(&out),
        [
            "window at y=1 size=1x2 has no wall to cut into (this struct declares no `walls` that paints — one with a positive `height=` and a `mat_slot=` that resolves)"
        ],
    );
    assert!(!contains_id(only_structure(&out), "minecraft:glass_pane"));
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

fn walkway_was_laid(source: &str) -> bool {
    let out = lowered(source);
    let dropped = out
        .diagnostics
        .iter()
        .any(|d| d.primary.contains("was skipped because port"));
    assert_eq!(
        !dropped,
        !out.walkways.is_empty(),
        "a walkway that was not dropped has to have been laid",
    );
    !dropped
}

#[test]
fn a_window_flush_with_the_top_course_anchors_a_walkway() {
    // `walls height=3` paints rows 1..=3 and the window occupies row 3 —
    // the last course `fill_walls` painted, and a cell the openings pass
    // carves. The port used to be refused here because its own bound
    // stopped one row short of the wall.
    assert!(walkway_was_laid(&site_with_window_port(3, 1, 3)));
}

#[test]
fn a_window_on_the_floor_plane_does_not_anchor_a_walkway() {
    // The mirror of the case above: `y=0` is not a wall row, so the cut
    // does not happen and the port must not pretend it did.
    assert!(!walkway_was_laid(&site_with_window_port(0, 1, 3)));
}

#[test]
fn the_port_accepts_exactly_the_rectangles_the_openings_pass_carves() {
    // §9.3.5's requirement, asserted as an equivalence rather than as two
    // independent lists — the failure mode it guards against is one of
    // the two limits being edited alone, which no test of either side by
    // itself can see.
    //
    // Every case here declares `walls` in the def body. The two tests
    // below carry the cases that used to fall outside the equivalence —
    // walls under a `level`, and walls whose material does not resolve —
    // because the port now reads the column the body was lowered with
    // rather than deriving its own from the `def`.
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
            defers(&lowered(&src)).is_empty()
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
    // The equivalence used to stop at the def body: the openings pass
    // read the flattened member list and so saw a `walls` under a
    // `level`, while port resolution walked the body only. The window was
    // cut and the walkway was dropped, so the author was told to move a
    // window that was already in masonry.
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
        defers(&out),
    );
    assert_eq!(out.walkways.len(), 1, "the strip is laid");
}

#[test]
fn the_port_contract_note_states_the_rule_the_port_applies() {
    // The note is the only place an author is told what a window port
    // needs, and it used to quote the bound that was wrong. A reader who
    // followed it — `y + size.h <= walls.height` — would move a legal
    // window down a row to satisfy a rule the compiler no longer has.
    let out = lowered(&site_with_window_port(0, 1, 3));
    let note = out
        .diagnostics
        .iter()
        .filter(|d| d.primary.contains("was skipped because port"))
        .flat_map(|d| &d.notes)
        .map(|n| n.message.as_str())
        .find(|m| m.starts_with("a `window` port requires"))
        .expect("the walkway defer lists the window-port contract");
    assert_eq!(
        note,
        "a `window` port requires `side=front|back|left|right`, plus `offset=` / `y=` / \
         `size=WxH` that fit inside the wall (`offset + size.w \u{2264} wall_length`, and every \
         row `y ..= y + size.h - 1` inside one course of the masonry — `walls height=H` under \
         `level y=N` fills rows `N + 1 ..= N + H`, and the floor slab owns row 0)",
    );
}

#[test]
fn walls_that_paint_nothing_are_masonry_to_neither_pass() {
    // The other case that used to fall outside the equivalence. The wall
    // is declared with a positive `height=`, and its material is an
    // abstract token that no registry pack is here to lift — so the pass
    // paints no wall row, defers the cut, and the port has to agree.
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
        defers(&out),
    );
    assert!(out.walkways.is_empty(), "and no strip is laid");
}
