//! `spec/compilation.md` §4.1 opens by promising that source "MAY be
//! written line-oriented, flat, and order-free" because the compiler
//! assigns every command to a phase and evaluates the phases in a fixed
//! order — "order accidents are eliminated".
//!
//! Two things have to hold for that to be true of the artifact and not
//! only of the paint order. The phase a member lands in has to be the one
//! §4.1 names, or two members that belong to different phases end up in one
//! bucket where the later line simply wins. And the palette has to describe
//! the finished grid rather than the sequence of writes that produced it,
//! or the loser's material rides along into the `.nbt` and the
//! `resolved_ir_hash`, and permuting two lines changes the artifact even
//! when it does not change a single voxel.
//!
//! Inside one phase §4.1 does grant last-wins, to "local overrides within
//! the same phase". An author restating a member is what that grant is for;
//! two footprints that happen to intersect is not, and the grid cannot tell
//! them apart. So the resolution stays last-wins and stops being silent —
//! `W_PHASE_CONFLICT` names both members and how many voxels changed hands.

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::DiagnosticCode;
use cairn_lang_core::{lower, parse, resolve};

fn lowered(source: &str) -> BlockArrayIr {
    let module = parse(source).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    lower_to_block_array(&ir, &resolution, None)
}

fn only_structure(ir: &BlockArrayIr) -> &BlockArray {
    assert_eq!(
        ir.structures.len(),
        1,
        "these sources declare exactly one struct",
    );
    ir.structures.values().next().expect("one structure")
}

fn conflicts(ir: &BlockArrayIr) -> Vec<&cairn_lang_core::check::Diagnostic> {
    ir.diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::PhaseConflict)
        .collect()
}

fn ids(ba: &BlockArray) -> Vec<&str> {
    ba.palette.entries.iter().map(|s| s.id.as_str()).collect()
}

const PRELUDE: &str = "theme t:\n  \
                       slot wall  -> @cobblestone\n  \
                       slot glass -> @glass_pane\n  \
                       slot deck  -> @oak_planks\n  \
                       slot stone_plate -> @stone_pressure_plate\n  \
                       slot wood_plate  -> @oak_pressure_plate\n\n\
                       struct t size=7x5\n  \
                       walls mat_slot=wall height=3\n";

fn source(body: &str) -> String {
    format!("{PRELUDE}{body}")
}

const PLATE: &str = "  pressure_plate id=p at=front.outside offset=3 y=0 -> sig.step\n";
const PANE: &str = "  window side=front y=0 offset=3 size=1x1 mat_slot=glass\n";

// --- the phase a member lands in ----------------------------------------

#[test]
fn a_plate_and_a_window_on_one_cell_lower_the_same_whichever_line_comes_first() {
    // The cell they contest is `(3, 0, 4)`: the window's authored cell on
    // the front wall, and the plate's `at=front.outside` anchor one step
    // beyond it, which at `y=0` is the same voxel. Sharing the openings
    // bucket, the two resolved by source order and the palette recorded
    // whichever lost.
    let plate_first = lowered(&source(&format!("{PLATE}{PANE}")));
    let pane_first = lowered(&source(&format!("{PANE}{PLATE}")));

    assert_eq!(only_structure(&plate_first), only_structure(&pane_first));
    // Guard: the comparison above is worth nothing if neither side painted
    // a plate. A sensor is a fixture and a window is an opening, so the
    // plate wins both times.
    assert!(
        ids(only_structure(&plate_first)).contains(&"minecraft:oak_pressure_plate"),
        "the plate must survive both orders, palette = {:?}",
        ids(only_structure(&plate_first)),
    );
    assert_eq!(conflicts(&plate_first).len(), 0);
    assert_eq!(conflicts(&pane_first).len(), 0);
}

#[test]
fn a_plate_reaches_the_grid_after_the_window_that_would_have_covered_it() {
    // The order-independence test above would also pass if both orders
    // agreed on the *window*. Name the winner.
    let out = lowered(&source(&format!("{PANE}{PLATE}")));
    let ba = only_structure(&out);
    let cell = ba.dims.index(3, 0, 4).expect("the contested cell exists");
    assert_eq!(
        ba.palette.entries[usize::from(ba.voxels[cell].0)].id,
        "minecraft:oak_pressure_plate",
        "a sensor is a fixture, and fixtures are evaluated after openings",
    );
}

// --- what the palette describes -----------------------------------------

#[test]
fn a_material_covered_by_a_later_phase_leaves_no_palette_entry() {
    let out = lowered(&source(&format!("{PANE}{PLATE}")));
    assert!(
        !ids(only_structure(&out)).contains(&"minecraft:glass_pane"),
        "the window's only voxel went to the plate, so the structure holds \
         no glass — palette = {:?}",
        ids(only_structure(&out)),
    );
}

#[test]
fn pruning_keeps_air_at_slot_zero_and_leaves_the_survivors_in_order() {
    // `walls` interns cobblestone, then the window interns glass, then the
    // plate covers the window's one cell. Dropping the middle entry must
    // renumber the plate down onto it rather than leave a hole, and air
    // keeps slot 0 — which this fixture does reference, since a wall ring
    // leaves the interior empty. The unreferenced-air case is the unit
    // test in `block_array::lower`.
    let out = lowered(&source(&format!("{PANE}{PLATE}")));
    assert_eq!(
        ids(only_structure(&out)),
        [
            "minecraft:air",
            "minecraft:cobblestone",
            "minecraft:oak_pressure_plate",
        ],
    );
}

#[test]
fn every_palette_entry_of_a_permuted_pair_survives_the_permutation() {
    // The palette is not only a set here: two members that never contest a
    // cell still intern in the order their phases run, so a permutation
    // that leaves the voxels alone must leave the entry order alone too.
    let a = lowered(&source(
        "  window side=front y=1 offset=1 size=1x1 mat_slot=glass\n  floor mat_slot=deck\n",
    ));
    let b = lowered(&source(
        "  floor mat_slot=deck\n  window side=front y=1 offset=1 size=1x1 mat_slot=glass\n",
    ));
    assert_eq!(only_structure(&a), only_structure(&b));
    assert_eq!(
        ids(only_structure(&a)),
        [
            "minecraft:air",
            "minecraft:cobblestone",
            "minecraft:oak_planks",
            "minecraft:glass_pane",
        ],
        "massing before openings, whichever line was written first",
    );
}

// --- conflicts inside one phase -----------------------------------------

#[test]
fn two_openings_on_one_cell_are_reported_with_both_members_and_the_count() {
    // A `door` at `at=center` on a 7-wide front wall carves x=3 at y=1..2;
    // a 1x2 window at the same column takes two of those cells back.
    let out = lowered(&source(
        "  door side=front at=center\n  \
         window side=front y=1 offset=3 size=1x2 mat_slot=glass\n",
    ));
    let found = conflicts(&out);
    assert_eq!(
        found.len(),
        1,
        "one pair of members, one finding: {found:#?}"
    );
    let d = found[0];
    assert!(
        d.primary
            .contains("`window` overwrites 2 voxels that `door` painted"),
        "the finding must name both members and the count, got: {}",
        d.primary,
    );
    let overwritten = d.notes[0]
        .span
        .as_ref()
        .expect("the note carries a span so the reader can reach the other member");
    assert!(
        d.span.start > overwritten.start,
        "the finding anchors at the member that wrote last, the way \
         `E_LOGIC_MULTIPLE_DRIVERS` anchors at the redefinition",
    );
    let src = source(
        "  door side=front at=center\n  \
         window side=front y=1 offset=3 size=1x2 mat_slot=glass\n",
    );
    assert!(
        src[overwritten.start..overwritten.end].starts_with("door"),
        "the note points at the `door` line, got `{}`",
        &src[overwritten.start..overwritten.end],
    );
}

#[test]
fn a_door_cutting_through_walls_is_not_a_conflict() {
    // Massing then openings: the whole point of the phase order. If this
    // fires, every building ever written warns.
    let out = lowered(&source("  door side=front at=center\n"));
    assert_eq!(conflicts(&out).len(), 0, "{:#?}", conflicts(&out));
}

#[test]
fn one_member_covering_its_own_cell_twice_is_not_a_conflict() {
    // `repeat=2 step=1` on a 2-wide window stamps columns `1..=2` and then
    // `2..=3`, so column 2 is written twice by one member. `step=` is only
    // checked against zero, so this is a shape the lowering accepts rather
    // than one contrived past a guard. (A `sym=true` window would not do:
    // its mirror is either disjoint or coalesced away, so it never writes
    // one cell twice at all.)
    let out = lowered(&source(
        "  window side=front y=1 offset=1 size=2x1 repeat=2 step=1 mat_slot=glass\n",
    ));
    assert_eq!(conflicts(&out).len(), 0, "{:#?}", conflicts(&out));
    // Guard: a `W_DEFERRED_MEMBER` here would mean the window never
    // painted, and a test that passes because nothing happened pins
    // nothing.
    assert_eq!(
        out.diagnostics.len(),
        0,
        "the overlapping stamps must lower cleanly: {:#?}",
        out.diagnostics,
    );
    let ba = only_structure(&out);
    let painted = (1..=3)
        .filter(|x| {
            let i = ba.dims.index(*x, 1, 4).expect("front wall cell");
            ba.palette.entries[usize::from(ba.voxels[i].0)].id == "minecraft:glass_pane"
        })
        .count();
    assert_eq!(painted, 3, "the two stamps cover columns 1..=3");
}

#[test]
fn two_members_writing_the_same_block_are_not_a_conflict() {
    // Two `walls` of one material, one of them raised by a `level`, meet
    // over the rows they share. Nothing about the grid depends on which ran
    // first, so nothing is reported.
    let out = lowered(&source(
        "  walls mat_slot=wall height=2\n  level y=0\n    walls mat_slot=wall height=3\n",
    ));
    assert_eq!(conflicts(&out).len(), 0, "{:#?}", conflicts(&out));
    // Guard: the two really do overlap — a test that passes because one of
    // them was deferred proves nothing.
    assert_eq!(
        only_structure(&out).dims.y,
        4,
        "both walls must have been painted for their rows to overlap",
    );
}

#[test]
fn the_conflict_warning_does_not_change_which_block_wins() {
    // §4.1 grants last-wins inside a phase; this reports it, it does not
    // overrule it. Permuting the two lines still swaps the winner — which
    // is exactly what the warning is there to tell the author.
    let door_first = lowered(&source(
        "  door side=front at=center\n  \
         window side=front y=1 offset=3 size=1x2 mat_slot=glass\n",
    ));
    let window_first = lowered(&source(
        "  window side=front y=1 offset=3 size=1x2 mat_slot=glass\n  \
         door side=front at=center\n",
    ));
    let cell = |ba: &BlockArray| {
        let i = ba.dims.index(3, 1, 4).expect("the contested cell exists");
        ba.palette.entries[usize::from(ba.voxels[i].0)].id.clone()
    };
    assert_eq!(cell(only_structure(&door_first)), "minecraft:glass_pane");
    assert_eq!(cell(only_structure(&window_first)), "minecraft:air");
    assert_eq!(conflicts(&door_first).len(), 1);
    assert_eq!(conflicts(&window_first).len(), 1);
}

#[test]
fn a_conflict_between_two_fixtures_is_reported_like_any_other() {
    // Every conflict fixture above lands in Openings, which is the bucket
    // that already existed. The Fixtures bucket is new, and what is new
    // with it is the `run_phase` wiring that names the member each write
    // belongs to — so the phase has to be exercised on the conflict path
    // too, not only on the "the plate wins" path.
    //
    // Two plates on one anchor with different materials. One voxel, so
    // this is also where the singular wording is pinned.
    let out = lowered(&source(
        "  pressure_plate id=a at=front.outside offset=3 y=0 mat_slot=stone_plate -> sig.x\n  \
         pressure_plate id=b at=front.outside offset=3 y=0 mat_slot=wood_plate -> sig.y\n",
    ));
    let found = conflicts(&out);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0]
            .primary
            .contains("`pressure_plate` overwrites 1 voxel that `pressure_plate` painted"),
        "one contested cell reads as `1 voxel`, not `1 voxels`: {}",
        found[0].primary,
    );
}

#[test]
fn a_conflict_between_two_massing_members_is_reported_like_any_other() {
    // The other end of the phase list. A wainscot course is the shape
    // §4.1's local-override grant is written for, and it is reported for
    // the same reason every other pair is: the grid cannot tell a
    // deliberate override from two footprints that happen to meet.
    let out = lowered(&source("  walls mat_slot=deck height=1\n"));
    let found = conflicts(&out);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        found[0]
            .primary
            .contains("`walls` overwrites 20 voxels that `walls` painted"),
        "the wainscot takes the whole ring of its one course: {}",
        found[0].primary,
    );
    assert!(
        found[0]
            .notes
            .iter()
            .any(|n| n.message.contains("If the override is deliberate")),
        "the note has to offer the reading §4.1 permits, not only the \
         accidental one: {:#?}",
        found[0].notes,
    );
}

#[test]
fn one_member_overwriting_two_others_is_reported_once_per_pair() {
    // `conflicts` is keyed by the pair, so a member that takes cells from
    // two different members owes two findings — one naming each — rather
    // than one finding with the counts added together.
    let out = lowered(&source(concat!(
        "  door side=front at=center\n",
        "  window id=low side=front y=1 offset=3 size=1x1 mat_slot=glass\n",
        "  window id=high side=front y=2 offset=3 size=1x1 mat_slot=deck\n",
        "  window id=both side=front y=1 offset=3 size=1x2 mat_slot=wall\n",
    )));
    let found = conflicts(&out);
    let primaries: Vec<&str> = found.iter().map(|d| d.primary.as_str()).collect();
    // Four pairs, not one finding with the counts added together: the
    // `door` loses one cell to each of the two single windows, and the
    // 1x2 window then takes one cell from each of those two in turn.
    assert_eq!(found.len(), 4, "{primaries:#?}");
    let overwrote_a_door = primaries
        .iter()
        .filter(|p| p.contains("that `door` painted"))
        .count();
    assert_eq!(overwrote_a_door, 2, "{primaries:#?}");
    assert!(
        primaries
            .iter()
            .all(|p| p.contains("`window` overwrites 1 voxel")),
        "each pair is counted on its own, so no finding adds two cells \
         together: {primaries:#?}",
    );
    // Every finding anchors at whichever of its pair came second.
    assert!(
        found
            .iter()
            .all(|d| { d.span.start > d.notes[0].span.as_ref().expect("note span").start }),
        "{primaries:#?}",
    );
    // Two of the four share an anchor — the 1x2 window is the later member
    // of both its pairs — so the span sort alone does not order them. It
    // is stable, and the pairs were recorded in paint order, so the one
    // over the lower cell comes first and the pair of findings does not
    // reshuffle between runs.
    let overwritten_lines: Vec<usize> = found
        .iter()
        .filter(|d| d.span == found[3].span)
        .map(|d| d.notes[0].span.as_ref().expect("note span").start)
        .collect();
    assert_eq!(
        overwritten_lines.len(),
        2,
        "the 1x2 window is the later member of two pairs: {primaries:#?}",
    );
    assert!(
        overwritten_lines[0] < overwritten_lines[1],
        "the cell it took from the `low` window is painted first, so that          pair is recorded and reported first",
    );
}

#[test]
fn a_conflict_across_a_level_anchors_at_the_member_and_not_the_level() {
    // `flatten_members` hands the phase buckets the members, not the
    // `level` that grouped them, so the span the finding carries is the
    // member's own line — the one an author would move.
    let src = source(concat!(
        "  window side=front y=1 offset=3 size=1x1 mat_slot=glass\n",
        "  level y=1\n",
        "    window side=front y=0 offset=3 size=1x1 mat_slot=deck\n",
    ));
    let out = lowered(&src);
    let found = conflicts(&out);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert!(
        src[found[0].span.start..found[0].span.end].starts_with("window"),
        "the finding anchors at the nested `window`, got `{}`",
        &src[found[0].span.start..found[0].span.end],
    );
}
