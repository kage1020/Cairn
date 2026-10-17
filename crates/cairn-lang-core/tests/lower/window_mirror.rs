//! A `sym=true` window's mirror is cut beside it unless the two share a cell, in which case only
//! the window is cut and `W_DEFERRED_MEMBER` says why.

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr};
use cairn_lang_core::check::DiagnosticCode;

use crate::common::{lowered, notes, only_structure, slice, state_at};

/// A struct `width`x`depth` with one course of cobblestone walls and `window` in it.
fn struct_with_window(width: u32, depth: u32, window: &str) -> String {
    format!(
        "theme t:\n  \
         slot wall -> @cobblestone\n  \
         slot glass -> @glass_pane\n\n\
         struct s size={width}x{depth}\n  \
         walls mat_slot=wall height=3\n  \
         {window}\n",
    )
}

/// Every finding of `out` as its code, the text it points at, its primary and its notes.
fn findings<'a>(
    src: &'a str,
    out: &'a BlockArrayIr,
) -> Vec<(DiagnosticCode, &'a str, &'a str, Vec<&'a str>)> {
    out.diagnostics
        .iter()
        .map(|d| (d.code, slice(src, d), d.primary.as_str(), notes(d)))
        .collect()
}

/// One character per cell: `#` cobblestone, `g` glass pane, `.` air.
fn glyph(array: &BlockArray, x: u32, y: u32, z: u32) -> char {
    match state_at(array, x, y, z).id.as_str() {
        "minecraft:cobblestone" => '#',
        "minecraft:glass_pane" => 'g',
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

/// Row `y` of the `left` wall (`-x`), from low `z`.
fn left_row(array: &BlockArray, y: u32) -> String {
    (0..array.dims.z).map(|z| glyph(array, 0, y, z)).collect()
}

#[test]
fn a_mirror_beginning_where_its_window_ends_is_cut_as_a_pair() {
    // The mirror is x in [3,4), the window x in [2,3).
    let window = "window side=front offset=2 y=1 size=1x1 sym=true mat_slot=glass";
    let src = struct_with_window(6, 5, window);
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), []);
    let ba = only_structure(&out);
    assert_eq!(front_row(ba, 1), "##gg##");
    assert_eq!(front_row(ba, 2), "######");
}

#[test]
fn a_mirror_ending_where_its_window_begins_is_cut_as_a_pair() {
    // The mirror is x in [2,3), the window x in [3,4).
    let window = "window side=front offset=3 y=1 size=1x1 sym=true mat_slot=glass";
    let src = struct_with_window(6, 5, window);
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), []);
    let ba = only_structure(&out);
    assert_eq!(front_row(ba, 1), "##gg##");
    assert_eq!(front_row(ba, 2), "######");
}

#[test]
fn a_mirror_sharing_cells_with_its_window_is_skipped_with_a_note_on_the_halves() {
    let window = "window side=front offset=1 y=1 size=3x1 sym=true mat_slot=glass";
    let src = struct_with_window(6, 5, window);
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::DeferredMember,
            window,
            "`sym=true` window at offset=1 size=3x1 on the `front` wall would overlap its mirror \
             (wall length=6); the mirror was skipped",
            vec![
                "a `sym=true` window and its mirror are a pair only when the window lies in one \
                 half of the wall (here `offset + size.w <= 3` or `offset >= 3`); move or narrow \
                 the window to cut a pair, or drop `sym=true` to keep the one opening that was cut"
            ],
        )],
    );
    assert_eq!(front_row(only_structure(&out), 1), "#ggg##");
}

#[test]
fn a_mirror_on_a_side_wall_is_measured_along_that_wall() {
    // A `9x7` struct's `left` wall is 7 long; read along `x` it would be 9 and mirror to u=5.
    // It is also the only note in this file whose two bounds differ, which takes an odd wall.
    let window = "window side=left offset=3 y=1 size=1x1 sym=true mat_slot=glass";
    let src = struct_with_window(9, 7, window);
    let out = lowered(&src);
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::DeferredMember,
            window,
            "`sym=true` window at offset=3 size=1x1 on the `left` wall coincides with its mirror \
             (wall length=7); the mirror was skipped",
            vec![
                "a `sym=true` window and its mirror are a pair only when the window lies in one \
                 half of the wall (here `offset + size.w <= 3` or `offset >= 4`); move or narrow \
                 the window to cut a pair, or drop `sym=true` to keep the one opening that was cut"
            ],
        )],
    );
    assert_eq!(left_row(only_structure(&out), 1), "###g###");
}

#[test]
fn a_wall_one_long_offers_only_dropping_sym() {
    let window = "window side=front offset=0 y=1 size=1x1 sym=true mat_slot=glass";
    let src = struct_with_window(1, 5, window);
    let out = lowered(&src);
    let all: Vec<_> = out.diagnostics.iter().flat_map(notes).collect();
    assert_eq!(
        all,
        [
            "a wall 1 long holds no `sym=true` pair; drop `sym=true` to keep the one opening that \
             was cut"
        ],
    );
    assert_eq!(front_row(only_structure(&out), 1), "g");
}

#[test]
fn a_window_refused_outright_carries_the_generic_deferral_note() {
    let window = "window side=front offset=0 y=1 size=1x1 repeat=0 mat_slot=glass";
    let src = struct_with_window(6, 5, window);
    let out = lowered(&src);
    let all: Vec<_> = out.diagnostics.iter().flat_map(notes).collect();
    assert_eq!(
        all,
        [
            "block-array lowering currently voxelises floor, walls, door, window, roof \
             (kind=gable|shed|hip|flat), stair (kind=stairs), pressure_plate \
             (at=<side>.outside|inside.<side>), and level y=N grouping, and recognises circuit \
             region=<label> void=<N> (u32, N>=1) plus door[id=<name>] opened_by=sig.<name> \
             actuator patches; other roles will be added as their lowering rules are spec'd"
        ],
    );
}
