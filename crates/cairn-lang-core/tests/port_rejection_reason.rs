//! A `connect` row whose port cannot be placed says which of the port's
//! rules it broke, and only that one.
//!
//! The walkway defer used to carry the same four notes whatever the cause —
//! the door contract, the window contract, the masonry contract and the
//! reserved roles — because the port lookup answered every refusal with one
//! `None`. A `connect` naming a `stair` was told about `at=` and `size=WxH`,
//! and nothing else in the build narrowed it down, since the stair itself
//! lowers clean. Each case below builds one refusal through the whole
//! pipeline and pins that the row names that reason, points at the member
//! it is about, and says nothing about the rest.

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::{DiagnosticCode, DiagnosticNote};

mod common;
use common::lowered;

/// A theme, a `hut` whose front door `e` anchors any walkway, and a
/// `probe` def holding `walls` plus the member under test as `p`.
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

/// The notes of the one walkway defer `out` carries.
fn skip_notes(out: &BlockArrayIr) -> Vec<&DiagnosticNote> {
    let skips: Vec<_> = out
        .diagnostics
        .iter()
        .filter(|d| {
            d.code == DiagnosticCode::DeferredMember
                && d.primary.contains("was skipped because port")
        })
        .collect();
    assert_eq!(skips.len(), 1, "one walkway defer: {:#?}", out.diagnostics);
    assert!(out.walkways.is_empty(), "and no strip is laid");
    skips[0].notes.iter().collect()
}

/// The contracts the note used to list whatever the cause. None of them
/// is the reason for any case here, so none may appear.
const CHECKLIST: [&str; 4] = [
    "a `door` port requires",
    "a `window` port requires",
    "both roles are cut into masonry",
    "cannot anchor a port yet",
];

/// Lower `probe_members`, and assert the row carries exactly one note,
/// that it begins with `reason`, and that it points at the line starting
/// with `member_line` (or at nothing, when `member_line` is `None`).
fn assert_reason(probe_members: &str, gap: u32, reason: &str, member_line: Option<&str>) {
    let src = site(probe_members, gap);
    let out = lowered(&src);
    let notes = skip_notes(&out);
    assert_eq!(notes.len(), 1, "one note for the one port: {notes:#?}");
    let note = notes[0];
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
    // The issue's own case: the stair lowers clean, so this note is the
    // only place the build says what is wrong.
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
        "`a.p` is a door at `at=middle`, which is not one of center, left, right",
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
        "`a.p` is a window without a non-negative integer `y=`",
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
    // `b` is placed so far east that its door does not fit an `i32`. The
    // note has no member to point at: the member is fine, the `place` is
    // what moved it.
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
        i32::MAX - 5,
    );
    let out = lowered(&src);
    let notes = skip_notes(&out);
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
    let notes = skip_notes(&out);
    let messages: Vec<_> = notes.iter().map(|n| n.message.as_str()).collect();
    assert_eq!(messages.len(), 2, "{messages:#?}");
    assert!(
        messages[0].starts_with("`a.typo` has `side=frnt`"),
        "{messages:#?}"
    );
    assert!(
        messages[1].starts_with("`b.mid` is a door at `at=middle`"),
        "{messages:#?}"
    );
}

#[test]
fn a_window_without_offset_anchors_a_walkway_as_it_is_cut() {
    // The openings pass reads an absent `offset=` as `0` and cuts the
    // window; the port refused the same member, so the strip was dropped
    // beside a window that was there.
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
