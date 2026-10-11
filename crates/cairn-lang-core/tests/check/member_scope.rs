//! Acceptance tests for the `member_scope` pass of
//! `cairn_lang_core::check`.
//!
//! `intent::role_of` is one global table, so every keyword classifies to
//! its role in every body. Nothing downstream puts the distinction back:
//! `block_array` buckets the geometry roles and the site passes match
//! `place` / `connect`, so a keyword written into the wrong body falls off
//! the end of both.
//!
//! The two halves failed differently, which is why both are covered here.
//! A `place` in a `def` body at least earned a `W_DEFERRED_MEMBER` during
//! lowering — invisible to `cairn check`, which does not lower, but
//! visible to `cairn lower`. A `floor` among a site's placements had no
//! reporter at either stage: the resolver's loop and the lowering loop
//! both `continue` past a non-`place` row without a word, so the member
//! produced no voxels and no diagnostic anywhere.

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

/// The bug as reported: `place` and `connect` written into a `struct` or
/// `def` body. `cairn check` exited 0 on this source.
#[test]
fn ms_1_a_site_row_in_a_geometry_body_is_an_error() {
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

/// The half with no reporter at all: a geometry keyword among a site's
/// placements produced no voxels and no diagnostic at either stage.
#[test]
fn ms_2_a_geometry_row_in_a_site_body_is_an_error() {
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

/// The negative space. Without this, a pass that reported every member
/// would satisfy the two tests above.
#[test]
fn ms_3_a_row_the_body_reads_is_not_reported() {
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

/// A `level y=N` groups members inside the geometry body it already sits
/// in — `flatten_members` splices its children into the same phase
/// buckets — so nesting does not open a body with different rules.
#[test]
fn ms_4_the_body_kind_carries_through_a_level() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  level y=0\n    \
         place id=extra use=hut theme=plain at=origin\n"
    );
    let d = one_of_code(&src, DiagnosticCode::MisplacedMember);
    assert!(d.primary.contains("lowers `place`"), "got {:?}", d.primary);
}

/// One finding per misplaced subtree. Everything indented under a row
/// this body cannot read goes with it, so descending would bill one
/// mistake once per line underneath.
#[test]
fn ms_5_a_subtree_is_reported_once_at_its_root() {
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

/// `nesting` and `member_scope` do not both fire on one row. The nesting
/// advice ("dedent these") does not fit a row that cannot be in this body
/// at any indentation.
#[test]
fn ms_6_nesting_does_not_also_report_a_misplaced_row() {
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

/// An unknown keyword is `keyword_allowlist`'s finding in either body:
/// the repair is the word, and there is no role to measure against the
/// body's readers.
#[test]
fn ms_7_an_unknown_keyword_is_left_to_the_allowlist() {
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

/// `logic` and `assert` lines are not members — `intent::lower` sorts
/// them into `MemberBody`'s own fields, so they never reach this walk —
/// and the pass must not touch them.
#[test]
fn ms_8_logic_and_assert_lines_are_not_members() {
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

/// The advice for a `level` in a `site` body is written separately: its
/// children are usually `place` rows that belong exactly where they are,
/// one indent to the left, and the generic "move it into a struct"
/// advice would cost the author the placement.
#[test]
fn ms_9_a_level_in_a_site_body_says_dedent_rather_than_relocate() {
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

/// The "expected one of" note is the closed set for the body the author
/// is actually in, not the whole keyword table.
#[test]
fn ms_10_the_candidate_list_is_scoped_to_the_body() {
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

/// The span underlines the offending row, not the whole body. A renderer
/// pointing at the `struct` header would send the author to a line that
/// is correct.
#[test]
fn ms_11_the_span_underlines_the_offending_row() {
    let src = site_with("floor id=stray mat_slot=floor");
    let d = one_of_code(&src, DiagnosticCode::MisplacedMember);
    assert_eq!(&src[d.span.clone()], "floor id=stray mat_slot=floor");
}

/// The `def` loop is a separate `run` arm from the `struct` loop, and a
/// `def` is what a `site` instantiates, so the geometry half of this code
/// reaches users through it first. Deleting the loop used to leave the
/// whole crate green.
#[test]
fn ms_12_the_def_loop_is_walked() {
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

/// "Produces nothing" is about the block array. `redstone::synth`'s
/// `collect_member` is role-agnostic and its `lower_site` walks a site's
/// rows, so a `-> sig.NAME` tail on a misplaced row does still reach
/// synthesis — the message says so rather than overclaiming, and says why
/// that is not a reason to keep the row where it is.
#[test]
fn ms_13_a_signal_bearing_row_is_not_told_it_reaches_nothing() {
    let hedge = "the `sig.` reference on this line does still reach redstone synthesis — but the block it would sense or drive is the one being dropped, so move the member and the signal keeps working";
    let sensor = one_of_code(
        &site_with("pressure_plate id=gate at=front.outside -> sig.step"),
        DiagnosticCode::MisplacedMember,
    );
    assert!(notes(&sensor).contains(&hedge), "got {:#?}", notes(&sensor));

    // An actuator argument counts too, and a row with no signal at all
    // must not carry the note — otherwise it would be noise on every
    // finding rather than a correction on the ones that need it.
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

/// The lost-member count covers the whole subtree. Counting direct
/// children only would report "the 1 member indented under it" for a
/// three-deep run — understating the cost by exactly the part the author
/// cannot see from the reported line.
#[test]
fn ms_14_the_nested_count_covers_the_whole_subtree() {
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

/// The other half of `ms_6`: a row this body *does* read, whose indented
/// body `nesting` is about to declare lost. Descending into it would bill
/// one mistake twice with repairs that disagree — following nesting's
/// "dedent these" leaves the `E_MISPLACED_MEMBER` standing.
#[test]
fn ms_15_a_child_of_a_lost_body_is_left_to_nesting() {
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

/// A subtree hanging off an unknown keyword is skipped by this pass, as
/// it is by `nesting`: the row names nothing the compiler builds, so the
/// repair is the word and everything below it is re-judged afterwards.
#[test]
fn ms_16_a_subtree_under_an_unknown_keyword_is_left_alone() {
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

/// The two row tables above are hand-written, and a new `MemberRole` would
/// shrink the matrix without failing anything. Their combined length is the
/// table's, so an addition has to land in one of them.
#[test]
fn ms_17_the_row_tables_cover_every_known_keyword() {
    assert_eq!(
        GEOMETRY_ROWS.len() + SITE_ROWS.len(),
        cairn_lang_core::intent::known_keywords().len(),
        "a keyword was added to the role table without a row here, so the \
         matrix these tests walk is no longer the whole table",
    );
}

mod lowering {
    //! What a misplaced member actually costs, measured on the build rather
    //! than on the diagnostic.
    //!
    //! `check_member_scope.rs` pins the wording and the anchoring of
    //! `E_MISPLACED_MEMBER`; every assertion there filters to that code and
    //! never lowers, so none of them would notice if the premise — "nothing
    //! in this body reads that keyword" — stopped being true. These do the
    //! lowering and compare against the same source with the row moved to
    //! the body that does read it.

    use cairn_lang_core::block_array::{BlockArray, BlockArrayIr, lower_to_block_array};
    use cairn_lang_core::{lower, parse, resolve};

    use crate::common::PRELUDE;

    fn build(source: &str) -> BlockArrayIr {
        let module = parse(source).expect("parse");
        let ir = lower(&module);
        let resolution = resolve(&ir, None);
        lower_to_block_array(&ir, &resolution, None)
    }

    fn solid_cells(array: &BlockArray) -> usize {
        array.voxels.iter().filter(|c| c.0 != 0).count()
    }

    /// A geometry row among a site's placements lays no voxels. Same source,
    /// same site, one row apart — and the site's own placement is unchanged,
    /// so the row contributed nothing rather than displacing something.
    #[test]
    fn msl_1_a_geometry_row_in_a_site_body_builds_nothing() {
        let without =
            format!("{PRELUDE}site s:\n  place id=anchor use=hut theme=plain at=origin\n");
        let with = format!(
            "{PRELUDE}site s:\n  place id=anchor use=hut theme=plain at=origin\n  \
         floor id=stray mat_slot=floor\n  \
         walls id=stray_walls mat_slot=wall height=3\n"
        );
        let base = build(&without);
        let extended = build(&with);
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

    /// A `place` in a `def` body produces no placement, and a `connect`
    /// there lays no walkway. Both are what `W_DEFERRED_MEMBER` says during
    /// `cairn lower` — the code `cairn check` never sees, which is why the
    /// same fact needs a check-layer owner.
    #[test]
    fn msl_2_a_site_row_in_a_geometry_body_builds_nothing() {
        let src = format!(
            "{PRELUDE}def lodge size=5x5:\n  floor id=floor mat_slot=floor\n  \
         place id=inner use=hut theme=plain at=origin\n  \
         connect inner.entry to inner.entry path=@gravel\n\n\
         site s:\n  place id=anchor use=lodge theme=plain at=origin\n"
        );
        let built = build(&src);
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

    /// The other half of the message's claim: a `place` in a geometry body
    /// is dropped from the build, not from resolution. The struct's own
    /// members still resolve around it, so the finding is about the one row.
    #[test]
    fn msl_3_a_misplaced_row_does_not_disturb_its_siblings() {
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
            solid_cells(&build(&src).structures["struct::s"]),
            solid_cells(&build(&clean).structures["struct::s"]),
            "the misplaced row is the only thing lost",
        );
    }
}
