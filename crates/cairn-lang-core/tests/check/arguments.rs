//! `E_UNKNOWN_ARGUMENT`: a `key=` that no member of that role reads is an error
//! at the key, so a misspelt argument is named itself rather than surfacing
//! later as whatever the member lacks.

use cairn_lang_core::intent::{
    MemberRole, UNIVERSAL_ARGUMENTS, Unread, UnreadArgument, known_keywords, role_of,
};
use cairn_lang_core::{Diagnostic, DiagnosticCode, Edition, check, lower, parse};

use crate::common::{codes, diagnose, exactly_one};

/// The one finding `source` raises with `code`, beside exactly `total`
/// findings in all, or a panic naming every finding it raised.
///
/// For a source that also earns, say, a selector row's `W_IGNORED_ARGUMENT`
/// or an unplaced `def`'s `W_UNUSED_DEF`. Asserting the total still catches
/// a finding added or lost around the one under test.
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
    // The anchor is the value, not the key, because the IR carries no span
    // for the key; the message names the key, so the two read as one
    // argument.
    let src = "struct s size=5x5\n  walls class=outer mat_slot=wall hieght=3\n";
    let d = exactly_one(diagnose(src));
    assert_eq!(&src[d.span.clone()], "3");
    assert!(d.primary.contains("`hieght=`"), "got: {}", d.primary);
}

#[test]
fn the_suggestion_is_drawn_from_the_role_and_from_the_universal_keys() {
    // `class` is a universal key, not one of `walls`' own, so only a
    // candidate list that includes the universal keys can offer it.
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
    // `floor` reads nothing of its own, so the closed set is the universal
    // keys alone, and it is still printed.
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
    // The keyword is the mistake, and its arguments have no vocabulary to
    // be judged against.
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
    // A `def` body is walked by a loop of its own.
    let src = "def hut size=3x3:\n  walls hieght=3 mat_slot=m\n\n\
               theme t:\n  slot m -> @oak_planks\n\n\
               site v:\n  place id=a use=hut theme=t at=origin\n";
    let d = exactly_one(diagnose(src));
    assert_eq!(d.code.as_str(), "E_UNKNOWN_ARGUMENT");
    assert!(d.primary.contains("`hieght=`"), "got: {}", d.primary);
}

#[test]
fn a_struct_header_key_outside_its_vocabulary_is_refused_with_the_closed_set() {
    // The note offers the header's closed set, not the `floor` member's, so
    // there is no suggestion: `hieght` is too far from `size` and `class`,
    // and `height` is a member's key.
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
    // `spec/components-editing-sites` "`def`, the component construct"
    // writes `def cottage class=house size=9x7:`, so refusing the unread
    // `class=` would refuse the spec. The unplaced `def` also earns
    // `W_UNUSED_DEF`.
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
    // `class` is in the header vocabulary although no pass reads it; that
    // it is unread is the next finding, not this one.
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
    // A `size=` that is not `WxH` stays in the header's residual arguments,
    // but the key is the right word; the type check reports the value.
    assert_eq!(
        codes("struct s size=5\n  floor mat_slot=m\n"),
        ["E_TYPE_MISMATCH_SIZE"],
    );
}

#[test]
fn a_theme_selector_widens_the_vocabulary_of_the_keyword_it_names() {
    // No lowering rule reads `tags=`, but the resolver's selector matcher
    // does, so in a module that selects on it, writing it is not a mistake.
    // What remains is the row's own binding, which nothing lowers.
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
    // Widening every keyword from any selector row would let one theme line
    // excuse a typo on another member. The window is there so the row
    // matches and raises no `E_THEME_SELECTOR_UNMATCHED`.
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
    // `window shape=` is in `spec/components-editing-sites` "Editing model"
    // and no pass reads it. The key is the author's to write and the gap is
    // the implementation's, so it is a warning.
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
    // Either argument may be the mistake, which is why this is a warning;
    // naming only one would pick for the author.
    let d = exactly_one(diagnose(
        "struct s size=9x9\n  roof kind=flat slope_to=front\n",
    ));
    let notes = notes(&d);
    assert!(notes.contains("write `kind=shed`"), "got: {notes}");
    assert!(notes.contains("drop `slope_to=`"), "got: {notes}");
}

#[test]
fn the_conditional_finding_points_at_the_value_it_is_about() {
    // Not at the `kind=`: the likeliest repair is on the ignored argument,
    // and the message quotes the selector.
    let src = "struct s size=9x9\n  roof kind=hip slope_to=left\n";
    let d = exactly_one(diagnose(src));
    assert_eq!(&src[d.span.clone()], "left");
}

#[test]
fn the_rule_that_reads_the_argument_lints_clean() {
    let src = "theme t:\n  slot roof -> @spruce_stairs\n\nstruct s size=9x9\n  roof kind=shed slope_to=front mat_slot=roof\n";
    assert_eq!(codes(src), Vec::<&str>::new(), "source:\n{src}");
}

/// The one finding is the lowering's `W_DEFERRED_MEMBER`, which `check`
/// does not run; `lower/conditional_arguments.rs` pins that it is raised.
#[test]
fn a_selector_that_names_no_rule_is_not_billed_twice() {
    // Absent.
    assert_eq!(
        codes("struct s size=9x9\n  roof slope_to=front\n"),
        Vec::<&str>::new(),
    );
    // Outside the dispatch table, spelled so no future roof kind can take
    // it.
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
    // Selecting on `slope_to=` does not make the roof read it, and the
    // row's binding is lowered by nothing either.
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
    // `spec/components-editing-sites` "Origin selectors" and "Cross-scope
    // references": a name, what to instantiate, what to resolve materials
    // against, and exactly one origin selector. "`def`, the component
    // construct" reserves parameterisation, which nothing forwards.
    let src = "def hut size=3x3:\n  floor mat_slot=floor\n\n\
               theme t:\n  slot floor -> @oak_planks\n\n\
               site v:\n  place id=a use=hut theme=t at=origin size=3x3\n";
    let d = exactly_one(diagnose(src));
    assert_eq!(d.code.as_str(), "E_UNKNOWN_ARGUMENT");
    assert!(d.primary.contains("`size=`"), "got: {}", d.primary);
}

/// One line per role, carrying every argument that role's vocabulary lists
/// except its `unread_arguments`, which lint as `W_IGNORED_ARGUMENT`.
///
/// Each line is one literal with no `\` continuation: `rustfmt` reflows a
/// continuation when the line grows, and the continued line's leading
/// spaces then land inside the literal as indentation.
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

/// The sweep line for one keyword.
fn sweep_line(keyword: &str) -> &'static str {
    SWEEP
        .iter()
        .find(|(swept, _)| *swept == keyword)
        .unwrap_or_else(|| panic!("the sweep covers every keyword, and not `{keyword}`"))
        .1
}

/// The `key=` names a sweep line writes, in source order.
fn keys_written(line: &str) -> Vec<&str> {
    line.split_whitespace()
        .filter_map(|word| word.split_once('='))
        .map(|(key, _)| key)
        .collect()
}

/// A key a reader reads and the table omits refuses a source the compiler
/// is built to accept, and neither the shipped examples nor a lowering test
/// that never reaches `check` would see it.
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
    // Selecting on `shape=` does not make the lowering read it, and the
    // row's binding is lowered by nothing either.
    let src = "theme t:\n  slot glass -> @glass_pane\n  window[shape=slit] -> frame=@spruce_wood\n\nstruct s size=9x7\n  window side=front y=1 size=1x2 shape=slit mat_slot=glass\n";
    assert_eq!(codes(src), ["W_IGNORED_ARGUMENT", "W_IGNORED_ARGUMENT"]);
    assert_eq!(ignored_at(src), ["@spruce_wood", "slit"]);
}

#[test]
fn a_selector_does_not_coin_a_word_one_edit_from_a_real_one() {
    // Otherwise the only finding would be a `W_DEFERRED_MEMBER` about the
    // absent `height=`.
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
    // The shipped `stair` line carries no `y=`.
    let src = "theme t:\n  slot m -> @spruce_stairs\n\nstruct s size=9x9\n  stair kind=stairs mat_slot=m side=front half=top facing=out y=2\n";
    assert_eq!(codes(src), Vec::<&str>::new(), "source:\n{src}");
}

#[test]
fn every_argument_the_spec_writes_on_a_known_keyword_is_accepted_and_reported() {
    // The worked example in `spec/entities` "Anchor conventions" writes
    // these four, which no pass reads, on known keywords. Its `painting`
    // line is not a keyword, and `E_UNKNOWN_KEYWORD` owns it.
    for (line, key) in [
        (
            "  window side=front y=2 offset=4 size=3x3 shape=arch\n",
            "shape",
        ),
        (
            "  window side=front y=2 offset=4 size=3x3 anchor=bottom_center\n",
            "anchor",
        ),
        ("  roof kind=gable footprint=struct\n", "footprint"),
        ("  roof kind=gable bounds=expand\n", "bounds"),
    ] {
        let src = format!("struct s size=9x9\n{line}");
        let d = exactly_one(diagnose(&src));
        assert_eq!(d.code.as_str(), "W_IGNORED_ARGUMENT", "source:\n{src}");
        assert!(
            d.primary.contains(&format!("`{key}=`")),
            "got: {}",
            d.primary,
        );
    }
}

/// A key the table lists that no source can legally carry fails here.
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

    // `place` and `connect` live in a `site` body and need a `def` to place.
    let site = format!(
        "def hut size=3x3:\n  floor mat_slot=m\n  door id=entry side=front at=center\n\ntheme t:\n  slot m -> @oak_planks\n\nsite v:\n{}{}",
        sweep_line("place"),
        sweep_line("connect"),
    );
    assert_eq!(codes(&site), Vec::<&str>::new(), "source:\n{site}");
}

/// The invariants the vocabulary tables hold about each other: `unread` is a
/// subset of what the role accepts and an inapplicable entry is a universal
/// key, no role restates a universal key, the conditional table's selectors
/// and keys are arguments the role accepts, and only an unknown keyword
/// declines to answer.
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
            // A role's own key is there because the spec wrote it on this
            // keyword, so nothing reading it is a gap, not `Inapplicable`.
            assert!(
                *why != Unread::Inapplicable || UNIVERSAL_ARGUMENTS.contains(key),
                "`{keyword}` calls its own `{key}` inapplicable",
            );
        }
        // The universal keys are added by `accepted_arguments`, so listing
        // one in a role's own vocabulary would print it twice in the note.
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
                // `Absent` is one arm like any other: two of them would be
                // two rules for the same line.
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
                    // `unread_arguments` says no rule consults a key, and
                    // an arm here says one does.
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
//
// Only the `door` actuator-patch recogniser and redstone's binding-key walk
// read a selector's keys, so a typo in one would lose the value silently.

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
    // `id` is a universal key. Whether the selector binds a fresh id or
    // references a member is undecided, and the word is read either way.
    assert_eq!(
        codes("struct s size=9x7\n  door[id=front] side=front at=center opened_by=sig.f\n"),
        Vec::<&str>::new(),
    );
    // A key of the role's own vocabulary, likewise.
    assert_eq!(
        codes("struct s size=9x7\n  window[side=front] side=front offset=2 y=2 size=2x2\n"),
        Vec::<&str>::new(),
    );
}

#[test]
fn a_theme_selector_widens_the_bracket_as_well_as_the_line() {
    // The member carries `tags=` as an argument too: the theme selector
    // matches arguments, not the member's own bracket, so without it the
    // row would be `E_THEME_SELECTOR_UNMATCHED` and widen nothing.
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

    // A near-miss of the role's own vocabulary is a typo written twice, in
    // the bracket as on the line.
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
    // The keyword is the repair; its selector has no vocabulary to answer
    // to.
    assert_eq!(
        codes("struct s size=9x7\n  windwo[clas=outer] side=front\n"),
        ["E_UNKNOWN_KEYWORD"],
    );
}

/// A cottage with one theme selector row, whose binding the build never
/// reads.
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
    // The note makes no claim about the members the row selects, since it is
    // printed on a row that matched nothing too.
    assert_eq!(
        notes(&d),
        "the build is the same with this binding or without it; delete it, or the whole row \
         if it binds nothing else, or keep it and expect no effect until selector bindings \
         are lowered",
    );
}

#[test]
fn every_binding_is_reported_whatever_its_key_or_value() {
    // No pass reads a binding, so there is no vocabulary of keys one may
    // set.
    let src = "theme t:\n  slot glass -> @glass_pane\n  \
               window[class=small] -> frame=@spruce_wood fram=42\n\n\
               struct s size=9x7\n  \
               window class=small side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
    assert_eq!(ignored_at(src), ["@spruce_wood", "42"]);
    assert_eq!(codes(src), ["W_IGNORED_ARGUMENT", "W_IGNORED_ARGUMENT"]);
}

#[test]
fn a_row_that_matches_nothing_is_reported_for_its_binding_too() {
    // Matched or not, the binding reaches no block.
    let src = "theme t:\n  slot glass -> @glass_pane\n  \
               window[class=big] -> frame=@spruce_wood\n\n\
               struct s size=9x7\n  \
               window class=small side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
    assert_eq!(ignored_at(src), ["@spruce_wood"]);
}

#[test]
fn a_binding_in_a_theme_no_scope_applies_is_reported_too() {
    // Two themes and no `theme=`: neither is applied, so
    // `E_THEME_SELECTOR_UNMATCHED` stays quiet, and the binding is reported
    // from the rows themselves.
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
fn a_selector_row_with_an_unknown_keyword_answers_only_for_the_keyword() {
    // As for a member, the keyword is the repair.
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

/// A theme declaring `wall` and `door`, for the fixtures below. `door` is
/// there so a fixture can name a slot the theme does declare and still be
/// told it is ignored.
const SLOTS: &str = "theme t:\n  slot wall -> @oak_planks\n  slot door -> @iron_door\n\n";

/// Where a fixture's `mat_slot=` value goes.
const SLOT: &str = "%SLOT%";

/// One role whose `mat_slot=` nothing reads, written so that the line
/// carrying [`SLOT`] is the only thing in the source anything reports.
struct UnreadSlot {
    keyword: &'static str,
    /// Everything after [`SLOTS`].
    body: &'static str,
    /// The kind of unread key the note is worded for. Written here by hand
    /// rather than read from the table, so a table that moves a role to the
    /// other kind fails [`every_role_whose_mat_slot_is_unread_has_a_fixture`]
    /// instead of rewording the note under a test that agrees with it.
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

/// What the finding says, by kind: the sentence, and the note that carries
/// the repair. Spelled out in full so a note worded for the other kind
/// fails on its text.
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

/// The note every one of these carries, whichever kind it is: what not
/// looking the name up costs.
const NOT_LOOKED_UP: &str =
    "the slot name is not looked up in the theme, so a misspelling of it is not caught";

#[test]
fn a_slot_name_on_a_role_that_reads_none_is_ignored_and_not_looked_up() {
    // `spec/lint` "Error vs warning": the key is carried and never
    // consulted, and the name is not looked up, so a slot the theme lacks
    // refuses nothing and one it declares is ignored all the same.
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
    // A `mat_slot=` that is not a label is not hoisted, and the key is not
    // the mistake; reporting it as ignored too would put "write a label"
    // and "drop it" on one span.
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
    // The first occurrence is hoisted and reported as ignored; the second
    // stays in the fields, where `E_DUPLICATE_ARG` is its finding.
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

/// A role that joins the table without a fixture fails here, and so does
/// one that moves to the other kind without its fixture.
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
    // Not looking the name up is per role.
    let src = format!(
        "{SLOTS}struct s size=7x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  window side=front y=2 offset=2 size=2x1 mat_slot=nosuch\n"
    );
    assert_eq!(codes(&src), ["E_UNRESOLVED_SLOT"], "{:#?}", diagnose(&src));
}

#[test]
fn an_unknown_keyword_still_has_its_slot_looked_up() {
    // An unknown keyword has no unread arguments, and the slot finding is
    // one the author needs once the keyword is repaired.
    let src = format!("{SLOTS}struct s size=7x5\n  floor mat_slot=wall\n  torch mat_slot=nosuch\n");
    assert_eq!(
        codes(&src),
        ["E_UNKNOWN_KEYWORD", "E_UNRESOLVED_SLOT"],
        "{:#?}",
        diagnose(&src),
    );
}

/// The full finding list `source` raises under `--edition java`.
fn codes_under_java(source: &str) -> Vec<&'static str> {
    let module = parse(source).expect("parse");
    check(&module, &lower(&module), Some(Edition::Java))
        .iter()
        .map(|d| d.code.as_str())
        .collect()
}

#[test]
fn a_slot_nothing_reads_does_not_starve_a_struct_of_a_theme_but_a_place_still_binds_one() {
    // A theme with only a Bedrock variant, under a Java pin. The struct's
    // only `mat_slot=` is one nothing reads, so no slot is starved.
    let theme = "theme t_bedrock:\n  slot wall -> @oak_planks\n\n";
    let in_a_struct = format!(
        "{theme}struct s size=5x5\n  pressure_plate at=front.outside offset=0 y=0\n  circuit region=floor void=2 mat_slot=wall\n"
    );
    assert_eq!(
        codes_under_java(&in_a_struct),
        ["W_IGNORED_ARGUMENT"],
        "source:\n{in_a_struct}",
    );
    // A theme a `place` binds is more than its slots: the lockfile records
    // it and its version floor applies, whatever its def reads.
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
