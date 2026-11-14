use std::collections::BTreeSet;

use cairn_lang_core::ast::{Item, ThemeRule};
use cairn_lang_core::parse;

use super::*;

/// Position immediately after the first occurrence of `needle`.
fn at_end_of(source: &str, needle: &str) -> lsp_types::Position {
    let offset = source.find(needle).expect("needle in source") + needle.len();
    LineIndex::new(source).position(source, offset)
}

fn complete(source: &str, needle: &str) -> Vec<lsp_types::CompletionItem> {
    completions(source, at_end_of(source, needle))
        .expect("position within document")
        .items
}

fn labels(items: &[lsp_types::CompletionItem]) -> Vec<&str> {
    items.iter().map(|i| i.label.as_str()).collect()
}

fn edit_range(item: &lsp_types::CompletionItem) -> lsp_types::Range {
    match item.text_edit.as_ref().expect("text_edit present") {
        lsp_types::CompletionTextEdit::Edit(edit) => edit.range,
        lsp_types::CompletionTextEdit::InsertAndReplace(_) => {
            panic!("expected plain TextEdit")
        }
    }
}

fn range(line: u32, start_character: u32, end_character: u32) -> lsp_types::Range {
    lsp_types::Range {
        start: lsp_types::Position {
            line,
            character: start_character,
        },
        end: lsp_types::Position {
            line,
            character: end_character,
        },
    }
}

#[test]
fn top_level_line_start_offers_exactly_the_item_keywords() {
    let source = "st";
    let items = complete(source, "st");
    assert_eq!(labels(&items), vec!["theme", "def", "site", "struct"]);
    for item in &items {
        assert_eq!(item.kind, Some(lsp_types::CompletionItemKind::KEYWORD));
        assert_eq!(edit_range(item), range(0, 0, 2));
    }
}

#[test]
fn member_position_offers_the_bodys_keywords_with_replace_range() {
    let source = "struct s size=2x2\n  flo";
    let items = complete(source, "flo");
    assert_eq!(labels(&items), BodyKind::Geometry.allowed_keywords());
    for item in &items {
        assert_eq!(item.kind, Some(lsp_types::CompletionItemKind::KEYWORD));
        assert_eq!(edit_range(item), range(1, 2, 5));
    }
}

#[test]
fn mid_token_cursor_replaces_the_whole_token() {
    let source = "struct s size=2x2\n  flotsam";
    let items = complete(source, "  flo");
    assert_eq!(labels(&items), BodyKind::Geometry.allowed_keywords());
    for item in &items {
        assert_eq!(edit_range(item), range(1, 2, 9));
    }
}

#[test]
fn theme_body_offers_slot_and_selector_keywords() {
    let source = "theme x:\n  s";
    let items = complete(source, "\n  s");
    assert_eq!(items[0].label, "slot");
    assert_eq!(items[0].detail.as_deref(), Some("slot binding"));
    let window = items
        .iter()
        .find(|i| i.label == "window")
        .expect("member keywords offered as selectors");
    assert_eq!(window.detail.as_deref(), Some("theme selector"));
    assert_eq!(
        items.len(),
        1 + cairn_lang_core::intent::known_keywords().len(),
        "a selector is matched against every body, so its vocabulary stays the whole table",
    );
}

#[test]
fn nested_block_body_offers_the_enclosing_items_keywords() {
    let source = "struct t size=5x5\n  level h=4\n    flo";
    let items = complete(source, "    flo");
    assert_eq!(labels(&items), BodyKind::Geometry.allowed_keywords());
    assert!(!labels(&items).contains(&"slot"));
}

#[test]
fn a_site_body_offers_only_the_rows_it_reads() {
    let source = "site s:\n  p";
    let items = complete(source, "\n  p");
    assert_eq!(labels(&items), BodyKind::Site.allowed_keywords());
}

#[test]
fn a_truth_table_at_column_zero_is_no_item_header() {
    for source in [
        "struct s size=3x3\n  assert truth(a -> b) {\n0 -> 1\n  }\n  flo",
        "struct s size=3x3\n  assert truth(a -> b) {\n    0 -> 1\n}\n  flo",
    ] {
        let items = complete(source, "  flo");
        assert_eq!(
            labels(&items),
            BodyKind::Geometry.allowed_keywords(),
            "for {source:?}",
        );
    }
}

#[test]
fn a_line_inside_a_truth_table_offers_nothing() {
    for (source, needle) in [
        ("struct s size=3x3\n  assert truth(a -> b) {\n0", "{\n0"),
        (
            "struct s size=3x3\n  assert truth(a -> b) {\n    0 -> 1\n    \n  }\n",
            "0 -> 1\n    ",
        ),
    ] {
        assert_eq!(
            labels(&complete(source, needle)),
            Vec::<&str>::new(),
            "for {source:?}"
        );
    }
}

#[test]
fn an_unresolved_header_offers_the_whole_table_rather_than_nothing() {
    for source in [
        "  flo",
        "stru s size=2x2
  flo",
    ] {
        let items = complete(source, "flo");
        assert_eq!(
            labels(&items),
            cairn_lang_core::intent::known_keywords().to_vec(),
            "for {source:?}",
        );
    }
}

#[test]
fn slot_names_union_across_themes_survives_parse_failure() {
    let source = "theme a:\n  slot floor -> @oak_planks\n\
                  theme b:\n  slot wall -> @cobblestone\n\
                  struct s size=2x2\n  floor mat_slot=";
    parse(source).expect_err("document mid-keystroke should not parse");
    let items = complete(source, "mat_slot=");
    assert_eq!(labels(&items), vec!["floor", "wall"]);
    for item in &items {
        assert_eq!(item.kind, Some(lsp_types::CompletionItemKind::VARIABLE));
    }
    assert_eq!(items[0].detail.as_deref(), Some("-> @oak_planks (theme a)"),);
}

#[test]
fn slot_value_replace_range_counts_utf16_units() {
    let source = "theme a:\n  slot walls -> @cobblestone\n\
                  struct s size=2x2\n  window id=\"😀\" mat_slot=wa";
    let items = complete(source, "mat_slot=wa");
    assert_eq!(labels(&items), vec!["walls"]);
    let line = "  window id=\"😀\" mat_slot=wa";
    let byte_col = line.find("wa").expect("partial token");
    let utf16_col = u32::try_from(line[..byte_col].encode_utf16().count()).expect("fits u32");
    assert_ne!(u32::try_from(byte_col).expect("fits"), utf16_col);
    assert_eq!(edit_range(&items[0]), range(3, utf16_col, utf16_col + 2));
}

#[test]
fn the_line_scan_answers_both_questions_from_one_walk() {
    let cases = [
        ("door id=front", None, false),
        ("door id=\"front\"", None, false),
        ("door id=\"front", None, true),
        // A `#` inside a string is content, not an opener.
        ("door id=\"a # b\"", None, false),
        ("door id=\"a # b", None, true),
        // Quotes past a comment opener are comment text.
        ("door id=a # say \"hi", Some(10), false),
        ("# \"", Some(0), false),
    ];
    for (line, comment, open_string) in cases {
        let scan = scan_line(line);
        assert_eq!(scan.comment, comment, "comment offset for {line:?}");
        assert_eq!(scan.open_string, open_string, "open string for {line:?}");
    }
}

#[test]
fn a_material_token_inside_a_string_offers_nothing() {
    let source = "theme a:\n  slot floor -> @oak_planks\n\
                  struct s size=2x2\n  door id=\"@oa";
    // `@oa` alone would match `@oak_planks` above.
    assert_eq!(
        completions(source, at_end_of(source, "id=\"@oa")).map(|list| list.items),
        Some(vec![]),
    );
}

#[test]
fn a_mat_slot_key_inside_a_string_offers_nothing() {
    let source = "theme a:\n  slot floor -> @oak_planks\n\
                  struct s size=2x2\n  window id=\"mat_slot=";
    assert_eq!(
        completions(source, at_end_of(source, "id=\"mat_slot=")).map(|list| list.items),
        Some(vec![]),
    );
}

#[test]
fn a_mat_slot_key_inside_a_longer_string_offers_nothing() {
    let source = "theme a:\n  slot floor -> @oak_planks\n\
                  struct s size=2x2\n  window label=\"pick mat_slot=fl";
    assert_eq!(
        completions(source, at_end_of(source, "pick mat_slot=fl")).map(|list| list.items),
        Some(vec![]),
    );
}

#[test]
fn a_cursor_after_a_closing_quote_still_completes() {
    let source = "theme a:\n  slot floor -> @oak_planks\n\
                  struct s size=2x2\n  window id=\"front\" mat_slot=";
    let items = complete(source, "mat_slot=");
    assert_eq!(labels(&items), vec!["floor"]);
}

#[test]
fn an_unterminated_string_suppresses_only_its_own_line() {
    let source = "theme a:\n  slot floor -> @oak_planks\n\
                  struct s size=2x2\n  door id=\"oops\n  floor mat_slot=";
    let items = complete(source, "mat_slot=");
    assert_eq!(labels(&items), vec!["floor"]);
}

#[test]
fn hash_inside_a_string_literal_is_not_a_comment() {
    let source = "theme a:\n  slot floor -> @oak_planks\n\
                  struct s size=2x2\n  window id=\"#front\" mat_slot=";
    let items = complete(source, "mat_slot=");
    assert_eq!(labels(&items), vec!["floor"]);
}

#[test]
fn mat_slot_offers_no_slot_on_a_keyword_that_reads_none() {
    let theme = "theme a:\n  slot floor -> @oak_planks\n";
    for (rest, expected) in [
        (
            "struct s size=2x2\n  circuit region=floor mat_slot=",
            vec![],
        ),
        (
            "struct s size=2x2\n  circuit region=floor mat_slot=fl",
            vec![],
        ),
        (
            "struct s size=2x2\n  window side=front mat_slot=",
            vec!["floor"],
        ),
        (
            "struct s size=2x2\n  door side=front mat_slot=",
            vec!["floor"],
        ),
        ("theme b:\n  circuit[mat_slot=", vec![]),
        ("theme b:\n  window[mat_slot=", vec!["floor"]),
    ] {
        let source = format!("{theme}{rest}");
        let items = completions(&source, at_end_of(&source, rest))
            .expect("in the document")
            .items;
        assert_eq!(labels(&items), expected, "{rest:?}");
    }
    for keyword in cairn_lang_core::intent::known_keywords() {
        let rest = format!("struct s size=2x2\n  {keyword} mat_slot=");
        let source = format!("{theme}{rest}");
        let items = completions(&source, at_end_of(&source, &rest))
            .expect("in the document")
            .items;
        let unread = role_of(keyword).unread_argument("mat_slot").is_some();
        assert_eq!(
            labels(&items).is_empty(),
            unread,
            "`{keyword}`: unread = {unread}, offered {:?}",
            labels(&items),
        );
    }
}

#[test]
fn slot_target_detail_strips_comments_outside_strings_only() {
    let source = "theme a:\n  slot floor -> @oak_planks # the default\n\
                  theme b:\n  slot sign -> \"#1\"\n\
                  struct s size=2x2\n  floor mat_slot=";
    let items = complete(source, "mat_slot=");
    assert_eq!(items[0].detail.as_deref(), Some("-> @oak_planks (theme a)"),);
    assert_eq!(items[1].detail.as_deref(), Some("-> \"#1\" (theme b)"));
}

/// The same document written the three ways Cairn accepts.
fn renderings(lf_source: &str) -> [(&'static str, String); 3] {
    [
        ("LF", lf_source.to_owned()),
        ("CRLF", lf_source.replace('\n', "\r\n")),
        ("CR", lf_source.replace('\n', "\r")),
    ]
}

#[test]
fn line_endings_do_not_shift_keyword_classification_or_replace_ranges() {
    // `wal`, not `flo`: `floor` appears above.
    for (label, source) in
        renderings("theme a:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  wal")
    {
        let items = complete(&source, "wal");
        assert_eq!(
            labels(&items),
            BodyKind::Geometry.allowed_keywords(),
            "{label}"
        );
        assert_eq!(edit_range(&items[0]), range(3, 2, 5), "{label}");
    }
}

#[test]
fn line_endings_do_not_shift_the_slot_scan() {
    for (label, source) in
        renderings("theme a:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  floor mat_slot=")
    {
        let items = complete(&source, "mat_slot=");
        assert_eq!(labels(&items), vec!["floor"], "{label}");
        assert_eq!(
            items[0].detail.as_deref(),
            Some("-> @oak_planks (theme a)"),
            "{label}",
        );
    }
}

#[test]
fn position_far_past_the_document_is_refused() {
    let source = "st";
    for line in [1, 99] {
        let position = lsp_types::Position { line, character: 0 };
        assert_eq!(
            completions(source, position),
            None,
            "line {line} does not exist in a one-line document",
        );
    }
}

/// The material-token answer at the end of `@typed` on a theme's slot row.
fn complete_material(typed: &str) -> lsp_types::CompletionList {
    let source = format!("theme a:\n  slot floor -> @{typed}");
    completions(&source, at_end_of(&source, &format!("@{typed}"))).expect("in the document")
}

/// Every id some version of either built-in pack declares, as a canonical token writes it.
fn declared_by_a_pack() -> BTreeSet<String> {
    [Edition::Java, Edition::Bedrock]
        .into_iter()
        .flat_map(|edition| builtin_pack(edition).blocks().declared_ids())
        .map(|id| id.strip_prefix("minecraft:").unwrap_or(id).to_owned())
        .collect()
}

/// Each label `list` offers, with its detail.
fn offered(list: &lsp_types::CompletionList) -> Vec<(&str, &str)> {
    list.items
        .iter()
        .map(|item| {
            let detail = item.detail.as_deref().expect("detail present");
            (item.label.as_str(), detail)
        })
        .collect()
}

#[test]
fn a_bare_at_offers_the_abstract_tokens_then_the_first_canonical_ids_cut_at_the_limit() {
    let list = complete_material("");
    assert!(
        list.is_incomplete,
        "the vocabulary is longer than the limit"
    );
    assert_eq!(list.items.len(), MATERIAL_ITEM_LIMIT);
    let java = builtin_pack(Edition::Java).materials();
    let bedrock = builtin_pack(Edition::Bedrock).materials();
    let mut seen = HashSet::new();
    let abstract_tokens: Vec<&str> = java
        .tokens()
        .chain(bedrock.tokens())
        .filter(|token| seen.insert(*token))
        .collect();
    let canonical: Vec<String> = declared_by_a_pack().into_iter().collect();
    let expected: Vec<&str> = abstract_tokens
        .iter()
        .copied()
        .chain(canonical.iter().map(String::as_str))
        .take(MATERIAL_ITEM_LIMIT)
        .collect();
    assert_eq!(labels(&list.items), expected);
    assert_eq!(
        offered(&list)[0],
        (
            "floor.wood.broadleaf",
            "java minecraft:oak_planks; bedrock minecraft:oak_planks"
        ),
    );
    let sort_texts: Vec<&str> = list
        .items
        .iter()
        .map(|i| i.sort_text.as_deref().expect("sort_text present"))
        .collect();
    assert!(
        sort_texts.is_sorted(),
        "items must carry their own order: {sort_texts:?}",
    );
    for item in &list.items {
        assert_eq!(item.kind, Some(lsp_types::CompletionItemKind::VALUE));
    }
}

#[test]
fn an_abstract_token_is_offered_with_what_it_resolves_to_in_each_edition() {
    assert_eq!(
        offered(&complete_material("floor.stone.smooth")),
        [(
            "floor.stone.smooth",
            "java minecraft:stone_bricks; bedrock minecraft:stone_bricks \
             (1.21.0: minecraft:stonebrick)",
        )],
    );
    assert_eq!(
        offered(&complete_material("sign.oak")),
        [
            (
                "sign.oak",
                "java minecraft:oak_sign; bedrock minecraft:standing_sign"
            ),
            (
                "sign.oak_wall",
                "java minecraft:oak_wall_sign; bedrock minecraft:wall_sign"
            ),
        ],
    );
}

#[test]
fn a_canonical_id_only_the_blocks_tables_declare_is_offered_with_who_declares_it() {
    assert_eq!(
        offered(&complete_material("cherry_planks")),
        [(
            "cherry_planks",
            "java 1.20.4, 1.21, 1.21.4, 26.3; bedrock 1.21.0, 1.21.40, 1.21.60, 1.26.60",
        )],
    );
    assert_eq!(
        offered(&complete_material("pale_oak_planks")),
        [(
            "pale_oak_planks",
            "java 1.21.4, 26.3; bedrock 1.21.60, 1.26.60"
        )],
    );
    assert_eq!(
        offered(&complete_material("border_block")),
        [("border_block", "bedrock 1.21.0, 1.21.40, 1.21.60, 1.26.60")],
    );
    assert_eq!(
        offered(&complete_material("stonebrick"))[0],
        ("stonebrick", "bedrock 1.21.0"),
    );
}

#[test]
fn abstract_tokens_rank_ahead_of_canonical_ids_and_leading_matches_ahead_of_inner_then_scattered_ones()
 {
    let cobble = complete_material("cobble");
    assert!(!cobble.is_incomplete);
    assert_eq!(
        labels(&cobble.items),
        [
            "floor.stone.cobble",
            "wall.stone.cobble",
            "cobbled_deepslate",
            "cobbled_deepslate_double_slab",
            "cobbled_deepslate_slab",
            "cobbled_deepslate_stairs",
            "cobbled_deepslate_wall",
            "cobblestone",
            "cobblestone_double_slab",
            "cobblestone_slab",
            "cobblestone_stairs",
            "cobblestone_wall",
            "infested_cobblestone",
            "mossy_cobblestone",
            "mossy_cobblestone_double_slab",
            "mossy_cobblestone_slab",
            "mossy_cobblestone_stairs",
            "mossy_cobblestone_wall",
            "polished_blackstone_brick_double_slab",
        ],
    );
}

#[test]
fn a_material_token_matches_every_label_holding_its_characters_in_order() {
    for (typed, expected) in [
        (
            "fwb",
            ["floor.wood.broadleaf", "potted_flowering_azalea_bush"],
        ),
        ("fl.wo", ["floor.wood.broadleaf", "floor.wood.conifer"]),
    ] {
        let list = complete_material(typed);
        assert!(!list.is_incomplete, "`@{typed}`");
        assert_eq!(labels(&list.items), expected, "`@{typed}`");
    }
    for typed in ["stonebri", "stonebrick"] {
        let list = complete_material(typed);
        assert!(!list.is_incomplete, "`@{typed}`");
        assert_eq!(
            labels(&list.items),
            [
                "stonebrick",
                "chiseled_stone_bricks",
                "cracked_polished_blackstone_bricks",
                "cracked_stone_bricks",
                "end_stone_brick_double_slab",
                "end_stone_brick_slab",
                "end_stone_brick_stairs",
                "end_stone_brick_wall",
                "end_stone_bricks",
                "infested_chiseled_stone_bricks",
                "infested_cracked_stone_bricks",
                "infested_mossy_stone_bricks",
                "infested_stone_bricks",
                "mossy_stone_brick_double_slab",
                "mossy_stone_brick_slab",
                "mossy_stone_brick_stairs",
                "mossy_stone_brick_wall",
                "mossy_stone_bricks",
                "polished_blackstone_brick_double_slab",
                "polished_blackstone_brick_slab",
                "polished_blackstone_brick_stairs",
                "polished_blackstone_brick_wall",
                "polished_blackstone_bricks",
                "stone_brick_double_slab",
                "stone_brick_slab",
                "stone_brick_stairs",
                "stone_brick_wall",
                "stone_bricks",
            ],
            "`@{typed}`",
        );
    }
}

#[test]
fn every_canonical_id_offered_is_one_a_blocks_table_declares_and_none_is_left_out() {
    let declared = declared_by_a_pack();
    for first in 'a'..='z' {
        let list = complete_material(&first.to_string());
        for label in labels(&list.items) {
            let is_abstract = abstract_tokens().iter().any(|token| token.label == label);
            assert!(
                is_abstract || declared.contains(label),
                "`{label}` is declared by no table",
            );
        }
    }
    for id in &declared {
        let list = complete_material(id);
        let first_canonical = labels(&list.items)
            .into_iter()
            .find(|label| !abstract_tokens().iter().any(|token| token.label == *label));
        assert_eq!(first_canonical, Some(id.as_str()), "`@{id}` typed in full");
    }
}

#[test]
fn a_longer_token_narrows_the_answer_until_it_is_complete() {
    assert!(complete_material("").is_incomplete);
    let narrowed = complete_material("stone_bri");
    assert!(!narrowed.is_incomplete);
    assert_eq!(
        labels(&narrowed.items),
        [
            "stone_brick_double_slab",
            "stone_brick_slab",
            "stone_brick_stairs",
            "stone_brick_wall",
            "stone_bricks",
            "chiseled_stone_bricks",
            "cracked_stone_bricks",
            "end_stone_brick_double_slab",
            "end_stone_brick_slab",
            "end_stone_brick_stairs",
            "end_stone_brick_wall",
            "end_stone_bricks",
            "infested_chiseled_stone_bricks",
            "infested_cracked_stone_bricks",
            "infested_mossy_stone_bricks",
            "infested_stone_bricks",
            "mossy_stone_brick_double_slab",
            "mossy_stone_brick_slab",
            "mossy_stone_brick_stairs",
            "mossy_stone_brick_wall",
            "mossy_stone_bricks",
            "cracked_polished_blackstone_bricks",
            "polished_blackstone_brick_double_slab",
            "polished_blackstone_brick_slab",
            "polished_blackstone_brick_stairs",
            "polished_blackstone_brick_wall",
            "polished_blackstone_bricks",
        ],
    );
}

#[test]
fn a_material_token_is_matched_on_what_precedes_the_cursor_and_replaced_whole() {
    let source = "theme a:\n  slot floor -> @cherry_planzzz";
    let list = completions(source, at_end_of(source, "@cherry_plan")).expect("in the document");
    assert_eq!(labels(&list.items), ["cherry_planks"]);
    assert_eq!(edit_range(&list.items[0]), range(1, 17, 31));
}

#[test]
fn a_material_token_in_capitals_matches_as_lowercase_and_is_replaced_whole() {
    let source = "theme a:\n  slot floor -> @Cherry_PL";
    let list = completions(source, at_end_of(source, "@Cherry_PL")).expect("in the document");
    assert_eq!(
        labels(&list.items),
        [
            "cherry_planks",
            "cherry_pressure_plate",
            "cherry_sapling",
            "potted_cherry_sapling"
        ],
    );
    for item in &list.items {
        assert_eq!(edit_range(item), range(1, 17, 26));
    }
}

/// Candidates labelled `labels`, in that order.
fn vocabulary<'a>(labels: impl IntoIterator<Item = &'a str>) -> Vec<MaterialCandidate> {
    labels
        .into_iter()
        .map(|label| MaterialCandidate {
            label: label.to_owned(),
            detail: String::new(),
        })
        .collect()
}

/// The labels of what [`rank_and_cut`] kept.
fn kept_labels<'c>(kept: &[&'c MaterialCandidate]) -> Vec<&'c str> {
    kept.iter()
        .map(|candidate| candidate.label.as_str())
        .collect()
}

#[test]
fn candidates_rank_by_vocabulary_then_by_how_they_match() {
    let first = vocabulary(["xbc", "bcx", "x_bc"]);
    let second = vocabulary(["b_c", "bc", "cb"]);
    let (kept, is_incomplete) = rank_and_cut(&[&first, &second], "bc");
    assert!(!is_incomplete);
    assert_eq!(kept_labels(&kept), ["bcx", "x_bc", "xbc", "bc", "b_c"]);
}

#[test]
fn a_vocabulary_as_long_as_the_limit_is_answered_whole() {
    let labels: Vec<String> = (0..MATERIAL_ITEM_LIMIT)
        .map(|n| format!("c{n:03}"))
        .collect();
    let candidates = vocabulary(labels.iter().map(String::as_str));
    let (kept, is_incomplete) = rank_and_cut(&[&candidates], "c");
    assert!(!is_incomplete);
    assert_eq!(kept_labels(&kept), labels);
}

#[test]
fn a_vocabulary_one_past_the_limit_drops_the_candidate_ranked_last() {
    // The one leading match comes last, so the cut must follow the ranking to keep it.
    let mut labels: Vec<String> = (0..MATERIAL_ITEM_LIMIT)
        .map(|n| format!("x_c{n:03}"))
        .collect();
    labels.push("c".to_owned());
    let candidates = vocabulary(labels.iter().map(String::as_str));
    let (kept, is_incomplete) = rank_and_cut(&[&candidates], "c");
    assert!(is_incomplete);
    let expected: Vec<&str> = std::iter::once("c")
        .chain(labels[..MATERIAL_ITEM_LIMIT - 1].iter().map(String::as_str))
        .collect();
    assert_eq!(kept_labels(&kept), expected);
}

#[test]
fn a_keyword_answer_is_never_cut() {
    let list = completions("st", at_end_of("st", "st")).expect("in the document");
    assert!(!list.is_incomplete);
    assert_eq!(labels(&list.items), ["theme", "def", "site", "struct"]);
}

#[test]
fn an_edition_guard_completes_its_word_its_edition_and_the_escape_hatches_under_it() {
    let guard = "struct s size=3x3\n  @";
    assert_eq!(labels(&complete(guard, "@")), ["edition"]);
    let edition = "struct s size=3x3\n  @edition ";
    assert_eq!(labels(&complete(edition, "@edition ")), ["java", "bedrock"]);
    let under = "struct s size=3x3\n  floor mat_slot=f\n  @edition java\n    ";
    assert_eq!(
        labels(&complete(under, "@edition java\n    ")),
        [
            "repeater",
            "comparator",
            "redstone_torch",
            "redstone_dust",
            "raw_block",
            "raw_fill"
        ]
    );
    let after = "struct s size=3x3\n  @edition java\n    raw_block at=1,0,1\n  ";
    assert_eq!(
        labels(&complete(after, "at=1,0,1\n  ")),
        [
            "floor",
            "walls",
            "fill",
            "door",
            "window",
            "roof",
            "stair",
            "level",
            "pressure_plate",
            "lever",
            "button",
            "daylight",
            "observer",
            "lamp",
            "piston",
            "dispenser",
            "chest",
            "repeater",
            "comparator",
            "redstone_torch",
            "redstone_dust",
            "circuit",
            "raw_block",
            "raw_fill",
        ],
    );
    let in_site = "site town size=9x9\n  @";
    assert_eq!(labels(&complete(in_site, "@")), ["edition"]);
}

#[test]
fn no_guard_is_offered_where_one_is_refused() {
    let in_theme = "theme t\n  slot f -> @stone\n  @";
    assert_eq!(complete(in_theme, "\n  @"), vec![]);
    let in_theme_edition = "theme t\n  @edition ";
    assert_eq!(complete(in_theme_edition, "@edition "), vec![]);
    let in_guard = "struct s size=3x3\n  @edition java\n    @";
    assert_eq!(complete(in_guard, "java\n    @"), vec![]);
    let in_guard_edition = "struct s size=3x3\n  @edition java\n    @edition ";
    assert_eq!(complete(in_guard_edition, "    @edition "), vec![]);
    let under_in_site = "site town size=9x9\n  @edition java\n    ";
    assert_eq!(complete(under_in_site, "java\n    "), vec![]);
}

#[test]
fn closed_set_free_positions_offer_nothing() {
    let comment = "# @f";
    assert_eq!(complete(comment, "@f"), vec![]);
    let free_value = "struct s size=2x2\n  walls height=";
    assert_eq!(complete(free_value, "height="), vec![]);
    let header = "@c";
    assert_eq!(complete(header, "@c"), vec![]);
}

#[test]
fn slot_scan_matches_parsed_theme_slots_on_all_examples() {
    let examples = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples");
    let mut themed_examples = 0;
    for entry in std::fs::read_dir(examples).expect("examples directory") {
        let path = entry.expect("directory entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("crn") {
            continue;
        }
        let source = std::fs::read_to_string(&path).expect("readable example");
        let module = parse(&source).expect("example parses");
        let expected: BTreeSet<&str> = module
            .items
            .iter()
            .filter_map(|item| match item {
                Item::Theme { body, .. } => Some(body),
                _ => None,
            })
            .flatten()
            .filter_map(|rule| match rule {
                ThemeRule::Slot { slot, .. } => Some(slot.as_str()),
                _ => None,
            })
            .collect();
        let scanned: BTreeSet<String> = document_slot_names(&source)
            .into_iter()
            .map(|decl| decl.name)
            .collect();
        let scanned: BTreeSet<&str> = scanned.iter().map(String::as_str).collect();
        assert_eq!(scanned, expected, "slot-scan drift in {}", path.display());
        if !expected.is_empty() {
            themed_examples += 1;
        }
    }
    assert!(themed_examples > 0, "guard must cover at least one theme");
}
