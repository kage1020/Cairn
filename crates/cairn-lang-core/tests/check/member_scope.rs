//! `E_MISPLACED_MEMBER`: a keyword in a body with no reader for its role, such as a `place` in a
//! `def`, is an error at the row.

use cairn_lang_core::{DiagnosticCode, Severity};

use crate::common::{PRELUDE, def_with, diagnose, notes, of_code, one_of_code, struct_with};

/// Every keyword the role table knows, each well-formed; `connect` names `site_with`'s places.
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

#[test]
fn the_body_kind_carries_through_a_level() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  level y=0\n    \
         place id=extra use=hut theme=plain at=origin\n"
    );
    let d = one_of_code(&src, DiagnosticCode::MisplacedMember);
    assert!(d.primary.contains("lowers `place`"), "got {:?}", d.primary);
}

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

/// `intent::lower` sorts them into `MemberBody`'s own fields, so they never reach this walk.
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

#[test]
fn the_span_underlines_the_offending_row() {
    let src = site_with("floor id=stray mat_slot=floor");
    let d = one_of_code(&src, DiagnosticCode::MisplacedMember);
    assert_eq!(&src[d.span.clone()], "floor id=stray mat_slot=floor");
}

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

/// Redstone synthesis walks a site's rows whatever their role, so a signal tail still reaches it.
#[test]
fn a_signal_bearing_row_is_not_told_it_reaches_nothing() {
    let hedge = "the `sig.` reference on this line does still reach redstone synthesis — but the block it would sense or drive is the one being dropped, so move the member and the signal keeps working";
    let sensor = one_of_code(
        &site_with("pressure_plate id=gate at=front.outside -> sig.step"),
        DiagnosticCode::MisplacedMember,
    );
    assert!(notes(&sensor).contains(&hedge), "got {:#?}", notes(&sensor));

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

#[test]
fn the_row_tables_cover_every_known_keyword() {
    assert_eq!(
        GEOMETRY_ROWS.len() + SITE_ROWS.len(),
        cairn_lang_core::intent::known_keywords().len(),
        "a keyword was added to the role table without a row here, so the \
         matrix these tests walk is no longer the whole table",
    );
}

mod circuit_under_a_level {
    //! A `circuit` line under a `level`, at any depth, is an error on that line: place-and-route
    //! reads a reservation only from a line among the scope's own members.

    use std::ops::Range;

    use cairn_lang_core::{Diagnostic, Severity};

    use crate::common::{diagnose, notes, nth};

    const PRIMARY: &str = "this `circuit` line reserves nothing: it is written under a `level`, \
                           and only a `circuit` line at the scope's top level is read as a \
                           reservation";

    const BELONGS: &str = "a `circuit` line belongs among the `struct`'s or `def`'s own members: \
                           its reservation spans the scope's `size=WxH`, and no pass reads a \
                           `level`'s `y=` for it";

    const MOVE: &str =
        "move it out of the `level` to the scope's top level; nothing else about it changes";

    const KEEP_ONE: &str = "a scope reserves one region for its redstone: delete this line, or \
                            move it to the scope's top level in place of that one";

    /// A door two plates open in `keyword gatehouse`, then `lines` at the body's top level.
    fn gatehouse(keyword: &str, lines: &str) -> String {
        format!(
            "@cairn 2026.06\n\ntheme entry:\n  slot wall -> @oak_planks\n  slot door -> @oak_door\n\n\
             {keyword} gatehouse size=7x5\n  floor mat_slot=wall\n  \
             walls class=outer mat_slot=wall height=3\n  door  id=front side=front at=center\n  \
             pressure_plate id=plate at=front.outside offset=0 y=0 -> sig.step\n  \
             pressure_plate id=inner at=inside.front  offset=1 y=0 -> sig.exit\n  \
             logic sig.open = sig.step or sig.exit\n  door[id=front] opened_by=sig.open\n{lines}"
        )
    }

    /// The byte range of the `n`th (0-based) occurrence of `text` in `source`.
    fn span_of(source: &str, text: &str, n: usize) -> Range<usize> {
        let start = nth(source, text, n);
        start..start + text.len()
    }

    fn found(diags: &[Diagnostic]) -> Vec<(&'static str, Range<usize>)> {
        diags
            .iter()
            .map(|d| (d.code.as_str(), d.span.clone()))
            .collect()
    }

    fn note_spans(diag: &Diagnostic) -> Vec<Option<Range<usize>>> {
        diag.notes.iter().map(|n| n.span.clone()).collect()
    }

    #[test]
    fn a_line_under_a_level_beside_the_scopes_own_is_refused_and_pointed_at_it() {
        let src = gatehouse(
            "struct",
            "  circuit region=floor void=2\n  level y=1\n    circuit region=basement void=3\n",
        );
        let diags = diagnose(&src);
        assert_eq!(
            found(&diags),
            [(
                "E_MISPLACED_MEMBER",
                span_of(&src, "circuit region=basement void=3", 0),
            )],
            "{diags:#?}",
        );
        let d = &diags[0];
        assert_eq!(d.severity(), Severity::Error);
        assert_eq!(d.primary, PRIMARY);
        assert_eq!(
            notes(d),
            [BELONGS, "the scope's own `circuit` line is here", KEEP_ONE],
        );
        assert_eq!(
            note_spans(d),
            [
                None,
                Some(span_of(&src, "circuit region=floor void=2", 0)),
                None,
            ],
        );
    }

    #[test]
    fn a_line_only_under_a_level_is_refused_with_the_move_that_repairs_it() {
        let src = gatehouse(
            "struct",
            "  level y=1\n    circuit region=basement void=3\n",
        );
        let diags = diagnose(&src);
        assert_eq!(
            found(&diags),
            [(
                "E_MISPLACED_MEMBER",
                span_of(&src, "circuit region=basement void=3", 0),
            )],
            "{diags:#?}",
        );
        let d = &diags[0];
        assert_eq!(d.severity(), Severity::Error);
        assert_eq!(d.primary, PRIMARY);
        assert_eq!(notes(d), [BELONGS, MOVE]);
        assert_eq!(note_spans(d), [None, None]);
    }

    /// The `def` loop is walked as well; nothing places the `def`, hence `W_UNUSED_DEF`.
    #[test]
    fn a_line_under_a_level_in_a_def_is_refused() {
        let src = gatehouse("def", "  level y=1\n    circuit region=basement void=3\n");
        let line = span_of(&src, "circuit region=basement void=3", 0);
        let diags = diagnose(&src);
        assert_eq!(
            found(&diags),
            [
                ("W_UNUSED_DEF", nth(&src, "def gatehouse", 0)..line.end),
                ("E_MISPLACED_MEMBER", line),
            ],
            "{diags:#?}",
        );
        assert_eq!(diags[1].primary, PRIMARY);
        assert_eq!(notes(&diags[1]), [BELONGS, MOVE]);
    }

    /// A `level` that groups nothing is `E_UNSUPPORTED_NESTING` as well, about the blocks its
    /// members lose; a `circuit` line writes none, and what it loses is reported on the line.
    #[test]
    fn a_line_under_a_level_at_any_depth_is_refused_on_that_line() {
        let circuit = "circuit region=attic void=1";
        let level_in_level = format!("  level y=1\n    level y=2\n      {circuit}\n");
        let level_without_offset = format!("  level\n    {circuit}\n");
        let three_deep = format!(
            "  level y=1\n    level y=2\n      walls mat_slot=m height=2\n      level y=3\n        \
             {circuit}\n"
        );
        for (levels, lost_from) in [
            (level_in_level, circuit),
            (level_without_offset, circuit),
            (three_deep, "walls mat_slot=m height=2"),
        ] {
            let src = format!("struct s size=5x5\n  floor mat_slot=m\n{levels}");
            let line = span_of(&src, circuit, 0);
            let lost = nth(&src, lost_from, 0)..line.end;
            let misplaced = ("E_MISPLACED_MEMBER", line.clone());
            let nesting = ("E_UNSUPPORTED_NESTING", lost.clone());
            let expected = if lost == line {
                [misplaced, nesting]
            } else {
                [nesting, misplaced]
            };
            let diags = diagnose(&src);
            assert_eq!(found(&diags), expected, "{src}");
            let d = diags
                .iter()
                .find(|d| d.code.as_str() == "E_MISPLACED_MEMBER")
                .expect("found above");
            assert_eq!(d.primary, PRIMARY, "{src}");
            assert_eq!(notes(d), [BELONGS, MOVE], "{src}");
        }
    }

    #[test]
    fn a_line_among_the_scopes_own_members_beside_a_level_is_not_reported() {
        let src = gatehouse(
            "struct",
            "  circuit region=floor void=2\n  level y=1\n    walls mat_slot=wall height=2\n",
        );
        let diags = diagnose(&src);
        assert!(diags.is_empty(), "{diags:#?}");
    }
}

mod lowering {
    //! What a misplaced member costs, measured by lowering each source beside the same source
    //! without the row.

    use crate::common::{PRELUDE, lowered, solid_cells};

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

    /// `W_DEFERRED_MEMBER` says so during `cairn lower`, which `cairn check` never runs.
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
