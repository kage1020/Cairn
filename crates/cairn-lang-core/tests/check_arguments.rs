//! `E_UNKNOWN_ARGUMENT` — a `key=` no member of that role reads.
//!
//! The keyword allowlist has always answered this question one level up. A
//! misspelled *argument* was accepted in silence, and the only thing the
//! author eventually saw was a `W_DEFERRED_MEMBER` naming the argument that
//! is now absent rather than the one that is wrong.

use cairn_lang_core::intent::{MemberRole, UNIVERSAL_ARGUMENTS, known_keywords, role_of};
use cairn_lang_core::{Diagnostic, check, lower, parse};

fn diagnose(source: &str) -> Vec<Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    check(&module, &ir, None)
}

fn codes(source: &str) -> Vec<&'static str> {
    diagnose(source).iter().map(|d| d.code.as_str()).collect()
}

/// The one finding `source` is expected to raise, or a panic naming what it
/// raised instead.
fn only(source: &str) -> Diagnostic {
    let diags = diagnose(source);
    assert_eq!(
        diags.len(),
        1,
        "expected exactly one finding, got {diags:#?}"
    );
    diags.into_iter().next().expect("length checked above")
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
    // The issue's own repro. One letter, and the wall is built without the
    // height it asked for.
    let d = only("struct s size=5x5\n  walls class=outer mat_slot=wall hieght=3\n");
    assert_eq!(d.code.as_str(), "E_UNKNOWN_ARGUMENT");
    assert!(d.primary.contains("`hieght=`"), "got: {}", d.primary);
    assert!(d.primary.contains("`walls`"), "got: {}", d.primary);
}

#[test]
fn the_finding_points_at_the_argument_and_not_at_the_line() {
    // The member may carry several arguments and only one of them is
    // wrong; a span covering the whole statement would make the reader
    // find it again. The anchor is the value rather than the key, because
    // the key's own byte range is not carried into the IR — `IntentState`
    // maps a key to its value and the value's span — and the message names
    // the key, so the two read as one argument between them.
    let src = "struct s size=5x5\n  walls class=outer mat_slot=wall hieght=3\n";
    let d = only(src);
    assert_eq!(&src[d.span.clone()], "3");
    assert!(d.primary.contains("`hieght=`"), "got: {}", d.primary);
}

#[test]
fn the_suggestion_is_drawn_from_the_role_and_from_the_universal_keys() {
    // `class` never reaches the dedicated field here — `intent::lower`
    // hoists it only when the value is label-shaped, and a *misspelled*
    // key is not the key at all — so the repair is a word the role's own
    // arguments do not contain. A candidate list without the universal
    // keys could not offer it.
    let role_key = only("struct s size=5x5\n  walls mat_slot=wall hieght=3\n");
    assert!(
        notes(&role_key).contains("did you mean `height`?"),
        "got: {}",
        notes(&role_key),
    );
    let universal = only("struct s size=5x5\n  walls height=3 clas=outer\n");
    assert!(
        notes(&universal).contains("did you mean `class`?"),
        "got: {}",
        notes(&universal),
    );
}

#[test]
fn a_key_nothing_resembles_is_named_with_the_closed_set_and_no_suggestion() {
    let d = only("struct s size=5x5\n  floor totally_unrelated=1\n");
    let notes = notes(&d);
    assert!(!notes.contains("did you mean"), "got: {notes}");
    // `floor` reads nothing of its own, so the note is the universal keys
    // alone — and it still has to be printed, or the message names no
    // alternative at all.
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
    // Two findings for one repair is what this avoids: the keyword is the
    // mistake, and `torch`'s arguments have no vocabulary to be judged
    // against until `torch` is one.
    assert_eq!(
        codes("struct s size=5x5\n  torch brightness=15\n"),
        ["E_UNKNOWN_KEYWORD"],
    );
}

#[test]
fn a_nested_member_is_checked_at_any_depth() {
    let d = only("struct s size=5x5\n  level y=0\n    walls hieght=3\n");
    assert_eq!(d.code.as_str(), "E_UNKNOWN_ARGUMENT");
}

#[test]
fn a_member_inside_a_def_is_checked_like_any_other() {
    // A `def` body is walked by a loop of its own, and a `def` is where a
    // typo costs the most: the component is instantiated once per `place`,
    // so one misspelled key builds every copy wrong.
    let src = "def hut size=3x3:\n  walls hieght=3\n\n\
               theme t:\n  slot m -> @oak_planks\n\n\
               site v:\n  place id=a use=hut theme=t at=origin\n";
    let d = only(src);
    assert_eq!(d.code.as_str(), "E_UNKNOWN_ARGUMENT");
    assert!(d.primary.contains("`hieght=`"), "got: {}", d.primary);
}

#[test]
fn a_theme_selector_widens_the_vocabulary_of_the_keyword_it_names() {
    // `tags=` is read by nothing in the lowering, and it is read: the
    // resolver's selector matcher reads it. The rule is "a word nothing
    // will read", so a module that selects on the key is a module where
    // writing it is not a mistake.
    let selected = "theme t:\n  slot glass -> @glass_pane\n  \
                    window[tags=[a,b]] -> frame=@spruce_wood\n\n\
                    struct s size=9x7\n  \
                    window tags=[a,b] side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
    assert_eq!(codes(selected), Vec::<&str>::new());

    // The same line without the selector row is the ordinary case again.
    let unselected = "theme t:\n  slot glass -> @glass_pane\n\n\
                      struct s size=9x7\n  \
                      window tags=[a,b] side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
    assert_eq!(codes(unselected), ["E_UNKNOWN_ARGUMENT"]);
}

#[test]
fn a_selector_widens_one_keyword_and_not_the_others() {
    // `window[tags=...]` says something reads `tags=` on a window. It says
    // nothing about a door, and widening every keyword from any selector
    // row would make one theme line excuse a typo three members away.
    // The selector row needs a window to match, or its own
    // `E_THEME_SELECTOR_UNMATCHED` joins the finding under test.
    let src = "theme t:\n  slot glass -> @glass_pane\n  \
               window[tags=[a,b]] -> frame=@spruce_wood\n\n\
               struct s size=9x7\n  \
               window tags=[a,b] side=front offset=2 y=2 size=2x2 mat_slot=glass\n  \
               door side=front at=center tags=[a,b]\n";
    let d = only(src);
    assert_eq!(d.code.as_str(), "E_UNKNOWN_ARGUMENT");
    assert!(d.primary.contains("`door`"), "got: {}", d.primary);
}

#[test]
fn an_argument_the_spec_defines_and_nothing_reads_is_reported_as_ignored() {
    // `window shape=` is in `spec/components-editing-sites` §9.2 and no
    // pass reads it. Refusing it would refuse the spec; accepting it in
    // silence is the defect this whole pass is about, one door along. The
    // key is the author's to write and the gap is the implementation's, so
    // it is a warning and it names the consequence.
    let d = only("struct s size=9x7\n  window side=front y=2 offset=2 size=2x2 shape=arch\n");
    assert_eq!(d.code.as_str(), "W_IGNORED_ARGUMENT");
    assert!(d.primary.contains("`shape=`"), "got: {}", d.primary);
    assert!(
        d.primary.contains("no pass reads yet"),
        "got: {}",
        d.primary,
    );
}

#[test]
fn place_takes_the_closed_set_the_spec_fixes_and_nothing_else() {
    // `spec/components-editing-sites` §9.3.2 / §9.3.3: a name, what to
    // instantiate, what to resolve materials against, and exactly one
    // origin selector. §9.1 reserves parameterisation, which nothing
    // forwards today — the day it lands, this is the arm that opens.
    let src = "def hut size=3x3:\n  floor mat_slot=floor\n\n\
               theme t:\n  slot floor -> @oak_planks\n\n\
               site v:\n  place id=a use=hut theme=t at=origin size=3x3\n";
    let d = only(src);
    assert_eq!(d.code.as_str(), "E_UNKNOWN_ARGUMENT");
    assert!(d.primary.contains("`size=`"), "got: {}", d.primary);
}

/// Every key in every role's vocabulary, written on a member of that role,
/// in a source that has to lint clean.
///
/// Two things at once. A key a reader reads and the table omits fails the
/// corpus — the shipped examples are held to `cairn check` cleanliness —
/// and a key the table lists that no source can legally carry fails here,
/// so neither direction of the table can be edited without a source to go
/// with it.
#[test]
fn every_argument_in_every_role_vocabulary_is_written_by_some_clean_source() {
    const PROLOGUE: &str = "theme t:\n  slot m -> @oak_planks\n\n";
    // One line per role, carrying every argument that role's vocabulary
    // lists. `unread_arguments` are excluded: they lint as
    // `W_IGNORED_ARGUMENT` by design, and
    // `an_argument_the_spec_defines_and_nothing_reads_is_reported_as_ignored`
    // is where they are asked about.
    let lines: &[(&str, &str)] = &[
        ("floor", "  floor id=f class=c mat_slot=m\n"),
        ("walls", "  walls id=w class=outer mat_slot=m height=3\n"),
        (
            "door",
            "  door id=d class=c mat_slot=m side=front at=center opened_by=sig.a\n",
        ),
        (
            "window",
            "  window id=n class=c mat_slot=m side=front y=2 offset=1 size=1x1 sym=false \
             repeat=1 step=1\n",
        ),
        (
            "roof",
            "  roof id=r class=c mat_slot=m kind=shed overhang=1 slope_to=front\n",
        ),
        (
            "stair",
            "  stair id=s class=c mat_slot=m kind=stairs side=front half=top facing=out \
             shape=straight\n",
        ),
        ("level", "  level id=l class=c mat_slot=m y=0\n"),
        (
            "pressure_plate",
            "  pressure_plate id=p class=c mat_slot=m at=front.outside offset=0 y=0\n",
        ),
        (
            "circuit",
            "  circuit id=c class=c mat_slot=m region=r void=2\n",
        ),
    ];
    for (keyword, line) in lines {
        let src = format!("{PROLOGUE}struct s size=9x9\n{line}");
        assert_eq!(
            codes(&src),
            Vec::<&str>::new(),
            "`{keyword}`'s own vocabulary must lint clean, source:\n{src}",
        );
    }

    // `place` and `connect` live in a `site` body and need a `def` to
    // instantiate, so they take a prologue of their own rather than a
    // thirteenth line above.
    let site = "def hut size=3x3:\n  floor mat_slot=m\n\n\
                theme t:\n  slot m -> @oak_planks\n\n\
                site v:\n\
                \x20\x20place id=a use=hut theme=t at=origin\n\
                \x20\x20place id=b use=hut theme=t east_of=a gap=1\n\
                \x20\x20place id=c use=hut theme=t north_of=a gap=1\n";
    assert_eq!(codes(site), Vec::<&str>::new(), "source:\n{site}");
}

/// Every key any role lists is covered by the sweep above or by a test of
/// its own, so no entry in the table is dead.
#[test]
fn no_argument_in_the_table_is_unreachable() {
    for keyword in known_keywords() {
        let role = role_of(keyword);
        let vocabulary = role
            .arguments()
            .unwrap_or_else(|| panic!("`{keyword}` is in the table, so it has a vocabulary"));
        for unread in role.unread_arguments() {
            assert!(
                vocabulary.contains(unread),
                "`{keyword}` calls `{unread}` unread but does not list it",
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
    // And the keyword table and the vocabulary table describe the same
    // roles: a role reachable from a keyword must answer, and only
    // `MemberRole::Other` may decline.
    assert_eq!(
        MemberRole::Other("torch".to_owned()).arguments(),
        None,
        "an unknown keyword has no vocabulary rather than an empty one",
    );
}
