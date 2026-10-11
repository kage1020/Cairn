//! `E_MISPLACED_MEMBER`: a keyword written into a body that has no reader for
//! its role, such as a `place` in a `def` or a `floor` among a site's
//! placements, is an error at the row, because every loop that would build it
//! passes it by without a word.

use cairn_lang_core::{DiagnosticCode, Severity};

use crate::common::{PRELUDE, def_with, diagnose, notes, of_code, one_of_code, struct_with};

/// Every keyword the role table knows, with a well-formed argument list
/// for each so the finding under test is the only thing wrong with the
/// line. `connect` names the two places `site_with` declares.
const GEOMETRY_ROWS: &[&str] = &[
    "floor mat_slot=floor",
    "walls mat_slot=wall height=3",
    "door id=d side=front at=center",
    "window id=w side=front y=1 offset=1 size=1x1",
    "roof kind=flat mat_slot=wall",
    "stair kind=stairs mat_slot=wall",
    "level y=0",
    "pressure_plate id=p at=front.outside",
    "circuit region=floor void=1",
];

const SITE_ROWS: &[&str] = &[
    "place id=extra use=hut theme=plain at=origin",
    "connect anchor.entry to peer.entry path=@gravel",
];

fn site_with(row: &str) -> String {
    format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n  {row}\n"
    )
}

#[test]
fn a_site_row_in_a_geometry_body_is_an_error() {
    for row in SITE_ROWS {
        for src in [struct_with(row), def_with(row)] {
            let d = one_of_code(&src, DiagnosticCode::MisplacedMember);
            assert_eq!(d.severity(), Severity::Error, "for `{row}`");
            assert!(
                d.primary
                    .starts_with("nothing in a `struct` or `def` body lowers `"),
                "for `{row}`, got {:?}",
                d.primary,
            );
        }
    }
}

#[test]
fn a_geometry_row_in_a_site_body_is_an_error() {
    for row in GEOMETRY_ROWS {
        let src = site_with(row);
        let d = one_of_code(&src, DiagnosticCode::MisplacedMember);
        assert_eq!(d.severity(), Severity::Error, "for `{row}`");
        assert!(
            d.primary.starts_with("nothing in a `site` body lowers `"),
            "for `{row}`, got {:?}",
            d.primary,
        );
    }
}

/// Without this, a pass that reported every member would pass the two
/// tests above.
#[test]
fn a_row_the_body_reads_is_not_reported() {
    for row in GEOMETRY_ROWS {
        for src in [struct_with(row), def_with(row)] {
            assert!(
                of_code(&src, DiagnosticCode::MisplacedMember).is_empty(),
                "`{row}` belongs in a geometry body, got {:#?}",
                of_code(&src, DiagnosticCode::MisplacedMember),
            );
        }
    }
    for row in SITE_ROWS {
        let src = site_with(row);
        assert!(
            of_code(&src, DiagnosticCode::MisplacedMember).is_empty(),
            "`{row}` belongs in a `site` body, got {:#?}",
            of_code(&src, DiagnosticCode::MisplacedMember),
        );
    }
}

/// A `level` groups members inside the body it sits in, so nesting does
/// not open a body with different rules.
#[test]
fn the_body_kind_carries_through_a_level() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  level y=0\n    \
         place id=extra use=hut theme=plain at=origin\n"
    );
    let d = one_of_code(&src, DiagnosticCode::MisplacedMember);
    assert!(d.primary.contains("lowers `place`"), "got {:?}", d.primary);
}

/// Everything indented under a row this body cannot read goes with it, so
/// descending would bill one mistake once per line underneath.
#[test]
fn a_subtree_is_reported_once_at_its_root() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         walls mat_slot=wall height=3\n    floor mat_slot=floor\n    \
         door id=d side=front at=center\n"
    );
    let d = one_of_code(&src, DiagnosticCode::MisplacedMember);
    assert!(
        d.primary.contains("lowers `walls`"),
        "the outermost misplaced row is the one reported, got {:?}",
        d.primary,
    );
    assert!(
        notes(&d).contains(&"the 2 members indented under it go with it"),
        "the lost children have to be counted here — `nesting` stays quiet \
         about them precisely because this finding covers them, got {:#?}",
        notes(&d),
    );
}

/// The nesting advice, "dedent these", does not fit a row that cannot be
/// in this body at any indentation.
#[test]
fn nesting_does_not_also_report_a_misplaced_row() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         walls mat_slot=wall height=3\n    floor mat_slot=floor\n"
    );
    let codes: Vec<&str> = diagnose(&src).iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes
            .iter()
            .filter(|c| **c == "E_UNSUPPORTED_NESTING")
            .count(),
        0,
        "got {codes:?}",
    );
}

/// The repair is the word, and there is no role to measure against the
/// body's readers.
#[test]
fn an_unknown_keyword_is_left_to_the_allowlist() {
    for src in [
        struct_with("frame id=f side=front"),
        site_with("frame id=f"),
    ] {
        assert!(
            of_code(&src, DiagnosticCode::MisplacedMember).is_empty(),
            "got {:#?}",
            of_code(&src, DiagnosticCode::MisplacedMember),
        );
        assert!(
            diagnose(&src)
                .iter()
                .any(|d| d.code == DiagnosticCode::UnknownKeyword),
            "the allowlist still owns it",
        );
    }
}

/// `intent::lower` sorts them into `MemberBody`'s own fields, so they never
/// reach this walk.
#[test]
fn logic_and_assert_lines_are_not_members() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         logic sig.open = sig.step\n"
    );
    assert!(
        of_code(&src, DiagnosticCode::MisplacedMember).is_empty(),
        "got {:#?}",
        of_code(&src, DiagnosticCode::MisplacedMember),
    );
}

/// Its children are usually `place` rows that belong one indent to the
/// left, and "move it into a struct" would cost the author the placement.
#[test]
fn a_level_in_a_site_body_says_dedent_rather_than_relocate() {
    let src = format!(
        "{PRELUDE}site duo:\n  level y=0\n    \
         place id=anchor use=hut theme=plain at=origin\n"
    );
    let d = one_of_code(&src, DiagnosticCode::MisplacedMember);
    assert!(
        notes(&d).contains(
            &"a `site` body has no grouping construct: dedent the rows to the site's own indentation and drop the `level`"
        ),
        "got {:#?}",
        notes(&d),
    );
}

#[test]
fn the_candidate_list_is_scoped_to_the_body() {
    let geometry = one_of_code(
        &struct_with("place id=extra use=hut theme=plain at=origin"),
        DiagnosticCode::MisplacedMember,
    );
    assert!(
        notes(&geometry).contains(
            &"expected one of: floor, walls, door, window, roof, stair, level, pressure_plate, circuit"
        ),
        "got {:#?}",
        notes(&geometry),
    );
    let site = one_of_code(
        &site_with("floor mat_slot=floor"),
        DiagnosticCode::MisplacedMember,
    );
    assert!(
        notes(&site).contains(&"expected one of: place, connect"),
        "got {:#?}",
        notes(&site),
    );
}

/// Not the whole body: the header is a line that is correct.
#[test]
fn the_span_underlines_the_offending_row() {
    let src = site_with("floor id=stray mat_slot=floor");
    let d = one_of_code(&src, DiagnosticCode::MisplacedMember);
    assert_eq!(&src[d.span.clone()], "floor id=stray mat_slot=floor");
}

/// A `def` body is walked by a loop of its own.
#[test]
fn the_def_loop_is_walked() {
    let d = one_of_code(
        &def_with("place id=extra use=hut theme=plain at=origin"),
        DiagnosticCode::MisplacedMember,
    );
    assert!(
        d.primary
            .starts_with("nothing in a `struct` or `def` body lowers `place`"),
        "got {:?}",
        d.primary,
    );
}

/// "Produces nothing" is about the block array: redstone synthesis walks a
/// site's rows whatever their role, so a `-> sig.NAME` tail on a misplaced
/// row still reaches it.
#[test]
fn a_signal_bearing_row_is_not_told_it_reaches_nothing() {
    let hedge = "the `sig.` reference on this line does still reach redstone synthesis — but the block it would sense or drive is the one being dropped, so move the member and the signal keeps working";
    let sensor = one_of_code(
        &site_with("pressure_plate id=gate at=front.outside -> sig.step"),
        DiagnosticCode::MisplacedMember,
    );
    assert!(notes(&sensor).contains(&hedge), "got {:#?}", notes(&sensor));

    // A row with no signal must not carry the note, or it would be noise on
    // every finding.
    let actuator = one_of_code(
        &site_with("door[id=entry] opened_by=sig.open"),
        DiagnosticCode::MisplacedMember,
    );
    assert!(
        notes(&actuator).contains(&hedge),
        "got {:#?}",
        notes(&actuator),
    );
    let plain = one_of_code(
        &site_with("floor mat_slot=floor"),
        DiagnosticCode::MisplacedMember,
    );
    assert!(!notes(&plain).contains(&hedge), "got {:#?}", notes(&plain));
}

/// Counting direct children only would understate the cost by exactly the
/// part the author cannot see from the reported line.
#[test]
fn the_nested_count_covers_the_whole_subtree() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         walls mat_slot=wall height=3\n    level y=0\n      floor mat_slot=floor\n"
    );
    let d = one_of_code(&src, DiagnosticCode::MisplacedMember);
    assert!(
        notes(&d).contains(&"the 2 members indented under it go with it"),
        "got {:#?}",
        notes(&d),
    );
}

/// The body reads this row, and `nesting` declares its indented body lost;
/// descending would bill one mistake twice with repairs that disagree.
#[test]
fn a_child_of_a_lost_body_is_left_to_nesting() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n    \
         floor mat_slot=floor\n"
    );
    assert!(
        of_code(&src, DiagnosticCode::MisplacedMember).is_empty(),
        "got {:#?}",
        of_code(&src, DiagnosticCode::MisplacedMember),
    );
    let codes: Vec<&str> = diagnose(&src).iter().map(|d| d.code.as_str()).collect();
    assert_eq!(
        codes
            .iter()
            .filter(|c| **c == "E_UNSUPPORTED_NESTING")
            .count(),
        1,
        "got {codes:?}",
    );
}

/// As in `nesting`: the repair is the word, and everything below it is
/// judged again afterwards.
#[test]
fn a_subtree_under_an_unknown_keyword_is_left_alone() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         bogus id=b\n    floor mat_slot=floor\n"
    );
    assert!(
        of_code(&src, DiagnosticCode::MisplacedMember).is_empty(),
        "got {:#?}",
        of_code(&src, DiagnosticCode::MisplacedMember),
    );
    assert!(
        diagnose(&src)
            .iter()
            .any(|d| d.code == DiagnosticCode::UnknownKeyword),
    );
}

/// The two row tables are hand-written, so a new role has to land in one
/// of them.
#[test]
fn the_row_tables_cover_every_known_keyword() {
    assert_eq!(
        GEOMETRY_ROWS.len() + SITE_ROWS.len(),
        cairn_lang_core::intent::known_keywords().len(),
        "a keyword was added to the role table without a row here, so the \
         matrix these tests walk is no longer the whole table",
    );
}

mod lowering {
    //! What a misplaced member costs, measured on the build: each source is
    //! lowered and compared with the same source without the row, or with the
    //! row in the body that reads it.

    use crate::common::{PRELUDE, lowered, solid_cells};

    /// The site's own placement is unchanged, so the row contributed nothing
    /// rather than displacing something.
    #[test]
    fn a_geometry_row_in_a_site_body_builds_nothing() {
        let without =
            format!("{PRELUDE}site s:\n  place id=anchor use=hut theme=plain at=origin\n");
        let with = format!(
            "{PRELUDE}site s:\n  place id=anchor use=hut theme=plain at=origin\n  \
         floor id=stray mat_slot=floor\n  \
         walls id=stray_walls mat_slot=wall height=3\n"
        );
        let base = lowered(&without);
        let extended = lowered(&with);
        assert_eq!(
            extended.placements.len(),
            base.placements.len(),
            "the stray geometry rows must not become placements",
        );
        assert_eq!(
            extended.structures.len(),
            base.structures.len(),
            "nor structures of their own",
        );
        let anchor = "site::s::anchor";
        assert_eq!(
            extended.placements[anchor].dims, base.placements[anchor].dims,
            "and the placement that is there must be untouched by them",
        );
        assert_eq!(
            solid_cells(&extended.structures[anchor]),
            solid_cells(&base.structures[anchor]),
        );
    }

    /// `W_DEFERRED_MEMBER` says so during `cairn lower`, but `cairn check`
    /// never sees it.
    #[test]
    fn a_site_row_in_a_geometry_body_builds_nothing() {
        let src = format!(
            "{PRELUDE}def lodge size=5x5:\n  floor id=floor mat_slot=floor\n  \
         place id=inner use=hut theme=plain at=origin\n  \
         connect inner.entry to inner.entry path=@gravel\n\n\
         site s:\n  place id=anchor use=lodge theme=plain at=origin\n"
        );
        let built = lowered(&src);
        assert_eq!(
            built.placements.len(),
            1,
            "only the site's own `place` reaches the build",
        );
        assert!(
            built.walkways.is_empty(),
            "the `connect` in the def body lays no walkway",
        );
    }

    /// The row is dropped from the build, not from resolution.
    #[test]
    fn a_misplaced_row_does_not_disturb_its_siblings() {
        let src = format!(
            "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         place id=inner use=hut theme=plain at=origin\n  \
         walls mat_slot=wall height=3\n"
        );
        let clean = format!(
            "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls mat_slot=wall height=3\n"
        );
        assert_eq!(
            solid_cells(&lowered(&src).structures["struct::s"]),
            solid_cells(&lowered(&clean).structures["struct::s"]),
            "the misplaced row is the only thing lost",
        );
    }
}
