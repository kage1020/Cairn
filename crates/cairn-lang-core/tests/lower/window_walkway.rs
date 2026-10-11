//! `window-walkway.crn` lowers to a flat strip from `home1.entry`, a door, to
//! `home2.front`, a window, which pins the window port's wall, horizontal
//! centre and height where the door-only examples cannot reach them.

use cairn_lang_core::block_array::{BlockArrayIr, Footprint, PaletteIndex};
use cairn_lang_core::check::{DiagnosticCode, Severity};

use crate::common::{lowered_with_resolver_diagnostics, read_example};

fn lower_window_walkway() -> BlockArrayIr {
    lowered_with_resolver_diagnostics(&read_example("window-walkway.crn"))
}

const WALKWAY_KEY: &str = "walkway::pair::home1.entry__home2.front";

#[test]
fn window_walkway_endpoints_pin_door_and_window_ports() {
    let out = lower_window_walkway();
    let walkway = out.walkways.get(WALKWAY_KEY).expect("walkway present");
    assert_eq!(walkway.site, "pair");
    assert_eq!(walkway.from.place, "home1");
    assert_eq!(walkway.from.port, "entry");
    assert_eq!(walkway.to.place, "home2");
    assert_eq!(walkway.to.port, "front");
    assert_eq!(walkway.path_material, "minecraft:gravel");
    // home1 origin (0,0,0), size=5x5, no roof so overhang=0; door
    // `entry` at front + `at=center` resolves to (2, 0, 5).
    // home2 origin (9,0,0) via `east_of=home1 gap=4`; window `front`
    // on front wall with offset=2 size=1x1 has u = 2 + 1/2 = 2, so the
    // wall world cell is (9 + 2, 0 + 4) = (11, 4) and the port is one
    // step outward in +z → (11, 0, 5). The L-walkway bounding box is
    // dx = 11 - 2 + 1 = 10, dz = 1 (single z = 5); origin pins to the
    // (min_x, _, min_z) corner = (2, 0, 5).
    assert_eq!(
        walkway.origin,
        (2, 0, 5),
        "window-walkway origin pins the (min_x, _, min_z) corner",
    );
    assert_eq!(
        walkway.footprint,
        Footprint { x: 10, z: 1 },
        "window-walkway runs purely along +x at z = 5",
    );
}

#[test]
fn window_walkway_block_array_paints_ten_gravel_cells() {
    // A pure x-leg of 10 cells at z=5 (x = 2..=11). An off-by-one or an
    // unintended Y lift (honouring the window's authored `y=`, whatever
    // course it sits on) would shift the strip into a blocked floor cell
    // and drop the gravel count.
    let out = lower_window_walkway();
    let ba = out
        .structures
        .get(WALKWAY_KEY)
        .expect("walkway BlockArray present");
    assert_eq!(ba.dims.y, 1, "walkway is a 1-block-thick strip");
    let gravel_idx = ba
        .palette
        .entries
        .iter()
        .position(|s| s.id == "minecraft:gravel")
        .map(|p| PaletteIndex(u16::try_from(p).expect("palette index fits in u16")))
        .expect("walkway palette contains gravel");
    let gravel_count = ba.voxels.iter().filter(|i| **i == gravel_idx).count();
    assert_eq!(
        gravel_count, 10,
        "expected 10 gravel cells (single x-leg), got {gravel_count}",
    );
}

#[test]
fn window_walkway_openings_do_not_contest_a_cell() {
    // The window sits one course above the door's opening, and the wall
    // carries the extra course so it still fits inside. Put the window
    // back in the doorway and `door` and `window` — both openings — decide
    // the cell by line order. The example's header says why; this is the
    // assertion that holds it, next to the comment.
    let out = lower_window_walkway();
    let contested: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::PhaseConflict)
        .collect();
    assert!(
        contested.is_empty(),
        "window-walkway's openings must not contest a cell, got {contested:#?}",
    );
}

#[test]
fn window_walkway_emits_no_resolver_errors() {
    let out = lower_window_walkway();
    let errors: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.severity() == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "window-walkway must not produce error-severity diagnostics, got {errors:#?}",
    );
}

/// Walkway anchored to a window whose `offset + size.w` overflows the
/// wall must cascade exactly one `W_DEFERRED_MEMBER`, with the door
/// half of the row identified as the side that *did* place: a walkway
/// dropped on its first refused port without a word fails here.
#[test]
fn window_walkway_overflowing_window_cascades_one_deferred_member() {
    // size=3x3 → wall_length(Front) = 3. The overflow window declares
    // `offset=2 size=2x2`, so `offset + size.w = 4 > 3` and the window
    // port is refused as past the wall. The peer's door
    // port still resolves cleanly, so the cascade names the window side.
    let src = "@cairn 2026.06\n\n\
def hut size=3x3:\n  \
floor mat_slot=floor\n  \
walls mat_slot=wall height=3\n  \
door   id=entry    side=front at=center\n  \
window id=overflow side=front y=1 offset=2 size=2x2 mat_slot=glass\n\n\
theme plain:\n  \
slot floor -> @oak_planks\n  \
slot wall  -> @cobblestone\n  \
slot glass -> @glass_pane\n\n\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
place id=peer   use=hut theme=plain east_of=anchor gap=4\n  \
connect anchor.entry to peer.overflow path=@gravel\n";
    let out = lowered_with_resolver_diagnostics(src);
    // Two `W_DEFERRED_MEMBER` warnings come from the def-side openings
    // pass dropping the overflow window itself (one per placement —
    // `anchor` and `peer` both reuse `def hut`); the cascade we are
    // pinning is the one anchored on the `connect` row. Filtering by that
    // anchor keeps the assertion robust against the unrelated def-side
    // count.
    let walkway_cascades: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.code == DiagnosticCode::DeferredMember)
        .filter(|d| &src[d.span.clone()] == "connect anchor.entry to peer.overflow path=@gravel")
        .collect();
    assert_eq!(
        walkway_cascades.len(),
        1,
        "overflowing window must surface exactly one walkway-cascade W_DEFERRED_MEMBER, got {:#?}",
        out.diagnostics,
    );
    // The door side resolves, so only the window side is named.
    assert_eq!(
        walkway_cascades[0].primary,
        "walkway `anchor.entry ↔ peer.overflow` was skipped because port `peer.overflow` \
         could not be placed",
    );
    // The note names the rule this window broke, with the numbers that
    // broke it — not every contract a port could have hit.
    let note_messages: Vec<_> = walkway_cascades[0]
        .notes
        .iter()
        .map(|n| n.message.as_str())
        .collect();
    assert_eq!(
        note_messages,
        vec![
            "`peer.overflow` is a window that runs past the end of its wall \
             (`offset + size.w` = 2 + 2, wall length 3); the member says so on its own line too",
        ],
    );
    // The walkway IR must drop the row rather than lay a partial strip.
    assert!(
        out.walkways.is_empty(),
        "overflowing-window row must drop its walkway, got {:?}",
        out.walkways.keys().collect::<Vec<_>>(),
    );
}
