//! Source is order-free (`spec/compilation` "Phase evaluation"): members land in phases, the
//! palette describes the finished grid, and a last write within a phase is a `W_PHASE_CONFLICT`.

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr};
use cairn_lang_core::check::DiagnosticCode;

use crate::common::{lowered, only_structure};

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

/// A one-wide struct under a roof overhang, whose front eave is a single ring voxel.
const EAVE_PRELUDE: &str = "theme t:\n  \
                            slot wall -> @cobblestone\n  \
                            slot deck -> @oak_planks\n  \
                            slot eave -> @dark_oak_stairs\n\n\
                            struct t size=1x3\n  \
                            walls mat_slot=wall height=3\n  \
                            roof kind=flat mat_slot=deck overhang=1\n";

const EAVE: &str = "  stair kind=stairs side=front mat_slot=eave\n";
const PLATE: &str = "  pressure_plate id=p at=front.outside offset=0 y=0 -> sig.step\n";

fn eave_source(body: &str) -> String {
    format!("{EAVE_PRELUDE}{body}")
}

// --- the phase a member lands in ----------------------------------------

#[test]
fn a_plate_and_an_eave_stair_on_one_cell_lower_the_same_whichever_line_comes_first() {
    // They contest `(1, 0, 4)`: the eave's one ring voxel and the plate's `at=front.outside`
    // anchor.
    let plate_first = lowered(&eave_source(&format!("{PLATE}{EAVE}")));
    let eave_first = lowered(&eave_source(&format!("{EAVE}{PLATE}")));

    assert_eq!(only_structure(&plate_first), only_structure(&eave_first));
    // Guard: the comparison is vacuous if neither side painted a plate.
    assert!(
        ids(only_structure(&plate_first)).contains(&"minecraft:oak_pressure_plate"),
        "the plate must survive both orders, palette = {:?}",
        ids(only_structure(&plate_first)),
    );
    assert_eq!(conflicts(&plate_first).len(), 0);
    assert_eq!(conflicts(&eave_first).len(), 0);
}

#[test]
fn a_plate_reaches_the_grid_after_the_eave_stair_that_would_have_covered_it() {
    // The test above would also pass if both orders agreed on the stair, so name the winner.
    let out = lowered(&eave_source(&format!("{PLATE}{EAVE}")));
    let ba = only_structure(&out);
    let cell = ba.dims.index(1, 0, 4).expect("the contested cell exists");
    assert_eq!(
        ba.palette.entries[usize::from(ba.voxels[cell].0)].id,
        "minecraft:oak_pressure_plate",
        "a sensor is a fixture, and fixtures are evaluated after the envelope",
    );
}

// --- what the palette describes -----------------------------------------

#[test]
fn a_material_covered_by_a_later_phase_leaves_no_palette_entry() {
    // Guard: the eave paints its one voxel when nothing covers it.
    let uncovered = lowered(&eave_source(EAVE));
    assert!(
        ids(only_structure(&uncovered)).contains(&"minecraft:dark_oak_stairs"),
        "palette = {:?}",
        ids(only_structure(&uncovered)),
    );
    let out = lowered(&eave_source(&format!("{EAVE}{PLATE}")));
    assert!(
        !ids(only_structure(&out)).contains(&"minecraft:dark_oak_stairs"),
        "the eave's only voxel went to the plate, so the structure holds no stair — palette = \
         {:?}",
        ids(only_structure(&out)),
    );
}

#[test]
fn pruning_keeps_air_at_slot_zero_and_leaves_the_survivors_in_order() {
    // The plate covers the eave's only cell, so the stair drops out of the palette's middle. The
    // survivors are already sorted: this pins the prune, not the sort.
    let out = lowered(&eave_source(&format!("{EAVE}{PLATE}")));
    assert_eq!(
        ids(only_structure(&out)),
        [
            "minecraft:air",
            "minecraft:cobblestone",
            "minecraft:oak_planks",
            "minecraft:oak_pressure_plate",
        ],
    );
}

#[test]
fn every_palette_entry_of_a_permuted_pair_survives_the_permutation() {
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
            "minecraft:glass_pane",
            "minecraft:oak_planks",
        ],
        "air at slot 0, the rest ascending by id",
    );
}

#[test]
fn permuting_two_members_of_one_phase_that_share_no_voxel_leaves_the_array_equal() {
    // A first-use palette would record their order, so the whole `BlockArray` is compared.
    let glass_first = lowered(&source(concat!(
        "  window side=front y=1 offset=1 size=1x1 mat_slot=glass\n",
        "  window side=back  y=1 offset=1 size=1x1 mat_slot=deck\n",
    )));
    let deck_first = lowered(&source(concat!(
        "  window side=back  y=1 offset=1 size=1x1 mat_slot=deck\n",
        "  window side=front y=1 offset=1 size=1x1 mat_slot=glass\n",
    )));
    assert_eq!(only_structure(&glass_first), only_structure(&deck_first));
    assert_eq!(
        ids(only_structure(&glass_first)),
        [
            "minecraft:air",
            "minecraft:cobblestone",
            "minecraft:glass_pane",
            "minecraft:oak_planks",
        ],
    );
    // Guard: two windows that never painted would compare equal too.
    assert_eq!(
        glass_first.diagnostics.len(),
        0,
        "both windows must lower cleanly: {:#?}",
        glass_first.diagnostics,
    );
}

#[test]
fn the_palette_is_sorted_rather_than_left_in_the_order_the_phases_painted() {
    // Paint order here is not sorted: `birch_fence` is painted last and sorts first.
    let out = lowered(concat!(
        "theme t:\n",
        "  slot wall  -> @cobblestone\n",
        "  slot floor -> @spruce_planks\n",
        "  slot bars  -> @birch_fence\n",
        "\n",
        "struct t size=7x5\n",
        "  walls mat_slot=wall height=3\n",
        "  floor mat_slot=floor\n",
        "  window side=front y=1 offset=1 size=1x1 mat_slot=bars\n",
    ));
    assert_eq!(
        ids(only_structure(&out)),
        [
            "minecraft:air",
            "minecraft:birch_fence",
            "minecraft:cobblestone",
            "minecraft:spruce_planks",
        ],
        "the opening's material sorts first even though massing painted first",
    );
}

// --- conflicts inside one phase -----------------------------------------

#[test]
fn two_openings_on_one_cell_are_reported_with_both_members_and_the_count() {
    // The door carves x=3 at y=1..2, and the 1x2 window takes both cells back.
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
    let out = lowered(&source("  door side=front at=center\n"));
    assert_eq!(conflicts(&out).len(), 0, "{:#?}", conflicts(&out));
}

#[test]
fn one_member_covering_its_own_cell_twice_is_not_a_conflict() {
    // `repeat=2 step=1` on a 2-wide window writes column 2 twice from one member.
    let out = lowered(&source(
        "  window side=front y=1 offset=1 size=2x1 repeat=2 step=1 mat_slot=glass\n",
    ));
    assert_eq!(conflicts(&out).len(), 0, "{:#?}", conflicts(&out));
    // Guard: a `W_DEFERRED_MEMBER` here would mean the window never painted.
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
    let out = lowered(&source(
        "  walls mat_slot=wall height=2\n  level y=0\n    walls mat_slot=wall height=3\n",
    ));
    assert_eq!(conflicts(&out).len(), 0, "{:#?}", conflicts(&out));
    assert_eq!(
        only_structure(&out).dims.y,
        4,
        "both walls must have been painted for their rows to overlap",
    );
}

#[test]
fn the_conflict_warning_does_not_change_which_block_wins() {
    // `spec/compilation` "Phase evaluation" grants last-wins inside a phase.
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
    // The other conflicts here are in Openings; one contested voxel pins the singular wording.
    let out = lowered(&source(
        "  roof kind=flat mat_slot=deck overhang=1\n  \
         pressure_plate id=a at=front.outside offset=3 y=0 mat_slot=stone_plate -> sig.x\n  \
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
    // The local override `spec/compilation` "Phase evaluation" grants is still reported.
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
        "the note has to offer the reading `spec/compilation` permits, not \
         only the accidental one: {:#?}",
        found[0].notes,
    );
}

#[test]
fn one_member_overwriting_two_others_is_reported_once_per_pair() {
    let out = lowered(&source(concat!(
        "  door side=front at=center\n",
        "  window id=low side=front y=1 offset=3 size=1x1 mat_slot=glass\n",
        "  window id=high side=front y=2 offset=3 size=1x1 mat_slot=deck\n",
        "  window id=both side=front y=1 offset=3 size=1x2 mat_slot=wall\n",
    )));
    let found = conflicts(&out);
    let primaries: Vec<&str> = found.iter().map(|d| d.primary.as_str()).collect();
    // The door loses one cell to each single window, and the 1x2 window takes one from each.
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
    // Two findings share an anchor; the stable sort keeps them in paint order.
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
        "the cell it took from the `low` window is painted first, so that pair is recorded \
         and reported first",
    );
}

#[test]
fn a_conflict_across_a_level_anchors_at_the_member_and_not_the_level() {
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
