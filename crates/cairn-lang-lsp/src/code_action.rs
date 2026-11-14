//! Quick-fixes for the diagnostics a `textDocument/codeAction` request carries back, each built
//! from the diagnostic's `data` payload and the text its range holds
//! (`spec/lint` "Machine-readable payload").

use std::collections::HashMap;

use cairn_lang_core::Span;
use cairn_lang_core::check::DiagnosticData;

use crate::line_index::LineIndex;

/// The client command that pins another target, its one argument the version.
pub const SET_TARGET: &str = "cairn.setTarget";

/// The namespace of a block id written without one (`spec/materials-themes` "Canonical vocabulary").
const VANILLA: &str = "minecraft:";

/// One quick-fix, before it is addressed to a document.
struct Fix {
    title: String,
    change: Change,
    preferred: bool,
}

/// What a [`Fix`] does.
enum Change {
    /// Replace the text at a byte range.
    Replace(Span, String),
    /// Have the client pin this version of the edition it pins.
    SetTarget(String),
}

/// The quick-fixes for `diagnostics` in the document at `uri`, whose stored text is `source`.
///
/// `runs_set_target` says whether the client runs [`SET_TARGET`]. A diagnostic whose range no
/// longer holds what its payload names gets none.
#[must_use]
pub fn code_actions(
    uri: &lsp_types::Uri,
    source: &str,
    diagnostics: &[lsp_types::Diagnostic],
    runs_set_target: bool,
) -> Vec<lsp_types::CodeActionOrCommand> {
    let index = LineIndex::new(source);
    diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.source.as_deref() == Some("cairn"))
        .flat_map(|diagnostic| {
            fixes(source, &index, diagnostic, runs_set_target)
                .into_iter()
                .map(|fix| addressed(uri, source, &index, diagnostic, fix))
        })
        .collect()
}

/// The fixes one diagnostic's payload states.
fn fixes(
    source: &str,
    index: &LineIndex,
    diagnostic: &lsp_types::Diagnostic,
    runs_set_target: bool,
) -> Vec<Fix> {
    let (Some(lsp_types::NumberOrString::String(code)), Some(data)) =
        (&diagnostic.code, &diagnostic.data)
    else {
        return Vec::new();
    };
    let (Ok(data), Some(start), Some(end)) = (
        serde_json::from_value::<DiagnosticData>(data.clone()),
        index.offset_at(source, diagnostic.range.start),
        index.offset_at(source, diagnostic.range.end),
    ) else {
        return Vec::new();
    };
    let span = start..end;
    match (code.as_str(), data) {
        (
            "E_UNKNOWN_ID",
            DiagnosticData::UnknownId {
                id,
                origin,
                suggestion,
                aliases,
                ..
            },
        ) if origin == "authored" => id_fixes(source, &span, &id, &aliases, suggestion.as_deref()),
        (
            "E_UNKNOWN_KEYWORD",
            DiagnosticData::UnknownKeyword {
                keyword,
                suggestion: Some(suggestion),
            },
        ) => keyword_fix(source, &span, &keyword, suggestion)
            .into_iter()
            .collect(),
        (
            "E_UNKNOWN_ARGUMENT",
            DiagnosticData::UnknownArgument {
                key,
                suggestion: Some(suggestion),
            },
        ) => key_fix(source, &span, &key, suggestion)
            .into_iter()
            .collect(),
        (
            "E_VERSION_CAP",
            DiagnosticData::VersionCap {
                edition,
                target,
                floor,
                valid,
            },
        ) => {
            let Some(lowered) = floor_fix(source, &span, &floor, &target) else {
                return Vec::new();
            };
            let mut fixes = vec![lowered];
            if runs_set_target && let Some(lowest) = valid.into_iter().next() {
                fixes.push(Fix {
                    title: format!("Build against {edition} {lowest}"),
                    change: Change::SetTarget(lowest),
                    preferred: false,
                });
            }
            fixes
        }
        _ => Vec::new(),
    }
}

/// An alias for an id the author wrote, the first preferred since an alias names the same block,
/// then the nearest spelling, which is a guess and so is not.
fn id_fixes(
    source: &str,
    span: &Span,
    id: &str,
    aliases: &[String],
    suggestion: Option<&str>,
) -> Vec<Fix> {
    let Some(written) = written_id(source, span, id) else {
        return Vec::new();
    };
    let text = &source[written.clone()];
    // Spelled the way the author spelled the id, with or without its namespace.
    let spelled = |replacement: &str| {
        if text.contains(':') {
            replacement
        } else {
            replacement.strip_prefix(VANILLA).unwrap_or(replacement)
        }
        .to_owned()
    };
    let alias_fixes = aliases.iter().enumerate().map(|(i, alias)| {
        let alias = spelled(alias);
        Fix {
            title: format!("Replace `{text}` with its alias `{alias}`"),
            change: Change::Replace(written.clone(), alias),
            preferred: i == 0,
        }
    });
    let suggestion_fix = suggestion.map(|suggestion| {
        let suggestion = spelled(suggestion);
        Fix {
            title: format!("Replace `{text}` with `{suggestion}`"),
            change: Change::Replace(written.clone(), suggestion),
            preferred: false,
        }
    });
    alias_fixes.chain(suggestion_fix).collect()
}

/// Where `span`, a value such as `@grass[snowy=false]`, writes `id`: after any `@` and before any
/// state literal, or `None` when it writes another.
fn written_id(source: &str, span: &Span, id: &str) -> Option<Span> {
    let value = source.get(span.clone())?;
    let start = span.start + usize::from(value.starts_with('@'));
    let body = &source[start..span.end];
    let end = start + body.find('[').unwrap_or(body.len());
    let written = &source[start..end];
    let names = if written.contains(':') {
        written == id
    } else {
        id.strip_prefix(VANILLA) == Some(written)
    };
    names.then_some(start..end)
}

/// The suggested keyword in place of the one that opens `span`.
fn keyword_fix(source: &str, span: &Span, keyword: &str, suggestion: String) -> Option<Fix> {
    let end = span.start + keyword.len();
    let whole = source.get(span.start..end) == Some(keyword)
        && !source[end..].starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_');
    whole.then(|| Fix {
        title: format!("Change `{keyword}` to `{suggestion}`"),
        change: Change::Replace(span.start..end, suggestion),
        preferred: false,
    })
}

/// The suggested key in place of the one written before `value`'s `=`; the diagnostic's range is
/// the value, since the IR keeps no span for a key.
fn key_fix(source: &str, value: &Span, key: &str, suggestion: String) -> Option<Fix> {
    let before = source.get(..value.start)?;
    let before = before
        .trim_end_matches([' ', '\t'])
        .strip_suffix('=')?
        .trim_end_matches([' ', '\t']);
    let start = before.strip_suffix(key)?.len();
    if before[..start].ends_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
        return None;
    }
    Some(Fix {
        title: format!("Change `{key}=` to `{suggestion}=`"),
        change: Change::Replace(start..start + key.len(), suggestion),
        preferred: false,
    })
}

/// `floor`, as the payload renders it, lowered to `target` at the version its line ends with.
fn floor_fix(source: &str, span: &Span, floor: &str, target: &str) -> Option<Fix> {
    let (scope, version) = floor.rsplit_once(">=")?;
    let start = span.end.checked_sub(version.len())?;
    (source.get(start..span.end) == Some(version)).then(|| Fix {
        title: format!("Lower the floor to `{scope}>={target}`"),
        change: Change::Replace(start..span.end, target.to_owned()),
        preferred: false,
    })
}

/// `fix` as the quick-fix the protocol carries, for `diagnostic` in the document at `uri`.
#[expect(
    clippy::mutable_key_type,
    reason = "`WorkspaceEdit::changes` is keyed by `Uri`, which caches its authority in a `Cell`"
)]
fn addressed(
    uri: &lsp_types::Uri,
    source: &str,
    index: &LineIndex,
    diagnostic: &lsp_types::Diagnostic,
    fix: Fix,
) -> lsp_types::CodeActionOrCommand {
    let (edit, command) = match fix.change {
        Change::Replace(span, new_text) => {
            let edit = lsp_types::TextEdit {
                range: index.range(source, &span),
                new_text,
            };
            let changes = HashMap::from([(uri.clone(), vec![edit])]);
            let edit = lsp_types::WorkspaceEdit {
                changes: Some(changes),
                ..lsp_types::WorkspaceEdit::default()
            };
            (Some(edit), None)
        }
        Change::SetTarget(version) => {
            let command = lsp_types::Command {
                title: fix.title.clone(),
                command: SET_TARGET.to_owned(),
                arguments: Some(vec![serde_json::Value::String(version)]),
            };
            (None, Some(command))
        }
    };
    lsp_types::CodeActionOrCommand::CodeAction(lsp_types::CodeAction {
        title: fix.title,
        kind: Some(lsp_types::CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diagnostic.clone()]),
        edit,
        command,
        is_preferred: fix.preferred.then_some(true),
        ..lsp_types::CodeAction::default()
    })
}

#[cfg(test)]
mod tests;
