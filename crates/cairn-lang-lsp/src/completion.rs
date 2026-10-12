//! Source text + cursor position → the whole closed-vocabulary set valid there.
//!
//! The context is read from the text, not a parse: a document mid-keystroke rarely parses, and
//! Cairn is line-oriented (`spec/syntax` "Lexical").

use std::collections::HashSet;

use cairn_lang_core::Edition;
use cairn_lang_core::Span;
use cairn_lang_core::ast::ItemKind;
use cairn_lang_core::intent::{BodyKind, role_of};
use cairn_lang_formats::builtin_pack;

use crate::line_index::LineIndex;

/// Where the cursor sits, and the byte span of the partial token to replace.
enum Context {
    /// First word of an indent-0 line: a top-level item keyword.
    TopLevelKeyword { replace: Span },
    /// First word of an indented line in an item body: a member keyword that body reads.
    MemberKeyword { replace: Span, body: BodyKind },
    /// First word of an indented line under a header that does not resolve: any member keyword.
    UnscopedMemberKeyword { replace: Span },
    /// First word of an indented line in a `theme` body: `slot` or a selector's member keyword.
    ThemeBodyKeyword { replace: Span },
    /// Value position of `mat_slot=`: a slot name declared by a theme.
    SlotName { replace: Span },
    /// Right after `@` outside the header position: a material token.
    MaterialToken { replace: Span },
}

/// A `slot NAME -> TARGET` declaration found by the document scan.
struct SlotDecl {
    name: String,
    /// The arrow's right side, comment stripped.
    target: String,
    theme: Option<String>,
}

/// Every completion candidate at `position`, or `None` when the document has no such position.
#[must_use]
pub fn completions(
    source: &str,
    position: lsp_types::Position,
) -> Option<Vec<lsp_types::CompletionItem>> {
    let index = LineIndex::new(source);
    let offset = index.offset_at(source, position)?;
    Some(match context_at(source, offset) {
        None => Vec::new(),
        Some(Context::TopLevelKeyword { replace }) => keyword_items(
            &index,
            source,
            &replace,
            ItemKind::ALL
                .into_iter()
                .map(|kind| (kind.keyword(), "top-level item")),
        ),
        Some(Context::MemberKeyword { replace, body }) => keyword_items(
            &index,
            source,
            &replace,
            body.allowed_keywords()
                .into_iter()
                .map(|kw| (kw, "member command")),
        ),
        Some(Context::UnscopedMemberKeyword { replace }) => keyword_items(
            &index,
            source,
            &replace,
            cairn_lang_core::intent::known_keywords()
                .iter()
                .map(|kw| (*kw, "member command")),
        ),
        Some(Context::ThemeBodyKeyword { replace }) => keyword_items(
            &index,
            source,
            &replace,
            std::iter::once(("slot", "slot binding")).chain(
                cairn_lang_core::intent::known_keywords()
                    .iter()
                    .map(|kw| (*kw, "theme selector")),
            ),
        ),
        Some(Context::SlotName { replace }) => slot_name_items(&index, source, &replace),
        Some(Context::MaterialToken { replace }) => material_items(&index, source, &replace),
    })
}

/// Characters a partial token may contain, the dot for abstract material tokens.
fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.'
}

/// Where a line's comment opens, and whether the line ends inside a string.
struct LineScan {
    /// Byte offset of the first `#` outside a string literal.
    comment: Option<usize>,
    /// `false` past a comment opener, whose quotes are comment text.
    open_string: bool,
}

/// The [`LineScan`] of `line`, exact by quote parity: core's lexer reads no escapes in a string.
fn scan_line(line: &str) -> LineScan {
    let mut open_string = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => open_string = !open_string,
            '#' if !open_string => {
                return LineScan {
                    comment: Some(i),
                    open_string: false,
                };
            }
            _ => {}
        }
    }
    LineScan {
        comment: None,
        open_string,
    }
}

/// Byte offset of the first `#` in `line` that opens a comment.
fn comment_start(line: &str) -> Option<usize> {
    scan_line(line).comment
}

/// Classify the cursor's byte offset into a completion [`Context`].
fn context_at(source: &str, offset: usize) -> Option<Context> {
    let line_start = cairn_lang_core::lines::start_of(source, offset);
    let prefix = &source[line_start..offset];
    // Comments and strings take free-form text.
    let scan = scan_line(prefix);
    if scan.comment.is_some() || scan.open_string {
        return None;
    }
    let token_start = line_start + token_start_of(prefix);
    let token_end = offset
        + source[offset..]
            .find(|c: char| !is_token_char(c))
            .unwrap_or(source.len() - offset);
    let replace = token_start..token_end;
    let before_token = &source[line_start..token_start];
    if let Some(head) = before_token.strip_suffix('@') {
        // `@` opening a line is a header directive.
        if head.is_empty() {
            return None;
        }
        return Some(Context::MaterialToken { replace });
    }
    if let Some(head) = before_token.strip_suffix('=') {
        return match &head[token_start_of(head)..] {
            // Not where `mat_slot=` is unread: `check` would report any slot offered as ignored.
            "mat_slot"
                if role_of(line_keyword(before_token))
                    .unread_argument("mat_slot")
                    .is_none() =>
            {
                Some(Context::SlotName { replace })
            }
            _ => None,
        };
    }
    if !before_token.trim_start().is_empty() {
        return None;
    }
    let indent = before_token.len();
    if indent == 0 {
        return Some(Context::TopLevelKeyword { replace });
    }
    match enclosing_item_keyword(source, line_start) {
        Some("theme") => Some(Context::ThemeBodyKeyword { replace }),
        Some("site") => Some(Context::MemberKeyword {
            replace,
            body: BodyKind::Site,
        }),
        Some("struct" | "def") => Some(Context::MemberKeyword {
            replace,
            body: BodyKind::Geometry,
        }),
        _ => Some(Context::UnscopedMemberKeyword { replace }),
    }
}

/// The word `line` opens with after its indent.
fn line_keyword(line: &str) -> &str {
    let trimmed = line.trim_start();
    let end = trimmed
        .find(|c: char| !is_token_char(c))
        .unwrap_or(trimmed.len());
    &trimmed[..end]
}

/// First word of the nearest non-blank, non-comment indent-0 line above `line_start`.
fn enclosing_item_keyword(source: &str, line_start: usize) -> Option<&str> {
    cairn_lang_core::lines::split(&source[..line_start])
        .rev()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            Some((line.len() - trimmed.len(), trimmed))
        })
        .find(|(line_indent, _)| *line_indent == 0)
        .map(|(_, trimmed)| line_keyword(trimmed))
}

/// Every `slot NAME -> TARGET` line, found without parsing: the cursor's own line rarely parses.
fn document_slot_names(source: &str) -> Vec<SlotDecl> {
    let mut decls = Vec::new();
    let mut current_theme: Option<String> = None;
    for raw_line in cairn_lang_core::lines::split(source) {
        let line = comment_start(raw_line).map_or(raw_line, |i| &raw_line[..i]);
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        let indent = line.len() - trimmed.len();
        if indent == 0 {
            current_theme = trimmed.strip_prefix("theme").and_then(|rest| {
                let name = rest
                    .trim_start()
                    .chars()
                    .take_while(|c| is_token_char(*c))
                    .collect::<String>();
                (rest.starts_with(char::is_whitespace) && !name.is_empty()).then_some(name)
            });
            continue;
        }
        let Some(rest) = trimmed.strip_prefix("slot") else {
            continue;
        };
        if !rest.starts_with(char::is_whitespace) {
            continue;
        }
        let rest = rest.trim_start();
        let name: String = rest.chars().take_while(|c| is_token_char(*c)).collect();
        if name.is_empty() {
            continue;
        }
        let Some(target) = rest[name.len()..].trim_start().strip_prefix("->") else {
            continue;
        };
        let target = target.trim().to_owned();
        decls.push(SlotDecl {
            name,
            target,
            theme: current_theme.clone(),
        });
    }
    decls
}

/// Byte offset where the token ending `text` begins, or `text.len()`.
fn token_start_of(text: &str) -> usize {
    text.char_indices()
        .rev()
        .take_while(|(_, c)| is_token_char(*c))
        .last()
        .map_or(text.len(), |(i, _)| i)
}

/// One item replacing `replace` with `label`; `order` holds its place against client re-sorting.
fn completion_item(
    index: &LineIndex,
    source: &str,
    replace: &Span,
    label: &str,
    kind: lsp_types::CompletionItemKind,
    detail: String,
    order: usize,
) -> lsp_types::CompletionItem {
    lsp_types::CompletionItem {
        label: label.to_owned(),
        kind: Some(kind),
        detail: Some(detail),
        // Six digits keep lexicographic order across tens of thousands of block ids.
        sort_text: Some(format!("{order:06}")),
        text_edit: Some(lsp_types::CompletionTextEdit::Edit(lsp_types::TextEdit {
            range: index.range(source, replace),
            new_text: label.to_owned(),
        })),
        ..lsp_types::CompletionItem::default()
    }
}

/// Keyword candidates: `(label, detail)` pairs in their canonical order.
fn keyword_items<'a>(
    index: &LineIndex,
    source: &str,
    replace: &Span,
    candidates: impl Iterator<Item = (&'a str, &'a str)>,
) -> Vec<lsp_types::CompletionItem> {
    candidates
        .enumerate()
        .map(|(order, (label, detail))| {
            completion_item(
                index,
                source,
                replace,
                label,
                lsp_types::CompletionItemKind::KEYWORD,
                detail.to_owned(),
                order,
            )
        })
        .collect()
}

/// The slot names every theme in the document declares, in declaration order.
fn slot_name_items(
    index: &LineIndex,
    source: &str,
    replace: &Span,
) -> Vec<lsp_types::CompletionItem> {
    let mut seen = HashSet::new();
    document_slot_names(source)
        .into_iter()
        .filter(|decl| seen.insert(decl.name.clone()))
        .enumerate()
        .map(|(order, decl)| {
            let detail = match &decl.theme {
                Some(theme) => format!("-> {} (theme {theme})", decl.target),
                None => format!("-> {}", decl.target),
            };
            completion_item(
                index,
                source,
                replace,
                &decl.name,
                lsp_types::CompletionItemKind::VARIABLE,
                detail,
                order,
            )
        })
        .collect()
}

/// The built-in packs' abstract tokens, Java's first, then the canonical ids they resolve to.
fn material_items(
    index: &LineIndex,
    source: &str,
    replace: &Span,
) -> Vec<lsp_types::CompletionItem> {
    let java = &builtin_pack(Edition::Java).materials;
    let bedrock = &builtin_pack(Edition::Bedrock).materials;
    let mut items = Vec::new();
    let mut order = 0;
    let mut seen = HashSet::new();
    for token in java.tokens().chain(bedrock.tokens()) {
        if !seen.insert(token) {
            continue;
        }
        let Some(id) = java.lookup_id(token).or_else(|| bedrock.lookup_id(token)) else {
            continue;
        };
        items.push(completion_item(
            index,
            source,
            replace,
            token,
            lsp_types::CompletionItemKind::VALUE,
            id.to_owned(),
            order,
        ));
        order += 1;
    }
    // Both packs' ids: a token may resolve differently per edition, and both are canonical.
    let mut seen_ids = HashSet::new();
    let resolved_ids = java
        .tokens()
        .filter_map(|t| java.lookup_id(t))
        .chain(bedrock.tokens().filter_map(|t| bedrock.lookup_id(t)));
    for id in resolved_ids {
        if !seen_ids.insert(id) {
            continue;
        }
        // Built-in canonical tokens are written without `minecraft:`.
        let label = id.strip_prefix("minecraft:").unwrap_or(id);
        items.push(completion_item(
            index,
            source,
            replace,
            label,
            lsp_types::CompletionItemKind::VALUE,
            "canonical block id".to_owned(),
            order,
        ));
        order += 1;
    }
    items
}

#[cfg(test)]
mod tests {
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
        completions(source, at_end_of(source, needle)).expect("position within document")
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
            completions(source, at_end_of(source, "id=\"@oa")),
            Some(vec![]),
        );
    }

    #[test]
    fn a_mat_slot_key_inside_a_string_offers_nothing() {
        let source = "theme a:\n  slot floor -> @oak_planks\n\
                      struct s size=2x2\n  window id=\"mat_slot=";
        assert_eq!(
            completions(source, at_end_of(source, "id=\"mat_slot=")),
            Some(vec![]),
        );
    }

    #[test]
    fn a_mat_slot_key_inside_a_longer_string_offers_nothing() {
        let source = "theme a:\n  slot floor -> @oak_planks\n\
                      struct s size=2x2\n  window label=\"pick mat_slot=fl";
        assert_eq!(
            completions(source, at_end_of(source, "pick mat_slot=fl")),
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
            ("struct s size=2x2\n  door side=front mat_slot=", vec![]),
            ("struct s size=2x2\n  door side=front mat_slot=fl", vec![]),
            (
                "struct s size=2x2\n  window side=front mat_slot=",
                vec!["floor"],
            ),
            ("theme b:\n  door[mat_slot=", vec![]),
            ("theme b:\n  window[mat_slot=", vec!["floor"]),
        ] {
            let source = format!("{theme}{rest}");
            let items = completions(&source, at_end_of(&source, rest)).expect("in the document");
            assert_eq!(labels(&items), expected, "{rest:?}");
        }
        for keyword in cairn_lang_core::intent::known_keywords() {
            let rest = format!("struct s size=2x2\n  {keyword} mat_slot=");
            let source = format!("{theme}{rest}");
            let items = completions(&source, at_end_of(&source, &rest)).expect("in the document");
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
        for (label, source) in renderings(
            "theme a:\n  slot floor -> @oak_planks\nstruct s size=2x2\n  floor mat_slot=",
        ) {
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

    #[test]
    fn material_token_position_offers_registry_catalog_in_order() {
        let source = "theme a:\n  slot floor -> @";
        let items = complete(source, "@");
        assert_eq!(items[0].label, "floor.wood.broadleaf");
        assert_eq!(items[0].detail.as_deref(), Some("minecraft:oak_planks"));
        for item in &items {
            assert_eq!(item.kind, Some(lsp_types::CompletionItemKind::VALUE));
        }
        let sort_texts: Vec<&str> = items
            .iter()
            .map(|i| i.sort_text.as_deref().expect("sort_text present"))
            .collect();
        let mut sorted = sort_texts.clone();
        sorted.sort_unstable();
        assert_eq!(sort_texts, sorted, "items must carry their own order");

        let java = &builtin_pack(Edition::Java).materials;
        let bedrock = &builtin_pack(Edition::Bedrock).materials;
        let expected_abstract: BTreeSet<&str> = java.tokens().chain(bedrock.tokens()).collect();
        let (canonical, abstract_items): (Vec<_>, Vec<_>) = items
            .iter()
            .partition(|i| i.detail.as_deref() == Some("canonical block id"));
        let abstract_labels: BTreeSet<&str> =
            abstract_items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(abstract_labels, expected_abstract);
        let expected_canonical: BTreeSet<&str> = java
            .tokens()
            .filter_map(|t| java.lookup_id(t))
            .chain(bedrock.tokens().filter_map(|t| bedrock.lookup_id(t)))
            .map(|id| id.strip_prefix("minecraft:").unwrap_or(id))
            .collect();
        let canonical_labels: Vec<&str> = canonical.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(
            canonical_labels.iter().copied().collect::<BTreeSet<_>>(),
            expected_canonical,
            "every id either catalog resolves to is offered, Bedrock-only ones included",
        );
        assert_eq!(
            canonical_labels.len(),
            expected_canonical.len(),
            "canonical ids are deduplicated across catalog rows",
        );
    }

    #[test]
    fn material_token_partial_prefix_returns_full_set_replacing_after_at() {
        let source = "theme a:\n  slot floor -> @flo";
        let items = complete(source, "@flo");
        let empty_prefix_source = "theme a:\n  slot floor -> @";
        let full = complete(empty_prefix_source, "@");
        assert_eq!(items.len(), full.len());
        let line = "  slot floor -> @flo";
        let after_at = u32::try_from(line.find('@').expect("at sign") + 1).expect("fits u32");
        for item in &items {
            assert_eq!(edit_range(item), range(1, after_at, after_at + 3));
        }
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
}
