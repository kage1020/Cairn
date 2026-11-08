//! `E_SEMANTIC_IN_EDITION_GUARD`: an `@edition` guard holds raw members only, since the semantic
//! layer is edition-neutral
//! (`spec/versioning-editions` "An `@edition` guard holds the escape hatch").

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
        "a `walls` stands under an `@edition java` guard, which holds only raw members: the \
         semantic layer is edition-neutral",
    );
    assert_eq!(
        notes(d),
        [
            "the guard opens here",
            "dedent the line out of the guard so that every edition builds it, and bind what \
             differs per edition from a `theme NAME_java` and a `theme NAME_bedrock`; the guard is \
             for `raw_block` and `raw_fill`",
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
         assert always(sig.a -> eventually sig.b within 4)\n",
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
                "a `logic` line stands under an `@edition bedrock` guard, which holds only raw \
                 members: the semantic layer is edition-neutral",
            ),
            (
                "E_SEMANTIC_IN_EDITION_GUARD",
                "assert always(sig.a -> eventually sig.b within 4)",
                "an `assert` line stands under an `@edition bedrock` guard, which holds only raw \
                 members: the semantic layer is edition-neutral",
            ),
        ],
    );
}

#[test]
fn a_logic_or_assert_line_under_a_guarded_raw_member_is_refused() {
    let source = struct_with(
        "  @edition java\n    raw_block mat=minecraft:stone at=1,0,1\n      \
         logic sig.a = sig.b\n      assert always(sig.a -> eventually sig.b within 4)\n",
    );
    assert_eq!(
        located(&source),
        [
            ("E_SEMANTIC_IN_EDITION_GUARD", "logic sig.a = sig.b"),
            (
                "E_SEMANTIC_IN_EDITION_GUARD",
                "assert always(sig.a -> eventually sig.b within 4)",
            ),
        ],
    );
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
fn an_unknown_keyword_under_a_guard_is_offered_the_raw_members() {
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
            "expected one of: raw_block, raw_fill"
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
            "nothing builds under an `@edition` guard in a `site` body: the raw members a guard \
          holds go in a `struct` or `def` body"
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
