//! Argument vocabularies: `E_UNKNOWN_ARGUMENT` for a key the role does not take, and
//! `W_IGNORED_ARGUMENT` for one it takes and nothing reads.

use cairn_lang_core::intent::{
    MemberRole, UNIVERSAL_ARGUMENTS, Unread, UnreadArgument, known_keywords, role_of,
};
use cairn_lang_core::{Diagnostic, DiagnosticCode, Edition, check, lower, parse};

use crate::common::{codes, diagnose, exactly_one, of_code};

/// The one finding `source` raises with `code`, among exactly `total` findings in all.
fn only_with_code(source: &str, code: &str, total: usize) -> Diagnostic {
    let diags = diagnose(source);
    assert_eq!(
        diags.len(),
        total,
        "expected {total} findings in all, got {diags:#?}"
    );
    let mut found = diags.iter().filter(|d| d.code.as_str() == code);
    match (found.next(), found.next()) {
        (Some(d), None) => d.clone(),
        _ => panic!("expected exactly one {code}, got {diags:#?}"),
    }
}

/// The source text each `W_IGNORED_ARGUMENT` in `source` points at.
fn ignored_at(source: &str) -> Vec<&str> {
    diagnose(source)
        .iter()
        .filter(|d| d.code == DiagnosticCode::IgnoredArgument)
        .map(|d| &source[d.span.clone()])
        .collect()
}

fn notes(d: &Diagnostic) -> String {
    d.notes
        .iter()
        .map(|n| n.message.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn a_key_the_role_does_not_read_is_refused() {
    let d = exactly_one(diagnose(
        "struct s size=5x5\n  walls class=outer mat_slot=wall hieght=3\n",
    ));
    assert_eq!(d.code.as_str(), "E_UNKNOWN_ARGUMENT");
    assert!(d.primary.contains("`hieght=`"), "got: {}", d.primary);
    assert!(d.primary.contains("`walls`"), "got: {}", d.primary);
}

#[test]
fn the_finding_points_at_the_argument_and_not_at_the_line() {
    // The IR carries no span for the key, so the anchor is the value.
    let src = "struct s size=5x5\n  walls class=outer mat_slot=wall hieght=3\n";
    let d = exactly_one(diagnose(src));
    assert_eq!(&src[d.span.clone()], "3");
    assert!(d.primary.contains("`hieght=`"), "got: {}", d.primary);
}

#[test]
fn the_suggestion_is_drawn_from_the_role_and_from_the_universal_keys() {
    // `class` is a universal key, not one of `walls`' own.
    let role_key = exactly_one(diagnose(
        "struct s size=5x5\n  walls mat_slot=wall hieght=3\n",
    ));
    assert!(
        notes(&role_key).contains("did you mean `height`?"),
        "got: {}",
        notes(&role_key),
    );
    let universal = exactly_one(diagnose(
        "struct s size=5x5\n  walls height=3 clas=outer mat_slot=m\n",
    ));
    assert!(
        notes(&universal).contains("did you mean `class`?"),
        "got: {}",
        notes(&universal),
    );
}

#[test]
fn a_key_nothing_resembles_is_named_with_the_closed_set_and_no_suggestion() {
    let d = exactly_one(diagnose(
        "struct s size=5x5\n  floor totally_unrelated=1 mat_slot=m\n",
    ));
    let notes = notes(&d);
    assert!(!notes.contains("did you mean"), "got: {notes}");
    assert!(
        notes.contains(&format!(
            "expected one of: {}",
            UNIVERSAL_ARGUMENTS.join(", ")
        )),
        "got: {notes}",
    );
}

#[test]
fn an_unknown_keyword_answers_for_its_own_arguments() {
    assert_eq!(
        codes("struct s size=5x5\n  torch brightness=15\n"),
        ["E_UNKNOWN_KEYWORD"],
    );
}

#[test]
fn a_nested_member_is_checked_at_any_depth() {
    let d = exactly_one(diagnose(
        "struct s size=5x5\n  level y=0\n    walls hieght=3 mat_slot=m\n",
    ));
    assert_eq!(d.code.as_str(), "E_UNKNOWN_ARGUMENT");
}

#[test]
fn a_member_inside_a_def_is_checked_like_any_other() {
    let src = "def hut size=3x3:\n  walls hieght=3 mat_slot=m\n\n\
               theme t:\n  slot m -> @oak_planks\n\n\
               site v:\n  place id=a use=hut theme=t at=origin\n";
    let d = exactly_one(diagnose(src));
    assert_eq!(d.code.as_str(), "E_UNKNOWN_ARGUMENT");
    assert!(d.primary.contains("`hieght=`"), "got: {}", d.primary);
}

#[test]
fn a_struct_header_key_outside_its_vocabulary_is_refused_with_the_closed_set() {
    // `height` is a member's key, not the header's, so nothing is suggested.
    let src = "struct s size=5x5 hieght=3\n  floor mat_slot=m\n";
    let d = exactly_one(diagnose(src));
    assert_eq!(d.code.as_str(), "E_UNKNOWN_ARGUMENT");
    assert_eq!(&src[d.span.clone()], "3");
    assert!(
        d.primary
            .contains("`hieght=` is not an argument a `struct` header reads"),
        "got: {}",
        d.primary,
    );
    let notes = notes(&d);
    assert!(!notes.contains("did you mean"), "got: {notes}");
    assert!(
        notes.contains("expected one of: size, class"),
        "got: {notes}"
    );
}

#[test]
fn a_misspelled_size_on_a_header_is_told_it_is_misspelled() {
    // The unplaced `def` also earns `W_UNUSED_DEF`.
    for (src, keyword, total) in [
        ("struct s siz=7x7\n  floor mat_slot=m\n", "struct", 1),
        ("def hut siz=7x7:\n  floor mat_slot=m\n", "def", 2),
    ] {
        let d = only_with_code(src, "E_UNKNOWN_ARGUMENT", total);
        assert_eq!(&src[d.span.clone()], "7x7", "source:\n{src}");
        assert!(
            d.primary.contains(&format!(
                "`siz=` is not an argument a `{keyword}` header reads"
            )),
            "got: {}",
            d.primary,
        );
        assert!(
            notes(&d).contains("did you mean `size`?"),
            "got: {}",
            notes(&d),
        );
    }
}

#[test]
fn a_header_class_is_accepted_and_reported_as_ignored() {
    // `spec/components-editing-sites` "`def`, the component construct" writes `class=` on a header.
    // The unplaced `def` also earns `W_UNUSED_DEF`.
    for (src, keyword, total) in [
        (
            "def cottage class=house size=9x7:\n  floor mat_slot=m\n",
            "def",
            2,
        ),
        (
            "struct cottage class=house size=9x7\n  floor mat_slot=m\n",
            "struct",
            1,
        ),
    ] {
        let d = only_with_code(src, "W_IGNORED_ARGUMENT", total);
        assert_eq!(&src[d.span.clone()], "house", "source:\n{src}");
        assert!(
            d.primary.contains(&format!(
                "`class=` is an argument a `{keyword}` header takes and no pass reads yet"
            )),
            "got: {}",
            d.primary,
        );
    }
}

#[test]
fn a_near_miss_on_a_header_class_is_offered_the_unread_key() {
    let src = "struct s size=5x5 clas=house\n  floor mat_slot=m\n";
    let d = exactly_one(diagnose(src));
    assert_eq!(d.code.as_str(), "E_UNKNOWN_ARGUMENT");
    assert_eq!(&src[d.span.clone()], "house");
    assert!(
        notes(&d).contains("did you mean `class`?"),
        "got: {}",
        notes(&d),
    );
}

#[test]
fn a_header_size_of_the_wrong_shape_is_the_type_check_s_alone() {
    assert_eq!(
        codes("struct s size=5\n  floor mat_slot=m\n"),
        ["E_TYPE_MISMATCH_SIZE"],
    );
}

#[test]
fn a_theme_selector_widens_the_vocabulary_of_the_keyword_it_names() {
    let selected = "theme t:\n  slot glass -> @glass_pane\n  \
                    window[tags=[a,b]] -> frame=@spruce_wood\n\n\
                    struct s size=9x7\n  \
                    window tags=[a,b] side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
    assert_eq!(codes(selected), ["W_IGNORED_ARGUMENT"]);
    assert_eq!(ignored_at(selected), ["@spruce_wood"]);

    let unselected = "theme t:\n  slot glass -> @glass_pane\n\n\
                      struct s size=9x7\n  \
                      window tags=[a,b] side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
    assert_eq!(codes(unselected), ["E_UNKNOWN_ARGUMENT"]);
}

#[test]
fn a_selector_widens_one_keyword_and_not_the_others() {
    // The window is there so the row matches and raises no `E_THEME_SELECTOR_UNMATCHED`.
    let src = "theme t:\n  slot glass -> @glass_pane\n  \
               window[tags=[a,b]] -> frame=@spruce_wood\n\n\
               struct s size=9x7\n  \
               window tags=[a,b] side=front offset=2 y=2 size=2x2 mat_slot=glass\n  \
               door side=front at=center tags=[a,b]\n";
    // Beside it, only the row's binding.
    let d = only_with_code(src, "E_UNKNOWN_ARGUMENT", 2);
    assert!(d.primary.contains("`door`"), "got: {}", d.primary);
}

#[test]
fn an_argument_the_spec_defines_and_nothing_reads_is_reported_as_ignored() {
    // `window shape=` is in `spec/components-editing-sites` "Editing model" and no pass reads it.
    let d = exactly_one(diagnose(
        "struct s size=9x7\n  window side=front y=2 offset=2 size=2x2 shape=arch\n",
    ));
    assert_eq!(d.code.as_str(), "W_IGNORED_ARGUMENT");
    assert!(d.primary.contains("`shape=`"), "got: {}", d.primary);
    assert!(
        d.primary.contains("no pass reads yet"),
        "got: {}",
        d.primary,
    );
}

#[test]
fn a_key_a_sibling_argument_routed_past_is_reported_as_ignored() {
    // A roof reads `slope_to=` only for `kind=shed`.
    let d = exactly_one(diagnose(
        "struct s size=9x9\n  roof kind=gable slope_to=front\n",
    ));
    assert_eq!(d.code.as_str(), "W_IGNORED_ARGUMENT");
    assert!(d.primary.contains("`slope_to=`"), "got: {}", d.primary);
    assert!(d.primary.contains("`kind=shed`"), "got: {}", d.primary);
    assert!(d.primary.contains("`kind=gable`"), "got: {}", d.primary);
}

#[test]
fn the_conditional_finding_names_both_repair_sites() {
    let d = exactly_one(diagnose(
        "struct s size=9x9\n  roof kind=flat slope_to=front\n",
    ));
    let notes = notes(&d);
    assert!(notes.contains("write `kind=shed`"), "got: {notes}");
    assert!(notes.contains("drop `slope_to=`"), "got: {notes}");
}

#[test]
fn the_conditional_finding_points_at_the_value_it_is_about() {
    let src = "struct s size=9x9\n  roof kind=hip slope_to=left\n";
    let d = exactly_one(diagnose(src));
    assert_eq!(&src[d.span.clone()], "left");
}

#[test]
fn the_rule_that_reads_the_argument_lints_clean() {
    let src = "theme t:\n  slot roof -> @spruce_stairs\n\nstruct s size=9x9\n  roof kind=shed slope_to=front mat_slot=roof\n";
    assert_eq!(codes(src), Vec::<&str>::new(), "source:\n{src}");
}

/// The lowering's `W_DEFERRED_MEMBER` is the one finding, and `check` does not raise it.
#[test]
fn a_selector_that_names_no_rule_is_not_billed_twice() {
    // Absent.
    assert_eq!(
        codes("struct s size=9x9\n  roof slope_to=front\n"),
        Vec::<&str>::new(),
    );
    // Outside the dispatch table, spelled so no future roof kind can take it.
    assert_eq!(
        codes("struct s size=9x9\n  roof kind=not_a_roof_kind slope_to=front\n"),
        Vec::<&str>::new(),
    );
    // Not a word; `check::type_mismatch` does not cover `kind=`.
    assert_eq!(
        codes("struct s size=9x9\n  roof kind=2 slope_to=front\n"),
        Vec::<&str>::new(),
    );
}

#[test]
fn a_selector_matched_conditional_key_is_still_reported() {
    let src = "theme t:\n  slot roof -> @spruce_stairs\n  roof[slope_to=front] -> frame=@spruce_wood\n\nstruct s size=9x9\n  roof kind=gable slope_to=front mat_slot=roof\n";
    assert_eq!(ignored_at(src), ["@spruce_wood", "front"], "source:\n{src}");
    let found = diagnose(src);
    assert!(
        found.iter().any(|d| d
            .primary
            .contains("`slope_to=` is an argument `roof` reads only with")),
        "the argument's finding is the routed-past one, got {found:#?}",
    );
}

#[test]
fn place_takes_the_closed_set_the_spec_fixes_and_nothing_else() {
    // The set `spec/components-editing-sites` "Origin selectors" and "Cross-scope references" fix.
    let src = "def hut size=3x3:\n  floor mat_slot=floor\n\n\
               theme t:\n  slot floor -> @oak_planks\n\n\
               site v:\n  place id=a use=hut theme=t at=origin size=3x3\n";
    let d = exactly_one(diagnose(src));
    assert_eq!(d.code.as_str(), "E_UNKNOWN_ARGUMENT");
    assert!(d.primary.contains("`size=`"), "got: {}", d.primary);
}

/// One line per role, with every argument its vocabulary lists except its `unread_arguments`.
/// No `\` continuations: rustfmt would reindent them into the literal.
const SWEEP: &[(&str, &str)] = &[
    ("floor", "  floor id=f class=c mat_slot=m\n"),
    ("walls", "  walls id=w class=outer mat_slot=m height=3\n"),
    (
        "door",
        "  door id=d class=c side=front at=center opened_by=sig.a\n",
    ),
    (
        "window",
        "  window id=n class=c mat_slot=m side=front y=2 offset=1 size=1x1 sym=false repeat=1 step=1\n",
    ),
    (
        "roof",
        "  roof id=r class=c mat_slot=m kind=shed overhang=1 slope_to=front\n",
    ),
    (
        "stair",
        "  stair id=s class=c mat_slot=m kind=stairs side=front half=top facing=out shape=straight y=0\n",
    ),
    ("level", "  level id=l class=c y=0\n"),
    (
        "pressure_plate",
        "  pressure_plate id=p class=c mat_slot=m at=front.outside offset=0 y=0\n",
    ),
    ("circuit", "  circuit id=c class=c region=r void=2\n"),
    (
        "place",
        "  place id=a class=c use=hut theme=t at=origin\n  place id=b use=hut theme=t east_of=a gap=1\n  place id=c use=hut theme=t north_of=a gap=1\n",
    ),
    (
        "connect",
        "  connect id=e class=c a.entry to b.entry path=@gravel\n",
    ),
];

fn sweep_line(keyword: &str) -> &'static str {
    SWEEP
        .iter()
        .find(|(swept, _)| *swept == keyword)
        .unwrap_or_else(|| panic!("the sweep covers every keyword, and not `{keyword}`"))
        .1
}

fn keys_written(line: &str) -> Vec<&str> {
    line.split_whitespace()
        .filter_map(|word| word.split_once('='))
        .map(|(key, _)| key)
        .collect()
}

#[test]
fn the_table_and_the_sweep_agree_key_for_key() {
    for (keyword, line) in SWEEP {
        let role = role_of(keyword);
        let vocabulary = role
            .arguments()
            .unwrap_or_else(|| panic!("`{keyword}` is in the table, so it has a vocabulary"));
        let mut expected: Vec<&str> = vocabulary
            .iter()
            .chain(UNIVERSAL_ARGUMENTS)
            .copied()
            .filter(|key| role.unread_argument(key).is_none())
            .collect();
        expected.sort_unstable();

        let mut written = keys_written(line);
        written.sort_unstable();
        written.dedup();

        assert_eq!(
            written, expected,
            "`{keyword}`: the sweep line and `arguments()` disagree",
        );
    }
    // So a new role cannot land without a line.
    let mut swept: Vec<&str> = SWEEP.iter().map(|(keyword, _)| *keyword).collect();
    swept.sort_unstable();
    let mut known: Vec<&str> = known_keywords().to_vec();
    known.sort_unstable();
    assert_eq!(swept, known);
}

#[test]
fn a_selector_matched_key_is_still_reported_as_ignored() {
    let src = "theme t:\n  slot glass -> @glass_pane\n  window[shape=slit] -> frame=@spruce_wood\n\nstruct s size=9x7\n  window side=front y=1 size=1x2 shape=slit mat_slot=glass\n";
    assert_eq!(codes(src), ["W_IGNORED_ARGUMENT", "W_IGNORED_ARGUMENT"]);
    assert_eq!(ignored_at(src), ["@spruce_wood", "slit"]);
}

#[test]
fn a_selector_does_not_coin_a_word_one_edit_from_a_real_one() {
    let src = "theme t:\n  slot wall -> @wall.stone.cobble\n  walls[hieght=3] -> frame=@spruce_wood\n\nstruct s size=5x5\n  floor mat_slot=wall\n  walls hieght=3 mat_slot=wall\n";
    // Beside it, only the row's binding.
    let d = only_with_code(src, "E_UNKNOWN_ARGUMENT", 2);
    assert!(
        notes(&d).contains("did you mean `height`?"),
        "got: {}",
        notes(&d),
    );
    assert!(
        notes(&d).contains("typo written twice"),
        "got: {}",
        notes(&d),
    );
}

#[test]
fn a_selector_coins_a_word_two_edits_from_a_real_one() {
    let theme =
        "theme t:\n  slot wall -> @wall.stone.cobble\n  walls[hiegt=3] -> frame=@spruce_wood\n\n";
    // The member's own selector filters on a value it does not carry, so the row matches nothing.
    for (member, found) in [
        ("walls hiegt=3 height=3", &["W_IGNORED_ARGUMENT"][..]),
        (
            "walls[hiegt=3] height=3",
            &["E_THEME_SELECTOR_UNMATCHED", "W_IGNORED_ARGUMENT"][..],
        ),
    ] {
        let src =
            format!("{theme}struct s size=5x5\n  floor mat_slot=wall\n  {member} mat_slot=wall\n");
        assert_eq!(codes(&src), found, "source:\n{src}");
        assert_eq!(ignored_at(&src), ["@spruce_wood"], "source:\n{src}");
    }
}

#[test]
fn a_widened_key_appears_once_in_the_closed_set() {
    // `shape` is both in the role's vocabulary and selected on.
    let src = "theme t:\n  slot glass -> @glass_pane\n  window[shape=slit] -> frame=@spruce_wood\n\nstruct s size=9x7\n  window side=front y=1 size=1x2 shape=slit mat_slot=glass zzz=1\n";
    // Beside it, the row's binding and the unread `shape=slit`.
    let d = only_with_code(src, "E_UNKNOWN_ARGUMENT", 3);
    let listed = notes(&d);
    let line = listed
        .lines()
        .find(|l| l.starts_with("expected one of: "))
        .expect("the closed set is printed");
    let mut words: Vec<&str> = line["expected one of: ".len()..].split(", ").collect();
    let before = words.len();
    words.sort_unstable();
    words.dedup();
    assert_eq!(words.len(), before, "got: {line}");
}

#[test]
fn a_stair_reads_the_height_it_is_written_at() {
    let src = "theme t:\n  slot m -> @spruce_stairs\n\nstruct s size=9x9\n  stair kind=stairs mat_slot=m side=front half=top facing=out y=2\n";
    assert_eq!(codes(src), Vec::<&str>::new(), "source:\n{src}");
}

#[test]
fn every_argument_the_spec_writes_on_a_known_keyword_is_accepted_and_reported() {
    // Each is written on a known keyword in `spec/entities` "Anchor conventions" or the prose of
    // `spec/blockstate` "`waterlogged`".
    let rows = [
        ("window side=front y=2 offset=4 size=3x3", "shape", "arch"),
        (
            "window side=front y=2 offset=4 size=3x3",
            "anchor",
            "bottom_center",
        ),
        ("roof kind=gable", "footprint", "struct"),
        ("roof kind=gable", "bounds", "expand"),
        ("stair kind=stairs side=front", "waterlogged", "false"),
    ];
    for (line, key, value) in rows {
        let keyword = keyword_of(line);
        let src = format!("struct s size=9x9\n  {line} {key}={value}\n");
        let d = exactly_one(diagnose(&src));
        assert_eq!(d.code.as_str(), "W_IGNORED_ARGUMENT", "source:\n{src}");
        assert_eq!(
            d.primary,
            format!(
                "`{key}=` is an argument `{keyword}` takes and no pass reads yet; the value was \
                 ignored"
            ),
        );
        assert_eq!(&src[d.span.clone()], value, "source:\n{src}");
    }

    // `mat_slot` has tests of its own in `tests/lower/unread_mat_slot.rs`.
    let mut unreached: Vec<(&str, &str)> = known_keywords()
        .iter()
        .flat_map(|keyword| {
            role_of(keyword)
                .unread_arguments()
                .iter()
                .filter(|entry| entry.why == Unread::Unreached && entry.key != "mat_slot")
                .map(move |entry| (*keyword, entry.key))
        })
        .collect();
    unreached.sort_unstable();
    let mut written: Vec<(&str, &str)> = rows
        .iter()
        .map(|(line, key, _)| (keyword_of(line), *key))
        .collect();
    written.sort_unstable();
    assert_eq!(written, unreached);
}

#[test]
fn a_state_override_on_a_member_that_lays_many_blocks_is_refused() {
    // `spec/blockstate` "`waterlogged`" takes the key on a member that places one block.
    for line in [
        "roof kind=gable waterlogged=false",
        "window side=front y=2 offset=4 size=3x3 waterlogged=false",
    ] {
        let src = format!("struct s size=9x9\n  {line}\n");
        let d = exactly_one(diagnose(&src));
        assert_eq!(d.code.as_str(), "E_UNKNOWN_ARGUMENT", "source:\n{src}");
        assert_eq!(
            d.primary,
            format!(
                "`waterlogged=` is not an argument `{}` reads",
                keyword_of(line)
            ),
        );
        assert_eq!(&src[d.span.clone()], "false", "source:\n{src}");
    }
}

/// The keyword a member line opens with.
fn keyword_of(line: &str) -> &str {
    line.split(' ')
        .next()
        .expect("a member line opens with its keyword")
}

#[test]
fn every_argument_in_every_role_vocabulary_is_written_by_some_clean_source() {
    const PROLOGUE: &str = "theme t:\n  slot m -> @oak_planks\n\n";
    for (keyword, line) in SWEEP
        .iter()
        .filter(|(keyword, _)| !matches!(*keyword, "place" | "connect"))
    {
        let src = format!("{PROLOGUE}struct s size=9x9\n{line}");
        assert_eq!(
            codes(&src),
            Vec::<&str>::new(),
            "`{keyword}`'s own vocabulary must lint clean, source:\n{src}",
        );
    }

    let site = format!(
        "def hut size=3x3:\n  floor mat_slot=m\n  door id=entry side=front at=center\n\ntheme t:\n  slot m -> @oak_planks\n\nsite v:\n{}{}",
        sweep_line("place"),
        sweep_line("connect"),
    );
    assert_eq!(codes(&site), Vec::<&str>::new(), "source:\n{site}");
}

#[test]
fn the_vocabulary_tables_are_consistent_with_each_other() {
    for keyword in known_keywords() {
        let role = role_of(keyword);
        let vocabulary = role
            .arguments()
            .unwrap_or_else(|| panic!("`{keyword}` is in the table, so it has a vocabulary"));
        for UnreadArgument { key, why } in role.unread_arguments() {
            assert!(
                vocabulary.contains(key) || UNIVERSAL_ARGUMENTS.contains(key),
                "`{keyword}` calls `{key}` unread but does not accept it",
            );
            // A role's own key is spec-written, so its being unread is a gap, not `Inapplicable`.
            assert!(
                *why != Unread::Inapplicable || UNIVERSAL_ARGUMENTS.contains(key),
                "`{keyword}` calls its own `{key}` inapplicable",
            );
        }
        // `accepted_arguments` adds the universal keys, so a role restating one prints it twice.
        for key in vocabulary {
            assert!(
                !UNIVERSAL_ARGUMENTS.contains(key),
                "`{keyword}` lists the universal key `{key}` a second time",
            );
        }
    }
    // Otherwise the finding would quote a word the keyword does not take.
    for keyword in known_keywords() {
        let role = role_of(keyword);
        let vocabulary = role
            .arguments()
            .unwrap_or_else(|| panic!("`{keyword}` is in the table, so it has a vocabulary"));
        for axis in role.conditional_arguments() {
            assert!(
                vocabulary.contains(&axis.selector),
                "`{keyword}` dispatches on `{}`, which is not one of its arguments",
                axis.selector,
            );
            let mut seen: Vec<Option<&str>> = Vec::new();
            for arm in axis.arms {
                // Two `Absent` arms would be two rules for one line.
                let written = arm.value.ident();
                let spelled = written.map_or_else(
                    || format!("no `{}=`", axis.selector),
                    |value| format!("`{}={value}`", axis.selector),
                );
                assert!(
                    !seen.contains(&written),
                    "`{keyword}` lists {spelled} twice",
                );
                seen.push(written);
                for key in arm.reads {
                    assert!(
                        vocabulary.contains(key),
                        "`{keyword}` says {spelled} reads `{key}`, which is not one of its \
                         arguments",
                    );
                    // An arm consults the key, so `unread_arguments` must not list it.
                    assert!(
                        role.unread_argument(key).is_none(),
                        "`{keyword}` calls `{key}` unread and has {spelled} read it",
                    );
                }
            }
            assert!(
                !axis.is_conditional(axis.selector),
                "`{keyword}`'s `{}` selects the rule, so no arm reads it as a conditional key",
                axis.selector,
            );
        }
    }
    assert_eq!(
        MemberRole::Other("torch".to_owned()).arguments(),
        None,
        "an unknown keyword has no vocabulary rather than an empty one",
    );
}

// -- a member's own `[key=value]` -----------------------------------------
// Few passes read a selector's keys, so a typo there would lose the value silently.

#[test]
fn a_selector_key_no_member_of_the_role_carries_is_refused() {
    let d = exactly_one(diagnose(
        "struct s size=9x7\n  walls id=shell mat_slot=wall height=5\n  \
         window[clas=outer] side=front offset=2 y=2 size=2x2 mat_slot=glass\n",
    ));
    assert_eq!(d.code.as_str(), "E_UNKNOWN_ARGUMENT");
    assert!(d.primary.contains("`clas=`"), "got: {}", d.primary);
    assert!(
        d.primary.contains("`window`"),
        "the role is the vocabulary it answers to: {}",
        d.primary,
    );
    assert!(
        notes(&d).contains("did you mean `class`?"),
        "got: {}",
        notes(&d),
    );
}

#[test]
fn the_finding_lands_on_the_selector_attribute_that_carries_it() {
    let src = "struct s size=9x7\n  window[clas=outer] side=front offset=2 y=2 size=2x2\n";
    let d = exactly_one(diagnose(src));
    assert_eq!(
        &src[d.span.clone()],
        "outer",
        "the span should be the value the key was written against",
    );
}

#[test]
fn a_selector_key_the_role_does_carry_is_accepted() {
    // A universal key.
    assert_eq!(
        codes("struct s size=9x7\n  door[id=front] side=front at=center opened_by=sig.f\n"),
        Vec::<&str>::new(),
    );
    // A key of the role's own vocabulary.
    assert_eq!(
        codes("struct s size=9x7\n  window[side=front] side=front offset=2 y=2 size=2x2\n"),
        Vec::<&str>::new(),
    );
}

#[test]
fn a_theme_selector_widens_the_bracket_as_well_as_the_line() {
    // The member carries `tags=` on its line too, or the theme row would match nothing.
    let selected = "theme t:\n  slot glass -> @glass_pane\n  \
                    window[tags=[a,b]] -> frame=@spruce_wood\n\n\
                    struct s size=9x7\n  \
                    window[tags=[a,b]] tags=[a,b] side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
    assert_eq!(
        ignored_at(selected),
        ["@spruce_wood"],
        "only the row's binding"
    );
    assert_eq!(codes(selected), ["W_IGNORED_ARGUMENT"]);

    // A near-miss of the role's own vocabulary is still a typo, in the bracket as on the line.
    let near_miss = "theme t:\n  slot glass -> @glass_pane\n  \
                     window[sied=front] -> frame=@spruce_wood\n\n\
                     struct s size=9x7\n  \
                     window[sied=front] sied=front side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
    let found = diagnose(near_miss);
    assert_eq!(
        codes(near_miss),
        [
            "W_IGNORED_ARGUMENT",
            "E_UNKNOWN_ARGUMENT",
            "E_UNKNOWN_ARGUMENT"
        ],
        "the row's binding, then once on the line and once in the bracket",
    );
    for d in found
        .iter()
        .filter(|d| d.code == DiagnosticCode::UnknownArgument)
    {
        assert!(
            notes(d).contains("did you mean `side`?"),
            "got: {}",
            notes(d),
        );
    }
}

#[test]
fn an_unknown_keyword_answers_for_its_own_bracket() {
    assert_eq!(
        codes("struct s size=9x7\n  windwo[clas=outer] side=front\n"),
        ["E_UNKNOWN_KEYWORD"],
    );
}

/// A cottage with one theme selector row, whose binding the build never reads.
const COTTAGE_WITH_A_SELECTOR_ROW: &str = "theme medieval:\n  \
    slot floor -> @oak_planks\n  slot wall  -> @cobblestone\n  \
    slot roof  -> @spruce_stairs\n  slot glass -> @glass_pane\n  \
    window[class=small] -> frame=@spruce_wood\n\n\
    struct cottage size=9x7\n  floor  mat_slot=floor\n  \
    walls  class=outer mat_slot=wall height=4\n  door   side=front at=center\n  \
    window class=small side=front offset=2 y=2 size=2x2 sym=true mat_slot=glass\n  \
    roof   kind=gable mat_slot=roof overhang=1\n";

#[test]
fn a_theme_selector_binding_is_reported_as_ignored() {
    let src = COTTAGE_WITH_A_SELECTOR_ROW;
    let d = exactly_one(diagnose(src));
    assert_eq!(d.code.as_str(), "W_IGNORED_ARGUMENT");
    assert_eq!(&src[d.span.clone()], "@spruce_wood");
    assert!(
        d.primary.contains("`frame=`") && d.primary.contains("`window`"),
        "got: {}",
        d.primary,
    );
    assert!(
        d.primary.contains("no pass lowers a selector's bindings"),
        "got: {}",
        d.primary,
    );
    assert_eq!(
        notes(&d),
        "the build is the same with this binding or without it; delete it, or the whole row \
         if it binds nothing else, or keep it and expect no effect until selector bindings \
         are lowered",
    );
}

#[test]
fn every_binding_is_reported_whatever_its_key_or_value() {
    let src = "theme t:\n  slot glass -> @glass_pane\n  \
               window[class=small] -> frame=@spruce_wood fram=42\n\n\
               struct s size=9x7\n  \
               window class=small side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
    assert_eq!(ignored_at(src), ["@spruce_wood", "42"]);
    assert_eq!(codes(src), ["W_IGNORED_ARGUMENT", "W_IGNORED_ARGUMENT"]);
}

#[test]
fn the_first_value_of_a_key_repeated_in_one_row_is_reported_as_it_would_be_alone() {
    let row = |bindings: &str| {
        format!(
            "theme t:\n  slot glass -> @glass_pane\n  window[class=small] -> {bindings}\n\n\
             struct s size=9x7\n  \
             window class=small side=front offset=2 y=2 size=2x2 mat_slot=glass\n"
        )
    };
    let src = row("frame=@no_such_block frame=@spruce_wood");
    let found = diagnose(&src);
    let at: Vec<(&str, &str)> = found
        .iter()
        .map(|d| (d.code.as_str(), &src[d.span.clone()]))
        .collect();
    assert_eq!(
        at,
        [
            ("W_IGNORED_ARGUMENT", "@no_such_block"),
            ("E_DUPLICATE_ARG", "frame=@spruce_wood"),
            ("W_IGNORED_ARGUMENT", "@spruce_wood"),
        ],
    );
    assert_eq!(
        found[0],
        exactly_one(diagnose(&row("frame=@no_such_block")))
    );
}

#[test]
fn a_row_that_matches_nothing_is_reported_for_its_binding_too() {
    let src = "theme t:\n  slot glass -> @glass_pane\n  \
               window[class=big] -> frame=@spruce_wood\n\n\
               struct s size=9x7\n  \
               window class=small side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
    assert_eq!(ignored_at(src), ["@spruce_wood"]);
}

#[test]
fn a_binding_in_a_theme_no_scope_applies_is_reported_too() {
    // Two themes and no `theme=`, so neither is applied.
    let src = "theme a:\n  slot glass -> @glass_pane\n  \
               window[class=small] -> frame=@no_such_block\n\n\
               theme b:\n  slot glass -> @glass_pane\n\n\
               struct s size=9x7\n  \
               window class=small side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
    assert_eq!(
        ignored_at(src),
        ["@no_such_block"],
        "got {:#?}",
        diagnose(src)
    );
}

#[test]
fn a_binding_in_a_theme_whose_name_an_earlier_theme_took_is_reported_too() {
    let src = "theme t:\n  slot glass -> @glass_pane\n\n\
               theme t:\n  slot glass -> @glass_pane\n  \
               window[class=small] -> frame=@no_such_block frame=@spruce_wood\n\n\
               struct s size=9x7\n  \
               window class=small side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
    let found = diagnose(src);
    let at: Vec<(&str, &str)> = found
        .iter()
        .map(|d| (d.code.as_str(), &src[d.span.clone()]))
        .collect();
    assert_eq!(
        at,
        [
            ("E_DUPLICATE_ITEM", "t"),
            ("W_IGNORED_ARGUMENT", "@no_such_block"),
            ("E_DUPLICATE_ARG", "frame=@spruce_wood"),
            ("W_IGNORED_ARGUMENT", "@spruce_wood"),
        ],
    );
}

#[test]
fn a_selector_row_with_an_unknown_keyword_answers_only_for_the_keyword() {
    let src = "theme t:\n  slot glass -> @glass_pane\n  \
               windw[class=small] -> frame=@spruce_wood\n\n\
               struct s size=9x7\n  \
               window class=small side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
    assert_eq!(
        ignored_at(src),
        Vec::<&str>::new(),
        "got {:#?}",
        diagnose(src)
    );
    assert!(
        codes(src).contains(&"E_UNKNOWN_KEYWORD"),
        "got {:?}",
        codes(src)
    );
}

/// A theme declaring `wall` and `door`; `door` lets a fixture name a declared slot.
const SLOTS: &str = "theme t:\n  slot wall -> @oak_planks\n  slot door -> @iron_door\n\n";

/// Where a fixture's `mat_slot=` value goes.
const SLOT: &str = "%SLOT%";

/// One role whose `mat_slot=` nothing reads; the [`SLOT`] line is all its source reports.
struct UnreadSlot {
    keyword: &'static str,
    /// Everything after [`SLOTS`].
    body: &'static str,
    /// The kind of unread key the note is worded for, written by hand so a table change fails
    /// [`every_role_whose_mat_slot_is_unread_has_a_fixture`].
    why: Unread,
}

impl UnreadSlot {
    fn source(&self, value: &str) -> String {
        format!("{SLOTS}{}", self.body.replace(SLOT, value))
    }
}

const UNREAD_SLOTS: &[UnreadSlot] = &[
    UnreadSlot {
        keyword: "door",
        body: "struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  door side=front at=center mat_slot=%SLOT%\n",
        why: Unread::Unreached,
    },
    UnreadSlot {
        keyword: "level",
        body: "struct s size=7x5\n  floor mat_slot=wall\n  level y=0 mat_slot=%SLOT%\n    walls mat_slot=wall height=3\n",
        why: Unread::Inapplicable,
    },
    UnreadSlot {
        keyword: "circuit",
        body: "struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  circuit region=floor void=2 mat_slot=%SLOT%\n",
        why: Unread::Inapplicable,
    },
    UnreadSlot {
        keyword: "place",
        body: "def d size=5x5:\n  floor mat_slot=wall\n\nsite v:\n  place id=a use=d theme=t at=origin mat_slot=%SLOT%\n",
        why: Unread::Inapplicable,
    },
    UnreadSlot {
        keyword: "connect",
        body: "def hut size=3x3:\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  door id=entry side=front at=center\n\nsite v:\n  place id=a use=hut theme=t at=origin\n  place id=b use=hut theme=t east_of=a gap=4\n  connect a.entry to b.entry path=@gravel mat_slot=%SLOT%\n",
        why: Unread::Inapplicable,
    },
];

/// The sentence and repair note the finding should carry for `why`, spelled out in full.
fn expected_wording(keyword: &str, why: Unread) -> (String, String) {
    match why {
        Unread::Unreached => (
            format!(
                "`mat_slot=` is an argument `{keyword}` takes and no pass reads yet; the value \
                 was ignored"
            ),
            "the member is built without it — remove the argument, or keep it and expect no \
             effect until the lowering rule lands"
                .to_owned(),
        ),
        Unread::Inapplicable => (
            format!(
                "`mat_slot=` is accepted on every member, and nothing a `{keyword}` builds reads \
                 it; the value was ignored"
            ),
            format!(
                "the `{keyword}` is built the same with it or without it — remove the argument"
            ),
        ),
    }
}

/// The note every one of these carries, whichever kind it is.
const NOT_LOOKED_UP: &str =
    "the slot name is not looked up in the theme, so a misspelling of it is not caught";

#[test]
fn a_slot_name_on_a_role_that_reads_none_is_ignored_and_not_looked_up() {
    // `spec/lint` "Error vs warning": the key is carried but never consulted.
    for fixture in UNREAD_SLOTS {
        for name in ["nosuch", "door"] {
            let src = fixture.source(name);
            let keyword = fixture.keyword;
            let found = diagnose(&src);
            assert_eq!(
                found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
                ["W_IGNORED_ARGUMENT"],
                "{keyword} mat_slot={name}: {found:#?}",
            );
            let finding = &found[0];
            assert_eq!(&src[finding.span.clone()], name, "{keyword}: on the value");
            let (primary, note) = expected_wording(keyword, fixture.why);
            assert_eq!(finding.primary, primary, "{keyword}");
            assert_eq!(
                finding
                    .notes
                    .iter()
                    .map(|n| n.message.as_str())
                    .collect::<Vec<_>>(),
                [note.as_str(), NOT_LOOKED_UP],
                "{keyword}",
            );
        }
    }
}

#[test]
fn a_slot_value_that_is_no_label_is_the_type_mismatch_alone() {
    for fixture in UNREAD_SLOTS {
        let src = fixture.source("3");
        assert_eq!(
            codes(&src),
            ["E_TYPE_MISMATCH_LABEL"],
            "{}: {:#?}",
            fixture.keyword,
            diagnose(&src),
        );
    }
}

#[test]
fn a_repeated_slot_is_ignored_once_beside_the_duplicate() {
    for fixture in UNREAD_SLOTS {
        let src = fixture.source("a mat_slot=b");
        let found = diagnose(&src);
        assert_eq!(
            found.iter().map(|d| d.code.as_str()).collect::<Vec<_>>(),
            ["W_IGNORED_ARGUMENT", "E_DUPLICATE_ARG"],
            "{}: {found:#?}",
            fixture.keyword,
        );
        assert_eq!(&src[found[0].span.clone()], "a", "{}", fixture.keyword);
    }
}

#[test]
fn every_role_whose_mat_slot_is_unread_has_a_fixture() {
    let mut from_table: Vec<(&str, Unread)> = known_keywords()
        .iter()
        .filter_map(|keyword| {
            role_of(keyword)
                .unread_argument("mat_slot")
                .map(|why| (*keyword, why))
        })
        .collect();
    from_table.sort_unstable_by_key(|(keyword, _)| *keyword);
    let mut from_fixtures: Vec<(&str, Unread)> = UNREAD_SLOTS
        .iter()
        .map(|fixture| (fixture.keyword, fixture.why))
        .collect();
    from_fixtures.sort_unstable_by_key(|(keyword, _)| *keyword);
    assert_eq!(from_fixtures, from_table);
}

#[test]
fn a_slot_name_the_theme_lacks_on_a_role_that_reads_it_is_still_refused() {
    let src = format!(
        "{SLOTS}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  window side=front y=2 offset=2 size=2x1 mat_slot=nosuch\n"
    );
    assert_eq!(codes(&src), ["E_UNRESOLVED_SLOT"], "{:#?}", diagnose(&src));
}

#[test]
fn an_unknown_keyword_still_has_its_slot_looked_up() {
    let src = format!("{SLOTS}struct s size=7x5\n  floor mat_slot=wall\n  torch mat_slot=nosuch\n");
    assert_eq!(
        codes(&src),
        ["E_UNKNOWN_KEYWORD", "E_UNRESOLVED_SLOT"],
        "{:#?}",
        diagnose(&src),
    );
}

fn codes_under_java(source: &str) -> Vec<&'static str> {
    let module = parse(source).expect("parse");
    check(&module, &lower(&module), Some(Edition::Java))
        .iter()
        .map(|d| d.code.as_str())
        .collect()
}

#[test]
fn a_slot_nothing_reads_does_not_starve_a_struct_of_a_theme_but_a_place_still_binds_one() {
    // A Bedrock-only theme under a Java pin, read only by a `mat_slot=` nothing consults.
    let theme = "theme t_bedrock:\n  slot wall -> @oak_planks\n\n";
    let in_a_struct = format!(
        "{theme}struct s size=5x5\n  pressure_plate at=front.outside offset=0 y=0\n  circuit region=floor void=2 mat_slot=wall\n"
    );
    assert_eq!(
        codes_under_java(&in_a_struct),
        ["W_IGNORED_ARGUMENT"],
        "source:\n{in_a_struct}",
    );
    // A theme a `place` binds is recorded in the lockfile whatever its def reads.
    let def = "def d size=5x5:\n  pressure_plate at=front.outside offset=0 y=0\n\n";
    let with_slot =
        format!("{theme}{def}site v:\n  place id=a use=d theme=t at=origin mat_slot=wall\n");
    assert_eq!(
        codes_under_java(&with_slot),
        ["E_THEME_VARIANT_MISSING", "W_IGNORED_ARGUMENT"],
        "source:\n{with_slot}",
    );
    let without_slot = format!("{theme}{def}site v:\n  place id=a use=d theme=t at=origin\n");
    assert_eq!(
        codes_under_java(&without_slot),
        ["E_THEME_VARIANT_MISSING"],
        "source:\n{without_slot}",
    );
}

#[test]
fn an_actuator_key_on_a_member_that_does_not_read_it_names_its_component() {
    let expected_window = "expected one of: side, y, offset, size, sym, repeat, step, shape, \
                           anchor, id, class, mat_slot";
    for (line, host_note) in [
        (
            "window side=front y=1 offset=1 size=1x1 opened_by=sig.a",
            "`opened_by=` binds a signal to a `door`, so it belongs on a `door` line",
        ),
        (
            "window side=front y=1 offset=1 size=1x1 powered_by=sig.a",
            "`powered_by=` binds a signal to a `piston`, which is not a keyword yet",
        ),
    ] {
        let src = format!("struct s size=5x5\n  {line}\n");
        let found = exactly_one(of_code(&src, DiagnosticCode::UnknownArgument));
        let found: Vec<&str> = found.notes.iter().map(|n| n.message.as_str()).collect();
        assert_eq!(found, [expected_window, host_note], "for `{line}`");
    }
}
