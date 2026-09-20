//! A `door` is an opening cut into a wall, and the row it opens at has to
//! be a row some `walls` member paints.
//!
//! `walls height=H` under `level y=N` paints the world rows
//! `N + 1 ..= N + H` — the floor slab owns the base plane — and a door
//! under `level y=M` opens at row `M + 1`. Asked only whether the struct
//! paints *any* wall row, the cut could not see the body whose walls
//! start a storey up: the carve painted AIR over rows that were already
//! air and said nothing, while a `window` on the same body was deferred
//! with the rows the walls occupy quoted back. One shape, two answers,
//! and the silent one belonged to the member an author is most likely to
//! write first.
//!
//! The gate is what ends the silence. The course it finds is also the
//! cap — a doorway takes two rows where that course has them and one
//! where it does not — which keeps the opening inside the masonry it was
//! cut from rather than running into the roof over a short wall.
//!
//! `spec/components-editing-sites` "Ports and `connect`" requires the
//! walkway port to draw the same line as the openings pass, so the last
//! group here asserts the two answers case by case rather than each in
//! isolation.

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr};
use cairn_lang_core::check::DiagnosticCode;
use cairn_lang_core::{lower, parse, resolve};

mod common;
use common::{lowered, only_structure};

const THEME: &str = "theme t:\n  \
                     slot floor -> @oak_planks\n  \
                     slot wall  -> @cobblestone\n  \
                     slot roof  -> @spruce_stairs\n  \
                     slot gravel -> @gravel\n\n";

/// Every `W_DEFERRED_MEMBER` message, in the order the pass raised them.
fn defers(ir: &BlockArrayIr) -> Vec<&str> {
    ir.diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::DeferredMember)
        .map(|d| d.primary.as_str())
        .collect()
}

/// Nothing at all was reported — not a deferral and not a refusal.
///
/// A case that expects the carve to happen asserts on the whole list
/// rather than on [`defers`], which filters to `W_DEFERRED_MEMBER` and so
/// would let an `E_INCOMPATIBLE_MATERIAL` on the source pass unseen.
fn assert_clean(ir: &BlockArrayIr) {
    assert!(
        ir.diagnostics.is_empty(),
        "expected a source that compiles clean, got {:?}",
        ir.diagnostics,
    );
}

fn block_id(ba: &BlockArray, x: u32, y: u32, z: u32) -> &str {
    let i = ((y * ba.dims.z + z) * ba.dims.x + x) as usize;
    ba.palette.entries[ba.voxels[i].0 as usize].id.as_str()
}

// ------------------------------------------------- the openings pass

#[test]
fn a_door_under_walls_that_start_a_storey_up_is_refused_instead_of_carving_air() {
    // The issue's repro. The only masonry is `level y=6 walls height=4`,
    // which paints rows 7..=10, and the door opens at row 1 — seven rows
    // below the first course. The carve painted AIR over air and the
    // author heard nothing about a door written into a building whose
    // walls are not there yet.
    let src = format!(
        "{THEME}struct hut size=5x5\n  \
         floor mat_slot=floor\n  \
         level id=upper y=6\n    \
         walls id=w mat_slot=wall height=4\n  \
         door id=e side=front at=center\n",
    );
    let out = lowered(&src);
    assert_eq!(
        defers(&out),
        ["door opens at y=1, which is not inside any wall course (the walls occupy y=7..=10)"],
    );
}

#[test]
fn the_door_and_the_window_on_that_body_give_the_same_answer() {
    // The asymmetry the report is about, asserted as one source rather
    // than two: the window's rows and the door's row are both outside
    // every course, so both members defer and each names the rows the
    // walls do occupy.
    let src = format!(
        "{THEME}struct hut size=5x5\n  \
         level id=upper y=6\n    \
         walls id=w mat_slot=wall height=4\n  \
         door   id=e side=front at=center\n  \
         window id=v side=front offset=1 y=1 size=1x2 mat_slot=wall\n",
    );
    let out = lowered(&src);
    assert_eq!(
        defers(&out),
        [
            "door opens at y=1, which is not inside any wall course (the walls occupy y=7..=10)",
            "window rows y=1..=2 are not all inside one wall course (size=1x2; the walls occupy y=7..=10)",
        ],
    );
}

#[test]
fn a_door_inside_the_level_whose_walls_it_opens_is_carved() {
    // The same body with the door moved under the `level` its masonry
    // belongs to: it opens at row 7, the course runs 7..=10, and the two
    // rows a doorway wants are cut out of the front wall.
    let src = format!(
        "{THEME}struct hut size=5x5\n  \
         floor mat_slot=floor\n  \
         level id=upper y=6\n    \
         walls id=w mat_slot=wall height=4\n    \
         door  id=e side=front at=center\n",
    );
    let out = lowered(&src);
    assert_clean(&out);
    let ba = only_structure(&out);
    for y in 7..=8 {
        assert_eq!(block_id(ba, 2, y, 4), "minecraft:air", "row y={y}");
    }
    // The rows above the opening are still the wall they were cut from.
    for y in 9..=10 {
        assert_eq!(block_id(ba, 2, y, 4), "minecraft:cobblestone", "row y={y}");
    }
}

#[test]
fn a_door_is_checked_against_its_own_course_and_not_the_tallest_one() {
    // Two courses with air between them: 1..=2 and 7..=10. A door under
    // `level y=2` opens at row 3, which is in the gap — and a check that
    // read the struct's highest wall row saw `10 > 3` and carved. The
    // door one level further up opens at row 7 and is cut.
    let two_courses = |door_level: u32| {
        format!(
            "{THEME}struct hut size=5x5\n  \
             walls id=lower mat_slot=wall height=2\n  \
             level id=upper y=6\n    \
             walls id=w mat_slot=wall height=4\n  \
             level id=door_level y={door_level}\n    \
             door id=e side=front at=center\n",
        )
    };
    let out = lowered(&two_courses(2));
    assert_eq!(
        defers(&out),
        [
            "door opens at y=3, which is not inside any wall course (the walls occupy y=1..=2, y=7..=10)"
        ],
    );
    let out = lowered(&two_courses(6));
    assert_clean(&out);
    assert_eq!(block_id(only_structure(&out), 2, 7, 4), "minecraft:air");
}

#[test]
fn two_courses_that_touch_are_one_wall_to_the_doorway_too() {
    // `walls height=1` paints row 1 and `level y=1 walls height=3` paints
    // 2..=4; they abut, so the struct has one wall from 1 to 4 and the
    // door opens the two rows a doorway wants — although the member it
    // opens into paints one of them. The cap is the course's, and a
    // course is what the merge says it is; reading the door's own
    // `walls` member instead would make a legal opening depend on where
    // the author put a `level` line.
    let src = format!(
        "{THEME}struct hut size=5x5\n  \
         walls id=lower mat_slot=wall height=1\n  \
         level id=up y=1\n    \
         walls id=w mat_slot=wall height=3\n  \
         door id=e side=front at=center\n",
    );
    let out = lowered(&src);
    assert_clean(&out);
    let ba = only_structure(&out);
    for y in 1..=2 {
        assert_eq!(block_id(ba, 2, y, 4), "minecraft:air", "row y={y}");
    }
    for y in 3..=4 {
        assert_eq!(block_id(ba, 2, y, 4), "minecraft:cobblestone", "row y={y}");
    }
}

#[test]
fn a_door_on_a_struct_with_no_walls_says_there_is_no_wall() {
    // The empty column keeps its own sentence: "nowhere" and "not here"
    // are different findings, and quoting `none` as the rows the walls
    // occupy would answer a question the author did not ask.
    let src = format!(
        "{THEME}struct hut size=5x5\n  \
         floor mat_slot=floor\n  \
         door id=e side=front at=center\n",
    );
    let out = lowered(&src);
    assert_eq!(
        defers(&out),
        [
            "door requires a `walls` member that paints — a positive `height=` and a `mat_slot=` that resolves — to carve into"
        ],
    );
}

#[test]
fn a_door_under_a_one_row_course_opens_that_row_and_stops() {
    // `walls height=1` paints row 1 only, and the gable's front eave sits
    // at row 2. The opening is the one row the course has; the row above
    // it stays the roof it was. A sloped roof reads `facing` / `half` /
    // `shape` off the geometry, so its slot has to be a stair family —
    // pointed at the wall's cobblestone this source is
    // `E_INCOMPATIBLE_MATERIAL` and the gable only draws at all through
    // its own fallback.
    let src = format!(
        "{THEME}struct hut size=5x5\n  \
         walls mat_slot=wall height=1\n  \
         roof kind=gable mat_slot=roof\n  \
         door id=e side=front at=center\n",
    );
    let out = lowered(&src);
    assert_clean(&out);
    let ba = only_structure(&out);
    assert_eq!(block_id(ba, 2, 1, 4), "minecraft:air");
    assert_eq!(block_id(ba, 2, 2, 4), "minecraft:spruce_stairs");
}

// --------------------------------------------- the walkway port agrees

/// A two-hut site whose walkway anchors on `a.e`, the door of a def
/// whose only `walls` member sits under `level y={wall_level}`.
fn site_with_door_port(wall_level: u32) -> String {
    format!(
        "{THEME}def hut size=5x5:\n  \
         level id=up y={wall_level}\n    \
         walls id=w mat_slot=wall height=4\n  \
         door id=e side=front at=center\n\n\
         site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=4\n  \
         connect a.e to b.e path=@gravel\n",
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
fn the_port_anchors_exactly_the_doorways_the_openings_pass_carves() {
    // The walkway-port requirement of
    // `spec/components-editing-sites` "Ports and `connect`", asserted as an
    // equivalence rather than as two independent lists — the failure mode it
    // guards against is one of the two gates being edited alone, which no
    // test of either side by itself can see. `level y=0` is the only level
    // whose walls reach the row a def-body door opens at; every higher one
    // leaves it in air.
    for wall_level in 0..=3 {
        let carved = {
            let src = format!(
                "{THEME}struct hut size=5x5\n  \
                 level id=up y={wall_level}\n    \
                 walls id=w mat_slot=wall height=4\n  \
                 door id=e side=front at=center\n",
            );
            defers(&lowered(&src)).is_empty()
        };
        let anchored = walkway_was_laid(&site_with_door_port(wall_level));
        assert_eq!(
            carved, anchored,
            "walls under `level y={wall_level}`: \
             the openings pass carves the doorway = {carved}, the port anchors it = {anchored}",
        );
    }
}

#[test]
fn a_door_under_a_level_is_not_a_port_at_all() {
    // The invariant the port side's base row rests on. Port resolution
    // walks the `def` body only — both the resolver's port check and
    // `port_world_position`'s member lookup — so the door a port names
    // has never been shifted by a `level`, and the row it opens at is
    // always `1`. Pinned here because that is what makes one constant on
    // the port side answer the same question as `carve_door`'s
    // `y_offset + 1`: were port lookup ever to walk the flattened
    // members, a door under `level y=6` would resolve, and the two
    // would disagree about the row — the fault this file exists for.
    let src = format!(
        "{THEME}def hut size=5x5:\n  \
         walls id=w mat_slot=wall height=4\n  \
         level id=up y=0\n    \
         door id=e side=front at=center\n\n\
         site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=4\n  \
         connect a.e to b.e path=@gravel\n",
    );
    let module = parse(&src).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    assert!(
        resolution
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::UnresolvedPort),
        "a door nested under a `level` has to be refused as a port: {:?}",
        resolution.diagnostics,
    );
}

#[test]
fn the_port_contract_note_states_the_rule_the_port_applies() {
    // The note is the only place an author is told what a door port
    // needs, and it used to name `side=` and `at=` alone — so a reader
    // whose walls started a storey up checked the two arguments that were
    // already right and was left with the row that was wrong.
    let out = lowered(&site_with_door_port(6));
    let note = out
        .diagnostics
        .iter()
        .filter(|d| d.primary.contains("was skipped because port"))
        .flat_map(|d| &d.notes)
        .map(|n| n.message.as_str())
        .find(|m| m.starts_with("a `door` port requires"))
        .expect("the walkway defer lists the door-port contract");
    assert_eq!(
        note,
        "a `door` port requires `side=front|back|left|right` and `at=center|left|right`, \
         with the row it opens at — `y=1`, the row above the floor slab — inside one course \
         of the masonry",
    );
    // …and the member that cannot be built says so on its own line, which
    // is where the note sends the author.
    assert!(
        defers(&out)
            .iter()
            .any(|d| d.starts_with("door opens at y=1")),
        "the doorway that could not be cut says so itself: {:?}",
        defers(&out),
    );
}
