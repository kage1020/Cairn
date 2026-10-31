//! A `door` opens only at a row some `walls` member paints, capped by that course, and a walkway
//! port draws the same line as the openings pass.

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::DiagnosticCode;
use cairn_lang_core::{lower, parse, resolve};

use crate::common::{defers, lowered, only_structure, refusals, state_at, walkway_was_laid};

const THEME: &str = "theme t:\n  \
                     slot floor -> @oak_planks\n  \
                     slot wall  -> @cobblestone\n  \
                     slot roof  -> @spruce_stairs\n  \
                     slot gravel -> @gravel\n\n";

/// Nothing at all was reported, where [`refusals`] would see only `E_INVALID_MEMBER`.
fn assert_clean(ir: &BlockArrayIr) {
    assert!(
        ir.diagnostics.is_empty(),
        "expected a source that compiles clean, got {:?}",
        ir.diagnostics,
    );
}

// ------------------------------------------------- the openings pass

#[test]
fn a_door_under_walls_that_start_a_storey_up_is_refused_instead_of_carving_air() {
    // The only course paints rows 7..=10, and the door opens at row 1.
    let src = format!(
        "{THEME}struct hut size=5x5\n  \
         floor mat_slot=floor\n  \
         level id=upper y=6\n    \
         walls id=w mat_slot=wall height=4\n  \
         door id=e side=front at=center\n",
    );
    let out = lowered(&src);
    assert_eq!(
        refusals(&out),
        ["door opens at y=1, which is not inside any wall course (the walls occupy y=7..=10)"],
    );
}

#[test]
fn the_door_and_the_window_on_that_body_give_the_same_answer() {
    let src = format!(
        "{THEME}struct hut size=5x5\n  \
         level id=upper y=6\n    \
         walls id=w mat_slot=wall height=4\n  \
         door   id=e side=front at=center\n  \
         window id=v side=front offset=1 y=1 size=1x2 mat_slot=wall\n",
    );
    let out = lowered(&src);
    assert_eq!(
        refusals(&out),
        [
            "door opens at y=1, which is not inside any wall course (the walls occupy y=7..=10)",
            "window rows y=1..=2 are not all inside one wall course (size=1x2; the walls occupy y=7..=10)",
        ],
    );
}

#[test]
fn a_door_inside_the_level_whose_walls_it_opens_is_carved() {
    // The door under the `level` opens at row 7, inside the 7..=10 course.
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
        assert_eq!(state_at(ba, 2, y, 4).id, "minecraft:air", "row y={y}");
    }
    // The rows above the opening are still the wall they were cut from.
    for y in 9..=10 {
        assert_eq!(
            state_at(ba, 2, y, 4).id,
            "minecraft:cobblestone",
            "row y={y}"
        );
    }
}

#[test]
fn a_door_is_checked_against_its_own_course_and_not_the_tallest_one() {
    // Courses at 1..=2 and 7..=10; the `level y=2` door opens at row 3, in the gap.
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
        refusals(&out),
        [
            "door opens at y=3, which is not inside any wall course (the walls occupy y=1..=2, y=7..=10)"
        ],
    );
    let out = lowered(&two_courses(6));
    assert_clean(&out);
    assert_eq!(state_at(only_structure(&out), 2, 7, 4).id, "minecraft:air");
}

#[test]
fn two_courses_that_touch_are_one_wall_to_the_doorway_too() {
    // The courses abut at 1 and 2..=4, so the struct has one wall from 1 to 4.
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
        assert_eq!(state_at(ba, 2, y, 4).id, "minecraft:air", "row y={y}");
    }
    for y in 3..=4 {
        assert_eq!(
            state_at(ba, 2, y, 4).id,
            "minecraft:cobblestone",
            "row y={y}"
        );
    }
}

#[test]
fn a_door_on_a_struct_with_no_walls_says_there_is_no_wall() {
    let src = format!(
        "{THEME}struct hut size=5x5\n  \
         floor mat_slot=floor\n  \
         door id=e side=front at=center\n",
    );
    let out = lowered(&src);
    assert_eq!(
        refusals(&out),
        ["door has no wall to cut into: this struct declares no `walls` with a positive `height=`"],
    );
    assert_eq!(defers(&out), Vec::<&str>::new());
}

#[test]
fn a_door_in_walls_that_paint_nothing_defers_to_them() {
    // With no registry pack the abstract token gives the walls no block, so they paint nothing.
    let src = "theme t:\n  slot wall -> @wall.stone.cobble\n\n\
               struct hut size=5x5\n  \
               walls mat_slot=wall height=3\n  \
               door id=e side=front at=center\n";
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
            "door has no wall to cut into: no `walls` member on this struct paints",
            vec![
                "a `walls` member written to hold these rows paints nothing, and its own finding \
                 says why; the opening is cut once that member paints"
            ],
        )],
    );
}

#[test]
fn a_door_under_a_one_row_course_opens_that_row_and_stops() {
    // `walls height=1` paints row 1 only. The roof slot is a stair, as a sloped roof needs.
    let src = format!(
        "{THEME}struct hut size=5x5\n  \
         walls mat_slot=wall height=1\n  \
         roof kind=gable mat_slot=roof\n  \
         door id=e side=front at=center\n",
    );
    let out = lowered(&src);
    assert_clean(&out);
    let ba = only_structure(&out);
    assert_eq!(state_at(ba, 2, 1, 4).id, "minecraft:air");
    assert_eq!(state_at(ba, 2, 2, 4).id, "minecraft:spruce_stairs");
}

// --------------------------------------------- the walkway port agrees

/// A two-hut site whose walkway anchors on a door under walls at `level y={wall_level}`.
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

#[test]
fn the_port_anchors_exactly_the_doorways_the_openings_pass_carves() {
    // `spec/components-editing-sites` "Ports and `connect`", as an equivalence of the two gates.
    for wall_level in 0..=3 {
        let carved = {
            let src = format!(
                "{THEME}struct hut size=5x5\n  \
                 level id=up y={wall_level}\n    \
                 walls id=w mat_slot=wall height=4\n  \
                 door id=e side=front at=center\n",
            );
            lowered(&src).diagnostics.is_empty()
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
    // Port resolution walks the `def` body only, so a port's door always opens at row 1.
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
    let refusals: Vec<_> = resolution
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::UnresolvedPort)
        .collect();
    assert_eq!(refusals.len(), 2, "{:?}", resolution.diagnostics);
    for (refusal, place) in refusals.iter().zip(["a", "b"]) {
        assert_eq!(
            refusal.primary,
            format!(
                "port `e` of `def hut` (used by `place {place}`) is declared under \
                 `level id=up`, and a member under a `level` cannot be a port yet"
            ),
        );
        let notes: Vec<&str> = refusal.notes.iter().map(|n| n.message.as_str()).collect();
        assert_eq!(
            notes,
            [
                "a port has to be a door or window declared directly in the body of `def hut`: \
                 use one of those as the endpoint; level-scoped ports are a future extension"
            ],
        );
    }
}

#[test]
fn the_walkway_defer_states_the_rule_the_door_broke() {
    let src = site_with_door_port(6);
    let out = lowered(&src);
    let notes: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.primary.contains("was skipped because port"))
        .flat_map(|d| &d.notes)
        .collect();
    assert_eq!(notes.len(), 2, "one note per refused port: {notes:?}");
    assert_eq!(
        notes[0].message,
        "`a.e` is a door that opens at y=1, the row above the floor slab, which is not inside \
         any wall course (the walls occupy y=7..=10); the member says so on its own line too",
    );
    // The second note is the other endpoint's, not a copy of the first.
    assert_eq!(
        notes[1].message,
        "`b.e` is a door that opens at y=1, the row above the floor slab, which is not inside \
         any wall course (the walls occupy y=7..=10); the member says so on its own line too",
    );
    let at = notes[0].span.clone().expect("the note points at the door");
    assert!(
        src[at].starts_with("door id=e"),
        "the note points at the door's own line"
    );
    // The member that cannot be built says so on its own line.
    assert!(
        refusals(&out)
            .iter()
            .any(|d| d.starts_with("door opens at y=1")),
        "the doorway that could not be cut says so itself: {:?}",
        refusals(&out),
    );
}
