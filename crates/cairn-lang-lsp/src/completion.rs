//! Source text + cursor position → completion candidates.
//!
//! [`completions`] classifies the cursor into one of a handful of
//! closed-vocabulary contexts and returns the *entire* valid set for that
//! context, each item carrying a `TextEdit` that replaces the partial token
//! under the cursor — prefix filtering is the client's job (it re-filters on
//! every keystroke without a round-trip), the server's job is to never offer
//! a candidate outside the closed set (principles P3: the registry table is
//! the source of truth, so suggestions cannot hallucinate identifiers).
//!
//! Context detection is a line-local text heuristic, not a parse: Cairn is
//! strictly line-oriented (one line = one command, spec syntax §5.1), so the
//! line prefix up to the cursor is grammatically sufficient — and the parser
//! stops at its first error, which a document mid-keystroke almost always
//! contains. The one cross-line lookup (which block encloses the cursor
//! line) walks indentation upward, and `mat_slot=` values are collected by
//! scanning `slot NAME -> TARGET` lines document-wide; a drift-guard test
//! asserts that scan agrees with the parser on every shipped example.

use std::collections::HashSet;

use cairn_lang_core::Span;
use cairn_lang_core::intent::known_keywords;
use cairn_lang_formats::{builtin_bedrock, builtin_java};

use crate::line_index::LineIndex;

/// Keywords that may open a top-level (indent 0) line, in the order the
/// parser names them when rejecting anything else.
const TOP_LEVEL_KEYWORDS: &[&str] = &["theme", "def", "site", "struct"];

/// Where the cursor sits, and the byte span of the partial token to replace.
enum Context {
    /// First word of an indent-0 line: a top-level item keyword.
    TopLevelKeyword { replace: Span },
    /// First word of an indented line inside a `struct`/`def`/`site` (or
    /// a deeper `level`) body: a member command keyword.
    MemberKeyword { replace: Span },
    /// First word of an indented line inside a `theme` body: `slot` or a
    /// member keyword opening a selector rule.
    ThemeBodyKeyword { replace: Span },
    /// Value position of `mat_slot=`: a slot name declared by a theme.
    SlotName { replace: Span },
    /// Right after `@` outside the header position: a material token.
    MaterialToken { replace: Span },
}

/// A `slot NAME -> TARGET` declaration found by the document scan.
struct SlotDecl {
    /// Slot name (the completion label).
    name: String,
    /// RHS of the arrow, comment-stripped — shown in the item detail.
    target: String,
    /// Name of the enclosing theme, when the declaration sits under a
    /// `theme` header.
    theme: Option<String>,
}

/// Return every completion candidate for `position`; an empty list when the
/// cursor is not in any closed-vocabulary context (never invent candidates
/// where the grammar accepts free-form input), and `None` when the position
/// does not exist in the document at all (see [`LineIndex::offset_at`]) —
/// the transport layer turns that into a request error.
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
            TOP_LEVEL_KEYWORDS.iter().map(|kw| (*kw, "top-level item")),
        ),
        Some(Context::MemberKeyword { replace }) => keyword_items(
            &index,
            source,
            &replace,
            known_keywords().iter().map(|kw| (*kw, "member command")),
        ),
        Some(Context::ThemeBodyKeyword { replace }) => keyword_items(
            &index,
            source,
            &replace,
            std::iter::once(("slot", "slot binding"))
                .chain(known_keywords().iter().map(|kw| (*kw, "theme selector"))),
        ),
        Some(Context::SlotName { replace }) => slot_name_items(&index, source, &replace),
        Some(Context::MaterialToken { replace }) => material_items(&index, source, &replace),
    })
}

/// Characters a partial token under the cursor may contain. The dot admits
/// abstract material tokens (`floor.wood.broadleaf`); keywords and slot
/// names never contain one, so a dotted prefix simply fails their client
/// filter instead of splitting the replace range.
fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.'
}

/// Byte offset of the first `#` in `line` that opens a comment — i.e. sits
/// outside a string literal. Exact by quote parity: the lexer scans strings
/// atomically with no escape sequences, and a string cannot span lines, so
/// counting `"` toggles is the full string grammar.
fn comment_start(line: &str) -> Option<usize> {
    let mut in_string = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_string = !in_string,
            '#' if !in_string => return Some(i),
            _ => {}
        }
    }
    None
}

/// Classify the cursor's byte offset into a completion [`Context`].
fn context_at(source: &str, offset: usize) -> Option<Context> {
    let line_start = source[..offset].rfind('\n').map_or(0, |i| i + 1);
    let prefix = &source[line_start..offset];
    // A comment opener before the cursor puts it in a comment.
    if comment_start(prefix).is_some() {
        return None;
    }
    let token_start = line_start
        + prefix
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_token_char(*c))
            .last()
            .map_or(prefix.len(), |(i, _)| i);
    // The token continues past the cursor: replace all of it, or accepting
    // an item would leave the token's tail glued to the inserted text.
    let token_end = offset
        + source[offset..]
            .find(|c: char| !is_token_char(c))
            .unwrap_or(source.len() - offset);
    let replace = token_start..token_end;
    let before_token = &source[line_start..token_start];
    if let Some(head) = before_token.strip_suffix('@') {
        // `@` opening an indent-0 line is a header directive (`@cairn`,
        // `@requires`, ...), not a material token position.
        if head.is_empty() {
            return None;
        }
        return Some(Context::MaterialToken { replace });
    }
    if let Some(head) = before_token.strip_suffix('=') {
        let key_start = head
            .char_indices()
            .rev()
            .take_while(|(_, c)| is_token_char(*c))
            .last()
            .map_or(head.len(), |(i, _)| i);
        return match &head[key_start..] {
            "mat_slot" => Some(Context::SlotName { replace }),
            // Other keys take free-form or not-yet-tabled values; offering
            // anything would be an invented vocabulary.
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
    match enclosing_block_keyword(source, line_start, indent) {
        Some("theme") => Some(Context::ThemeBodyKeyword { replace }),
        _ => Some(Context::MemberKeyword { replace }),
    }
}

/// First word of the nearest non-blank, non-comment line above `line_start`
/// with strictly smaller indentation — the block header enclosing the
/// cursor line.
fn enclosing_block_keyword(source: &str, line_start: usize, indent: usize) -> Option<&str> {
    source[..line_start]
        .lines()
        .rev()
        .filter_map(|line| {
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            Some((line.len() - trimmed.len(), trimmed))
        })
        .find(|(line_indent, _)| *line_indent < indent)
        .map(|(_, trimmed)| {
            let end = trimmed
                .find(|c: char| !is_token_char(c))
                .unwrap_or(trimmed.len());
            &trimmed[..end]
        })
}

/// Scan the whole document for `slot NAME -> TARGET` lines, tracking the
/// enclosing `theme` header. Line-based on purpose: this has to work while
/// the cursor's own line (or any other line) fails to parse.
fn document_slot_names(source: &str) -> Vec<SlotDecl> {
    let mut decls = Vec::new();
    let mut current_theme: Option<String> = None;
    for raw_line in source.lines() {
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

/// Build one [`lsp_types::CompletionItem`] replacing `replace` with `label`.
/// `order` freezes the server's candidate order against client re-sorting —
/// closed sets are curated (declaration / catalog order), not alphabetical.
fn item(
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
        // Six digits leave headroom for the full canonical block vocabulary
        // (tens of thousands of ids) without breaking lexicographic order.
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
            item(
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

/// Slot-name candidates for a `mat_slot=` value: the union of slot names
/// declared by every theme in the document, in declaration order. Unpinned
/// editions union naturally — a `_java`/`_bedrock` variant theme is just
/// another theme to the scan, matching how `cairn check` unions slot names
/// when no edition is picked.
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
            item(
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

/// Material-token candidates after `@`: the built-in registry union
/// (java ∪ bedrock, java order first) — abstract tokens with their resolved
/// canonical id as detail, then the deduplicated canonical ids themselves.
/// The canonical list is the catalog's value column, not a full block
/// vocabulary: a complete canonical set needs a blocks table the registry
/// packs do not carry yet.
fn material_items(
    index: &LineIndex,
    source: &str,
    replace: &Span,
) -> Vec<lsp_types::CompletionItem> {
    let java = &builtin_java().materials;
    let bedrock = &builtin_bedrock().materials;
    let mut items = Vec::new();
    let mut order = 0;
    let mut seen = HashSet::new();
    for token in java.tokens().chain(bedrock.tokens()) {
        if !seen.insert(token) {
            continue;
        }
        // A token without a resolvable id cannot be offered with a truthful
        // detail; skip it rather than crash the whole server on a future
        // catalog shape this code did not anticipate.
        let Some(id) = java.lookup_id(token).or_else(|| bedrock.lookup_id(token)) else {
            continue;
        };
        items.push(item(
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
    let mut seen_ids = HashSet::new();
    let resolved_ids = java
        .tokens()
        .filter_map(|t| java.lookup_id(t))
        .chain(bedrock.tokens().filter_map(|t| bedrock.lookup_id(t)));
    for id in resolved_ids {
        if !seen_ids.insert(id) {
            continue;
        }
        // The DSL writes built-in canonical tokens without the default
        // namespace (`@oak_planks`); ids from another namespace keep it.
        let label = id.strip_prefix("minecraft:").unwrap_or(id);
        items.push(item(
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

    /// Candidates at the position right after `needle`, asserting the
    /// position is inside the document.
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
        // The first word of an indent-0 line completes to the parser's
        // closed set of top-level items, replacing the partial token.
        let source = "st";
        let items = complete(source, "st");
        assert_eq!(labels(&items), vec!["theme", "def", "site", "struct"]);
        for item in &items {
            assert_eq!(item.kind, Some(lsp_types::CompletionItemKind::KEYWORD));
            assert_eq!(edit_range(item), range(0, 0, 2));
        }
    }

    #[test]
    fn member_position_offers_all_known_keywords_with_replace_range() {
        // Inside a struct body the full member-keyword closed set comes
        // back (no server-side prefix filter) and every item replaces the
        // partial token's span.
        let source = "struct s size=2x2\n  flo";
        let items = complete(source, "flo");
        assert_eq!(labels(&items), known_keywords().to_vec());
        for item in &items {
            assert_eq!(item.kind, Some(lsp_types::CompletionItemKind::KEYWORD));
            assert_eq!(edit_range(item), range(1, 2, 5));
        }
    }

    #[test]
    fn mid_token_cursor_replaces_the_whole_token() {
        // A cursor in the middle of a word must replace the entire token,
        // not just the half before the cursor — otherwise accepting an item
        // leaves the token's tail glued to the insertion (`structtsam`).
        let source = "struct s size=2x2\n  flotsam";
        let items = complete(source, "  flo");
        assert_eq!(labels(&items), known_keywords().to_vec());
        for item in &items {
            assert_eq!(edit_range(item), range(1, 2, 9));
        }
    }

    #[test]
    fn theme_body_offers_slot_and_selector_keywords() {
        // A theme body line starts with `slot` or a member keyword
        // opening a selector rule; the details distinguish the two.
        let source = "theme x:\n  s";
        let items = complete(source, "\n  s");
        assert_eq!(items[0].label, "slot");
        assert_eq!(items[0].detail.as_deref(), Some("slot binding"));
        let window = items
            .iter()
            .find(|i| i.label == "window")
            .expect("member keywords offered as selectors");
        assert_eq!(window.detail.as_deref(), Some("theme selector"));
        assert_eq!(items.len(), 1 + known_keywords().len());
    }

    #[test]
    fn nested_block_body_offers_member_keywords() {
        // Deeper nesting inside a struct (`level`) still resolves
        // to the member-command set: the enclosing-block walk stops at the
        // nearest shallower line, which is not a `theme` header.
        let source = "struct t size=5x5\n  level h=4\n    flo";
        let items = complete(source, "    flo");
        assert_eq!(labels(&items), known_keywords().to_vec());
        assert!(!labels(&items).contains(&"slot"));
    }

    #[test]
    fn slot_names_union_across_themes_survives_parse_failure() {
        // The cursor's own line (`mat_slot=` with no value) makes the
        // whole document unparseable — the main case completion exists for —
        // and slot names still arrive as the union across all themes.
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
        // An astral char earlier in the line shifts the replace range
        // by UTF-16 units, not bytes or scalars (same discrimination as the
        // diagnostics range test).
        let source = "theme a:\n  slot walls -> @cobblestone\n\
                      struct s size=2x2\n  door id=\"😀\" mat_slot=wa";
        let items = complete(source, "mat_slot=wa");
        assert_eq!(labels(&items), vec!["walls"]);
        let line = "  door id=\"😀\" mat_slot=wa";
        let byte_col = line.find("wa").expect("partial token");
        let utf16_col = u32::try_from(line[..byte_col].encode_utf16().count()).expect("fits u32");
        assert_ne!(u32::try_from(byte_col).expect("fits"), utf16_col);
        assert_eq!(edit_range(&items[0]), range(3, utf16_col, utf16_col + 2));
    }

    #[test]
    fn hash_inside_a_string_literal_is_not_a_comment() {
        // The lexer scans strings atomically (a `#` between quotes is
        // string content, not a comment opener), so completion must not go
        // dark on a line like `door id="#front" mat_slot=`.
        let source = "theme a:\n  slot floor -> @oak_planks\n\
                      struct s size=2x2\n  door id=\"#front\" mat_slot=";
        let items = complete(source, "mat_slot=");
        assert_eq!(labels(&items), vec!["floor"]);
    }

    #[test]
    fn slot_target_detail_strips_comments_outside_strings_only() {
        // A trailing comment on the slot line stays out of the detail, but
        // a `#` inside a quoted value survives into it.
        let source = "theme a:\n  slot floor -> @oak_planks # the default\n\
                      theme b:\n  slot sign -> \"#1\"\n\
                      struct s size=2x2\n  floor mat_slot=";
        let items = complete(source, "mat_slot=");
        assert_eq!(items[0].detail.as_deref(), Some("-> @oak_planks (theme a)"),);
        assert_eq!(items[1].detail.as_deref(), Some("-> \"#1\" (theme b)"));
    }

    #[test]
    fn crlf_documents_complete_and_scan_slots() {
        // CRLF line endings must not shift keyword classification, replace
        // ranges, or the slot scan.
        let keyword_source = "struct s size=2x2\r\n  flo";
        let items = complete(keyword_source, "flo");
        assert_eq!(labels(&items), known_keywords().to_vec());
        assert_eq!(edit_range(&items[0]), range(1, 2, 5));

        let slot_source = "theme a:\r\n  slot floor -> @oak_planks\r\n\
                           struct s size=2x2\r\n  floor mat_slot=";
        let items = complete(slot_source, "mat_slot=");
        assert_eq!(labels(&items), vec!["floor"]);
        assert_eq!(items[0].detail.as_deref(), Some("-> @oak_planks (theme a)"));
    }

    #[test]
    fn position_far_past_the_document_is_refused() {
        // A position beyond one line past the end is a client bug, not a
        // clampable race: the caller gets `None` (surfaced as InvalidParams
        // by the server) instead of candidates fabricated at EOF. One line
        // past the end still clamps — a didChange can land between the
        // request and its answer.
        let source = "st";
        let far = lsp_types::Position {
            line: 99,
            character: 0,
        };
        assert_eq!(completions(source, far), None);
        let one_past = lsp_types::Position {
            line: 1,
            character: 0,
        };
        let items = completions(source, one_past).expect("one line past the end clamps");
        assert_eq!(labels(&items), vec!["theme", "def", "site", "struct"]);
    }

    #[test]
    fn material_token_position_offers_registry_catalog_in_order() {
        // After `@` the registry union (java ∪ bedrock) arrives in
        // catalog insertion order — abstract tokens with their resolved id
        // as detail, then the deduplicated canonical ids.
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

        let java = &builtin_java().materials;
        let bedrock = &builtin_bedrock().materials;
        let expected_abstract: BTreeSet<&str> = java.tokens().chain(bedrock.tokens()).collect();
        let (canonical, abstract_items): (Vec<_>, Vec<_>) = items
            .iter()
            .partition(|i| i.detail.as_deref() == Some("canonical block id"));
        let abstract_labels: BTreeSet<&str> =
            abstract_items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(abstract_labels, expected_abstract);
        let oak_planks: Vec<_> = canonical
            .iter()
            .filter(|i| i.label == "oak_planks")
            .collect();
        assert_eq!(
            oak_planks.len(),
            1,
            "canonical ids are deduplicated across catalog rows",
        );
    }

    #[test]
    fn material_token_partial_prefix_returns_full_set_replacing_after_at() {
        // A typed prefix does not shrink the server's answer — the
        // client filters — but the replace range starts right after `@`.
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
        // No candidates in a comment, in a free-form value position,
        // or in the header directive position (`@` at indent 0).
        let comment = "# @f";
        assert_eq!(complete(comment, "@f"), vec![]);
        let free_value = "struct s size=2x2\n  walls height=";
        assert_eq!(complete(free_value, "height="), vec![]);
        let header = "@c";
        assert_eq!(complete(header, "@c"), vec![]);
    }

    #[test]
    fn slot_scan_matches_parsed_theme_slots_on_all_examples() {
        // Drift guard: the line scan and the parser must agree on the
        // slot names of every shipped example, so the scan cannot silently
        // fall behind grammar changes.
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
