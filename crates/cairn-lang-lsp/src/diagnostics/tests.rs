//! Source text to LSP diagnostics, checked against what `cairn-lang-core` reports.

use std::str::FromStr;

use cairn_lang_core::block_array::lower_to_block_array;
use cairn_lang_core::{Edition, resolve};
use cairn_lang_formats::builtin_pack;

use super::*;
use crate::test_fixtures::{CLEAN, DUPLICATE, PALE_OAK, SIGN, VARIANTS, java};

fn uri() -> lsp_types::Uri {
    lsp_types::Uri::from_str("file:///test.crn").expect("valid uri")
}

fn code_of(d: &lsp_types::Diagnostic) -> &str {
    match d.code.as_ref().expect("code present") {
        lsp_types::NumberOrString::String(s) => s,
        lsp_types::NumberOrString::Number(_) => panic!("expected string code"),
    }
}

#[test]
fn duplicate_fixture_reports_e_duplicate_size_with_error_severity() {
    let diagnostics = compute_diagnostics(&uri(), DUPLICATE, &Pin::Nothing);
    let dup = diagnostics
        .iter()
        .find(|d| code_of(d) == "E_DUPLICATE_SIZE")
        .expect("E_DUPLICATE_SIZE reported");
    assert_eq!(dup.severity, Some(lsp_types::DiagnosticSeverity::ERROR));
    assert_eq!(dup.source.as_deref(), Some("cairn"));
    assert_eq!(dup.range.start.line, 0);
    let second_size =
        u32::try_from(DUPLICATE.find("size=5x5").expect("second size")).expect("offset fits u32");
    assert_eq!(dup.range.start.character, second_size);
}

#[test]
fn span_notes_become_related_information() {
    let diagnostics = compute_diagnostics(&uri(), DUPLICATE, &Pin::Nothing);
    let dup = diagnostics
        .iter()
        .find(|d| code_of(d) == "E_DUPLICATE_SIZE")
        .expect("E_DUPLICATE_SIZE reported");
    let related = dup
        .related_information
        .as_ref()
        .expect("relatedInformation present");
    assert_eq!(related[0].location.uri, uri());
    let first_size =
        u32::try_from(DUPLICATE.find("size=4x4").expect("first size")).expect("offset fits u32");
    assert_eq!(related[0].location.range.start.character, first_size);
    assert!(
        related[0].message.contains("first"),
        "note should reference the first declaration, got: {}",
        related[0].message,
    );
}

#[test]
fn spanless_notes_append_to_message_as_note_lines() {
    // The "expected one of" footer on E_UNKNOWN_KEYWORD has no span.
    let source = "struct s size=2x2\n  wals height=4\n";
    let diagnostics = compute_diagnostics(&uri(), source, &Pin::Nothing);
    let unknown = diagnostics
        .iter()
        .find(|d| code_of(d) == "E_UNKNOWN_KEYWORD")
        .expect("E_UNKNOWN_KEYWORD reported");
    assert!(
        unknown.message.contains("\nnote: "),
        "spanless note should be folded into the message, got: {}",
        unknown.message,
    );
}

#[test]
fn parse_error_yields_exactly_one_error_diagnostic() {
    let source = "struct s size=2x2\n\tfloor\n";
    let err = parse(source).expect_err("tab indent should be rejected");
    let diagnostics = compute_diagnostics(&uri(), source, &Pin::Nothing);
    assert_eq!(diagnostics.len(), 1);
    let d = &diagnostics[0];
    assert_eq!(d.severity, Some(lsp_types::DiagnosticSeverity::ERROR));
    assert_eq!(d.message, err.user_message());
    assert_eq!(code_of(d), "E_PARSE");
    assert_eq!(d.range.start.line, 1);
    assert!(
        d.range.end > d.range.start,
        "range should be non-empty, got {:?}",
        d.range,
    );
}

#[test]
fn artifacts_that_share_a_file_name_report_e_output_name_collision() {
    let source = "theme t:\n  slot floor -> @oak_planks\n\n\
                  struct hut size=3x3\n  floor mat_slot=floor\n\n\
                  def house size=3x3:\n  floor mat_slot=floor\n\n\
                  site s:\n  place id=hut use=house theme=t at=origin\n";
    let diagnostics = compute_diagnostics(&uri(), source, &Pin::Nothing);
    let found: Vec<&lsp_types::Diagnostic> = diagnostics
        .iter()
        .filter(|d| code_of(d) == "E_OUTPUT_NAME_COLLISION")
        .collect();
    assert_eq!(found.len(), 1, "one collision expected: {diagnostics:?}");
    assert_eq!(
        found[0].severity,
        Some(lsp_types::DiagnosticSeverity::ERROR)
    );
    assert_eq!(found[0].range.start.line, 10);
    let related = found[0]
        .related_information
        .as_ref()
        .expect("relatedInformation present");
    assert_eq!(
        related[0].location.range.start.line, 3,
        "the related pointer names the struct",
    );
}

/// A shed roof outside the stair family, and a door whose side is a number.
const SHED_ON_COBBLESTONE: &str = "\
@cairn 2026.06

theme t:
  slot wall -> @cobblestone
  slot roof -> @cobblestone

struct hut size=5x5
  walls mat_slot=wall height=3
  roof kind=shed slope_to=front mat_slot=roof
  door side=3
";

/// Each diagnostic as its code, severity and range.
fn located(
    diagnostics: &[lsp_types::Diagnostic],
) -> Vec<(&str, lsp_types::DiagnosticSeverity, lsp_types::Range)> {
    diagnostics
        .iter()
        .map(|d| (code_of(d), d.severity.expect("severity present"), d.range))
        .collect()
}

/// The range from `start` to `end`, each a `(line, character)` pair.
fn range(start: (u32, u32), end: (u32, u32)) -> lsp_types::Range {
    lsp_types::Range::new(
        lsp_types::Position::new(start.0, start.1),
        lsp_types::Position::new(end.0, end.1),
    )
}

#[test]
fn a_lowering_finding_every_target_raises_is_reported_as_a_pinned_check_words_it() {
    let source = SHED_ON_COBBLESTONE;
    let diagnostics = compute_diagnostics(&uri(), source, &Pin::Nothing);
    assert_eq!(
        located(&diagnostics),
        [
            (
                "E_INCOMPATIBLE_MATERIAL",
                lsp_types::DiagnosticSeverity::ERROR,
                range((4, 15), (4, 27)),
            ),
            (
                "E_INVALID_MEMBER",
                lsp_types::DiagnosticSeverity::ERROR,
                range((9, 2), (9, 13)),
            ),
        ],
    );

    let module = parse(source).expect("parses");
    let ir = lower(&module);
    let registry = builtin_pack(Edition::Java).view(Some("1.21.4"));
    let pinned = lower_to_block_array(&ir, &resolve(&ir, Some(Edition::Java)), Some(&registry));
    let index = LineIndex::new(source);
    let expected: Vec<lsp_types::Diagnostic> = check(&module, &ir, None)
        .iter()
        .chain(&pinned.diagnostics)
        .map(|d| to_lsp_diagnostic(&uri(), source, &index, d))
        .collect();
    assert_eq!(diagnostics, expected);
}

#[test]
fn an_id_no_shipped_version_declares_is_left_to_a_pinned_check() {
    let source = "theme t:\n  slot wall -> @cobblestone\n  slot roof -> @oak_stair\n\n\
                  struct hut size=5x5\n  walls mat_slot=wall height=3\n  \
                  roof kind=gable mat_slot=roof\n";
    assert_eq!(compute_diagnostics(&uri(), source, &Pin::Nothing), vec![]);
}

#[test]
fn clean_fixture_reports_no_diagnostics() {
    assert_eq!(compute_diagnostics(&uri(), CLEAN, &Pin::Nothing), vec![]);
}

#[test]
fn ranges_count_utf16_code_units_on_non_ascii_lines() {
    let source = "struct s size=2x2\n  door id=\"😀\" id=x\n";
    let diagnostics = compute_diagnostics(&uri(), source, &Pin::Nothing);
    let dup = diagnostics
        .iter()
        .find(|d| code_of(d) == "E_DUPLICATE_ARG")
        .expect("E_DUPLICATE_ARG reported");
    let line = "  door id=\"😀\" id=x";
    let byte_col = line.find("id=x").expect("second id");
    let utf16_col = line[..byte_col].encode_utf16().count();
    let scalar_col = line[..byte_col].chars().count();
    assert_ne!(utf16_col, byte_col, "test must discriminate byte columns");
    assert_ne!(
        utf16_col, scalar_col,
        "test must discriminate scalar columns",
    );
    assert_eq!(dup.range.start.line, 1);
    assert_eq!(
        dup.range.start.character,
        u32::try_from(utf16_col).expect("fits u32"),
    );
}

#[test]
fn a_diagnostic_covers_the_same_text_whatever_the_line_ending() {
    let base = "struct s size=2x2\n  floor\n  wals height=4\nstruct t size=3x3 size=4x4\n";
    let mut selected = Vec::new();
    for rendering in [
        base.to_owned(),
        base.replace('\n', "\r\n"),
        base.replace('\n', "\r"),
    ] {
        let source = rendering.as_str();
        let index = LineIndex::new(source);
        let covered: Vec<(String, String)> = compute_diagnostics(&uri(), source, &Pin::Nothing)
            .iter()
            .map(|d| {
                let start = index
                    .offset_at(source, d.range.start)
                    .expect("range start is inside the document");
                let end = index
                    .offset_at(source, d.range.end)
                    .expect("range end is inside the document");
                (code_of(d).to_owned(), source[start..end].to_owned())
            })
            .collect();
        selected.push(covered);
    }
    assert_eq!(selected[1], selected[0], "CRLF disagrees with LF");
    assert_eq!(selected[2], selected[0], "lone CR disagrees with LF");
    assert!(
        selected[0]
            .iter()
            .any(|(code, text)| code == "E_UNKNOWN_KEYWORD" && text == "wals height=4"),
        "expected the unknown keyword itself to be covered, got {:?}",
        selected[0],
    );
}

#[test]
fn a_parse_error_lands_on_its_own_line_whatever_the_line_ending() {
    let base = "struct s size=2x2\n  floor mat_slot=\n  walls height=4\n";
    for rendering in [
        base.to_owned(),
        base.replace('\n', "\r\n"),
        base.replace('\n', "\r"),
    ] {
        let source = rendering.as_str();
        let diagnostics = compute_diagnostics(&uri(), source, &Pin::Nothing);
        assert_eq!(diagnostics.len(), 1, "{source:?}");
        let range = diagnostics[0].range;
        // The end of `  floor mat_slot=`, not the start of line 2.
        assert_eq!(range.start.line, 1, "{source:?}");
        assert_eq!(range.start.character, 17, "{source:?}");
        assert_eq!(range.end, range.start, "{source:?}");
    }
}

#[test]
fn a_parse_error_inside_a_line_covers_the_rest_of_it() {
    let source = "struct s size=2x2\n\tfloor\n";
    let diagnostics = compute_diagnostics(&uri(), source, &Pin::Nothing);
    assert_eq!(diagnostics.len(), 1);
    let range = diagnostics[0].range;
    assert_eq!(range.start.line, 1);
    assert!(range.end > range.start, "{range:?}");
}

/// Each finding's code and the source text its range covers.
fn covered<'s>(source: &'s str, diagnostics: &[lsp_types::Diagnostic]) -> Vec<(String, &'s str)> {
    let index = LineIndex::new(source);
    diagnostics
        .iter()
        .map(|d| {
            let start = index.offset_at(source, d.range.start).expect("in the text");
            let end = index.offset_at(source, d.range.end).expect("in the text");
            (code_of(d).to_owned(), &source[start..end])
        })
        .collect()
}

#[test]
fn a_pinned_target_reports_an_id_it_does_not_declare_with_the_payload_a_quick_fix_reads() {
    let found = compute_diagnostics(&uri(), SIGN, &java("1.21.4"));
    assert_eq!(
        covered(SIGN, &found),
        [("E_UNKNOWN_ID".to_owned(), "@standing_sign")],
    );
    assert_eq!(
        found[0].data,
        Some(serde_json::json!({
            "kind": "unknown_id",
            "id": "minecraft:standing_sign",
            "registry": "java 1.21.4",
            "origin": "authored",
            "aliases": ["minecraft:oak_sign"],
        })),
    );
}

#[test]
fn no_id_is_refused_unless_a_version_is_pinned() {
    assert_eq!(compute_diagnostics(&uri(), SIGN, &Pin::Nothing), vec![]);
    assert_eq!(
        compute_diagnostics(&uri(), SIGN, &Pin::Edition(Edition::Java)),
        vec![],
    );
}

#[test]
fn an_edition_pin_binds_that_edition_s_theme_variant() {
    let rebound = [(
        "W_THEME_VARIANT_REBOUND".to_owned(),
        "place id=a use=hut theme=t_java at=origin",
    )];
    let bedrock = compute_diagnostics(&uri(), VARIANTS, &Pin::Edition(Edition::Bedrock));
    assert_eq!(covered(VARIANTS, &bedrock), rebound);
    assert_eq!(
        compute_diagnostics(&uri(), VARIANTS, &Pin::Edition(Edition::Java)),
        vec![],
    );
    assert_eq!(compute_diagnostics(&uri(), VARIANTS, &Pin::Nothing), vec![]);
}

#[test]
fn a_target_a_floor_refuses_is_refused_at_the_floor_and_pins_no_version() {
    let found = compute_diagnostics(&uri(), PALE_OAK, &java("1.21"));
    assert_eq!(
        covered(PALE_OAK, &found),
        [("E_VERSION_CAP".to_owned(), "@requires version>=1.21.4")],
    );
    assert_eq!(
        found[0].message,
        "this file requires version>=1.21.4 (target 1.21)\n\
         note: valid java targets: 1.21.4, 26.3\n\
         note: fix: --target 1.21.4, or lower the `@requires` floor",
    );
    assert_eq!(
        found[0].data,
        Some(serde_json::json!({
            "kind": "version_cap",
            "edition": "java",
            "target": "1.21",
            "floor": "version>=1.21.4",
            "valid": ["1.21.4", "26.3"],
        })),
    );
    assert_eq!(
        compute_diagnostics(&uri(), PALE_OAK, &java("1.21.4")),
        vec![]
    );
}

#[test]
fn a_target_no_floor_refuses_is_checked_for_the_ids_it_declares() {
    let source = PALE_OAK.replace("@requires version>=1.21.4\n", "");
    assert_eq!(
        covered(
            &source,
            &compute_diagnostics(&uri(), &source, &java("1.21"))
        ),
        [("E_UNKNOWN_ID".to_owned(), "@pale_oak_planks")],
    );
}
