//! A `connect` row whose port cannot be placed names the one rule it broke and points at the
//! member it is about.

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::{Diagnostic, DiagnosticCode, DiagnosticNote};

use crate::common::lowered;

/// A theme, a `hut` whose front door `e` anchors any walkway, and a `probe` def holding `p`.
fn site(probe_members: &str, gap: u32) -> String {
    format!(
        "theme t:\n  \
         slot wall -> @cobblestone\n  \
         slot glass -> @glass_pane\n  \
         slot eave -> @spruce_stairs\n  \
         slot gravel -> @gravel\n\n\
         def hut size=5x5:\n  \
         walls id=w mat_slot=wall height=3\n  \
         door id=e side=front at=center\n\n\
         def probe size=5x5:\n\
         {probe_members}\n\
         site s:\n  \
         place id=a use=probe theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap={gap}\n  \
         connect a.p to b.e path=@gravel\n",
    )
}

/// The one walkway defer `out` carries: the `W_DEFERRED_MEMBER` on the `connect` row of `src`.
fn skip<'a>(src: &str, out: &'a BlockArrayIr) -> &'a Diagnostic {
    let skips: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == DiagnosticCode::DeferredMember && src[d.span.clone()].starts_with("connect ")
        })
        .collect();
    assert_eq!(skips.len(), 1, "one walkway defer: {:#?}", out.diagnostics);
    assert!(out.walkways.is_empty(), "and no strip is laid");
    let primary = &skips[0].primary;
    assert!(
        primary.starts_with("walkway `") && primary.contains("` was skipped because port"),
        "the defer names the walkway and why it was skipped: {primary}",
    );
    skips[0]
}

/// The notes of the one walkway defer `out` carries.
fn skip_notes<'a>(src: &str, out: &'a BlockArrayIr) -> Vec<&'a DiagnosticNote> {
    skip(src, out).notes.iter().collect()
}

/// The suffix a note carries when the member's own line defers too.
const OWN_LINE: &str = "; the member says so on its own line too";

/// Asserts the member's own line really carries the deferral `note` claims it does.
fn assert_own_line_holds(out: &BlockArrayIr, note: &DiagnosticNote) {
    if !note.message.ends_with(OWN_LINE) {
        return;
    }
    let span = note
        .span
        .clone()
        .expect("a note naming a line points at it");
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::DeferredMember
                && d.span == span
                && !d.primary.starts_with("walkway `")),
        "the note says the member's line defers too, and nothing is there: {:#?}",
        out.diagnostics,
    );
}

/// Every contract a port has, none of which is the reason for any case here.
const CHECKLIST: [&str; 4] = [
    "a `door` port requires",
    "a `window` port requires",
    "both roles are cut into masonry",
    "cannot anchor a port yet",
];

/// Asserts the row's one note begins with `reason` and points at the line starting `member_line`.
fn assert_reason(probe_members: &str, gap: u32, reason: &str, member_line: Option<&str>) {
    let src = site(probe_members, gap);
    let out = lowered(&src);
    let notes = skip_notes(&src, &out);
    assert_eq!(notes.len(), 1, "one note for the one port: {notes:#?}");
    let note = notes[0];
    assert_own_line_holds(&out, note);
    assert!(
        note.message.starts_with(reason),
        "expected the note to begin {reason:?}, got {:?}",
        note.message,
    );
    for item in CHECKLIST {
        assert!(
            !note.message.contains(item),
            "the note restates the old checklist: {:?}",
            note.message,
        );
    }
    match (member_line, &note.span) {
        (Some(line), Some(span)) => assert!(
            src[span.clone()].starts_with(line),
            "the note should point at `{line}`, points at {:?}",
            &src[span.clone()],
        ),
        (None, None) => {}
        (expected, got) => panic!("note span: expected {expected:?}, got {got:?}"),
    }
}

const WALLS: &str = "  walls id=w mat_slot=wall height=3\n";

#[test]
fn a_stair_is_refused_for_its_role() {
    assert_reason(
        &format!(
            "{WALLS}  stair id=p kind=stairs side=front mat_slot=eave\n  roof id=r kind=gable mat_slot=eave overhang=1\n"
        ),
        6,
        "`a.p` is a `stair`, and only a `door` or a `window` can anchor a port",
        Some("stair id=p"),
    );
}

#[test]
fn a_side_typo_is_refused_with_what_was_written() {
    assert_reason(
        &format!("{WALLS}  door id=p side=frnt at=center\n"),
        6,
        "`a.p` has `side=frnt`, which is not one of front, back, left, right",
        Some("door id=p"),
    );
}

#[test]
fn a_door_at_typo_is_refused_with_what_was_written() {
    assert_reason(
        &format!("{WALLS}  door id=p side=front at=middle\n"),
        6,
        "`a.p` is a door that has `at=middle`, which is not one of center, left, right",
        Some("door id=p"),
    );
}

#[test]
fn a_door_without_masonry_is_refused_for_the_wall() {
    assert_reason(
        "  door id=p side=front at=center\n",
        6,
        "`a.p` is a door with no wall to open",
        Some("door id=p"),
    );
}

#[test]
fn a_window_without_y_is_refused_for_that_argument() {
    assert_reason(
        &format!("{WALLS}  window id=p side=front offset=1 size=1x1 mat_slot=glass\n"),
        6,
        "`a.p` is a window that has no `y=`",
        Some("window id=p"),
    );
}

#[test]
fn a_window_past_the_wall_end_is_refused_with_its_extent() {
    assert_reason(
        &format!("{WALLS}  window id=p side=front offset=3 y=1 size=3x1 mat_slot=glass\n"),
        6,
        "`a.p` is a window that runs past the end of its wall (`offset + size.w` = 3 + 3, wall length 5)",
        Some("window id=p"),
    );
}

#[test]
fn a_window_outside_the_courses_is_refused_with_its_rows() {
    assert_reason(
        &format!("{WALLS}  window id=p side=front offset=1 y=3 size=1x2 mat_slot=glass\n"),
        6,
        "`a.p` is a window whose rows y=3..=4 are not all inside one wall course (the walls occupy y=1..=3)",
        Some("window id=p"),
    );
}

#[test]
fn a_port_past_the_coordinate_range_is_refused_as_out_of_range() {
    // `b`'s last column is `i32::MAX`, so the cell outside its east door does not fit an `i32`.
    let src = format!(
        "theme t:\n  \
         slot wall -> @cobblestone\n  \
         slot gravel -> @gravel\n\n\
         def hut size=5x5:\n  \
         walls id=w mat_slot=wall height=3\n  \
         door id=e side=right at=center\n\n\
         site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap={}\n  \
         connect a.e to b.e path=@gravel\n",
        i32::MAX - 9,
    );
    let out = lowered(&src);
    let notes = skip_notes(&src, &out);
    assert_eq!(notes.len(), 1, "{notes:#?}");
    assert!(
        notes[0]
            .message
            .starts_with("`b.e` would sit outside the coordinate range"),
        "{:?}",
        notes[0].message,
    );
    assert!(notes[0].span.is_none());
}

#[test]
fn two_refused_ports_get_one_note_each_in_row_order() {
    let src = String::from(
        "theme t:\n  \
         slot wall -> @cobblestone\n  \
         slot gravel -> @gravel\n\n\
         def hut size=5x5:\n  \
         walls id=w mat_slot=wall height=3\n  \
         door id=typo side=frnt at=center\n  \
         door id=mid side=front at=middle\n\n\
         site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t east_of=a gap=6\n  \
         connect a.typo to b.mid path=@gravel\n",
    );
    let out = lowered(&src);
    assert_eq!(
        skip(&src, &out).primary,
        "walkway `a.typo ↔ b.mid` was skipped because ports `a.typo` and `b.mid` could not be placed",
    );
    let notes = skip_notes(&src, &out);
    let messages: Vec<_> = notes.iter().map(|n| n.message.as_str()).collect();
    assert_eq!(messages.len(), 2, "{messages:#?}");
    assert!(
        messages[0].starts_with("`a.typo` has `side=frnt`"),
        "{messages:#?}"
    );
    assert!(
        messages[1].starts_with("`b.mid` is a door that has `at=middle`"),
        "{messages:#?}"
    );
    for note in notes {
        assert_own_line_holds(&out, note);
    }
}

#[test]
fn a_window_without_offset_anchors_a_walkway_as_it_is_cut() {
    let src = site(
        &format!("{WALLS}  window id=p side=front y=1 size=1x1 mat_slot=glass\n"),
        6,
    );
    let out = lowered(&src);
    assert!(
        !out.diagnostics
            .iter()
            .any(|d| d.primary.contains("was skipped because port")),
        "{:#?}",
        out.diagnostics,
    );
    assert_eq!(out.walkways.len(), 1, "the strip is laid");
}

#[test]
fn one_refused_port_is_named_in_the_singular() {
    let src = site(&format!("{WALLS}  door id=p side=frnt at=center\n"), 6);
    let out = lowered(&src);
    assert_eq!(
        skip(&src, &out).primary,
        "walkway `a.p ↔ b.e` was skipped because port `a.p` could not be placed",
    );
}

#[test]
fn a_numeric_door_at_reads_the_same_on_the_row_and_on_the_door() {
    let src = site(&format!("{WALLS}  door id=p side=front at=3\n"), 6);
    let out = lowered(&src);
    let clause =
        "has an `at=` that is not one of center, left, right (numeric offsets are reserved)";
    let notes = skip_notes(&src, &out);
    assert_eq!(notes.len(), 1, "{notes:#?}");
    assert_eq!(
        notes[0].message,
        format!("`a.p` is a door that {clause}{OWN_LINE}")
    );
    let span = notes[0].span.clone().expect("points at the door");
    let own: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| d.span == span)
        .map(|d| d.primary.as_str())
        .collect();
    assert_eq!(
        own,
        [format!("door {clause} — use `at=center | left | right`")]
    );
}

#[test]
fn a_window_the_cut_defers_for_its_repeat_anchors_no_walkway() {
    // The port reads `offset=`, `y=`, `size=`; the cut also defers on `repeat=`, `step=` or `sym=`.
    for (args, own_line) in [
        (
            "offset=0 y=1 size=1x1 repeat=0 mat_slot=glass",
            "window `repeat=0` would stamp no instances; drop the window instead",
        ),
        (
            "offset=0 y=1 size=1x1 repeat=abc mat_slot=glass",
            "`repeat=` must be a non-negative integer that fits in u32",
        ),
        (
            "offset=0 y=1 size=1x1 repeat=3 step=3 mat_slot=glass",
            "window extends beyond the `front` wall (offset=0 size=1x1 repeat=3 step=3, wall length=5)",
        ),
        (
            "offset=1 y=1 size=1x1 repeat=2 step=2 sym=true",
            "window with both `repeat=` and `sym=true` is not yet supported",
        ),
        (
            "offset=0 y=1 size=1x1 repeat=2 mat_slot=glass",
            "window `repeat=` requires a positive `step=` so instances do not overlap",
        ),
        (
            "offset=0 y=1 size=1x1 step=abc mat_slot=glass",
            "`step=` must be a non-negative integer that fits in u32",
        ),
    ] {
        let src = site(&format!("{WALLS}  window id=p side=front {args}\n"), 6);
        let out = lowered(&src);
        let notes = skip_notes(&src, &out);
        assert_eq!(notes.len(), 1, "`{args}`: {notes:#?}");
        assert!(
            notes[0]
                .message
                .starts_with("`a.p` is a window the openings pass did not cut"),
            "`{args}`: {:?}",
            notes[0].message,
        );
        let span = notes[0].span.clone().expect("points at the window");
        assert!(src[span.clone()].starts_with("window id=p"));
        assert!(
            out.diagnostics
                .iter()
                .any(|d| d.span == span && d.primary == own_line),
            "`{args}`: the window's own line says why: {:#?}",
            out.diagnostics,
        );
    }
}

#[test]
fn a_window_whose_material_does_not_resolve_anchors_no_walkway() {
    // Without a registry pack `@wood.dark` lowers to nothing, so the window paints no cell.
    let src = "theme t:\n  \
               slot wall -> @cobblestone\n  \
               slot trim -> @wood.dark\n  \
               slot gravel -> @gravel\n\n\
               def probe size=5x5:\n  \
               walls id=w mat_slot=wall height=3\n  \
               window id=p side=front offset=1 y=1 size=1x2 mat_slot=trim\n\n\
               def hut size=5x5:\n  \
               walls id=w mat_slot=wall height=3\n  \
               door id=e side=front at=center\n\n\
               site s:\n  \
               place id=a use=probe theme=t at=origin\n  \
               place id=b use=hut theme=t east_of=a gap=6\n  \
               connect a.p to b.e path=@gravel\n";
    let out = lowered(src);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::AbstractTokenDeferred),
        "{:#?}",
        out.diagnostics,
    );
    let notes = skip_notes(src, &out);
    assert_eq!(notes.len(), 1, "{notes:#?}");
    assert_eq!(
        notes[0].message,
        "`a.p` is a window the openings pass did not cut, so a strip would end against the \
         wall — the finding on its own line, or on the material its `mat_slot=` names, says why",
    );
}

#[test]
fn a_window_whose_rows_overflow_prints_no_backwards_range() {
    let src = site(
        &format!("{WALLS}  window id=p side=front offset=1 y=4294967295 size=1x2\n"),
        6,
    );
    let out = lowered(&src);
    let notes = skip_notes(&src, &out);
    assert_eq!(notes.len(), 1, "{notes:#?}");
    assert_eq!(
        notes[0].message,
        format!(
            "`a.p` is a window whose 2 rows from y=4294967295 run past the highest row a build \
             can address (the walls occupy y=1..=3){OWN_LINE}"
        ),
    );
    assert!(
        out.diagnostics.iter().any(|d| d.primary
            == "window rows from y=4294967295 run past the highest row a build can address \
                (size=1x2; the walls occupy y=1..=3)"),
        "{:#?}",
        out.diagnostics,
    );
}

#[test]
fn a_port_one_step_past_the_coordinate_range_is_refused_as_out_of_range() {
    // `gap=2147483643` puts `b`'s origin at `z = i32::MIN`, so only the step out of the wall leaves
    // the range.
    let gap = 2_147_483_643_u32;
    let src = format!(
        "theme t:\n  \
         slot wall -> @cobblestone\n  \
         slot gravel -> @gravel\n\n\
         def hut size=5x5:\n  \
         walls id=w mat_slot=wall height=3\n  \
         door id=e side=front at=center\n  \
         door id=n side=back at=center\n\n\
         site s:\n  \
         place id=a use=hut theme=t at=origin\n  \
         place id=b use=hut theme=t north_of=a gap={gap}\n  \
         connect a.e to b.n path=@gravel\n",
    );
    let out = lowered(&src);
    let notes = skip_notes(&src, &out);
    assert_eq!(notes.len(), 1, "{notes:#?}");
    assert!(
        notes[0]
            .message
            .starts_with("`b.n` would sit outside the coordinate range"),
        "{:?}",
        notes[0].message,
    );
}
