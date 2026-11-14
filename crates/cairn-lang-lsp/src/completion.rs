//! Source text + cursor position → the closed-vocabulary set valid there: whole for a keyword or a
//! slot name, and for a material token the tokens matching what is typed, bounded.
//!
//! The context is read from the text, not a parse: a document mid-keystroke rarely parses, and
//! Cairn is line-oriented (`spec/syntax` "Lexical").

use std::collections::{BTreeMap, HashSet};
use std::sync::OnceLock;

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
    /// Right after the `@` that opens an indented line: `edition`, the only guard there is.
    GuardWord { replace: Span },
    /// The word after an indented line's `@edition`: an edition.
    EditionName { replace: Span },
    /// First word of a line indented under an `@edition` guard: a raw member or a Tier 0 part.
    GuardedMemberKeyword { replace: Span },
}

/// A `slot NAME -> TARGET` declaration found by the document scan.
struct SlotDecl {
    name: String,
    /// The arrow's right side, comment stripped.
    target: String,
    theme: Option<String>,
}

/// How many items a material-token answer holds at most. A longer match is cut there and marked
/// incomplete, so the client asks again as the token grows.
pub const MATERIAL_ITEM_LIMIT: usize = 100;

/// The completion candidates at `position`, or `None` when the document has no such position.
#[must_use]
pub fn completions(
    source: &str,
    position: lsp_types::Position,
) -> Option<lsp_types::CompletionList> {
    let index = LineIndex::new(source);
    let offset = index.offset_at(source, position)?;
    let whole = |items| lsp_types::CompletionList {
        is_incomplete: false,
        items,
    };
    Some(match context_at(source, offset) {
        None => whole(Vec::new()),
        Some(Context::TopLevelKeyword { replace }) => whole(keyword_items(
            &index,
            source,
            &replace,
            ItemKind::ALL
                .into_iter()
                .map(|kind| (kind.keyword(), "top-level item")),
        )),
        Some(Context::MemberKeyword { replace, body }) => whole(keyword_items(
            &index,
            source,
            &replace,
            body.allowed_keywords()
                .into_iter()
                .map(|kw| (kw, "member command")),
        )),
        Some(Context::UnscopedMemberKeyword { replace }) => whole(keyword_items(
            &index,
            source,
            &replace,
            cairn_lang_core::intent::known_keywords()
                .iter()
                .map(|kw| (*kw, "member command")),
        )),
        Some(Context::ThemeBodyKeyword { replace }) => whole(keyword_items(
            &index,
            source,
            &replace,
            std::iter::once(("slot", "slot binding")).chain(
                cairn_lang_core::intent::known_keywords()
                    .iter()
                    .map(|kw| (*kw, "theme selector")),
            ),
        )),
        Some(Context::SlotName { replace }) => whole(slot_name_items(&index, source, &replace)),
        Some(Context::MaterialToken { replace }) => {
            material_items(&index, source, &replace, offset)
        }
        Some(Context::GuardWord { replace }) => whole(keyword_items(
            &index,
            source,
            &replace,
            std::iter::once(("edition", "edition guard")),
        )),
        Some(Context::EditionName { replace }) => whole(keyword_items(
            &index,
            source,
            &replace,
            [Edition::Java, Edition::Bedrock]
                .into_iter()
                .map(|edition| (edition.as_str(), "edition")),
        )),
        Some(Context::GuardedMemberKeyword { replace }) => whole(keyword_items(
            &index,
            source,
            &replace,
            cairn_lang_core::intent::known_keywords()
                .iter()
                .filter(|kw| role_of(kw).is_escape_hatch())
                .map(|kw| {
                    let detail = if role_of(kw).is_tier0_part() {
                        "Tier 0 part"
                    } else {
                        "raw member"
                    };
                    (*kw, detail)
                }),
        )),
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
    let (outside, in_table) = lines_outside_truth_tables(&source[..line_start]);
    // A truth table's rows are bits, which no vocabulary completes.
    if in_table {
        return None;
    }
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
        // `@` opening a line is a header directive, or in a body an edition guard.
        if head.is_empty() {
            return None;
        }
        if head.trim_start().is_empty() {
            return guard_may_open(&outside, head.len()).then_some(Context::GuardWord { replace });
        }
        return Some(Context::MaterialToken { replace });
    }
    if before_token.starts_with(char::is_whitespace)
        && before_token.ends_with(char::is_whitespace)
        && before_token.trim().strip_prefix('@').map(str::trim_start) == Some("edition")
    {
        let indent = before_token.len() - before_token.trim_start().len();
        return guard_may_open(&outside, indent).then_some(Context::EditionName { replace });
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
    if parent_line(&outside, indent).is_some_and(opens_a_guard) {
        // What a guard holds is misplaced in a `site` body, guarded or not.
        return (enclosing_item_keyword(&outside) != Some("site"))
            .then_some(Context::GuardedMemberKeyword { replace });
    }
    match enclosing_item_keyword(&outside) {
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

/// The lines of `text` outside every truth table's braces, and whether a table is still open
/// where `text` ends (`spec/syntax` "Literals and separators").
fn lines_outside_truth_tables(text: &str) -> (Vec<&str>, bool) {
    let mut outside = Vec::new();
    let mut open = false;
    for line in cairn_lang_core::lines::split(text) {
        let code = comment_start(line).map_or(line, |i| &line[..i]);
        // Core's lexer ends a table at a line no row or `}` can open.
        let continues =
            code.trim_start().starts_with(['0', '1', '-', ';', '}']) || code.trim().is_empty();
        if !(open && continues) {
            open = false;
            outside.push(line);
        }
        open = truth_table_open_after(code, open);
    }
    (outside, open)
}

/// Whether a truth table is open after `code`, a line without its comment, given whether one was
/// open before it.
fn truth_table_open_after(code: &str, mut open: bool) -> bool {
    let keyword = line_keyword(code);
    let opens = keyword == "assert" && line_keyword(&code.trim_start()[keyword.len()..]) == "truth";
    let mut in_string = false;
    for c in code.chars() {
        match c {
            '"' => in_string = !in_string,
            '{' if opens && !in_string => open = true,
            '}' if !in_string => open = false,
            _ => {}
        }
    }
    open
}

/// Whether a guard may open on a line indented `indent` after `lines`: in a `struct`, `def` or
/// `site` body, and not directly under another guard (`spec/versioning-editions` "An `@edition`
/// guard holds the escape hatch").
fn guard_may_open(lines: &[&str], indent: usize) -> bool {
    matches!(
        enclosing_item_keyword(lines),
        Some("struct" | "def" | "site")
    ) && !parent_line(lines, indent).is_some_and(opens_a_guard)
}

/// Whether `line`, trimmed, is an `@edition` guard's own.
fn opens_a_guard(line: &str) -> bool {
    line.strip_prefix('@')
        .is_some_and(|rest| line_keyword(rest) == "edition")
}

/// The last non-blank, non-comment line of `lines` indented less than `indent`, trimmed.
fn parent_line<'a>(lines: &[&'a str], indent: usize) -> Option<&'a str> {
    lines.iter().rev().find_map(|line| {
        let code = comment_start(line).map_or(*line, |i| &line[..i]);
        let trimmed = code.trim_start();
        (!trimmed.is_empty() && code.len() - trimmed.len() < indent).then_some(trimmed)
    })
}

/// First word of the last non-blank, non-comment indent-0 line in `lines`.
fn enclosing_item_keyword<'a>(lines: &[&'a str]) -> Option<&'a str> {
    lines
        .iter()
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

/// A material token a completion may offer, with its detail.
struct MaterialCandidate {
    label: String,
    detail: String,
}

/// The built-in catalogs' abstract tokens, Java's first, each with the id it resolves to.
fn abstract_tokens() -> &'static [MaterialCandidate] {
    static TOKENS: OnceLock<Vec<MaterialCandidate>> = OnceLock::new();
    TOKENS.get_or_init(|| {
        let java = builtin_pack(Edition::Java).materials();
        let bedrock = builtin_pack(Edition::Bedrock).materials();
        let mut seen = HashSet::new();
        java.tokens()
            .chain(bedrock.tokens())
            .filter(|token| seen.insert(*token))
            .filter_map(|token| {
                let id = java.lookup_id(token).or_else(|| bedrock.lookup_id(token))?;
                Some(MaterialCandidate {
                    label: token.to_owned(),
                    detail: id.to_owned(),
                })
            })
            .collect()
    })
}

/// Every canonical id a built-in pack's blocks table declares, Java's and Bedrock's, sorted, each
/// with the editions and versions that declare it.
fn canonical_ids() -> &'static [MaterialCandidate] {
    static IDS: OnceLock<Vec<MaterialCandidate>> = OnceLock::new();
    IDS.get_or_init(|| {
        let mut declared: BTreeMap<&str, Vec<(Edition, Vec<&str>)>> = BTreeMap::new();
        for edition in [Edition::Java, Edition::Bedrock] {
            let blocks = builtin_pack(edition).blocks();
            for version in blocks.versions() {
                for id in blocks.ids_for(version).unwrap_or_default() {
                    let editions = declared.entry(id).or_default();
                    match editions.last_mut() {
                        Some((seen, versions)) if *seen == edition => versions.push(version),
                        _ => editions.push((edition, vec![version])),
                    }
                }
            }
        }
        declared
            .into_iter()
            .map(|(id, editions)| MaterialCandidate {
                // Built-in canonical tokens are written without `minecraft:`.
                label: id.strip_prefix("minecraft:").unwrap_or(id).to_owned(),
                detail: editions
                    .iter()
                    .map(|(edition, versions)| {
                        format!("{} {}", edition.as_str(), versions.join(", "))
                    })
                    .collect::<Vec<_>>()
                    .join("; "),
            })
            .collect()
    })
}

/// How `label` matches the token typed so far: `Some(0)` when it starts with it, `Some(1)` when a
/// part after a `_` or `.` does, and `None` when neither does.
fn match_rank(label: &str, typed: &str) -> Option<u8> {
    if label.starts_with(typed) {
        return Some(0);
    }
    label
        .match_indices(['_', '.'])
        .any(|(at, _)| label[at + 1..].starts_with(typed))
        .then_some(1)
}

/// The material tokens matching what is typed between `replace`'s start and `cursor`: the
/// abstract tokens first, then the canonical ids, each those starting with it ahead of those with
/// a part that does, at most [`MATERIAL_ITEM_LIMIT`] of them.
fn material_items(
    index: &LineIndex,
    source: &str,
    replace: &Span,
    cursor: usize,
) -> lsp_types::CompletionList {
    let typed = source[replace.start..cursor].to_ascii_lowercase();
    let mut ranked: Vec<(u8, u8, &MaterialCandidate)> = abstract_tokens()
        .iter()
        .map(|candidate| (0, candidate))
        .chain(canonical_ids().iter().map(|candidate| (1, candidate)))
        .filter_map(|(tier, candidate)| {
            match_rank(&candidate.label, &typed).map(|rank| (tier, rank, candidate))
        })
        .collect();
    // Stable, so each group keeps the vocabulary's order.
    ranked.sort_by_key(|(tier, rank, _)| (*tier, *rank));
    let is_incomplete = ranked.len() > MATERIAL_ITEM_LIMIT;
    let items = ranked
        .into_iter()
        .take(MATERIAL_ITEM_LIMIT)
        .enumerate()
        .map(|(order, (_, _, candidate))| {
            completion_item(
                index,
                source,
                replace,
                &candidate.label,
                lsp_types::CompletionItemKind::VALUE,
                candidate.detail.clone(),
                order,
            )
        })
        .collect();
    lsp_types::CompletionList {
        is_incomplete,
        items,
    }
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
        assert_eq!(list.items[0].label, "floor.wood.broadleaf");
        assert_eq!(
            list.items[0].detail.as_deref(),
            Some("minecraft:oak_planks")
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
    fn a_canonical_id_only_the_blocks_tables_declare_is_offered_with_who_declares_it() {
        let cherry = complete_material("cherry_pl");
        assert!(!cherry.is_incomplete);
        assert_eq!(labels(&cherry.items), ["cherry_planks"]);
        assert_eq!(
            cherry.items[0].detail.as_deref(),
            Some("java 1.20.4, 1.21, 1.21.4, 26.3; bedrock 1.21.0, 1.21.40, 1.21.60, 1.26.60"),
        );
        let pale = complete_material("pale_oak_pl");
        assert_eq!(labels(&pale.items), ["pale_oak_planks"]);
        assert_eq!(
            pale.items[0].detail.as_deref(),
            Some("java 1.21.4, 26.3; bedrock 1.21.60, 1.26.60"),
        );
        let bedrock_only = complete_material("stonebr");
        assert_eq!(labels(&bedrock_only.items), ["stonebrick"]);
        assert_eq!(
            bedrock_only.items[0].detail.as_deref(),
            Some("bedrock 1.21.0")
        );
        let border = complete_material("border");
        assert_eq!(labels(&border.items), ["border_block"]);
        assert_eq!(
            border.items[0].detail.as_deref(),
            Some("bedrock 1.21.0, 1.21.40, 1.21.60, 1.26.60"),
        );
    }

    #[test]
    fn abstract_tokens_rank_ahead_of_canonical_ids_and_a_leading_match_ahead_of_an_inner_one() {
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
            ],
        );
    }

    #[test]
    fn every_canonical_id_offered_is_one_a_blocks_table_declares_and_none_is_left_out() {
        let declared = declared_by_a_pack();
        for first in 'a'..='z' {
            let typed = first.to_string();
            let list = complete_material(&typed);
            for label in labels(&list.items) {
                let is_abstract = abstract_tokens().iter().any(|token| token.label == label);
                assert!(
                    is_abstract || declared.contains(label),
                    "`{label}` is declared by no table",
                );
            }
            let matched = abstract_tokens()
                .iter()
                .chain(canonical_ids())
                .filter(|candidate| match_rank(&candidate.label, &typed).is_some())
                .count();
            assert_eq!(
                list.is_incomplete,
                matched > MATERIAL_ITEM_LIMIT,
                "`@{typed}`"
            );
            assert_eq!(
                list.items.len(),
                matched.min(MATERIAL_ITEM_LIMIT),
                "`@{typed}`"
            );
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
        for (typed, incomplete) in [("", true), ("s", true), ("stone_bri", false)] {
            let list = complete_material(typed);
            assert_eq!(list.is_incomplete, incomplete, "`@{typed}`");
            assert!(list.items.len() <= MATERIAL_ITEM_LIMIT, "`@{typed}`");
        }
        let narrowed = complete_material("stone_bri");
        assert!(
            labels(&narrowed.items)
                .iter()
                .all(|label| label.starts_with("stone_bri") || label.contains("_stone_bri")),
            "{:?}",
            labels(&narrowed.items),
        );
    }

    #[test]
    fn a_material_token_is_matched_on_what_precedes_the_cursor_and_replaced_whole() {
        let source = "theme a:\n  slot floor -> @cherry_planks";
        let position = at_end_of(source, "@cherry_pl");
        let list = completions(source, position).expect("in the document");
        assert_eq!(labels(&list.items), ["cherry_planks"]);
        let line = "  slot floor -> @cherry_planks";
        let after_at = u32::try_from(line.find('@').expect("at sign") + 1).expect("fits u32");
        let end = u32::try_from(line.len()).expect("fits u32");
        assert_eq!(edit_range(&list.items[0]), range(1, after_at, end));
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
}
