use super::*;
use crate::parse;

/// Reports with only the portable count set.
fn synthetic_portability(entries: &[(Edition, u32)]) -> Vec<EditionReport> {
    entries
        .iter()
        .map(|&(edition, portable)| EditionReport {
            edition,
            portable,
            unsupported_entries: Vec::new(),
            degraded_entries: Vec::new(),
            buildable: Vec::new(),
            considered: Vec::new(),
            refusal: None,
            semantic_sensitive: Vec::new(),
        })
        .collect()
}

#[test]
fn registry_compat_min_from_requires_header() {
    let src = "@requires version>=1.20\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
    let m = parse(src).expect("parse");
    let axes = compute_axes(&m, synthetic_portability(&[(Edition::Java, 0)]));
    assert_eq!(axes.registry_compat.min, "1.20");
    assert_eq!(axes.registry_compat.max, "latest");
}

#[test]
fn registry_compat_takes_max_when_multiple_requires_present() {
    let src = "@requires version>=1.20\n@requires version>=1.21\n\nstruct s size=4x4\n  walls mat_slot=wall height=3\n";
    let m = parse(src).expect("parse");
    let axes = compute_axes(&m, synthetic_portability(&[(Edition::Java, 0)]));
    assert_eq!(axes.registry_compat.min, "1.21");
}

#[test]
fn registry_compat_defaults_when_requires_absent() {
    let src = "struct s size=4x4\n  walls mat_slot=wall height=3\n";
    let m = parse(src).expect("parse");
    let axes = compute_axes(&m, synthetic_portability(&[(Edition::Java, 0)]));
    assert_eq!(axes.registry_compat.min, "0.0");
}

#[test]
fn edition_portability_is_forwarded_verbatim_from_caller() {
    let src = "struct s size=4x4\n  walls height=3\n  floor\n  roof kind=gable\n";
    let m = parse(src).expect("parse");
    let axes = compute_axes(
        &m,
        vec![
            EditionReport {
                edition: Edition::Java,
                portable: 3,
                unsupported_entries: Vec::new(),
                degraded_entries: Vec::new(),
                buildable: vec!["1.20.4".to_owned()],
                considered: vec!["1.20.4".to_owned()],
                refusal: None,
                semantic_sensitive: Vec::new(),
            },
            EditionReport {
                edition: Edition::Bedrock,
                portable: 1,
                unsupported_entries: one_entry_per_reason(),
                degraded_entries: vec![DegradedEntry {
                    id: "minecraft:spruce_stairs".to_owned(),
                    states: "facing=north,shape=outer_left".to_owned(),
                    dropped: vec![DroppedIntent {
                        key: "shape".to_owned(),
                        value: "outer_left".to_owned(),
                        mc_versions: vec!["1.21.0".to_owned()],
                        reason: "no stair shape".to_owned(),
                    }],
                }],
                buildable: vec!["1.21.0".to_owned()],
                considered: vec!["1.21.0".to_owned(), "1.21.40".to_owned()],
                refusal: None,
                semantic_sensitive: Vec::new(),
            },
        ],
    );
    assert_eq!(axes.edition_portability.len(), 2);
    assert_eq!(axes.edition_portability[0].edition, Edition::Java);
    assert_eq!(axes.edition_portability[0].portable, 3);
    assert_eq!(axes.edition_portability[0].degraded, 0);
    assert_eq!(axes.edition_portability[0].unsupported, 0);
    assert_eq!(axes.edition_portability[1].edition, Edition::Bedrock);
    assert_eq!(axes.edition_portability[1].portable, 1);
    assert_eq!(axes.edition_portability[1].degraded, 1);
    assert_eq!(axes.edition_portability[1].unsupported, 3);
    assert_eq!(
        axes.edition_portability[1].degraded as usize,
        axes.edition_portability[1].degraded_entries.len(),
    );
    assert_eq!(
        axes.edition_portability[1].unsupported as usize,
        axes.edition_portability[1].unsupported_entries.len(),
    );
    let json = serde_json::to_string(&axes).unwrap();
    assert!(json.contains(r#""edition":"java""#), "got: {json}");
    assert!(json.contains(r#""edition":"bedrock""#), "got: {json}");
    assert_eq!(axes.buildable_targets.len(), axes.edition_portability.len());
    let editions: Vec<Edition> = axes
        .buildable_targets
        .iter()
        .map(|row| row.edition)
        .collect();
    assert_eq!(
        editions,
        axes.edition_portability
            .iter()
            .map(|row| row.edition)
            .collect::<Vec<_>>(),
    );
    assert_eq!(axes.buildable_targets[1].buildable, ["1.21.0"]);
    assert_eq!(axes.buildable_targets[1].considered, ["1.21.0", "1.21.40"]);
    assert!(json.contains(r#""buildable":["1.21.0"]"#), "got: {json}");
    assert!(
        json.contains(r#""considered":["1.21.0","1.21.40"]"#),
        "got: {json}",
    );
    assert_eq!(
        axes.edition_portability[1].unsupported_entries,
        one_entry_per_reason(),
    );
    assert_eq!(axes.edition_portability[0].unsupported_entries, Vec::new());
}

/// One [`UnsupportedEntry`] per [`UnsupportedReason`] variant.
fn one_entry_per_reason() -> Vec<UnsupportedEntry> {
    vec![
        UnsupportedEntry {
            id: "minecraft:oak_sign".to_owned(),
            reason: UnsupportedReason::AbsentFromEdition {
                suggestion: Some("minecraft:oak_log".to_owned()),
                aliases: vec!["minecraft:standing_sign".to_owned()],
            },
        },
        UnsupportedEntry {
            id: "minecraft:border_block".to_owned(),
            reason: UnsupportedReason::NoCounterpart {
                detail: "the border block, which Bedrock uses to wall players in; Java has no \
                         such block"
                    .to_owned(),
            },
        },
        UnsupportedEntry {
            id: "minecraft:camera".to_owned(),
            reason: UnsupportedReason::StatesUnmapped {
                states: "facing_direction=1".to_owned(),
                detail: "no Java block maps onto `minecraft:camera` there".to_owned(),
            },
        },
    ]
}

/// One of these reasons is unreachable from a `.crn`, so no CLI test covers its JSON.
#[test]
fn every_unsupported_reason_carries_its_own_wire_shape() {
    let entries = serde_json::to_value(one_entry_per_reason()).expect("the entries serialize");
    assert_eq!(
        entries,
        serde_json::json!([
            {
                "id": "minecraft:oak_sign",
                "reason": "absent_from_edition",
                "suggestion": "minecraft:oak_log",
                "aliases": ["minecraft:standing_sign"],
            },
            {
                "id": "minecraft:border_block",
                "reason": "no_counterpart",
                "detail": "the border block, which Bedrock uses to wall players in; Java has no \
                           such block",
            },
            {
                "id": "minecraft:camera",
                "reason": "states_unmapped",
                "states": "facing_direction=1",
                "detail": "no Java block maps onto `minecraft:camera` there",
            },
        ]),
    );
    let absent = serde_json::to_value(UnsupportedEntry {
        id: "minecraft:nothing_like_this".to_owned(),
        reason: UnsupportedReason::AbsentFromEdition {
            suggestion: None,
            aliases: Vec::new(),
        },
    })
    .expect("the entry serializes");
    assert_eq!(
        absent,
        serde_json::json!({
            "id": "minecraft:nothing_like_this",
            "reason": "absent_from_edition",
            "suggestion": null,
            "aliases": [],
        }),
    );
}

#[test]
fn semantic_sensitive_carries_each_editions_findings_in_report_order() {
    let finding = |edition, member: &str, reason: &str, boundary: &str| SemanticSensitiveFinding {
        edition,
        member: member.to_owned(),
        reason: reason.to_owned(),
        boundary_version: boundary.to_owned(),
    };
    let mut reports = synthetic_portability(&[(Edition::Bedrock, 0), (Edition::Java, 0)]);
    reports[0].semantic_sensitive =
        vec![finding(Edition::Bedrock, "pantry", "item format", "1.21.0")];
    reports[1].semantic_sensitive = vec![
        finding(Edition::Java, "yard_water", "cauldron split", "1.17"),
        finding(Edition::Java, "walls", "wall connections", "1.16"),
    ];
    let m = parse("struct s size=4x4\n  walls height=3\n").expect("parse");
    let axes = compute_axes(&m, reports);
    assert_eq!(
        axes.semantic_sensitive,
        [
            finding(Edition::Bedrock, "pantry", "item format", "1.21.0"),
            finding(Edition::Java, "yard_water", "cauldron split", "1.17"),
            finding(Edition::Java, "walls", "wall connections", "1.16"),
        ],
    );
    assert_eq!(
        serde_json::to_value(&axes.semantic_sensitive[1]).expect("the finding serializes"),
        serde_json::json!({
            "edition": "java",
            "member": "yard_water",
            "reason": "cauldron split",
            "boundary_version": "1.17",
        }),
    );
}

#[test]
fn floor_part_spells_the_surface_keyword() {
    for (part, kind) in [
        (FloorPart::Def, ItemKind::Def),
        (FloorPart::Theme, ItemKind::Theme),
    ] {
        assert_eq!(
            serde_json::to_value(part).expect("serialize"),
            serde_json::Value::String(kind.keyword().to_owned()),
        );
    }
    for origin in [
        FloorOrigin::Module,
        FloorOrigin::Def("d".to_owned()),
        FloorOrigin::Theme("t".to_owned()),
    ] {
        let Some((keyword, _)) = origin.part() else {
            continue;
        };
        assert!(
            [FloorPart::Def, FloorPart::Theme]
                .iter()
                .any(|part| serde_json::to_value(part).expect("serialize") == keyword),
            "`{keyword}` can declare a floor but `FloorPart` has no variant for it",
        );
    }
}

#[test]
fn a_dropped_intent_serializes_its_pieces_and_not_its_sentence() {
    let dropped = DroppedIntent {
        key: "shape".to_owned(),
        value: "outer_left".to_owned(),
        mc_versions: vec!["1.21.0".to_owned(), "1.21.60".to_owned()],
        reason: "no stair shape".to_owned(),
    };
    assert_eq!(
        serde_json::to_value(dropped).expect("the intent serializes"),
        serde_json::json!({
            "key": "shape",
            "value": "outer_left",
            "mc_versions": ["1.21.0", "1.21.60"],
        }),
    );
}

/// The `recommended test targets` row of one Java report building `buildable`, whose members
/// cross `boundaries`.
fn recommended(buildable: &[&str], boundaries: &[&str]) -> Vec<String> {
    let mut reports = synthetic_portability(&[(Edition::Java, 0)]);
    reports[0].buildable = buildable
        .iter()
        .map(|&version| version.to_owned())
        .collect();
    reports[0].semantic_sensitive = boundaries
        .iter()
        .map(|&boundary| SemanticSensitiveFinding {
            edition: Edition::Java,
            member: "box".to_owned(),
            reason: "item format".to_owned(),
            boundary_version: boundary.to_owned(),
        })
        .collect();
    let m = parse("struct s size=4x4\n  walls height=3\n").expect("parse");
    let [row] =
        <[RecommendedTestTargets; 1]>::try_from(compute_axes(&m, reports).recommended_test_targets)
            .expect("one row per report");
    assert_eq!(row.edition, Edition::Java);
    row.versions
}

#[test]
fn the_recommended_targets_are_the_buildable_ends_and_either_side_of_each_boundary_inside_them() {
    let all = ["1.20.4", "1.21", "1.21.4", "26.3"];
    let cases: [(&[&str], &[&str], &[&str]); 10] = [
        (&all, &[], &["1.20.4", "26.3"]),
        (&all, &["1.20.5"], &["1.20.4", "1.21", "26.3"]),
        (&all, &["1.20.5", "1.20.5"], &["1.20.4", "1.21", "26.3"]),
        (&all, &["1.21"], &["1.20.4", "1.21", "26.3"]),
        (&all, &["26.1"], &["1.20.4", "1.21.4", "26.3"]),
        (
            &all,
            &["1.21.2", "1.20.5"],
            &["1.20.4", "1.21", "1.21.4", "26.3"],
        ),
        (&all, &["1.16", "1.17", "27.1"], &["1.20.4", "26.3"]),
        (&["1.21.4", "26.3"], &["1.20.5"], &["1.21.4", "26.3"]),
        (&["26.3"], &["1.20.5"], &["26.3"]),
        (&[], &["1.20.5"], &[]),
    ];
    for (buildable, boundaries, expected) in cases {
        assert_eq!(
            recommended(buildable, boundaries),
            expected,
            "{buildable:?} across {boundaries:?}",
        );
    }
}

#[test]
fn the_recommended_targets_are_one_row_per_edition_in_report_order() {
    let mut reports = synthetic_portability(&[(Edition::Bedrock, 0), (Edition::Java, 0)]);
    reports[0].buildable = vec!["1.21.0".to_owned(), "1.21.40".to_owned()];
    reports[1].semantic_sensitive = vec![SemanticSensitiveFinding {
        edition: Edition::Java,
        member: "box".to_owned(),
        reason: "item format".to_owned(),
        boundary_version: "1.20.5".to_owned(),
    }];
    let m = parse("struct s size=4x4\n  walls height=3\n").expect("parse");
    let axes = compute_axes(&m, reports);
    assert_eq!(
        serde_json::to_value(&axes.recommended_test_targets).expect("the rows serialize"),
        serde_json::json!([
            {"edition": "bedrock", "versions": ["1.21.0", "1.21.40"]},
            {"edition": "java", "versions": []},
        ]),
    );
}
