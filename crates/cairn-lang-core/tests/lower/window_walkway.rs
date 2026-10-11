//! `window-walkway.crn` lowers to a flat strip from a door to a window port, pinning the window
//! port's wall, centre and height.

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
    // The door port is (2, 0, 5); the window's wall cell is (11, 4), so its port is (11, 0, 5).
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
    // A pure x-leg of 10 cells at z=5; a Y lift to the window's `y=` would drop the count.
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
    // The window sits one course above the doorway, so no two openings share a cell.
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

#[test]
fn window_walkway_overflowing_window_cascades_one_deferred_member() {
    // `offset + size.w = 4` overflows the 3-long front wall.
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
    // The openings pass also defers the window itself; the span filter keeps that out.
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
    assert_eq!(
        walkway_cascades[0].primary,
        "walkway `anchor.entry ↔ peer.overflow` was skipped because port `peer.overflow` \
         could not be placed",
    );
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
    assert!(
        out.walkways.is_empty(),
        "overflowing-window row must drop its walkway, got {:?}",
        out.walkways.keys().collect::<Vec<_>>(),
    );
}
