use super::*;

#[test]
fn artifact_stem_names_each_scope_kind() {
    assert_eq!(artifact_stem("struct::cottage"), "cottage");
    assert_eq!(artifact_stem("site::hamlet::home1"), "home1");
    assert_eq!(
        artifact_stem("walkway::hamlet::home1.entry__home2.entry"),
        "hamlet_walkway_home1_entry__home2_entry",
    );
    // Anything else passes through, including a walkway key without
    // its site separator.
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
    // Every shape the output file name could take through a separator:
    // absolute, relative with a directory, and Windows-style. Each
    // newtype is checked, since all three become file-name segments.
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
    // Each newtype is checked, since all three become file-name
    // segments, and the rule must not depend on the host. U+0000 to
    // U+001F are the controls Windows reserves; both ends are pinned.
    for ch in ['*', '|', '?', '<', '>', '"', '\u{0}', '\u{1}', '\u{1f}'] {
        assert_refused_by_every_newtype(ch);
    }
}

#[test]
fn ident_new_rejects_the_controls_a_windows_file_name_allows_as_illegible() {
    // U+007F to U+009F are legal in a Windows file name. They are
    // refused because they print as nothing, which is a different
    // reason and so a different test: dropping the Windows half of the
    // rule must not take these with it, nor the other way round.
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
    // The list is what the `E_INVALID_PLACE_ID` note is built from, so
    // a mark listed and not refused would be advice the rule does not
    // follow.
    for ch in FORBIDDEN_IDENT_PUNCTUATION {
        assert_refused_by_every_newtype(ch);
    }
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
    // A character that reads as itself is quoted as itself: the
    // separators, and the plain space, which nobody mistakes for
    // anything else.
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
    // `serde_norway` emits the scalar with no extra structure, matching
    // what a bare `String` would produce — the `#[serde(transparent)]`
    // wrapper does not add a tag.
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
    // Hypothetical silent-disaster input: a port id that contains a
    // `.` would split as PLACE=`home1`, PORT=`a.b` and round-trip
    // back through `from_parts` would alias with PLACE=`home1.a`,
    // PORT=`b`. The validator must reject it instead.
    let err = WalkwayScopeKey::parse("walkway::hamlet::home1.a.b__home2.entry").unwrap_err();
    match err {
        KeyParseError::InvalidSegment { segment, .. } => {
            // `from` split-once on `.` consumes the first `.`, so
            // the parsed port id is `a.b` which fails validation.
            assert_eq!(segment, "a.b");
        }
        other => panic!("expected InvalidSegment, got {other:?}"),
    }
}

#[test]
fn walkway_scope_key_from_parts_rejects_dunder_in_segment() {
    // The lexer permits `__` in identifiers, but the canonical
    // scope key uses `__` as the `from`/`to` separator — so
    // `from_parts((home, b__c), (home2, entry))` and
    // `from_parts((home, b), (c__home2, entry))` would otherwise
    // both encode to `walkway::s::home.b__c__home2.entry`. The
    // validator surfaces this as a typed error so lowering can
    // emit a diagnostic on the originating `connect` row instead.
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
    // Symmetric: the same protection applies to place ids and the
    // site name.
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
    // Symmetric to the `from_parts` test: a hand-crafted wire
    // string carrying `__` inside a segment fails parsing with the
    // typed `ConsecutiveUnderscore` variant. Without this guard,
    // the canonical encoding would alias two distinct typed
    // endpoint pairs.
    let err = WalkwayScopeKey::parse("walkway::s::a.b__c__d.e").unwrap_err();
    match err {
        KeyParseError::ConsecutiveUnderscore { segment, .. } => {
            // `endpoints.split_once("__")` consumes the first
            // `__`, so the parsed `to_place` is `c__d`.
            assert_eq!(segment, "c__d");
        }
        other => panic!("expected ConsecutiveUnderscore, got {other:?}"),
    }
}

#[test]
fn walkway_scope_key_from_parts_rejects_edge_underscore_in_place_or_port() {
    // `(a, p_) → (b, p)` and `(a, p) → (_b, p)` both used to encode
    // to `walkway::s::a.p___b.p`: the edge `_` merges into the `__`
    // separator. Both ends of every place and port are refused, not
    // only the two that touch the separator in one direction.
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
    // The site sits between two `::` separators, which no identifier
    // can contain, so an edge `_` there cannot alias and stays legal.
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
    // The string both aliasing rows used to produce is refused on
    // the way back in, naming the segment the first `__` leaves
    // with a leading `_`.
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
    // Exhaustive over endpoint segments of `a` and `_` up to three
    // long — the only character that can merge into the `__`
    // separator is `_`, and three is enough to put `_` at an edge, in
    // the middle, and doubled. The site is capped at two: `::` fences
    // it on both sides, so a third character buys nothing and would
    // more than double the run. Every key `from_parts` accepts must
    // parse back to the exact parts it was built from, which rules out
    // two accepted inputs sharing a key: `parse` is a function, so a
    // shared key would parse back to only one of them.
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
    // Both refusal arms fired, so the edge rule is exercised and not
    // only the `__` one.
    assert!(rejected_dunder > 0 && rejected_edge > 0);
    // What survives: 5 sites (`a`, `_`, `aa`, `a_`, `_a`; `__` is
    // refused) times 4 endpoint segments (`a`, `aa`, `aaa`, `a_a`) to
    // the fourth power. A rule that narrowed or widened acceptance
    // moves this count.
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
    // `walkway::` followed by an `__`-separated endpoint pair with
    // no `::SITE::` in between hits the structural `MissingSite`
    // arm (the `split_once("__")` later picks up the joined
    // identifier-only form).
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
