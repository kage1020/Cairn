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
                       slot deck  -> @oak_planks\n\n\
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
    // renumber the plate down onto it rather than leave a hole, and slot 0
    // stays air even though this structure has no air cell to name it.
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
