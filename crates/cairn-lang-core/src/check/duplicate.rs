//! The `duplicate` pass: a `key=`, `slot`, `id=`, item name, single-valued header or `circuit`
//! line repeated in one scope, and a theme selector row that rebinds a key. It walks the surface
//! AST, since the IR's maps keep only the last write.

use indexmap::IndexMap;

use crate::ast::{Arg, Header, Item, ItemKind, Module, Statement, ThemeRule, ValueKind};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::error::Span;
use crate::intent::{IntentModule, MemberRole, SelectorRule, ThemeIr, role_of};
use crate::prose::{and_list, selector_text};
use crate::resolve::select_the_same_members;

use super::DiagnosticSink;

pub(super) fn run(module: &Module, ir: &IntentModule, sink: &mut DiagnosticSink) {
    check_headers(&module.headers, sink);
    check_item_names(&module.items, sink);
    for item in &module.items {
        match item {
            Item::Theme { body, .. } => check_theme_body(body, sink),
            Item::Def { args, body, .. } | Item::Struct { args, body, .. } => {
                check_arg_keys(args, ArgScope::Header, sink);
                check_circuit_lines(body, sink);
                check_body(body, PlaceIdOwner::ThisPass, sink);
            }
            Item::Site { body, .. } => check_body(body, PlaceIdOwner::Resolver, sink),
        }
    }
    for theme in &ir.themes {
        check_theme_selectors(theme, sink);
    }
}

/// `@requires` is exempt: its floors compose to the strictest, so nothing is discarded.
fn check_headers(headers: &[Header], sink: &mut DiagnosticSink) {
    let mut seen: IndexMap<&'static str, Span> = IndexMap::new();
    for header in headers {
        let directive = match header {
            Header::Cairn { .. } => "@cairn",
            Header::IntendedTargets { .. } => "@intended_targets",
            Header::Requires { .. } => continue,
        };
        let span = header.span().clone();
        if let Some(first_span) = seen.get(directive) {
            sink.push(Diagnostic {
                code: DiagnosticCode::DuplicateHeader,
                span,
                primary: format!("`{directive}` is declared more than once"),
                notes: vec![
                    first_declaration_note(first_span),
                    DiagnosticNote {
                        span: None,
                        message: format!(
                            "`{directive}` is single-valued: keep the line that states what you mean and delete the other",
                        ),
                    },
                ],
                data: None,
            });
        } else {
            seen.insert(directive, span);
        }
    }
}

fn check_item_names(items: &[Item], sink: &mut DiagnosticSink) {
    let mut seen: IndexMap<(ItemKind, &str), Span> = IndexMap::new();
    for item in items {
        let kind = item.kind();
        let keyword = kind.keyword();
        let name = item.name();
        let span = item.name_span().clone();
        if let Some(first_span) = seen.get(&(kind, name)) {
            sink.push(Diagnostic {
                code: DiagnosticCode::DuplicateItem,
                span,
                primary: format!("`{keyword} {name}` is declared more than once"),
                notes: vec![
                    first_declaration_note(first_span),
                    DiagnosticNote {
                        span: None,
                        message: repair_advice(kind, name),
                    },
                ],
                data: None,
            });
        } else {
            seen.insert((kind, name), span);
        }
    }
}

fn repair_advice(kind: ItemKind, name: &str) -> String {
    let keyword = kind.keyword();
    match kind {
        ItemKind::Site => format!(
            "two `{keyword} {name}` blocks share one `site::{name}::` namespace: places with distinct `id=` all build, a repeated `id=` keeps only the first, and `east_of=` cannot reach across the blocks — merge the bodies into one block, or give this one its own name"
        ),
        ItemKind::Theme | ItemKind::Def | ItemKind::Struct => format!(
            "the first `{keyword} {name}` is the one that resolves; rename this one, or merge the two bodies"
        ),
    }
}

fn check_theme_body(body: &[ThemeRule], sink: &mut DiagnosticSink) {
    let mut seen: IndexMap<String, Span> = IndexMap::new();
    for rule in body {
        match rule {
            ThemeRule::Slot { slot, span, .. } => {
                if let Some(first_span) = seen.get(slot) {
                    sink.push(Diagnostic {
                        code: DiagnosticCode::DuplicateSlot,
                        span: span.clone(),
                        primary: format!("`slot {slot}` is declared more than once"),
                        notes: vec![first_declaration_note(first_span)],
                        data: None,
                    });
                } else {
                    seen.insert(slot.clone(), span.clone());
                }
            }
            ThemeRule::Selector {
                attrs, bindings, ..
            } => {
                check_arg_keys(attrs, ArgScope::List, sink);
                check_arg_keys(bindings, ArgScope::List, sink);
            }
        }
    }
}

/// A class of selector rows that select the same members.
struct SelectorGroup<'a> {
    representative: &'a SelectorRule,
    /// Each key's most recent binding row.
    bound: IndexMap<&'a str, &'a Span>,
}

/// Two rows that select the same members and bind the same key. The note names the most recent
/// earlier row, since a binding can be displaced more than once.
fn check_theme_selectors(theme: &ThemeIr, sink: &mut DiagnosticSink) {
    let mut groups: Vec<SelectorGroup<'_>> = Vec::new();
    for rule in &theme.selectors {
        let existing = groups
            .iter()
            .position(|group| select_the_same_members(group.representative, rule));
        let index = if let Some(index) = existing {
            index
        } else {
            groups.push(SelectorGroup {
                representative: rule,
                bound: IndexMap::new(),
            });
            groups.len() - 1
        };
        let group = &mut groups[index];
        let rebound: Vec<(&str, &Span)> = rule
            .bindings
            .keys()
            .filter_map(|key| {
                group
                    .bound
                    .get(key.as_str())
                    .map(|span| (key.as_str(), *span))
            })
            .collect();
        if !rebound.is_empty() {
            sink.push(duplicate_selector_diag(rule, &theme.name, &rebound));
        }
        for key in rule.bindings.keys() {
            group.bound.insert(key.as_str(), &rule.span);
        }
    }
}

fn duplicate_selector_diag(
    rule: &SelectorRule,
    theme_name: &str,
    rebound: &[(&str, &Span)],
) -> Diagnostic {
    let quoted: Vec<String> = rebound.iter().map(|(key, _)| format!("`{key}=`")).collect();
    let listed = and_list(&quoted).expect("built only for a non-empty rebound set");
    let mut per_row: IndexMap<&Span, Vec<String>> = IndexMap::new();
    for (key, span) in rebound {
        per_row.entry(span).or_default().push(format!("`{key}=`"));
    }
    // Insertion follows this row's binding order, not the rows' source order.
    per_row.sort_unstable_by(|a, _, b, _| a.start.cmp(&b.start));
    let mut notes: Vec<DiagnosticNote> = per_row
        .into_iter()
        .map(|(span, keys)| DiagnosticNote {
            span: Some(span.clone()),
            message: format!(
                "{} bound here",
                and_list(&keys).expect("each row contributed at least one key"),
            ),
        })
        .collect();
    notes.push(DiagnosticNote {
        span: None,
        message: format!(
            "rows with the same attributes match exactly the same members, and bindings merge in source order, so every member's merged bindings keep this row's {listed} and the earlier value is dead text whatever a binding comes to mean (no pass lowers a selector's bindings yet, so neither value is built today)",
        ),
    });
    notes.push(DiagnosticNote {
        span: None,
        message: "delete the value you do not mean from one of the rows, or narrow one selector so they pick different members".into(),
    });
    Diagnostic {
        code: DiagnosticCode::DuplicateSelector,
        span: rule.span.clone(),
        primary: format!(
            "`{selector}` in theme `{theme_name}` selects the same members as an earlier row and rebinds {listed}",
            selector = selector_text(&rule.keyword, &rule.attrs),
        ),
        notes,
        data: Some(DiagnosticData::DuplicateSelector {
            rebound: rebound.iter().map(|(key, _)| (*key).to_owned()).collect(),
        }),
    }
}

#[derive(Clone, Copy)]
enum ArgScope {
    Header,
    List,
}

fn check_arg_keys(args: &[Arg], scope: ArgScope, sink: &mut DiagnosticSink) {
    let mut seen: IndexMap<String, Span> = IndexMap::new();
    for arg in args {
        let Some(first_span) = seen.get(&arg.key) else {
            seen.insert(arg.key.clone(), arg.span.clone());
            continue;
        };
        let (code, primary) = match scope {
            ArgScope::Header => (
                if arg.key == "size" {
                    DiagnosticCode::DuplicateSize
                } else {
                    DiagnosticCode::DuplicateArg
                },
                format!("`{}=` is declared more than once in this header", arg.key),
            ),
            ArgScope::List => (
                DiagnosticCode::DuplicateArg,
                format!("`{}=` is declared more than once", arg.key),
            ),
        };
        sink.push(Diagnostic {
            code,
            span: arg.span.clone(),
            primary,
            notes: vec![first_declaration_note(first_span)],
            data: None,
        });
    }
}

fn first_declaration_note(first_span: &Span) -> DiagnosticNote {
    DiagnosticNote {
        span: Some(first_span.clone()),
        message: "first declaration here".into(),
    }
}

/// A `struct` / `def` body's own members hold at most one `circuit` line, malformed ones counted.
fn check_circuit_lines(body: &[Statement], sink: &mut DiagnosticSink) {
    let mut first: Option<&Span> = None;
    for stmt in body {
        let Statement::Generic { keyword, span, .. } = stmt else {
            continue;
        };
        if keyword != "circuit" {
            continue;
        }
        match first {
            Some(first_span) => sink.push(Diagnostic {
                code: DiagnosticCode::DuplicateCircuit,
                span: span.clone(),
                primary: "this scope already has a `circuit` line, and a scope reserves one region for its redstone".into(),
                notes: vec![
                    first_declaration_note(first_span),
                    DiagnosticNote {
                        span: None,
                        message: "delete one of the two lines, or split the logic across several scopes, each with its own `circuit` line".into(),
                    },
                ],
                data: None,
            }),
            None => first = Some(span),
        }
    }
}

/// Who reports two `place` rows of one body that share an `id=`: the resolver, as
/// `E_DUPLICATE_PLACE_ID`, or this pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaceIdOwner {
    /// The top-level body of a `site`, the list the resolver's placement loop walks.
    Resolver,
    /// A `struct` or `def` body, or any body indented under a row.
    ThisPass,
}

struct FirstId {
    span: Span,
    owned_by_resolver: bool,
}

fn check_body(body: &[Statement], place_ids: PlaceIdOwner, sink: &mut DiagnosticSink) {
    let mut seen_ids: IndexMap<String, FirstId> = IndexMap::new();
    for stmt in body {
        if let Statement::Generic {
            keyword,
            args,
            selector,
            children,
            ..
        } = stmt
        {
            check_arg_keys(args, ArgScope::List, sink);
            if let Some(attrs) = selector {
                check_arg_keys(attrs, ArgScope::List, sink);
            }
            if let Some((id, id_span)) = extract_id(stmt) {
                let owned_by_resolver = place_ids == PlaceIdOwner::Resolver
                    && matches!(role_of(keyword), MemberRole::Place);
                if let Some(first) = seen_ids.get(&id) {
                    if !(owned_by_resolver && first.owned_by_resolver) {
                        sink.push(Diagnostic {
                            code: DiagnosticCode::DuplicateId,
                            span: id_span,
                            primary: format!("`id={id}` is declared more than once in this scope"),
                            notes: vec![first_declaration_note(&first.span)],
                            data: None,
                        });
                    }
                } else {
                    seen_ids.insert(
                        id,
                        FirstId {
                            span: id_span,
                            owned_by_resolver,
                        },
                    );
                }
            }
            check_body(children, PlaceIdOwner::ThisPass, sink);
        }
    }
}

/// The `id=` an argument list declares; a `[id=...]` selector references one instead.
fn extract_id(stmt: &Statement) -> Option<(String, Span)> {
    let Statement::Generic { args, .. } = stmt else {
        return None;
    };
    args.iter().find_map(id_declaration)
}

/// Must take the same `id=` as `hoist_label`, or a repeat falls between this pass and the resolver.
fn id_declaration(arg: &Arg) -> Option<(String, Span)> {
    if arg.key != "id" {
        return None;
    }
    match &arg.value.kind {
        ValueKind::Ident(s) | ValueKind::Str(s) => Some((s.clone(), arg.span.clone())),
        _ => None,
    }
}
