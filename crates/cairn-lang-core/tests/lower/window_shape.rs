//! A window's `shape=arch` leaves the two ends of each rectangle's top row to the wall
//! (`spec/syntax` "Selectors").

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::block_array::{BlockArray, BlockArrayIr};

use crate::common::{Finding, findings, lowered, only_structure, same_build, state_at};

/// A 9x5 struct with 4-row cobblestone walls and `window` in it; a `class=framed` window takes a
/// spruce frame.
fn struct_with_window(window: &str) -> String {
    format!(
        "theme t:\n  \
         slot wall -> @cobblestone\n  \
         slot glass -> @glass_pane\n  \
         window[class=framed] -> frame=@spruce_planks\n\n\
         struct s size=9x5\n  \
         walls mat_slot=wall height=4\n  \
         {window}\n",
    )
}

/// One character per cell: `#` cobblestone, `g` glass pane, `f` spruce planks, `.` air.
fn glyph(array: &BlockArray, x: u32, y: u32, z: u32) -> char {
    match state_at(array, x, y, z).id.as_str() {
        "minecraft:cobblestone" => '#',
        "minecraft:glass_pane" => 'g',
        "minecraft:spruce_planks" => 'f',
        "minecraft:air" => '.',
        other => panic!("unexpected block {other} at ({x}, {y}, {z})"),
    }
}

/// Rows 1 to 4 of the `front` wall (`+z`), from the top, each from low `x`.
fn front(array: &BlockArray) -> [String; 4] {
    [4, 3, 2, 1].map(|y| {
        (0..array.dims.x)
            .map(|x| glyph(array, x, y, array.dims.z - 1))
            .collect()
    })
}

/// The front wall `window` cuts, which it must cut with no finding.
fn cut(window: &str) -> [String; 4] {
    let src = struct_with_window(window);
    let out = lowered(&src);
    assert_eq!(findings(&src, &out), Vec::<Finding<'_>>::new(), "{window}");
    front(only_structure(&out))
}

/// A finding as code, text pointed at, primary and notes, owned.
type Owned = (DiagnosticCode, String, String, Vec<String>);

/// Each finding of `out` as [`Owned`].
fn owned(src: &str, out: &BlockArrayIr) -> Vec<Owned> {
    findings(src, out)
        .into_iter()
        .map(|(code, at, primary, notes)| {
            (
                code,
                at.to_owned(),
                primary.to_owned(),
                notes.into_iter().map(str::to_owned).collect(),
            )
        })
        .collect()
}

/// The findings `window` raises, which must cut nothing.
fn refused(window: &str) -> Vec<Owned> {
    let src = struct_with_window(window);
    let out = lowered(&src);
    assert!(
        same_build(&out, &lowered(&struct_with_window(""))),
        "{window}"
    );
    owned(&src, &out)
}

#[test]
fn an_arched_window_leaves_the_ends_of_its_top_row_to_the_wall() {
    assert_eq!(
        cut("window side=front y=2 offset=2 size=3x2 shape=arch mat_slot=glass"),
        ["#########", "###g#####", "##ggg####", "#########"],
    );
}

#[test]
fn a_window_without_a_shape_is_cut_as_rect() {
    let rect = ["#########", "##ggg####", "##ggg####", "#########"];
    assert_eq!(
        cut("window side=front y=2 offset=2 size=3x2 mat_slot=glass"),
        rect
    );
    assert_eq!(
        cut("window side=front y=2 offset=2 size=3x2 shape=rect mat_slot=glass"),
        rect
    );
}

#[test]
fn a_series_and_a_mirror_round_each_rectangle_they_cut() {
    let rounded = ["#########", "##g###g##", "#ggg#ggg#", "#########"];
    assert_eq!(
        cut("window side=front y=2 offset=1 size=3x2 repeat=2 step=4 shape=arch mat_slot=glass"),
        rounded,
    );
    assert_eq!(
        cut("window side=front y=2 offset=1 size=3x2 sym=true shape=arch mat_slot=glass"),
        rounded,
    );
}

#[test]
fn a_frame_round_an_arched_window_leaves_the_two_cells_to_the_wall() {
    assert_eq!(
        cut("window class=framed side=front y=2 offset=2 size=3x2 shape=arch mat_slot=glass"),
        ["#fffff###", "#f#g#f###", "#fgggf###", "#fffff###"],
    );
}

#[test]
fn an_arched_window_too_small_to_round_is_refused() {
    let note = "an arch leaves the two ends of its top row to the wall, so it needs a cell between \
                them and a row under them; widen or raise `size=`, or drop `shape=arch`";
    for size in ["2x2", "3x1"] {
        let line = format!("window side=front y=2 offset=2 size={size} shape=arch mat_slot=glass");
        assert_eq!(
            refused(&line),
            [(
                DiagnosticCode::InvalidMember,
                line.clone(),
                format!(
                    "window `shape=arch` needs a `size=` at least 3 wide and 2 high, not {size}"
                ),
                vec![note.to_owned()],
            )],
        );
    }
}

#[test]
fn a_shape_that_is_neither_rect_nor_arch_is_refused() {
    let line = "window side=front y=2 offset=2 size=3x2 shape=slit mat_slot=glass";
    assert_eq!(
        refused(line),
        [(
            DiagnosticCode::InvalidMember,
            line.to_owned(),
            "unknown window `shape=slit`".to_owned(),
            vec!["`shape=` takes rect or arch".to_owned()],
        )],
    );
}

#[test]
fn a_shape_that_is_no_identifier_is_ignored_and_the_window_cut_as_rect() {
    let src = struct_with_window("window side=front y=2 offset=2 size=3x2 shape=1 mat_slot=glass");
    let out = lowered(&src);
    assert_eq!(
        owned(&src, &out),
        [(
            DiagnosticCode::IgnoredArgument,
            "1".to_owned(),
            "`shape=` must be `rect` or `arch`, not integer `1`; the value was ignored".to_owned(),
            vec!["the window is cut as `rect`, as with no `shape=`".to_owned()],
        )],
    );
    assert_eq!(
        front(only_structure(&out)),
        ["#########", "##ggg####", "##ggg####", "#########"],
    );
}
