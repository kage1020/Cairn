use super::*;
use crate::parse;

/// A per-edition report with only the portable count set, for tests not about axis 2 or the version
/// lists.
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
            },
            EditionReport {
                edition: Edition::Bedrock,
                portable: 1,
                unsupported_entries: one_entry_per_reason(),
                degraded_entries: vec![DegradedEntry {
                    id: "minecraft:spruce_stairs".to_owned(),
                    states: "facing=north,shape=outer_left".to_owned(),
                    dropped: vec![DroppedIntent::Shape {
                        value: "outer_left".to_owned(),
                    }],
                }],
                buildable: vec!["1.21.0".to_owned()],
                considered: vec!["1.21.0".to_owned(), "1.21.40".to_owned()],
                refusal: None,
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
    // Both figures are the lengths of the lists beside them: the one thing this function decides
    // rather than forwards.
    assert_eq!(axes.edition_portability[1].degraded, 1);
    assert_eq!(axes.edition_portability[1].unsupported, 2);
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
    // The buildable row is fanned out of the same input, so it matches the portability rows one for
    // one, and keeps `considered` because an empty `buildable` alone says nothing about how many
    // versions were weighed.
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
    // An edition with nothing unsupported carries an empty list, not a missing one.
    assert_eq!(
        axes.edition_portability[1].unsupported_entries,
        one_entry_per_reason(),
    );
    assert_eq!(axes.edition_portability[0].unsupported_entries, Vec::new());
}

/// One [`UnsupportedEntry`] per [`UnsupportedReason`] variant, so a test about all of them cannot
/// quietly stop covering one.
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
            id: "minecraft:oak_door".to_owned(),
            reason: UnsupportedReason::StatesUnmapped {
                states: "facing=north".to_owned(),
                mapped: "the stair family".to_owned(),
            },
        },
    ]
}

/// Every reason's JSON, pinned here because one of the two is unreachable from a `.crn` and so
/// never passes through a CLI test.
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
                "id": "minecraft:oak_door",
                "reason": "states_unmapped",
                "states": "facing=north",
                "mapped": "the stair family",
            },
        ]),
    );
    // `suggestion` is null and `aliases` empty rather than absent, so a consumer reads one shape
    // per reason.
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
fn semantic_sensitive_is_empty_without_catalog() {
    let src = "struct s size=4x4\n  walls height=3\n";
    let m = parse(src).expect("parse");
    let axes = compute_axes(&m, synthetic_portability(&[(Edition::Java, 0)]));
    assert!(axes.semantic_sensitive.is_empty());
}

/// [`FloorPart`] copies two words [`ItemKind`] spells. Each variant serializes to its keyword, and
/// every [`FloorOrigin`] naming a part is one of the two, so a new route fails here rather than
/// reaching the wire as a new string.
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
