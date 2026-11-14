//! Quick-fixes for what `compute_diagnostics` reports, applied to the text they were offered for.

use std::str::FromStr;

use serde_json::json;

use super::*;
use crate::diagnostics::compute_diagnostics;
use crate::pin::Pin;
use crate::test_fixtures::{PALE_OAK, SIGN, java};

/// A part whose own floor is Java 1.21.4.
const HUT: &str = "\
def hut size=3x3:
  requires java version>=1.21.4
  walls mat_slot=wall height=3

theme t:
  slot wall -> @oak_planks

site v:
  place id=a use=hut theme=t at=origin
";

fn uri() -> lsp_types::Uri {
    lsp_types::Uri::from_str("file:///test.crn").expect("valid uri")
}

/// What a quick-fix does: the text its edit leaves, or the command it has the client run.
#[derive(Debug, PartialEq)]
enum Does {
    Edit(String),
    Run(String, Vec<serde_json::Value>),
}

/// Each quick-fix for what `pin` reports in `source`: its title, whether it is preferred, and
/// what it does.
fn offered(source: &str, pin: &Pin, runs_set_target: bool) -> Vec<(String, bool, Does)> {
    fixed(
        source,
        &compute_diagnostics(&uri(), source, pin),
        runs_set_target,
    )
}

/// Each quick-fix for `diagnostics` in `source`, as [`offered`] lists them.
fn fixed(
    source: &str,
    diagnostics: &[lsp_types::Diagnostic],
    runs_set_target: bool,
) -> Vec<(String, bool, Does)> {
    code_actions(&uri(), source, diagnostics, runs_set_target)
        .into_iter()
        .map(|action| {
            let lsp_types::CodeActionOrCommand::CodeAction(action) = action else {
                panic!("a bare command: {action:?}");
            };
            assert_eq!(action.kind, Some(lsp_types::CodeActionKind::QUICKFIX));
            assert!(
                matches!(action.diagnostics.as_deref(), Some([d]) if diagnostics.contains(d)),
                "{action:?} names the one diagnostic it fixes",
            );
            let does = match (action.edit, action.command) {
                (Some(edit), None) => Does::Edit(applied(source, edit)),
                (None, Some(command)) => {
                    assert_eq!(command.title, action.title);
                    Does::Run(command.command, command.arguments.unwrap_or_default())
                }
                neither_or_both => panic!("{neither_or_both:?}"),
            };
            (action.title, action.is_preferred == Some(true), does)
        })
        .collect()
}

/// `source` with `edit`, which changes one range of this document, applied.
fn applied(source: &str, edit: lsp_types::WorkspaceEdit) -> String {
    let changes: Vec<_> = edit.changes.expect("a change").into_iter().collect();
    let [(target, edits)] = changes.as_slice() else {
        panic!("one document changed: {changes:?}");
    };
    assert_eq!(*target, uri());
    let [edit] = edits.as_slice() else {
        panic!("one range changed: {edits:?}");
    };
    let index = LineIndex::new(source);
    let start = index
        .offset_at(source, edit.range.start)
        .expect("in the text");
    let end = index
        .offset_at(source, edit.range.end)
        .expect("in the text");
    format!("{}{}{}", &source[..start], edit.new_text, &source[end..])
}

fn edit(title: &str, preferred: bool, text: String) -> (String, bool, Does) {
    (title.to_owned(), preferred, Does::Edit(text))
}

#[test]
fn an_alias_replaces_the_id_and_is_the_preferred_fix() {
    assert_eq!(
        offered(SIGN, &java("1.21.4"), false),
        [edit(
            "Replace `standing_sign` with its alias `oak_sign`",
            true,
            SIGN.replace("@standing_sign", "@oak_sign"),
        )],
    );
}

#[test]
fn an_id_written_with_its_namespace_is_replaced_with_one_and_keeps_its_state_literal() {
    let source =
        "struct s size=3x3\n  raw_block mat=minecraft:standing_sign[rotation=4] at=1,0,1\n";
    assert_eq!(
        offered(source, &java("1.21.4"), false),
        [edit(
            "Replace `minecraft:standing_sign` with its alias `minecraft:oak_sign`",
            true,
            source.replace("standing_sign", "oak_sign"),
        )],
    );
}

#[test]
fn the_nearest_spelling_of_an_id_is_offered_but_not_preferred() {
    let source = SIGN.replace("@standing_sign", "@oak_plank");
    assert_eq!(
        offered(&source, &java("1.21.4"), false),
        [edit(
            "Replace `oak_plank` with `oak_planks`",
            false,
            SIGN.replace("@standing_sign", "@oak_planks"),
        )],
    );
}

#[test]
fn a_misspelled_keyword_is_offered_the_one_it_resembles() {
    let source = "struct s size=3x3\n  wals height=2\n";
    assert_eq!(
        offered(source, &Pin::Nothing, false),
        [edit(
            "Change `wals` to `walls`",
            false,
            source.replace("wals", "walls"),
        )],
    );
}

#[test]
fn a_misspelled_key_is_offered_the_one_it_resembles_wherever_the_line_writes_it() {
    for (source, written, key) in [
        ("struct s size=3x3\n  walls hieght=2\n", "hieght", "height"),
        (
            "struct s size=3x3\n  walls hieght = 2\n",
            "hieght",
            "height",
        ),
        ("struct s siz=3x3\n  walls height=2\n", "siz", "size"),
        (
            "struct s size=9x7\n  window[clas=outer] side=front offset=2 y=2 size=2x2\n",
            "clas",
            "class",
        ),
    ] {
        assert_eq!(
            offered(source, &Pin::Nothing, false),
            [edit(
                &format!("Change `{written}=` to `{key}=`"),
                false,
                source.replacen(written, key, 1),
            )],
            "{source}",
        );
    }
}

#[test]
fn a_misspelling_nothing_resembles_is_offered_nothing() {
    for source in [
        "struct s size=3x3\n  qqzx height=2\n",
        "struct s size=3x3\n  walls qqzx=2\n",
    ] {
        assert_eq!(offered(source, &Pin::Nothing, false), [], "{source}");
    }
}

#[test]
fn a_floor_that_refuses_the_target_is_offered_lowering_to_it() {
    assert_eq!(
        offered(PALE_OAK, &java("1.21"), false),
        [edit(
            "Lower the floor to `version>=1.21`",
            false,
            PALE_OAK.replacen("version>=1.21.4", "version>=1.21", 1),
        )],
    );
}

#[test]
fn a_client_that_runs_set_target_is_offered_the_lowest_target_the_floor_admits() {
    assert_eq!(
        offered(PALE_OAK, &java("1.21"), true),
        [
            edit(
                "Lower the floor to `version>=1.21`",
                false,
                PALE_OAK.replacen("version>=1.21.4", "version>=1.21", 1),
            ),
            (
                "Build against java 1.21.4".to_owned(),
                false,
                Does::Run(SET_TARGET.to_owned(), vec![json!("1.21.4")]),
            ),
        ],
    );
}

#[test]
fn a_part_s_floor_is_lowered_on_its_requires_line_with_its_edition() {
    assert_eq!(
        offered(HUT, &java("1.20.4"), true),
        [
            edit(
                "Lower the floor to `java version>=1.20.4`",
                false,
                HUT.replace("version>=1.21.4", "version>=1.20.4"),
            ),
            (
                "Build against java 1.21.4".to_owned(),
                false,
                Does::Run(SET_TARGET.to_owned(), vec![json!("1.21.4")]),
            ),
        ],
    );
}

#[test]
fn a_floor_no_target_meets_is_offered_no_target() {
    let source = PALE_OAK.replace("1.21.4", "99.0");
    assert_eq!(
        offered(&source, &java("26.3"), true),
        [edit(
            "Lower the floor to `version>=26.3`",
            false,
            source.replace("99.0", "26.3"),
        )],
    );
}

#[test]
fn a_diagnostic_whose_range_no_longer_holds_what_its_payload_names_is_offered_nothing() {
    for (source, pin, written, edited) in [
        (SIGN, java("1.21.4"), "standing_sign", "standing_sigx"),
        (
            "struct s size=3x3\n  wals height=2\n",
            Pin::Nothing,
            "wals",
            "walx",
        ),
        (
            "struct s size=3x3\n  wals height=2\n",
            Pin::Nothing,
            "wals ",
            "walsx",
        ),
        (
            "struct s size=3x3\n  walls hieght=2\n",
            Pin::Nothing,
            "hieght",
            "hieghx",
        ),
        (
            "struct s size=3x3\n  walls hieght=2\n",
            Pin::Nothing,
            "s hieght",
            "sxhieght",
        ),
        (PALE_OAK, java("1.21"), "1.21.4", "1.21.5"),
    ] {
        let diagnostics = compute_diagnostics(&uri(), source, &pin);
        assert_eq!(fixed(source, &diagnostics, false).len(), 1, "{source}");
        assert_eq!(
            fixed(&source.replace(written, edited), &diagnostics, true),
            [],
            "{source}",
        );
    }
}

#[test]
fn a_finding_another_server_made_or_whose_payload_names_no_authored_id_is_offered_nothing() {
    let [finding] = <[_; 1]>::try_from(compute_diagnostics(&uri(), SIGN, &java("1.21.4")))
        .expect("one finding");
    let foreign = lsp_types::Diagnostic {
        source: Some("another-server".to_owned()),
        ..finding.clone()
    };
    let mut from_a_catalog = finding.clone();
    from_a_catalog.data.as_mut().expect("a payload")["origin"] = json!("catalog");
    let unreadable = lsp_types::Diagnostic {
        data: Some(json!({ "kind": "unknown_id" })),
        ..finding.clone()
    };
    let unpaired = lsp_types::Diagnostic {
        code: Some(lsp_types::NumberOrString::String(
            "E_UNKNOWN_KEYWORD".to_owned(),
        )),
        ..finding
    };
    assert_eq!(
        fixed(SIGN, &[foreign, from_a_catalog, unreadable, unpaired], true),
        [],
    );
}
