//! `E_EDIT_UNMATCHED` and `E_EDIT_UNKNOWN_KEY`, and the findings a member line's keys draw, on a
//! top-level `edit` line (`spec/components-editing-sites` "Editing model").

use cairn_lang_core::DiagnosticCode;
use cairn_lang_core::intent::BodyKind;

use crate::common::{diagnose, notes, slice};

/// A clean struct of two front windows, with `edits` after it.
fn source(edits: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n  slot glass -> @glass_pane\n\n\
         struct s size=9x7\n  walls mat_slot=wall height=3\n  \
         window side=front offset=1 y=1 mat_slot=glass\n  \
         window side=front offset=5 y=1 mat_slot=glass\n\n{edits}"
    )
}

/// Each finding as its code, the text it points at, its primary and its notes.
fn found(src: &str) -> Vec<(&'static str, String, String, Vec<String>)> {
    diagnose(src)
        .iter()
        .map(|d| {
            (
                d.code.as_str(),
                slice(src, d).to_owned(),
                d.primary.clone(),
                notes(d).into_iter().map(str::to_owned).collect(),
            )
        })
        .collect()
}

fn finding(
    code: &'static str,
    at: &str,
    primary: &str,
    notes: &[&str],
) -> (&'static str, String, String, Vec<String>) {
    (
        code,
        at.to_owned(),
        primary.to_owned(),
        notes.iter().map(|note| (*note).to_owned()).collect(),
    )
}

const SELECTOR_NOTE: &str = "an edit selects among the members of every `struct` and `def` \
                             body as their lines write them, before any edit applies: check each \
                             attribute against the lines it means, or name one member by its \
                             address";

/// The note that lists where the two windows of [`source`] stand.
const WINDOWS_STAND: &str = "the `window` members stand at `window@front[0]` and `window@front[1]`";

#[test]
fn the_source_the_edits_stand_after_is_clean() {
    assert_eq!(found(&source("")), []);
}

#[test]
fn an_edit_that_selects_nothing_is_refused_with_what_it_could_name() {
    let src = source(
        "edit window@back[0] set y=2\n\
         edit window[class=vent] set y=2\n\
         edit door@front[0] set at=left\n\
         edit place[id=a] set gap=1\n\
         edit connect[class=road] set path=@gravel\n",
    );
    assert_eq!(
        found(&src),
        [
            finding(
                "E_EDIT_UNMATCHED",
                "window@back[0]",
                "`edit window@back[0]` selects no member, so it changes nothing",
                &[WINDOWS_STAND],
            ),
            finding(
                "E_EDIT_UNMATCHED",
                "window[class=vent]",
                "`edit window[class=vent]` selects no member, so it changes nothing",
                &[SELECTOR_NOTE, WINDOWS_STAND],
            ),
            finding(
                "E_EDIT_UNMATCHED",
                "door@front[0]",
                "`edit door@front[0]` selects no member, so it changes nothing",
                &["no `door` member stands in a `struct` or `def` body"],
            ),
            finding(
                "E_EDIT_UNMATCHED",
                "place[id=a]",
                "`edit place[id=a]` selects no member, so it changes nothing",
                &[
                    SELECTOR_NOTE,
                    "no `place` member stands in a `struct` or `def` body",
                ],
            ),
            finding(
                "E_EDIT_UNMATCHED",
                "connect[class=road]",
                "`edit connect[class=road]` selects no member, so it changes nothing",
                &[
                    "a `connect` line stands in a `site` body, and an edit reaches only the \
                     members of `struct` and `def` bodies",
                    SELECTOR_NOTE,
                    "no `connect` member stands in a `struct` or `def` body",
                ],
            ),
        ],
    );
}

#[test]
fn an_unmatched_selector_offers_the_addresses_its_keyword_stands_at() {
    let src = source("edit roof[kind=hip] set overhang=2\n")
        .replace("height=3\n", "height=3\n  roof kind=flat mat_slot=wall\n");
    assert_eq!(
        found(&src),
        [finding(
            "E_EDIT_UNMATCHED",
            "roof[kind=hip]",
            "`edit roof[kind=hip]` selects no member, so it changes nothing",
            &[SELECTOR_NOTE, "the `roof` members stand at `roof[0]`"],
        )],
    );
}

#[test]
fn an_edit_that_selects_a_member_draws_no_finding() {
    let src = source(
        "edit window@front[1] set y=2\n\
         edit window[offset=1][y=1] set y=3\n",
    );
    assert_eq!(found(&src), []);
}

#[test]
fn an_edit_sets_only_the_keys_its_keyword_takes() {
    let src = source("edit window@front[0] set shap=arch id=w y=2\n");
    assert_eq!(
        found(&src),
        [
            finding(
                "E_EDIT_UNKNOWN_KEY",
                "arch",
                "`shap=` is not an argument `window` reads, so no edit of one sets it; the edit \
                 sets the rest",
                &[
                    "did you mean `shape`?",
                    "an edit of a `window` sets one of: side, y, offset, size, sym, repeat, \
                     step, shape, anchor, class, mat_slot",
                ],
            ),
            finding(
                "E_EDIT_UNKNOWN_KEY",
                "w",
                "an edit does not set `id=`",
                &[
                    "an `id=` is what edits and references name a member by, so it is written \
                   on the member's own line: move it there"
                ],
            ),
        ],
    );
}

#[test]
fn an_edit_whose_keys_are_all_refused_is_not_said_to_set_the_rest() {
    let src = source("edit window@front[0] set shap=arch shp=arch\n");
    let primaries: Vec<String> = found(&src)
        .into_iter()
        .map(|(_, at, primary, _)| format!("{at}: {primary}"))
        .collect();
    assert_eq!(
        primaries,
        [
            "arch: `shap=` is not an argument `window` reads, so no edit of one sets it",
            "arch: `shp=` is not an argument `window` reads, so no edit of one sets it",
        ],
    );
}

#[test]
fn a_theme_row_that_selects_by_id_does_not_offer_id_to_an_edit() {
    let src = format!(
        "{}edit window@front[0] set shap=arch y=2\n",
        source("").replace("slot glass", "window[id=a] -> @glass_pane\n  slot glass"),
    );
    let unknown: Vec<(String, Vec<String>)> = found(&src)
        .into_iter()
        .filter(|(code, ..)| *code == "E_EDIT_UNKNOWN_KEY")
        .map(|(_, _, primary, notes)| (primary, notes))
        .collect();
    assert_eq!(
        unknown,
        [(
            "`shap=` is not an argument `window` reads, so no edit of one sets it; the edit sets \
             the rest"
                .to_owned(),
            vec![
                "did you mean `shape`?".to_owned(),
                "an edit of a `window` sets one of: side, y, offset, size, sym, repeat, step, \
                 shape, anchor, class, mat_slot"
                    .to_owned(),
            ],
        )],
    );
}

#[test]
fn a_key_a_theme_selector_coins_is_one_an_edit_may_set() {
    let src = format!(
        "{}edit window@front[0] set tags=lit\n",
        source("").replace("slot glass", "window[tags=lit] ->\n  slot glass"),
    );
    assert_eq!(found(&src), []);
}

#[test]
fn an_edit_of_an_unknown_keyword_is_refused_at_the_keyword_alone() {
    let src = source("edit windw[class=a] set shap=arch\n");
    let found = diagnose(&src);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].code, DiagnosticCode::UnknownKeyword);
    assert_eq!(slice(&src, &found[0]), "windw");
    assert_eq!(found[0].primary, "unknown keyword `windw`");
    assert_eq!(
        notes(&found[0]),
        [
            "did you mean `window`?".to_owned(),
            format!(
                "expected one of: {}",
                BodyKind::Geometry.allowed_keywords().join(", ")
            ),
        ],
    );
}

#[test]
fn an_unknown_keyword_at_any_step_of_an_address_is_refused_alone() {
    let src = source("edit levl[0].window@front[0] set y=2\n");
    let found = diagnose(&src);
    let codes: Vec<(&str, &str, &str)> = found
        .iter()
        .map(|d| (d.code.as_str(), slice(&src, d), d.primary.as_str()))
        .collect();
    assert_eq!(
        codes,
        [("E_UNKNOWN_KEYWORD", "levl", "unknown keyword `levl`")]
    );
    assert_eq!(
        notes(&found[0]),
        [
            "did you mean `level`?".to_owned(),
            format!(
                "expected one of: {}",
                BodyKind::Geometry.allowed_keywords().join(", ")
            ),
        ],
    );
}

#[test]
fn an_edit_repeating_or_mistyping_a_key_is_refused_as_a_line_is() {
    let src = source("edit window[y=1][y=1] set class=5 class=vent\n");
    let codes: Vec<(&str, String)> = diagnose(&src)
        .iter()
        .map(|d| (d.code.as_str(), slice(&src, d).to_owned()))
        .collect();
    assert_eq!(
        codes,
        [
            ("E_DUPLICATE_ARG", "y=1".to_owned()),
            ("E_TYPE_MISMATCH_LABEL", "5".to_owned()),
            ("E_DUPLICATE_ARG", "class=vent".to_owned()),
        ],
    );
}

#[test]
fn a_finding_on_an_edit_key_is_said_once_however_many_members_it_selects() {
    let src = "struct s size=9x7\n  level y=0\n  level y=3\n\nedit level[] set mat_slot=wall\n";
    assert_eq!(
        found(src),
        [finding(
            "W_IGNORED_ARGUMENT",
            "wall",
            "`mat_slot=` is accepted on every member, and nothing a `level` builds reads it; the \
             value was ignored",
            &[
                "the `level` is built the same with it or without it — remove the argument",
                "the slot name is not looked up in the theme, so a misspelling of it is not caught",
            ],
        )],
    );
}

#[test]
fn a_slot_an_edit_writes_is_refused_once_at_the_edit_value() {
    let src = source("edit window[] set mat_slot=nope\n");
    assert_eq!(
        found(&src),
        [finding(
            "E_UNRESOLVED_SLOT",
            "nope",
            "`mat_slot=nope` is not declared in theme `t`",
            &[
                "expected one of: wall, glass",
                "add `slot nope -> @...` to theme `t` or correct the slot name",
            ],
        )],
    );
}

#[test]
fn a_finding_of_check_on_an_edited_member_line_notes_the_edit() {
    let src = source("edit walls[0] set height=4\n").replace("walls mat_slot=wall", "walls");
    let found = diagnose(&src);
    assert_eq!(
        self::found(&src),
        [finding(
            "E_MISSING_MATERIAL",
            "walls height=3",
            "`walls` has no `mat_slot=`, so it paints nothing",
            &[
                "add `mat_slot=NAME` to the `walls`, naming a slot the applied theme declares",
                "this edit changes this member",
            ],
        )],
    );
    let edit = found[0].notes.last().and_then(|note| note.span.clone());
    assert_eq!(
        edit.map(|span| &src[span]),
        Some("edit walls[0] set height=4")
    );
}
