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
use cairn_lang_formats::registry::MaterialsIndex;

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

/// The most items a material-token answer holds; a longer answer is cut there and marked incomplete.
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

/// One item replacing `replace` with `label`, at `order` in the answer; a client that sorts by
/// its own match score keeps that order only among items that score alike.
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
        // A client compares `sortText` as a string.
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

/// The built-in catalogs' abstract tokens, Java's first, each with what it resolves to in each
/// edition: the default id, then any version that spells it otherwise.
fn abstract_tokens() -> &'static [MaterialCandidate] {
    static TOKENS: OnceLock<Vec<MaterialCandidate>> = OnceLock::new();
    TOKENS.get_or_init(|| {
        let catalogs =
            [Edition::Java, Edition::Bedrock].map(|edition| (edition, builtin_pack(edition)));
        let mut seen = HashSet::new();
        catalogs
            .iter()
            .flat_map(|(_, pack)| pack.materials().tokens())
            .filter(|token| seen.insert(*token))
            .map(|token| MaterialCandidate {
                label: token.to_owned(),
                detail: catalogs
                    .iter()
                    .filter_map(|(edition, pack)| resolution_in(*edition, pack.materials(), token))
                    .collect::<Vec<_>>()
                    .join("; "),
            })
            .collect()
    })
}

/// What `token` resolves to in `edition`'s `catalog`, as `java minecraft:stone_bricks` or
/// `bedrock minecraft:stone_bricks (1.21.0: minecraft:stonebrick)`, or `None` without the token.
fn resolution_in(edition: Edition, catalog: &MaterialsIndex, token: &str) -> Option<String> {
    let mut default = None;
    let mut overrides = Vec::new();
    for (_, version, id) in catalog.mappings().filter(|(mapped, ..)| *mapped == token) {
        match version {
            None => default = Some(id),
            Some(version) => overrides.push(format!("{version}: {id}")),
        }
    }
    let default = default?;
    let edition = edition.as_str();
    Some(if overrides.is_empty() {
        format!("{edition} {default}")
    } else {
        format!("{edition} {default} ({})", overrides.join(", "))
    })
}

/// Every canonical id a built-in pack's blocks table declares, Java's and Bedrock's, sorted, each
/// with the editions and versions that declare it.
fn canonical_ids() -> &'static [MaterialCandidate] {
    static IDS: OnceLock<Vec<MaterialCandidate>> = OnceLock::new();
    IDS.get_or_init(|| {
        let mut declared: BTreeMap<&str, Vec<(Edition, Vec<&str>)>> = BTreeMap::new();
        for edition in [Edition::Java, Edition::Bedrock] {
            for (version, ids) in builtin_pack(edition).blocks().ids_by_version() {
                for id in ids {
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

/// How `label` matches the token typed so far: `Some(0)` when it starts with it, `Some(1)` when
/// it does from just after some `_` or `.`, `Some(2)` when it holds its characters in order, and
/// `None` otherwise.
fn match_rank(label: &str, typed: &str) -> Option<u8> {
    if label.starts_with(typed) {
        return Some(0);
    }
    if label
        .match_indices(['_', '.'])
        .any(|(at, _)| label[at + 1..].starts_with(typed))
    {
        return Some(1);
    }
    // VS Code's fuzzy matcher accepts only these, so no label it would show is left out.
    let mut rest = label.chars();
    typed
        .chars()
        .all(|wanted| rest.any(|c| c == wanted))
        .then_some(2)
}

/// The candidates matching `typed`, every one of a tier before the next tier's and, within one,
/// in [`match_rank`]'s order, cut at [`MATERIAL_ITEM_LIMIT`]; and whether the cut dropped any.
fn rank_and_cut<'c>(
    tiers: &[&'c [MaterialCandidate]],
    typed: &str,
) -> (Vec<&'c MaterialCandidate>, bool) {
    let mut ranked: Vec<(usize, u8, &MaterialCandidate)> = tiers
        .iter()
        .enumerate()
        .flat_map(|(tier, candidates)| candidates.iter().map(move |candidate| (tier, candidate)))
        .filter_map(|(tier, candidate)| {
            match_rank(&candidate.label, typed).map(|rank| (tier, rank, candidate))
        })
        .collect();
    // Stable, so each group keeps the vocabulary's order.
    ranked.sort_by_key(|(tier, rank, _)| (*tier, *rank));
    let is_incomplete = ranked.len() > MATERIAL_ITEM_LIMIT;
    ranked.truncate(MATERIAL_ITEM_LIMIT);
    let kept = ranked
        .into_iter()
        .map(|(_, _, candidate)| candidate)
        .collect();
    (kept, is_incomplete)
}

/// The answer for what is typed between `replace`'s start and `cursor`: abstract tokens before
/// canonical ids and, within each, in [`match_rank`]'s order, cut at [`MATERIAL_ITEM_LIMIT`] and
/// marked incomplete when it is.
fn material_items(
    index: &LineIndex,
    source: &str,
    replace: &Span,
    cursor: usize,
) -> lsp_types::CompletionList {
    let typed = source[replace.start..cursor].to_ascii_lowercase();
    let (kept, is_incomplete) = rank_and_cut(&[abstract_tokens(), canonical_ids()], &typed);
    let items = kept
        .into_iter()
        .enumerate()
        .map(|(order, candidate)| {
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
mod tests;
