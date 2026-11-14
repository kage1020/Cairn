//! `E_SEMANTIC_IN_EDITION_GUARD`: an `@edition` guard holds only the escape-hatch layer, since the
//! semantic layer is edition-neutral, and `E_NBT_OUTSIDE_EDITION_GUARD`: an `nbt={...}` compound
//! stands under one (`spec/versioning-editions` "An `@edition` guard holds the escape hatch").

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::check::check;
use cairn_lang_core::{DiagnosticCode, Edition, lower, parse, resolve};

use crate::common::{diagnose, notes, slice};

const THEME: &str = "theme t\n  slot f -> @stone\n\n";

/// [`THEME`] and a struct whose body is a floor and then `lines`.
fn struct_with(lines: &str) -> String {
    format!("{THEME}struct s size=5x5\n  floor mat_slot=f\n{lines}")
}

/// Each finding of `source` as its code and the text it points at.
fn located(source: &str) -> Vec<(&'static str, &str)> {
    diagnose(source)
        .iter()
        .map(|d| (d.code.as_str(), slice(source, d)))
        .collect()
}

#[test]
fn a_guard_of_raw_members_is_clean() {
    let source = struct_with(
        "  @edition java\n    raw_block mat=minecraft:light[level=15] at=2,1,2\n  \
         @edition bedrock\n    raw_fill mat=minecraft:light_block_15 from=0,1,0 to=1,1,1\n",
    );
    assert_eq!(located(&source), []);
}

#[test]
fn a_guard_of_tier0_parts_is_clean() {
    let source = struct_with(
        "  @edition java\n    repeater at=1,1,1 facing=east delay=2\n    \
         comparator at=2,1,1 facing=east\n  @edition bedrock\n    redstone_torch at=3,1,1\n    \
         redstone_dust at=1,1,2\n",
    );
    assert_eq!(located(&source), []);
}

#[test]
fn a_semantic_member_under_a_guard_is_refused_at_its_line() {
    let source = struct_with(
        "  @edition java\n    walls mat_slot=f height=2\n    \
         raw_block mat=minecraft:stone at=1,0,1\n",
    );
    let found = diagnose(&source);
    let [d] = found.as_slice() else {
        panic!("expected one finding, got {found:#?}");
    };
    assert_eq!(d.code, DiagnosticCode::SemanticInEditionGuard);
    assert_eq!(slice(&source, d), "walls mat_slot=f height=2");
    assert_eq!(
        d.primary,
        "a `walls` stands under an `@edition java` guard, which holds only the escape-hatch layer: \
         the semantic layer is edition-neutral",
    );
    assert_eq!(
        notes(d),
        [
            "the guard opens here",
            "dedent the line out of the guard so that every edition builds it, and bind what \
             differs per edition from a `theme NAME_java` and a `theme NAME_bedrock`; the guard is \
             for `raw_block`, `raw_fill`, `spawn`, the Tier 0 parts `repeater`, `comparator`, \
             `redstone_torch` and `redstone_dust`, and an entity's line that writes `nbt={...}`",
        ],
    );
    assert_eq!(
        d.notes[0].span.clone().map(|span| &source[span]),
        Some("@edition java"),
    );
}

#[test]
fn a_logic_or_assert_line_under_a_guard_is_refused() {
    let source = struct_with(
        "  @edition bedrock\n    logic sig.a = sig.b\n    \
         assert always(sig.a -> eventually sig.b within 4)\n    \
         assert latency(sig.b -> sig.a) <= 2\n",
    );
    let found = diagnose(&source);
    let refused: Vec<(&str, &str, &str)> = found
        .iter()
        .map(|d| (d.code.as_str(), slice(&source, d), d.primary.as_str()))
        .collect();
    assert_eq!(
        refused,
        [
            (
                "E_SEMANTIC_IN_EDITION_GUARD",
                "logic sig.a = sig.b",
                "a `logic` line stands under an `@edition bedrock` guard, which holds only the \
                 escape-hatch layer: the semantic layer is edition-neutral",
            ),
            (
                "E_SEMANTIC_IN_EDITION_GUARD",
                "assert always(sig.a -> eventually sig.b within 4)",
                "an `assert` line stands under an `@edition bedrock` guard, which holds only the \
                 escape-hatch layer: the semantic layer is edition-neutral",
            ),
            (
                "E_SEMANTIC_IN_EDITION_GUARD",
                "assert latency(sig.b -> sig.a) <= 2",
                "an `assert` line stands under an `@edition bedrock` guard, which holds only the \
                 escape-hatch layer: the semantic layer is edition-neutral",
            ),
        ],
    );
}

#[test]
fn a_logic_or_assert_line_under_a_guarded_raw_member_is_refused() {
    let source = struct_with(
        "  @edition java\n    raw_block mat=minecraft:stone at=1,0,1\n      \
         logic sig.a = sig.b\n      assert always(sig.a -> eventually sig.b within 4)\n      \
         assert latency(sig.b -> sig.a) <= 2\n",
    );
    assert_eq!(
        located(&source),
        [
            ("E_SEMANTIC_IN_EDITION_GUARD", "logic sig.a = sig.b"),
            (
                "E_SEMANTIC_IN_EDITION_GUARD",
                "assert always(sig.a -> eventually sig.b within 4)",
            ),
            (
                "E_SEMANTIC_IN_EDITION_GUARD",
                "assert latency(sig.b -> sig.a) <= 2"
            ),
        ],
    );
}

#[test]
fn a_logic_line_under_a_guarded_tier0_part_is_refused() {
    let source = struct_with(
        "  @edition java\n    repeater at=1,1,1 facing=east delay=2\n      logic sig.a = sig.b\n",
    );
    assert_eq!(
        located(&source),
        [("E_SEMANTIC_IN_EDITION_GUARD", "logic sig.a = sig.b")],
    );
}

#[test]
fn a_logic_line_in_a_guard_under_a_guarded_raw_member_is_refused_and_lowered_into_no_body() {
    let source = struct_with(
        "  @edition java\n    raw_block mat=minecraft:stone at=1,0,1\n      @edition bedrock\n        \
         logic sig.a = sig.b\n        assert always(sig.a -> eventually sig.b within 4)\n",
    );
    let found = diagnose(&source);
    let refused: Vec<(&str, &str, &str)> = found
        .iter()
        .map(|d| (d.code.as_str(), slice(&source, d), d.primary.as_str()))
        .collect();
    assert_eq!(
        refused,
        [
            (
                "E_SEMANTIC_IN_EDITION_GUARD",
                "logic sig.a = sig.b",
                "a `logic` line stands under an `@edition bedrock` guard, which holds only the \
                 escape-hatch layer: the semantic layer is edition-neutral",
            ),
            (
                "E_SEMANTIC_IN_EDITION_GUARD",
                "assert always(sig.a -> eventually sig.b within 4)",
                "an `assert` line stands under an `@edition bedrock` guard, which holds only the \
                 escape-hatch layer: the semantic layer is edition-neutral",
            ),
        ],
    );
    let ir = lower(&parse(&source).expect("parses"));
    let built = &ir.structs[0];
    assert_eq!((built.logic.len(), built.asserts.len()), (0, 0));
    assert!(
        built.members[1].children.is_empty(),
        "{:#?}",
        built.members[1]
    );
}

#[test]
fn a_member_under_a_guarded_raw_member_is_unsupported_nesting_guarded_again_or_not() {
    for (under, nested) in [
        (
            "@edition bedrock\n        raw_block mat=minecraft:dirt at=2,0,2",
            "raw_block mat=minecraft:dirt at=2,0,2",
        ),
        ("walls mat_slot=f height=2", "walls mat_slot=f height=2"),
    ] {
        let source = struct_with(&format!(
            "  @edition java\n    raw_block mat=minecraft:stone at=1,0,1\n      {under}\n"
        ));
        assert_eq!(
            located(&source),
            [("E_UNSUPPORTED_NESTING", nested)],
            "{under}"
        );
    }
}

#[test]
fn a_raw_member_under_a_guard_gets_each_check_it_gets_outside_one() {
    let lines = [
        "raw_block id=@stone mat=minecraft:stone at=1,0,1",
        "raw_block mat=minecraft:stone mat=minecraft:dirt at=2,0,2",
    ];
    let expected = [
        ("E_TYPE_MISMATCH_LABEL", "@stone"),
        ("E_DUPLICATE_ARG", "mat=minecraft:dirt"),
    ];
    let bare = struct_with(&format!("  {}\n  {}\n", lines[0], lines[1]));
    let guarded = struct_with(&format!(
        "  @edition java\n    {}\n    {}\n",
        lines[0], lines[1]
    ));
    assert_eq!(located(&bare), expected);
    assert_eq!(located(&guarded), expected);
}

#[test]
fn a_refused_line_draws_no_finding_but_its_own() {
    let source = format!(
        "{THEME}struct s size=5x5\n  floor mat_slot=f\n  circuit region=floor void=2\n  \
         @edition java\n    walls mat_slot=f height=3 height=4\n    floor mat_slot=nope\n    \
         circuit id=c\n\ndef d size=3x3\n  floor mat_slot=f\n\nsite v\n  \
         place id=p use=d theme=t at=origin\n  @edition bedrock\n    \
         place id=q use=missing theme=t east_of=p gap=2\n"
    );
    assert_eq!(
        located(&source),
        [
            (
                "E_SEMANTIC_IN_EDITION_GUARD",
                "walls mat_slot=f height=3 height=4"
            ),
            ("E_SEMANTIC_IN_EDITION_GUARD", "floor mat_slot=nope"),
            ("E_SEMANTIC_IN_EDITION_GUARD", "circuit id=c"),
            (
                "E_SEMANTIC_IN_EDITION_GUARD",
                "place id=q use=missing theme=t east_of=p gap=2"
            ),
        ],
    );
}

/// A def placed under `theme=`, after `themes`, whose body is a floor, `body` and a raw member,
/// under a Java guard when `guarded` says so.
fn placed(themes: &str, body: &str, theme: &str, guarded: bool) -> String {
    let raw = if guarded {
        "  @edition java\n    raw_block mat=minecraft:stone at=1,0,1\n"
    } else {
        "  raw_block mat=minecraft:stone at=1,0,1\n"
    };
    format!(
        "{themes}def d size=5x5:\n  floor mat_slot=f\n{body}{raw}\nsite s:\n  \
         place id=a use=d theme={theme} at=origin\n"
    )
}

#[test]
fn a_guard_elsewhere_leaves_an_unguarded_lines_unpinned_verdict_as_it_was() {
    let java = "theme t_java:\n  slot f -> @stone\n\n";
    let both = "theme t_java:\n  slot f -> @stone\n\ntheme t_bedrock:\n  slot f -> @stone\n  \
                slot g -> @dirt\n\n";
    let cases = [
        (
            java,
            "",
            "t_bedrock",
            (
                "E_UNRESOLVED_THEME_REF",
                "`theme=t_bedrock` is not a declared theme",
            ),
        ),
        (
            both,
            "  walls class=outer mat_slot=g height=3\n",
            "t_java",
            (
                "E_UNRESOLVED_SLOT",
                "`mat_slot=g` is not declared in theme `t_java`",
            ),
        ),
        (
            both,
            "  walls class=outer mat_slot=nope height=3\n",
            "t",
            (
                "E_UNRESOLVED_SLOT",
                "`mat_slot=nope` is not declared in theme `t_java`",
            ),
        ),
    ];
    for (themes, body, theme, expected) in cases {
        for guarded in [false, true] {
            let source = placed(themes, body, theme, guarded);
            let diagnostics = diagnose(&source);
            let found: Vec<(&str, &str)> = diagnostics
                .iter()
                .map(|d| (d.code.as_str(), d.primary.as_str()))
                .collect();
            assert_eq!(found, [expected], "guarded: {guarded}\n{source}");
        }
    }
}

#[test]
fn a_refused_line_takes_what_is_indented_under_it_along() {
    let source = struct_with(
        "  @edition java\n    level y=1\n      walls mat_slot=f height=2\n      \
         @edition bedrock\n        raw_block mat=minecraft:stone at=1,0,1\n",
    );
    assert_eq!(
        located(&source),
        [("E_SEMANTIC_IN_EDITION_GUARD", "level y=1")],
    );
}

#[test]
fn a_refused_line_and_the_lines_under_it_name_no_missing_material() {
    let source = "struct s size=5x5\n  @edition java\n    walls class=outer height=3\n    \
                  level y=1\n      floor\n";
    assert_eq!(
        located(source),
        [
            ("E_SEMANTIC_IN_EDITION_GUARD", "walls class=outer height=3"),
            ("E_SEMANTIC_IN_EDITION_GUARD", "level y=1"),
        ],
    );
}

#[test]
fn a_guard_under_a_level_holds_raw_members_as_one_in_the_body_does() {
    let source = struct_with(
        "  walls mat_slot=f height=3\n  level y=1\n    @edition java\n      \
         raw_block mat=minecraft:stone at=1,0,1\n",
    );
    assert_eq!(located(&source), []);
}

#[test]
fn in_a_site_body_a_place_is_refused_for_its_guard_and_a_raw_member_for_its_body() {
    let source = format!(
        "{THEME}def d size=3x3\n  floor mat_slot=f\n\nsite v\n  \
         place id=p use=d theme=t at=origin\n  @edition java\n    \
         place id=q use=d theme=t east_of=p gap=2\n    raw_block mat=minecraft:stone at=1,0,1\n"
    );
    assert_eq!(
        located(&source),
        [
            (
                "E_SEMANTIC_IN_EDITION_GUARD",
                "place id=q use=d theme=t east_of=p gap=2"
            ),
            (
                "E_MISPLACED_MEMBER",
                "raw_block mat=minecraft:stone at=1,0,1"
            ),
        ],
    );
}

#[test]
fn an_unknown_keyword_under_a_guard_is_offered_what_a_guard_body_holds() {
    let source = struct_with("  @edition java\n    raw_blok mat=minecraft:stone at=1,0,1\n");
    let found = diagnose(&source);
    let [d] = found.as_slice() else {
        panic!("expected one finding, got {found:#?}");
    };
    assert_eq!(
        (d.code, slice(&source, d), d.primary.as_str()),
        (
            DiagnosticCode::UnknownKeyword,
            "raw_blok mat=minecraft:stone at=1,0,1",
            "unknown keyword `raw_blok`"
        ),
    );
    assert_eq!(
        notes(d),
        [
            "did you mean `raw_block`?",
            "expected one of: painting, item_frame, armor_stand, villager, text_display, \
             block_display, item_display, spawn, repeater, comparator, redstone_torch, \
             redstone_dust, raw_block, raw_fill"
        ],
    );
}

#[test]
fn an_unknown_keyword_under_a_guard_in_a_site_body_is_offered_nothing() {
    let source = "site v\n  @edition java\n    raw_blok mat=minecraft:stone at=1,0,1\n";
    let found = diagnose(source);
    let [d] = found.as_slice() else {
        panic!("expected one finding, got {found:#?}");
    };
    assert_eq!(
        (d.code, slice(source, d)),
        (
            DiagnosticCode::UnknownKeyword,
            "raw_blok mat=minecraft:stone at=1,0,1"
        ),
    );
    assert_eq!(
        notes(d),
        [
            "nothing builds under an `@edition` guard in a `site` body: the escape-hatch lines a \
             guard holds go in a `struct` or `def` body"
        ],
    );
}

#[test]
fn guards_for_different_editions_may_reuse_an_id_and_nothing_else_may() {
    let branches = struct_with(
        "  @edition java\n    raw_block id=lamp mat=minecraft:light at=1,0,1\n  \
         @edition bedrock\n    raw_block id=lamp mat=minecraft:light_block_15 at=1,0,1\n",
    );
    assert_eq!(located(&branches), []);
    let shared = struct_with(
        "  @edition java\n    raw_block id=lamp mat=minecraft:light at=1,0,1\n    \
         raw_block id=lamp mat=minecraft:glowstone at=2,0,1\n  @edition bedrock\n    \
         raw_block id=lamp mat=minecraft:light_block_15 at=1,0,1\n  \
         raw_block id=lamp mat=minecraft:glowstone at=3,0,1\n",
    );
    let at = |n: usize| shared.match_indices("id=lamp").nth(n).expect("an id").0;
    let repeats: Vec<(&str, usize, Option<usize>)> = diagnose(&shared)
        .iter()
        .map(|d| {
            (
                d.code.as_str(),
                d.span.start,
                d.notes[0].span.as_ref().map(|span| span.start),
            )
        })
        .collect();
    assert_eq!(
        repeats,
        [
            ("E_DUPLICATE_ID", at(1), Some(at(0))),
            ("E_DUPLICATE_ID", at(3), Some(at(0))),
        ],
    );
}

#[test]
fn an_unguarded_id_clashes_with_the_line_under_each_guard_wherever_it_stands() {
    let java = "  @edition java\n    raw_block id=a mat=minecraft:stone at=1,0,1\n";
    let bare = "  raw_block id=a mat=minecraft:stone at=2,0,1\n";
    let bedrock = "  @edition bedrock\n    raw_block id=a mat=minecraft:stone at=3,0,1\n";
    // Each repeat as (the `id=a` it is, the `id=a` its note names), counted in source order.
    for (body, expected) in [
        (format!("{java}{bare}{bedrock}"), [(1, 0), (2, 1)]),
        (format!("{bare}{java}{bedrock}"), [(1, 0), (2, 0)]),
    ] {
        let src = struct_with(&body);
        let at = |n: usize| src.match_indices("id=a").nth(n).expect("an id").0;
        let repeats: Vec<(&str, usize, Option<usize>)> = diagnose(&src)
            .iter()
            .map(|d| {
                (
                    d.code.as_str(),
                    d.span.start,
                    d.notes[0].span.as_ref().map(|span| span.start),
                )
            })
            .collect();
        let expected: Vec<(&str, usize, Option<usize>)> = expected
            .iter()
            .map(|&(repeat, first)| ("E_DUPLICATE_ID", at(repeat), Some(at(first))))
            .collect();
        assert_eq!(repeats, expected, "{body}");
    }
}

#[test]
fn one_id_under_two_guards_is_one_port_to_each_edition() {
    let src = "theme t:\n  slot wall -> @cobblestone\n\n\
               def cottage size=3x3:\n  walls mat_slot=wall height=2\n  \
               door id=entry side=front at=center\n  \
               @edition java\n    raw_block id=p mat=minecraft:stone at=1,0,1\n  \
               @edition bedrock\n    raw_block id=p mat=minecraft:stone at=0,0,0\n\n\
               site s:\n  place id=a use=cottage theme=t at=origin\n  \
               place id=b use=cottage theme=t east_of=a gap=2\n  \
               connect a.p to b.entry path=@gravel\n";
    let module = parse(src).expect("parses");
    let ir = lower(&module);
    for edition in [None, Some(Edition::Java), Some(Edition::Bedrock)] {
        let found: Vec<(&str, &str)> = check(&module, &ir, edition)
            .iter()
            .map(|d| (d.code.as_str(), slice(src, d)))
            .collect();
        assert_eq!(found, [], "{edition:?}");
    }
    // Lowering reads the same branch: its note names the edition's own `p`.
    for (edition, line) in [
        (Edition::Java, "raw_block id=p mat=minecraft:stone at=1,0,1"),
        (
            Edition::Bedrock,
            "raw_block id=p mat=minecraft:stone at=0,0,0",
        ),
    ] {
        let built = lower_to_block_array(&ir, &resolve(&ir, Some(edition)), None);
        let noted: Vec<(&str, Vec<&str>)> = built
            .diagnostics
            .iter()
            .map(|d| {
                let at = d.notes.iter().filter_map(|n| n.span.clone());
                (d.code.as_str(), at.map(|span| &src[span]).collect())
            })
            .collect();
        assert_eq!(noted, [("W_DEFERRED_MEMBER", vec![line])], "{edition:?}");
    }
}
