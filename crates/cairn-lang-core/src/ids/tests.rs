use super::*;

#[test]
fn artifact_stem_names_each_scope_kind() {
    assert_eq!(artifact_stem("struct::cottage"), "cottage");
    assert_eq!(artifact_stem("site::hamlet::home1"), "home1");
    assert_eq!(
        artifact_stem("walkway::hamlet::home1.entry__home2.entry"),
        "hamlet_walkway_home1_entry__home2_entry",
    );
    assert_eq!(artifact_stem("cottage"), "cottage");
    assert_eq!(artifact_stem("walkway::no_site"), "walkway::no_site");
}

#[test]
fn ident_new_accepts_plain_identifiers() {
    assert_eq!(PlaceId::new("home1").unwrap().as_str(), "home1");
    assert_eq!(PortId::new("entry").unwrap().as_str(), "entry");
    assert_eq!(SiteName::new("hamlet").unwrap().as_str(), "hamlet");
}

#[test]
fn ident_new_rejects_empty() {
    assert_eq!(PlaceId::new(""), Err(IdError::Empty));
    assert_eq!(PortId::new(""), Err(IdError::Empty));
    assert_eq!(SiteName::new(""), Err(IdError::Empty));
}

#[test]
fn ident_new_rejects_dot() {
    match PortId::new("foo.bar") {
        Err(IdError::ForbiddenChar { ident, ch }) => {
            assert_eq!(ident, "foo.bar");
            assert_eq!(ch, '.');
        }
        other => panic!("expected ForbiddenChar('.'), got {other:?}"),
    }
}

#[test]
fn ident_new_rejects_colon_and_whitespace() {
    assert!(matches!(
        PlaceId::new("foo:bar"),
        Err(IdError::ForbiddenChar { ch: ':', .. })
    ));
    assert!(matches!(
        SiteName::new("foo bar"),
        Err(IdError::ForbiddenChar { ch: ' ', .. })
    ));
}

#[test]
fn ident_new_rejects_path_separators() {
    for (ident, ch) in [("/tmp/x", '/'), ("sub/x", '/'), ("a\\b", '\\')] {
        for result in [
            PlaceId::new(ident).map(|_| ()),
            PortId::new(ident).map(|_| ()),
            SiteName::new(ident).map(|_| ()),
        ] {
            assert_eq!(
                result,
                Err(IdError::ForbiddenChar {
                    ident: ident.to_owned(),
                    ch,
                }),
                "`{ident}` must be refused on `{ch}`",
            );
        }
    }
}

#[test]
fn ident_new_rejects_what_a_windows_file_name_cannot_carry() {
    for ch in ['*', '|', '?', '<', '>', '"', '\u{0}', '\u{1}', '\u{1f}'] {
        assert_refused_by_every_newtype(ch);
    }
}

#[test]
fn ident_new_rejects_the_controls_a_windows_file_name_allows_as_illegible() {
    for ch in ['\u{7f}', '\u{80}', '\u{85}', '\u{9f}'] {
        assert_refused_by_every_newtype(ch);
    }
}

fn assert_refused_by_every_newtype(ch: char) {
    let ident = format!("a{ch}b");
    for result in [
        PlaceId::new(ident.as_str()).map(|_| ()),
        PortId::new(ident.as_str()).map(|_| ()),
        SiteName::new(ident.as_str()).map(|_| ()),
    ] {
        assert_eq!(
            result,
            Err(IdError::ForbiddenChar {
                ident: ident.clone(),
                ch,
            }),
            "`{}` must be refused",
            ch.escape_debug(),
        );
    }
}

#[test]
fn every_listed_punctuation_mark_is_refused() {
    for ch in FORBIDDEN_IDENT_PUNCTUATION {
        assert_refused_by_every_newtype(ch);
    }
}

fn assert_place_id_is_a_device(names: &[&str]) {
    for name in names {
        assert_eq!(
            PlaceId::new(*name),
            Err(IdError::ReservedDeviceName {
                ident: (*name).to_owned(),
            }),
            "`{name}` is a Windows device name",
        );
    }
}

#[test]
fn a_place_id_spelling_con_prn_aux_or_nul_is_refused_in_any_case() {
    assert_place_id_is_a_device(&[
        "CON", "con", "Con", "cON", "PRN", "prn", "pRn", "AUX", "aux", "Aux", "NUL", "nul", "nUL",
    ]);
}

#[test]
fn a_place_id_spelling_com_or_lpt_and_a_digit_is_refused_in_any_case() {
    for prefix in ["COM", "com", "Com", "LPT", "lpt", "lPt"] {
        let names: Vec<String> = ('0'..='9')
            .map(|digit| format!("{prefix}{digit}"))
            .collect();
        let names: Vec<&str> = names.iter().map(String::as_str).collect();
        assert_place_id_is_a_device(&names);
    }
}

#[test]
fn a_place_id_spelling_com_or_lpt_and_a_superscript_digit_is_refused_in_any_case() {
    assert_place_id_is_a_device(&[
        "COM¹", "COM²", "COM³", "com¹", "cOm²", "LPT¹", "LPT²", "LPT³", "lpt³", "Lpt¹",
    ]);
}

#[test]
fn a_place_id_that_only_resembles_a_device_is_accepted() {
    for name in [
        "CONSOLE", "ACON", "CON1", "CON_1", "NULL", "AUX_1", "PRNT", "COM", "LPT", "COM10",
        "LPT00", "COMx", "COM1a", "COM⁴", "LPT⁰", "COM¹¹", "Größe", "CO¹",
    ] {
        assert_eq!(
            PlaceId::new(name).map(PlaceId::into_inner),
            Ok(name.to_owned()),
            "`{name}` is not a Windows device name",
        );
    }
}

#[test]
fn a_port_id_or_site_name_may_spell_a_device() {
    for name in ["CON", "nul", "com1", "LPT³"] {
        assert_eq!(
            PortId::new(name).map(PortId::into_inner),
            Ok(name.to_owned())
        );
        assert_eq!(
            SiteName::new(name).map(SiteName::into_inner),
            Ok(name.to_owned())
        );
    }
}

#[test]
fn a_device_name_with_an_extension_is_refused_for_its_dot() {
    assert_eq!(
        PlaceId::new("CON.nbt"),
        Err(IdError::ForbiddenChar {
            ident: "CON.nbt".to_owned(),
            ch: '.',
        }),
    );
}

#[test]
fn a_refused_device_name_reads_as_itself() {
    assert_eq!(
        PlaceId::new("com²").unwrap_err().to_string(),
        "identifier `com²` is a device name Windows reserves",
    );
}

#[test]
fn the_device_names_listed_are_the_ones_refused() {
    assert_eq!(
        windows_device_names_listed(),
        "`CON`, `PRN`, `AUX`, `NUL`, and `COM` or `LPT` followed by one of `0` to `9`, `¹`, `²` or \
         `³`",
    );
}

#[test]
fn an_illegible_forbidden_character_is_quoted_as_its_escape() {
    for (ch, shown) in [
        ('\u{1}', "\\u{1}"),
        ('\u{7f}', "\\u{7f}"),
        ('\t', "\\t"),
        ('\u{a0}', "\\u{a0}"),
        ('\u{2007}', "\\u{2007}"),
        ('\u{3000}', "\\u{3000}"),
    ] {
        let ident = format!("a{ch}b");
        let err = PlaceId::new(ident.as_str()).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("identifier `{ident}` contains forbidden character `{shown}`"),
        );
    }
    for (ident, shown) in [("a\\b", "\\"), ("a b", " ")] {
        let err = PlaceId::new(ident).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!("identifier `{ident}` contains forbidden character `{shown}`"),
        );
    }
}

#[test]
fn ident_serializes_transparently() {
    let json = serde_json::to_string(&PlaceId::new("home1").unwrap()).unwrap();
    assert_eq!(json, "\"home1\"");
}

#[test]
fn ident_deserializes_through_validation() {
    let ok: PortId = serde_norway::from_str("entry").unwrap();
    assert_eq!(ok.as_str(), "entry");
    let err = serde_norway::from_str::<PortId>("foo.bar").unwrap_err();
    assert!(err.to_string().contains("forbidden character"));
}

fn endpoint(place: &str, port: &str) -> WalkwayEndpoint {
    WalkwayEndpoint {
        place: PlaceId::new(place).expect("place"),
        port: PortId::new(port).expect("port"),
    }
}

#[test]
fn walkway_scope_key_round_trips() {
    let site = SiteName::new("hamlet").expect("site");
    let key = WalkwayScopeKey::from_parts(
        &site,
        &endpoint("home1", "entry"),
        &endpoint("home2", "entry"),
    )
    .expect("from_parts");
    assert_eq!(key.as_str(), "walkway::hamlet::home1.entry__home2.entry");
    let parsed = WalkwayScopeKey::parse(key.as_str()).expect("parse");
    assert_eq!(parsed, key);
    let parts = parsed.parts();
    assert_eq!(parts.site, "hamlet");
    assert_eq!(parts.from_place, "home1");
    assert_eq!(parts.from_port, "entry");
    assert_eq!(parts.to_place, "home2");
    assert_eq!(parts.to_port, "entry");
}

#[test]
fn walkway_scope_key_parse_rejects_dot_in_port() {
    let err = WalkwayScopeKey::parse("walkway::hamlet::home1.a.b__home2.entry").unwrap_err();
    match err {
        KeyParseError::InvalidSegment { segment, .. } => {
            // The first `.` splits place from port.
            assert_eq!(segment, "a.b");
        }
        other => panic!("expected InvalidSegment, got {other:?}"),
    }
}

#[test]
fn walkway_scope_key_from_parts_rejects_dunder_in_segment() {
    let site = SiteName::new("s").expect("site");
    let err = WalkwayScopeKey::from_parts(
        &site,
        &endpoint("home", "b__c"),
        &endpoint("home2", "entry"),
    )
    .expect_err("port id with `__` must be rejected");
    match err {
        KeyConstructError::ConsecutiveUnderscore { role, segment } => {
            assert_eq!(role, KeySegmentRole::Port);
            assert_eq!(segment, "b__c");
        }
        other @ KeyConstructError::UnderscoreAtEdge { .. } => {
            panic!("expected ConsecutiveUnderscore, got {other:?}")
        }
    }
    assert!(matches!(
        WalkwayScopeKey::from_parts(
            &SiteName::new("dunder__site").expect("site"),
            &endpoint("a", "x"),
            &endpoint("b", "y"),
        ),
        Err(KeyConstructError::ConsecutiveUnderscore {
            role: KeySegmentRole::Site,
            ..
        })
    ));
    assert!(matches!(
        WalkwayScopeKey::from_parts(&site, &endpoint("a__b", "x"), &endpoint("c", "y"),),
        Err(KeyConstructError::ConsecutiveUnderscore {
            role: KeySegmentRole::Place,
            ..
        })
    ));
}

#[test]
fn walkway_scope_key_parse_rejects_dunder_in_segment() {
    let err = WalkwayScopeKey::parse("walkway::s::a.b__c__d.e").unwrap_err();
    match err {
        KeyParseError::ConsecutiveUnderscore { segment, .. } => {
            // The first `__` is the separator.
            assert_eq!(segment, "c__d");
        }
        other => panic!("expected ConsecutiveUnderscore, got {other:?}"),
    }
}

#[test]
fn walkway_scope_key_from_parts_rejects_edge_underscore_in_place_or_port() {
    let site = SiteName::new("s").expect("site");
    let cases = [
        (
            endpoint("a", "p_"),
            endpoint("b", "p"),
            EndpointSegmentRole::Port,
            "p_",
        ),
        (
            endpoint("a", "p"),
            endpoint("_b", "p"),
            EndpointSegmentRole::Place,
            "_b",
        ),
        (
            endpoint("a_", "p"),
            endpoint("b", "p"),
            EndpointSegmentRole::Place,
            "a_",
        ),
        (
            endpoint("_a", "p"),
            endpoint("b", "p"),
            EndpointSegmentRole::Place,
            "_a",
        ),
        (
            endpoint("a", "_p"),
            endpoint("b", "p"),
            EndpointSegmentRole::Port,
            "_p",
        ),
        (
            endpoint("a", "p"),
            endpoint("b_", "p"),
            EndpointSegmentRole::Place,
            "b_",
        ),
        (
            endpoint("a", "p"),
            endpoint("b", "_p"),
            EndpointSegmentRole::Port,
            "_p",
        ),
        (
            endpoint("a", "p"),
            endpoint("b", "p_"),
            EndpointSegmentRole::Port,
            "p_",
        ),
        (
            endpoint("a", "_"),
            endpoint("b", "_"),
            EndpointSegmentRole::Port,
            "_",
        ),
    ];
    for (from, to, want_role, want_segment) in cases {
        match WalkwayScopeKey::from_parts(&site, &from, &to) {
            Err(KeyConstructError::UnderscoreAtEdge { role, segment }) => {
                assert_eq!((role, segment.as_str()), (want_role, want_segment));
            }
            other => panic!("{from} → {to}: expected UnderscoreAtEdge, got {other:?}"),
        }
    }
    let key = WalkwayScopeKey::from_parts(
        &SiteName::new("_s_").expect("site"),
        &endpoint("a", "p"),
        &endpoint("b", "p"),
    )
    .expect("an edge `_` on the site is unambiguous");
    assert_eq!(key.as_str(), "walkway::_s_::a.p__b.p");
}

#[test]
fn walkway_scope_key_parse_rejects_the_edge_underscore_aliases() {
    // `(a, p_) → (b, p)` and `(a, p) → (_b, p)` would both encode as this.
    match WalkwayScopeKey::parse("walkway::s::a.p___b.p") {
        Err(KeyParseError::UnderscoreAtEdge { key, segment }) => {
            assert_eq!(key, "walkway::s::a.p___b.p");
            assert_eq!(segment, "_b");
        }
        other => panic!("expected UnderscoreAtEdge, got {other:?}"),
    }
    // A port named `_` splits into an empty port at the first `__`.
    assert!(matches!(
        WalkwayScopeKey::parse("walkway::s::a.___b._"),
        Err(KeyParseError::InvalidSegment {
            source: IdError::Empty,
            ..
        })
    ));
}

#[test]
fn walkway_scope_key_from_parts_is_injective_and_parses_back() {
    // `_` alone can merge into the separator; sites stop at two, fenced by `::`.
    let mut segments = vec![String::new()];
    let mut all = Vec::new();
    for _ in 0..3 {
        segments = segments
            .iter()
            .flat_map(|s| [format!("{s}a"), format!("{s}_")])
            .collect();
        all.extend(segments.iter().cloned());
    }
    let sites: Vec<&String> = all.iter().filter(|s| s.len() <= 2).collect();
    let mut accepted = 0_usize;
    let mut rejected_dunder = 0_usize;
    let mut rejected_edge = 0_usize;
    for site in &sites {
        let site_id = SiteName::new(site.as_str()).expect("site");
        for from_place in &all {
            for from_port in &all {
                for to_place in &all {
                    for to_port in &all {
                        let from = endpoint(from_place, from_port);
                        let to = endpoint(to_place, to_port);
                        let key = match WalkwayScopeKey::from_parts(&site_id, &from, &to) {
                            Ok(key) => key,
                            Err(KeyConstructError::ConsecutiveUnderscore { .. }) => {
                                rejected_dunder += 1;
                                continue;
                            }
                            Err(KeyConstructError::UnderscoreAtEdge { .. }) => {
                                rejected_edge += 1;
                                continue;
                            }
                        };
                        accepted += 1;
                        let parsed = WalkwayScopeKey::parse(key.as_str())
                            .unwrap_or_else(|e| panic!("`{key}` does not parse back: {e}"));
                        let parts = parsed.parts();
                        assert_eq!(
                            (
                                parts.site,
                                parts.from_place,
                                parts.from_port,
                                parts.to_place,
                                parts.to_port,
                            ),
                            (
                                site.as_str(),
                                from_place.as_str(),
                                from_port.as_str(),
                                to_place.as_str(),
                                to_port.as_str(),
                            ),
                            "`{key}` parses back to different parts",
                        );
                    }
                }
            }
        }
    }
    assert!(rejected_dunder > 0 && rejected_edge > 0);
    assert_eq!(accepted, 1280, "5 sites x 4^4 endpoint combinations");
}

#[test]
fn walkway_scope_key_parse_rejects_missing_prefix() {
    assert!(matches!(
        WalkwayScopeKey::parse("struct::cottage"),
        Err(KeyParseError::MissingPrefix(_))
    ));
}

#[test]
fn walkway_scope_key_parse_rejects_missing_site() {
    assert!(matches!(
        WalkwayScopeKey::parse("walkway::home1.entry__home2.entry"),
        Err(KeyParseError::MissingSite(_))
    ));
}

#[test]
fn walkway_scope_key_parse_rejects_missing_separator() {
    assert!(matches!(
        WalkwayScopeKey::parse("walkway::hamlet::home1.entry"),
        Err(KeyParseError::MissingFromToSeparator(_))
    ));
}

#[test]
fn walkway_scope_key_parse_malformed_endpoint_carries_full_key() {
    let err = WalkwayScopeKey::parse("walkway::s::abc__d.e").unwrap_err();
    match err {
        KeyParseError::MalformedEndpoint { key, endpoint } => {
            assert_eq!(endpoint, "abc");
            assert_eq!(key, "walkway::s::abc__d.e");
        }
        other => panic!("expected MalformedEndpoint, got {other:?}"),
    }
}

#[test]
fn walkway_scope_key_serde_round_trip() {
    let site = SiteName::new("hamlet").expect("site");
    let ep = endpoint("home1", "entry");
    let key = WalkwayScopeKey::from_parts(&site, &ep, &ep).expect("from_parts");
    let json = serde_json::to_string(&key).unwrap();
    assert_eq!(json, "\"walkway::hamlet::home1.entry__home1.entry\"");
    let parsed: WalkwayScopeKey = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed, key);
}
