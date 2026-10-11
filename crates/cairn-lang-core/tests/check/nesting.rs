//! `E_UNSUPPORTED_NESTING`: only a `level y=N` directly in a `struct` or `def` body has a reader
//! for its indented body, so any other indented body is reported.

use cairn_lang_core::{Diagnostic, DiagnosticCode, Severity};

use crate::common::{PRELUDE, diagnose, of_code, one_of_code, slice};

/// The advice note, the one without a span; the `declared here` note quotes the same keyword.
fn advice(diag: &Diagnostic) -> &str {
    diag.notes.iter().find(|n| n.span.is_none()).map_or_else(
        || panic!("no span-less advice note in {diag:#?}"),
        |n| n.message.as_str(),
    )
}

#[test]
fn every_nested_row_in_a_site_body_is_reported() {
    let cases = [
        (
            "place",
            "site duo:\n  place id=anchor use=hut theme=plain at=origin\n    \
             place id=peer use=hut theme=plain east_of=anchor gap=4\n",
        ),
        (
            "connect",
            "site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
             place id=peer use=hut theme=plain east_of=anchor gap=4\n  \
             connect anchor.entry to peer.entry path=@gravel\n    \
             place id=third use=hut theme=plain east_of=peer gap=4\n",
        ),
    ];
    for (keyword, body) in cases {
        let src = format!("{PRELUDE}{body}");
        let d = one_of_code(&src, DiagnosticCode::UnsupportedNesting);
        assert_eq!(d.severity(), Severity::Error);
        assert_eq!(
            d.primary,
            format!(
                "`{keyword}` does not group members: the 1 member indented under it is not part of the site"
            ),
        );
        assert!(
            advice(&d).contains("site"),
            "the advice should name the scope, got: {}",
            advice(&d),
        );
    }
}

/// Dedented by two spaces, the `door` would cut its opening.
#[test]
fn a_member_nested_under_a_geometry_member_is_reported() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n    \
         door id=d side=front at=center\n"
    );
    let d = one_of_code(&src, DiagnosticCode::UnsupportedNesting);
    assert_eq!(d.severity(), Severity::Error);
    assert_eq!(
        d.primary,
        "`walls` does not group members: the 1 member indented under it produces no blocks",
    );
    assert_eq!(slice(&src, &d), "door id=d side=front at=center");
    assert!(
        advice(&d).contains("level"),
        "the advice should point at the one construct that does group members, got: {}",
        advice(&d),
    );
    assert!(
        d.notes.iter().any(|n| n.message.contains("redstone")),
        "a note should say what the members still take part in, got: {:#?}",
        d.notes,
    );
}

#[test]
fn the_rule_reaches_def_bodies_too() {
    let src = format!(
        "{PRELUDE}def lodge size=5x5:\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n    \
         door id=d side=front at=center\n"
    );
    let d = one_of_code(&src, DiagnosticCode::UnsupportedNesting);
    assert_eq!(
        d.primary,
        "`walls` does not group members: the 1 member indented under it produces no blocks",
    );
}

#[test]
fn level_groups_its_children_and_the_walk_continues_inside() {
    let ok = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         level y=0\n    walls class=outer mat_slot=wall height=3\n    \
         door id=d side=front at=center\n"
    );
    assert!(
        of_code(&ok, DiagnosticCode::UnsupportedNesting).is_empty(),
        "got {:#?}",
        of_code(&ok, DiagnosticCode::UnsupportedNesting)
    );

    let nested = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         level y=0\n    walls class=outer mat_slot=wall height=3\n      \
         door id=d side=front at=center\n"
    );
    let d = one_of_code(&nested, DiagnosticCode::UnsupportedNesting);
    assert!(
        d.primary.starts_with("`walls` does not group members"),
        "got: {}",
        d.primary,
    );
}

#[test]
fn level_in_a_site_body_is_reported_as_a_misplaced_row_not_as_nesting() {
    let src = format!(
        "{PRELUDE}site duo:\n  level y=0\n    \
         place id=anchor use=hut theme=plain at=origin\n"
    );
    assert!(
        of_code(&src, DiagnosticCode::UnsupportedNesting).is_empty(),
        "a `site` body has no reader for `level` at all, so the finding is \
         about the row rather than about what is indented under it — \
         reporting both would bill one mistake twice, got {:#?}",
        of_code(&src, DiagnosticCode::UnsupportedNesting),
    );
    assert_eq!(
        diagnose(&src)
            .iter()
            .filter(|d| d.code == DiagnosticCode::MisplacedMember)
            .count(),
        1,
        "`member_scope` owns the row, and its advice is the one that fits: \
         the indented `place` belongs in the site, one indent to the left",
    );
}

/// A `Member`'s own span stops at its header line.
#[test]
fn a_subtree_is_reported_once_at_its_root_and_underlined_whole() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n    \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n      \
         connect anchor.entry to peer.entry path=@gravel\n"
    );
    let d = one_of_code(&src, DiagnosticCode::UnsupportedNesting);
    let text = slice(&src, &d);
    assert!(
        text.starts_with("place id=peer"),
        "the span should start at the outermost dropped member, got {text:?}",
    );
    assert!(
        text.ends_with("path=@gravel"),
        "the span should reach the end of the dropped subtree, got {text:?}",
    );
}

#[test]
fn sibling_nested_members_are_underlined_as_one_run() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n    \
         door id=d side=front at=center\n    \
         window id=w side=front y=1 offset=1 size=1x1\n"
    );
    let d = one_of_code(&src, DiagnosticCode::UnsupportedNesting);
    assert_eq!(
        d.primary,
        "`walls` does not group members: the 2 members indented under it produce no blocks",
    );
    let text = slice(&src, &d);
    assert!(
        text.starts_with("door") && text.ends_with("size=1x1"),
        "the span should cover both members, got {text:?}",
    );
}

/// `check` does not lower, and a `def` no site places is not lowered at all.
#[test]
fn a_level_inside_a_level_is_reported_by_check_not_left_to_lowering() {
    for (label, body) in [
        (
            "struct",
            "struct s size=5x5\n  floor mat_slot=floor\n  \
             level y=0\n    level y=1\n      walls class=outer mat_slot=wall height=3\n",
        ),
        (
            "unplaced def",
            "def lodge size=5x5:\n  floor mat_slot=floor\n  \
             level y=0\n    level y=1\n      walls class=outer mat_slot=wall height=3\n",
        ),
    ] {
        let src = format!("{PRELUDE}{body}");
        let d = one_of_code(&src, DiagnosticCode::UnsupportedNesting);
        assert_eq!(
            d.primary,
            "a `level` inside another `level` does not group members: the 1 member indented under it produces no blocks",
            "{label}",
        );
    }
}

#[test]
fn a_level_without_a_usable_offset_is_reported() {
    // No value position reads a `-`, so `y=-3` fails in the parser.
    for y in ["", " y=abc", " y=99999999999", " y=plain"] {
        let src = format!(
            "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
             level{y}\n    walls class=outer mat_slot=wall height=3\n"
        );
        let d = one_of_code(&src, DiagnosticCode::UnsupportedNesting);
        assert_eq!(
            d.primary,
            "`level` has no `y=` offset to place its children at: the 1 member indented under it produces no blocks",
            "for `level{y}`",
        );
        assert!(advice(&d).contains("y=N"), "for `level{y}`");
    }
}

#[test]
fn an_unknown_keyword_parent_is_left_to_the_allowlist() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         room name=hall\n    walls class=outer mat_slot=wall height=3\n"
    );
    assert!(
        of_code(&src, DiagnosticCode::UnsupportedNesting).is_empty(),
        "got {:#?}",
        of_code(&src, DiagnosticCode::UnsupportedNesting)
    );
    let codes: Vec<&str> = diagnose(&src).iter().map(|d| d.code.as_str()).collect();
    assert!(
        codes.contains(&"E_UNKNOWN_KEYWORD"),
        "the keyword itself must still be reported, got {codes:?}",
    );
}

#[test]
fn a_malformed_nested_connect_earns_both_findings_once() {
    let src = format!(
        "{PRELUDE}site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n  \
         connect anchor.entry to peer.entry path=@gravel\n    \
         connect 1 to 2 path=@gravel\n"
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
    assert_eq!(
        codes.iter().filter(|c| **c == "E_CONNECT_ARITY").count(),
        2,
        "both endpoints of the nested row are still checked, got {codes:?}",
    );
}

#[test]
fn flat_bodies_are_silent() {
    let src = format!(
        "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n  \
         door id=d side=front at=center\n\n\
         site duo:\n  place id=anchor use=hut theme=plain at=origin\n  \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n  \
         connect anchor.entry to peer.entry path=@gravel\n"
    );
    let found = of_code(&src, DiagnosticCode::UnsupportedNesting);
    assert!(found.is_empty(), "got {found:#?}");
}

mod lowering {
    //! What an indented body costs, measured by lowering each source beside its dedented twin.

    use cairn_lang_core::{lower, parse, resolve};

    use crate::common::{PRELUDE, lowered, solid_cells};

    #[test]
    fn a_nested_door_cuts_no_opening() {
        let flat = format!(
            "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n  \
         door id=d side=front at=center\n"
        );
        let nested = format!(
            "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n    \
         door id=d side=front at=center\n"
        );
        let flat_solid = solid_cells(&lowered(&flat).structures["struct::s"]);
        let nested_solid = solid_cells(&lowered(&nested).structures["struct::s"]);
        assert!(
            nested_solid > flat_solid,
            "the nested door should cut nothing, so the wall stays solid: \
         flat={flat_solid} nested={nested_solid}",
        );
    }

    #[test]
    fn a_door_under_a_level_still_cuts_its_opening() {
        let flat = format!(
            "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n  \
         door id=d side=front at=center\n"
        );
        let levelled = format!(
            "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n  \
         level y=0\n    door id=d side=front at=center\n"
        );
        assert_eq!(
            solid_cells(&lowered(&levelled).structures["struct::s"]),
            solid_cells(&lowered(&flat).structures["struct::s"]),
            "a `level` body reaches the build, so the opening is cut either way",
        );
    }

    #[test]
    fn a_nested_site_row_produces_neither_a_placement_nor_a_walkway() {
        let flat = format!(
            "{PRELUDE}site duo:\n  \
         place id=anchor use=hut theme=plain at=origin\n  \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n  \
         connect anchor.entry to peer.entry path=@gravel\n"
        );
        let built = lowered(&flat);
        assert_eq!(built.placements.len(), 2, "baseline: both places build");
        assert_eq!(built.walkways.len(), 1, "baseline: the walkway is laid");

        let nested_place = format!(
            "{PRELUDE}site duo:\n  \
         place id=anchor use=hut theme=plain at=origin\n    \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n"
        );
        assert_eq!(
            lowered(&nested_place).placements.len(),
            1,
            "the nested place must not reach the build",
        );

        let nested_connect = format!(
            "{PRELUDE}site duo:\n  \
         place id=anchor use=hut theme=plain at=origin\n  \
         place id=peer use=hut theme=plain east_of=anchor gap=4\n    \
         connect anchor.entry to peer.entry path=@gravel\n"
        );
        assert_eq!(
            lowered(&nested_connect).walkways.len(),
            0,
            "the nested connect must lay no walkway",
        );
    }

    #[test]
    fn a_nested_geometry_member_still_takes_a_theme_binding() {
        let src = format!(
            "{PRELUDE}struct s size=5x5\n  floor mat_slot=floor\n  \
         walls class=outer mat_slot=wall height=3\n    \
         window id=w side=front offset=1 y=1 size=1x1 mat_slot=wall\n"
        );
        let module = parse(&src).expect("parse");
        let ir = lower(&module);
        let resolution = resolve(&ir, None);
        let scope = &resolution.scopes["struct::s"];
        let nested_window = src
            .find("window id=w")
            .expect("the nested window is in the source");
        let binding = scope
            .members
            .get(&nested_window)
            .expect("the nested member is resolved");
        assert!(
            binding.slot_value.is_some(),
            "the nested member's `mat_slot=` should still resolve through the theme",
        );
    }
}
