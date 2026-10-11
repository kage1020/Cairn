use super::*;

/// The version a well-formed requirement declares, for the tests that
/// are not about the edition scope.
fn version_of(raw: &str) -> Option<&str> {
    parse_min_version(raw).map(|r| r.version)
}

#[test]
fn parse_min_version_strips_prefix() {
    assert_eq!(version_of("version>=1.20"), Some("1.20"));
}

#[test]
fn parse_min_version_tolerates_whitespace() {
    assert_eq!(version_of("  version>= 1.20.4 "), Some("1.20.4"));
}

#[test]
fn parse_min_version_returns_none_for_other_shapes() {
    assert_eq!(version_of("version<=1.21"), None);
    assert_eq!(version_of("1.20"), None);
}

/// A space before the operator used to hide the whole floor. It is
/// what a human writes, and `version>= 1.20` already worked, so the two
/// spellings differed for no reason a reader could see.
#[test]
fn parse_min_version_accepts_whitespace_before_the_operator() {
    for raw in [
        "version >= 1.20.4",
        "version >=1.20.4",
        "version>= 1.20.4",
        "  version  >=  1.20.4  ",
    ] {
        assert_eq!(version_of(raw), Some("1.20.4"), "{raw:?}");
    }
}

/// The scope is what tells the two numbering schemes apart, so it has
/// to come back out as the edition it names rather than as text.
#[test]
fn a_floor_may_name_the_edition_it_is_written_in() {
    for (raw, edition) in [
        ("java version>=1.21.4", Edition::Java),
        ("bedrock version>=1.21.40", Edition::Bedrock),
        ("  java   version >= 1.21.4  ", Edition::Java),
    ] {
        let parsed = parse_requirement(raw).unwrap_or_else(|e| panic!("{raw:?}: {e}"));
        assert_eq!(parsed.edition, Some(edition), "{raw:?}");
    }
    assert_eq!(
        parse_requirement("version>=1.21.4")
            .expect("unscoped")
            .edition,
        None,
    );
}

/// A typo in the scope is a floor with a typo, not a line that is not
/// a floor. The two send an author to different halves of the line.
#[test]
fn a_scope_that_is_not_an_edition_is_named_as_a_scope() {
    assert_eq!(
        parse_requirement("jaba version>=1.21"),
        Err(RequirementError::UnknownEditionScope("jaba".to_owned())),
    );
    // Without a subject after it there is no floor to scope, so the
    // line is reported as the whole mistake it is.
    assert_eq!(
        parse_requirement("jaba"),
        Err(RequirementError::NotAVersionRequirement),
    );
    // And a word abutting the subject is one word, not a scope.
    assert_eq!(
        parse_requirement("javaversion>=1.21"),
        Err(RequirementError::NotAVersionRequirement),
    );
}

/// The error says which part is wrong. Collapsing every shape into
/// `None` is what let the pass above report "not understood", and an
/// unactionable diagnostic on a one-constraint directive is barely
/// better than the silence it replaced.
#[test]
fn parse_requirement_names_the_part_that_is_wrong() {
    assert_eq!(
        parse_requirement("version<1.20"),
        Err(RequirementError::UnsupportedOperator("<".to_owned())),
    );
    assert_eq!(
        parse_requirement("version>="),
        Err(RequirementError::EmptyVersion),
    );
    assert_eq!(
        parse_requirement("nonsense"),
        Err(RequirementError::NotAVersionRequirement),
    );
    assert_eq!(
        parse_requirement("version>=1.21 extra"),
        Err(RequirementError::TrailingTokens("extra".to_owned())),
    );
    assert_eq!(
        parse_requirement("version>=1.a"),
        Err(RequirementError::Component {
            version: "1.a".to_owned(),
            component: "a".to_owned(),
        }),
    );
    assert_eq!(
        parse_requirement("version>=1..2"),
        Err(RequirementError::Component {
            version: "1..2".to_owned(),
            component: String::new(),
        }),
    );
}

/// The labels the spec says may exist (`spec/versioning-editions` "The
/// target is a compile-time parameter") are labels, not mistakes. Whether
/// one can be *ordered* is the edition table's answer, and refusing
/// them here pre-empts it with the wrong one.
#[test]
fn the_label_shapes_the_spec_names_are_accepted() {
    for raw in [
        "version>=1.21.4-rc1",
        "version>=1.21.4-pre.2",
        "version>=24w14a",
        "version>=2026.1",
        "version>=1.21.4",
    ] {
        assert!(parse_min_version(raw).is_some(), "{raw:?}");
    }
}

/// A component that opens with a letter names no version in any
/// scheme, so the wider grammar still refuses it.
#[test]
fn a_component_that_does_not_open_with_a_digit_is_refused() {
    for raw in ["version>=1.a", "version>=x", "version>=1.2.beta"] {
        assert!(
            matches!(
                parse_requirement(raw),
                Err(RequirementError::Component { .. }),
            ),
            "{raw:?} -> {:?}",
            parse_requirement(raw),
        );
    }
}

/// A `-` with nothing readable after it is a truncated label rather
/// than a pre-release, and says so in its own words.
#[test]
fn a_pre_release_tag_that_is_not_one_is_refused() {
    for raw in [
        "version>=1.21.4-",
        "version>=1.21.4-rc/1",
        "version>=1.21.4-.",
    ] {
        assert!(
            matches!(
                parse_requirement(raw),
                Err(RequirementError::PreRelease { .. }),
            ),
            "{raw:?} -> {:?}",
            parse_requirement(raw),
        );
    }
}

/// The two component failures say different things, because the repair
/// differs. A label Cairn cannot read is not a number out of range.
#[test]
fn the_two_component_failures_read_differently() {
    let letters = parse_requirement("version>=1.beta").expect_err("not a component");
    let overflow = parse_requirement("version>=4294967296").expect_err("too large");
    assert_eq!(letters.kind(), "component_not_a_number");
    assert_eq!(overflow.kind(), "component_too_large");
    assert!(letters.to_string().contains("beta"), "{letters}");
    assert!(
        !letters.to_string().contains("4294967295"),
        "a component of letters is not a number out of range: {letters}",
    );
    assert!(overflow.to_string().contains("4294967295"), "{overflow}");
}

/// An empty component gets its own sentence — `1..2` is a typo, not a
/// label Cairn cannot order, and the two repairs have nothing in
/// common.
#[test]
fn an_empty_component_says_so() {
    let text = parse_requirement("version>=1..2")
        .expect_err("empty component")
        .to_string();
    assert!(
        text.contains("empty component"),
        "the message should name the shape of the mistake: {text}",
    );
}

/// An operator is read as the run of operator characters it is, so a
/// transposed or doubled one is reported as an operator rather than as
/// "expected a version floor" — true, but not what the author typed.
#[test]
fn parse_requirement_names_an_operator_it_does_not_know() {
    for operator in ["<", "<=", ">", "==", "=", "=>", "=<", "!=", "~>", "^"] {
        assert!(
            matches!(
                parse_requirement(&format!("version{operator}1.20")),
                Err(RequirementError::UnsupportedOperator(ref found)) if found == operator,
            ),
            "{operator}",
        );
    }
}

/// `4294967296` is all ASCII digits and no version, so the digit-only
/// shape check has to be paired with a bound.
#[test]
fn parse_requirement_refuses_a_component_too_large_to_be_a_version() {
    assert_eq!(
        parse_requirement("version>=4294967296"),
        Err(RequirementError::ComponentTooLarge {
            version: "4294967296".to_owned(),
            component: "4294967296".to_owned(),
        }),
    );
    // One below the boundary is a number, however unlikely.
    assert_eq!(version_of("version>=4294967295"), Some("4294967295"));
}

/// Rust's integer `FromStr` accepts a leading `+`, so `parse::<u32>()`
/// alone is looser than the label grammar this documents. The lexer
/// refuses `+` in a `.crn` file, but both of these are `pub` and a
/// caller can hand them any string.
///
/// It is not only a tidiness point: `+0` is not a digit run to the
/// comparison, so it stops being the zero it looks like and
/// `1.20.+0 > 1.20`.
#[test]
fn parse_requirement_refuses_a_signed_component() {
    for raw in ["version>=+1", "version>=1.+0", "version>=+1.+20"] {
        assert!(
            matches!(
                parse_requirement(raw),
                Err(RequirementError::Component { .. }),
            ),
            "{raw:?} -> {:?}",
            parse_requirement(raw),
        );
    }
    // A leading `-` is read as an empty core with a pre-release tag,
    // which is the shape it is: `-1` names no release.
    assert!(
        matches!(
            parse_requirement("version>=-1"),
            Err(RequirementError::Component { .. }),
        ),
        "{:?}",
        parse_requirement("version>=-1"),
    );
}

#[test]
fn parse_min_version_rejects_empty_payload() {
    // Regression: `version>=` with no body used to return `Some("")`,
    // which `derive_min_version` then propagated to `min` and produced
    // a malformed `{"min":""}` in `cairn info --format json`.
    assert_eq!(version_of("version>="), None);
    assert_eq!(version_of("version>=  "), None);
}

#[test]
fn parse_min_version_rejects_non_numeric_segments() {
    // Regression: a non-numeric segment used to slip through and
    // `compare_versions` would fall back to ASCII compare, where
    // `'a' (0x61) > '2' (0x32)` made `1.a > 1.20`.
    assert_eq!(version_of("version>=1.a"), None);
    assert_eq!(version_of("version>=1..2"), None);
}

#[test]
fn compare_versions_is_numeric_per_component() {
    assert_eq!(compare_versions("1.20", "1.20"), Ordering::Equal);
    assert_eq!(compare_versions("1.20", "1.20.4"), Ordering::Less);
    assert_eq!(compare_versions("1.20.4", "1.20"), Ordering::Greater);
    assert_eq!(compare_versions("1.21", "1.20.4"), Ordering::Greater);
    assert_eq!(compare_versions("1.2", "1.10"), Ordering::Less);
}

/// A pre-release sits below the release it names. It is the one
/// relation a label's text does promise, and the table agrees wherever
/// it carries both.
#[test]
fn a_pre_release_sorts_below_its_release() {
    assert_eq!(compare_versions("1.21.4-rc1", "1.21.4"), Ordering::Less);
    assert_eq!(compare_versions("1.21.4", "1.21.4-rc1"), Ordering::Greater);
    assert_eq!(compare_versions("1.21.4-rc1", "1.21.4-rc2"), Ordering::Less);
    assert_eq!(
        compare_versions("1.21.4-rc1", "1.21.4-rc1"),
        Ordering::Equal,
    );
    // The core still decides first: a pre-release of a later version
    // is later.
    assert_eq!(compare_versions("1.21.4-rc1", "1.21"), Ordering::Greater);
}

#[test]
fn compare_versions_treats_trailing_zero_as_equal() {
    assert_eq!(compare_versions("1.20", "1.20.0"), Ordering::Equal);
    assert_eq!(compare_versions("1.20.0.0", "1.20"), Ordering::Equal);
    // Leading zeros are not a different number either.
    assert_eq!(compare_versions("1.20", "1.020"), Ordering::Equal);
    assert_eq!(compare_versions("1.0007", "1.7"), Ordering::Equal);
}

/// This is `pub`, so it has to be right on input the directive would
/// have refused: `4294967296` overflows `u32`, a string fallback
/// compares it by first character, and `'4' < '9'` puts it below
/// `999`.
#[test]
fn compare_versions_orders_digits_past_u32_by_value() {
    assert_eq!(compare_versions("4294967296", "999"), Ordering::Greater);
    assert_eq!(compare_versions("999", "4294967296"), Ordering::Less);
    // And among two that both overflow, by value rather than by the
    // first differing character.
    assert_eq!(
        compare_versions("99999999999", "100000000000"),
        Ordering::Less,
    );
    assert_eq!(compare_versions("4294967296", "4294967297"), Ordering::Less,);
    // Equal length, equal value.
    assert_eq!(
        compare_versions("4294967296", "4294967296"),
        Ordering::Equal,
    );
    // Leading zeros are padding, not digits of the number. Ordering by
    // length first is only numeric order once they are gone —
    // `04294967296` is eleven characters and the smaller value.
    assert_eq!(
        compare_versions("04294967296", "4294967296"),
        Ordering::Equal,
    );
    assert_eq!(
        compare_versions("04294967296", "4294967297"),
        Ordering::Less,
    );
    // And an all-zero run is zero rather than nothing, so a component
    // of `000` still stands in for an absent one.
    assert_eq!(compare_versions("1.20", "1.20.000"), Ordering::Equal);
    // The same, in the position a version would use it.
    assert_eq!(compare_versions("1.4294967296", "1.999"), Ordering::Greater);
}

/// A component that is not a number at all still orders, because this
/// function is reachable from `cairn info` on a hand-edited file and a
/// panic there would be worse than an arbitrary answer.
///
/// Which way such a version sorts is not a promise. That it sorts *at
/// all* is: `slice::sort_by` panics on a comparator that is not a total
/// order, and nothing stops a caller passing this one.
///
/// Antisymmetry alone does not get there — it survives cycles, and the
/// version this replaced had them. Every triple over a fixed alphabet
/// is the cheapest way to see the difference: 24 values is 13824
/// comparisons, run in well under a millisecond.
#[test]
fn compare_versions_is_a_total_order_on_input_the_grammar_refuses() {
    // Chosen to mix the classes the comparison distinguishes: short and
    // long digit runs, runs past `u32`, leading zeros, non-digits,
    // empties, pre-release tags, and multi-component versions of each.
    const ALPHABET: &[&str] = &[
        "",
        "0",
        "00",
        "1",
        "1.",
        ".1",
        "1.0",
        "1.00",
        "1.20",
        "1.21",
        "1.21.0",
        "999",
        "5abc",
        "a",
        "x",
        "24w14a",
        "4294967296",
        "04294967296",
        "4294967297",
        "1.4294967296",
        "1.+0",
        "-",
        "1.21-",
        "1.21-rc1",
    ];

    for &a in ALPHABET {
        assert_eq!(compare_versions(a, a), Ordering::Equal, "{a:?}");
        for &b in ALPHABET {
            assert_eq!(
                compare_versions(a, b).reverse(),
                compare_versions(b, a),
                "not antisymmetric: {a:?} vs {b:?}",
            );
            for &c in ALPHABET {
                // Transitivity of `<=`, which for a total order also
                // gives transitivity of `<` and of `==`.
                if compare_versions(a, b).is_le() && compare_versions(b, c).is_le() {
                    assert!(
                        compare_versions(a, c).is_le(),
                        "not transitive: {a:?} <= {b:?} <= {c:?} but {a:?} > {c:?}",
                    );
                }
            }
        }
    }
}

/// The classification the total order rests on: every version made of
/// numbers sorts below every version that is not, whatever the
/// characters would say. Mixing the two orders per pair is what put
/// `5abc` below `999` and above `4294967296` at the same time.
#[test]
fn a_component_that_is_not_a_number_sorts_above_every_number() {
    assert_eq!(compare_versions("5abc", "999"), Ordering::Greater);
    assert_eq!(compare_versions("5abc", "4294967296"), Ordering::Greater);
    assert_eq!(compare_versions("a", "99999999999"), Ordering::Greater);
    // An empty component is not a number either, so `1.` is not `1.0`.
    // Arbitrary, but fixed: the alternative is a comparison that calls
    // a version the grammar refuses equal to one it accepts.
    assert_eq!(compare_versions("1.", "1.0"), Ordering::Greater);
    assert_eq!(compare_versions("1.0", "1."), Ordering::Less);
}

/// What `order` asks before trusting the text: one numbering
/// scheme, not two.
#[test]
fn dotted_decimal_is_what_it_says() {
    for yes in ["1", "1.20", "1.21.4", "0.0", "4294967296"] {
        assert!(is_dotted_decimal(yes), "{yes:?}");
    }
    for no in ["1.21.4-rc1", "24w14a", "1.a", "1.", "", "1..2"] {
        assert!(!is_dotted_decimal(no), "{no:?}");
    }
}
