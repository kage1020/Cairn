//! Source text + cursor position → what is under the cursor: a keyword's or a `key=`'s entry in
//! `spec/syntax` "Keyword reference", the block a material token names on each edition, or a
//! member's `intent_state` and `resolved_state`.

use cairn_lang_core::ast::{Arg, Item, Module, Statement, ThemeRule, Value, ValueKind};
use cairn_lang_core::block_array::resolve_block_state;
use cairn_lang_core::intent::ValueWithSpan;
use cairn_lang_core::resolve::{TokenKind, classify_token};
use cairn_lang_core::{
    BlockState, Edition, IntentModule, Member, Span, lower, lower_to_block_array, parse, resolve,
};
use cairn_lang_docs::Part;
use cairn_lang_formats::builtin_pack;

use crate::line_index::LineIndex;

/// The site the reference's links are relative to.
const SITE: &str = "https://cairn.kage1020.com";

/// The page that holds the keyword reference.
const REFERENCE: &str = "spec/syntax";

/// The editions a hover answers for, in the order it lists them.
const EDITIONS: [Edition; 2] = [Edition::Java, Edition::Bedrock];

/// What hovering `position` shows, or `None` when nothing there has a description, the document
/// does not parse, or it has no such position.
#[must_use]
pub fn hover(source: &str, position: lsp_types::Position) -> Option<lsp_types::Hover> {
    let index = LineIndex::new(source);
    let offset = index.offset_at(source, position)?;
    let module = parse(source).ok()?;
    let target = target_at(&module, source, offset)?;
    let value = describe(&target, &module, source)?;
    Some(lsp_types::Hover {
        contents: lsp_types::HoverContents::Markup(lsp_types::MarkupContent {
            kind: lsp_types::MarkupKind::Markdown,
            value,
        }),
        range: Some(index.range(source, target.span())),
    })
}

/// A word the cursor can rest on, with the span it covers.
enum Target<'a> {
    /// The keyword opening a top-level item or a theme's selector row.
    Keyword { keyword: &'a str, span: Span },
    /// A member's line, anywhere a key or a material token does not cover.
    Member {
        keyword: &'a str,
        line: Span,
        span: Span,
    },
    /// The key of a `key=value` on a member's line.
    Argument {
        keyword: &'a str,
        key: &'a str,
        span: Span,
    },
    /// A `@TOKEN` value, and what binds it.
    Token {
        value: &'a Value,
        binder: Binder<'a>,
    },
}

impl Target<'_> {
    fn span(&self) -> &Span {
        match self {
            Self::Keyword { span, .. }
            | Self::Member { span, .. }
            | Self::Argument { span, .. } => span,
            Self::Token { value, .. } => &value.span,
        }
    }
}

/// What binds a material token.
enum Binder<'a> {
    /// A theme's `slot NAME -> @TOKEN` row.
    Slot { theme: &'a str, slot: &'a str },
    /// A `key=@TOKEN` binding on a theme's selector row, written as `row`.
    Selector {
        theme: &'a str,
        row: &'a str,
        key: &'a str,
    },
    /// A `connect` row's `path=`.
    Path,
}

fn target_at<'a>(module: &'a Module, source: &'a str, offset: usize) -> Option<Target<'a>> {
    for item in &module.items {
        let keyword = item.kind().keyword();
        if let Some(span) = keyword_span(source, item.span().start, keyword)
            && span.contains(&offset)
        {
            return Some(Target::Keyword { keyword, span });
        }
        let found = match item {
            Item::Theme { name, body, .. } => in_theme(name, body, source, offset),
            Item::Def { body, .. } | Item::Struct { body, .. } | Item::Site { body, .. } => {
                in_statements(body, source, offset)
            }
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

fn in_theme<'a>(
    theme: &'a str,
    rules: &'a [ThemeRule],
    source: &'a str,
    offset: usize,
) -> Option<Target<'a>> {
    for rule in rules {
        match rule {
            ThemeRule::Slot { slot, value, .. } if value.span.contains(&offset) => {
                return is_token(value).then_some(Target::Token {
                    value,
                    binder: Binder::Slot { theme, slot },
                });
            }
            ThemeRule::Selector {
                keyword,
                bindings,
                span,
                ..
            } if span.contains(&offset) => {
                if let Some(span) = keyword_span(source, span.start, keyword)
                    && span.contains(&offset)
                {
                    return Some(Target::Keyword { keyword, span });
                }
                let row = source[span.clone()]
                    .split("->")
                    .next()
                    .unwrap_or_default()
                    .trim();
                return bindings
                    .iter()
                    .find(|arg| is_token(&arg.value) && arg.value.span.contains(&offset))
                    .map(|arg| Target::Token {
                        value: &arg.value,
                        binder: Binder::Selector {
                            theme,
                            row,
                            key: &arg.key,
                        },
                    });
            }
            _ => {}
        }
    }
    None
}

fn in_statements<'a>(
    statements: &'a [Statement],
    source: &str,
    offset: usize,
) -> Option<Target<'a>> {
    for statement in statements {
        let found = match statement {
            Statement::Generic {
                keyword,
                args,
                span,
                ..
            } if span.contains(&offset) => {
                Some(on_member_line(keyword, args, span, source, offset))
            }
            Statement::Generic { children, .. } | Statement::EditionGuard { children, .. } => {
                in_statements(children, source, offset)
            }
            _ => None,
        };
        if found.is_some() {
            return found;
        }
    }
    None
}

fn on_member_line<'a>(
    keyword: &'a str,
    args: &'a [Arg],
    line: &Span,
    source: &str,
    offset: usize,
) -> Target<'a> {
    for arg in args {
        let key = arg.span.start..arg.span.start + arg.key.len();
        if key.contains(&offset) {
            return Target::Argument {
                keyword,
                key: &arg.key,
                span: key,
            };
        }
        if keyword == "connect"
            && arg.key == "path"
            && is_token(&arg.value)
            && arg.value.span.contains(&offset)
        {
            return Target::Token {
                value: &arg.value,
                binder: Binder::Path,
            };
        }
    }
    let span = keyword_span(source, line.start, keyword)
        .filter(|span| span.contains(&offset))
        .unwrap_or_else(|| line.clone());
    Target::Member {
        keyword,
        line: line.clone(),
        span,
    }
}

/// The span of `keyword` at `start`, if the source spells it there.
fn keyword_span(source: &str, start: usize, keyword: &str) -> Option<Span> {
    let span = start..start + keyword.len();
    (source.get(span.clone()) == Some(keyword)).then_some(span)
}

fn is_token(value: &Value) -> bool {
    classify_token(value) != TokenKind::NotAToken
}

fn describe(target: &Target<'_>, module: &Module, source: &str) -> Option<String> {
    match target {
        Target::Keyword { keyword, .. } => entry(keyword, None).map(own_text),
        Target::Argument { keyword, key, .. } => {
            let name = format!("{key}=");
            if let Some(owner) = entry(keyword, None)
                && let Some(argument) = entry(&name, Some(owner))
            {
                return Some(format!("{}\n\n{}", own_text(argument), own_text(owner)));
            }
            entry(&name, None).map(own_text)
        }
        Target::Member { keyword, line, .. } => {
            let own = own_text(entry(keyword, None)?);
            let ir = lower(module);
            let member = member_at(&ir, line.start)?;
            Some(format!(
                "{own}\n\n---\n\n{}",
                member_state(&ir, member, source)
            ))
        }
        Target::Token { value, binder } => Some(describe_token(value, binder)),
    }
}

/// Every entry the keyword reference gives `name`, a keyword or a `key=`, in page order.
fn entries(name: &str) -> Vec<&'static str> {
    let Some(page) = cairn_lang_docs::page(REFERENCE) else {
        return Vec::new();
    };
    page.cited(&format!("`{name}`"))
        .into_iter()
        .filter_map(|part| match part {
            Part::Term { markdown, .. } => Some(markdown),
            Part::Page | Part::Section(_) => None,
        })
        .collect()
}

/// The keyword reference's entry for `name`, the one nested in the entry `within` when given and
/// one nested in none otherwise.
fn entry(name: &str, within: Option<&'static str>) -> Option<&'static str> {
    let mut entries = entries(name).into_iter();
    match within {
        Some(outer) => entries.find(|inner| nests_in(inner, outer)),
        None => entries.find(|entry| !entry.starts_with(char::is_whitespace)),
    }
}

/// Whether `inner` lies inside `outer`; both slice the page's one Markdown, so that is nesting.
fn nests_in(inner: &str, outer: &str) -> bool {
    let inner = inner.as_bytes().as_ptr_range();
    let outer = outer.as_bytes().as_ptr_range();
    outer.start <= inner.start && inner.end <= outer.end
}

/// An entry's own lines, without the entries nested under it, as one paragraph whose links reach
/// the site.
fn own_text(entry: &str) -> String {
    let mut lines = entry.lines().map(str::trim_start);
    let first = lines.next().unwrap_or_default();
    let mut text = first.strip_prefix("- ").unwrap_or(first).to_owned();
    for line in lines.take_while(|line| !line.is_empty() && !line.starts_with("- ")) {
        text.push(' ');
        text.push_str(line);
    }
    text.replace("](/", &format!("]({SITE}/"))
        .replace("](#", &format!("]({SITE}/{REFERENCE}/#"))
}

/// The member whose own line starts at `start`, in any body.
fn member_at(ir: &IntentModule, start: usize) -> Option<&Member> {
    fn find(members: &[Member], start: usize) -> Option<&Member> {
        members.iter().find_map(|member| {
            if member.span.start == start {
                Some(member)
            } else {
                find(&member.children.members, start)
            }
        })
    }
    ir.structs
        .iter()
        .find_map(|s| find(&s.members, start))
        .or_else(|| ir.defs.iter().find_map(|d| find(&d.members, start)))
        .or_else(|| ir.sites.iter().find_map(|s| find(&s.placements, start)))
}

/// A member's `id=`, its `intent_state`, and its `resolved_state` on each edition.
fn member_state(ir: &IntentModule, member: &Member, source: &str) -> String {
    let mut lines = Vec::new();
    if let Some(id) = &member.id {
        lines.push(format!("`id={id}`"));
    }
    let written: Vec<String> = member
        .intent_state
        .iter()
        .map(|(key, value)| format!("`{key}={}`", &source[value.span.clone()]))
        .collect();
    lines.push(if written.is_empty() {
        "**`intent_state`**: nothing written".to_owned()
    } else {
        format!("**`intent_state`**: {}", written.join(", "))
    });
    for edition in EDITIONS {
        let resolution = resolve(ir, Some(edition));
        let registry = builtin_pack(edition).view(None);
        let built = lower_to_block_array(ir, &resolution, Some(&registry));
        let mut heading = format!(
            "**`resolved_state`** on {edition}, as `cairn lower --edition {edition}` resolves it:",
            edition = edition.as_str(),
        );
        let scopes: Vec<String> = built
            .members
            .iter()
            .flat_map(|(scope, members)| {
                members
                    .iter()
                    .filter(|resolved| resolved.span.start == member.span.start)
                    .map(move |resolved| {
                        let blocks: Vec<String> = resolved
                            .resolved_state
                            .iter()
                            .map(|block| {
                                let cells: u32 = block.runs.iter().map(|run| run.len).sum();
                                let unit = if cells == 1 { "cell" } else { "cells" };
                                format!("`{}` in {cells} {unit}", spelled(&block.state))
                            })
                            .collect();
                        format!("- `{scope}`: {}", blocks.join(", "))
                    })
            })
            .collect();
        if scopes.is_empty() {
            heading.push_str(" no cell");
            lines.push(heading);
        } else {
            lines.push(format!("{heading}\n{}", scopes.join("\n")));
        }
    }
    lines.join("\n\n")
}

/// `state` as a block id with its state literal.
fn spelled(state: &BlockState) -> String {
    if state.properties.is_empty() {
        state.id.clone()
    } else {
        format!("{}[{}]", state.id, state.properties_text())
    }
}

fn describe_token(value: &Value, binder: &Binder<'_>) -> String {
    let ValueKind::Token(inner) = &value.kind else {
        unreachable!("a target's token is a `ValueKind::Token`");
    };
    let bound = match binder {
        Binder::Slot { theme, slot } => format!("bound to slot `{slot}` by theme `{theme}`"),
        Binder::Selector { theme, row, key } => {
            format!("bound to `{key}=` by the `{row}` row of theme `{theme}`")
        }
        Binder::Path => "the `path=` of this `connect` row".to_owned(),
    };
    if matches!(binder, Binder::Selector { .. }) {
        return format!(
            "`@{inner}`, {bound}\n\nSelector bindings are reserved, so no block is resolved for \
             this value. See [`spec/materials-themes` \"Slots as dependency injection\"]\
             ({SITE}/spec/materials-themes/#71-slots-as-dependency-injection)."
        );
    }
    let editions: Vec<String> = EDITIONS
        .into_iter()
        .map(|edition| {
            let resolved = resolved_on(value, inner, edition);
            format!("- {}: {resolved}", edition.as_str())
        })
        .collect();
    format!("`@{inner}`, {bound}\n\n{}", editions.join("\n"))
}

/// The block `@inner` names on `edition`, with no version pinned, and the versions it differs on.
fn resolved_on(value: &Value, inner: &str, edition: Edition) -> String {
    let pack = builtin_pack(edition);
    if classify_token(value) == TokenKind::Abstract {
        let materials = pack.materials();
        let Some(default) = materials.lookup_id(inner) else {
            return format!("the materials catalog has no `@{inner}`");
        };
        let overrides: String = materials
            .mappings()
            .filter(|(token, _, _)| *token == inner)
            .filter_map(|(_, version, id)| Some(format!(", `{id}` at {}", version?)))
            .collect();
        return format!("`{default}`{overrides}");
    }
    let Ok(state) = resolve_block_state(&ValueWithSpan::from_value(value.clone()), None) else {
        unreachable!("a canonical token resolves without a registry");
    };
    let blocks = pack.blocks();
    let versions: Vec<&str> = blocks.versions().collect();
    let declaring: Vec<&str> = versions
        .iter()
        .copied()
        .filter(|version| {
            blocks.ids_for(version).is_some_and(|ids| {
                ids.binary_search_by(|id| id.as_str().cmp(&state.id))
                    .is_ok()
            })
        })
        .collect();
    let written = spelled(&state);
    if declaring.len() == versions.len() {
        format!("`{written}`")
    } else if declaring.is_empty() {
        format!(
            "`{written}`, which no {} version declares",
            edition.as_str()
        )
    } else {
        format!("`{written}`, declared by {}", declaring.join(", "))
    }
}

#[cfg(test)]
mod tests;
