//! `duplicate` pass: flags a `key=`, `slot`, `id=` or item name repeated in
//! one scope, a `theme` selector row repeated as a whole, and a second
//! `circuit` line in a `struct` / `def` body.
//!
//! It walks the surface AST because the IR's
//! [`IntentState`](crate::intent::IntentState) and `args` maps are
//! last-write-wins, so only the surface still holds both occurrences. The
//! exception is [`check_theme_selectors`], whose rows the IR keeps intact.
//!
//! Scopes and codes:
//! - `E_DUPLICATE_HEADER`: a single-valued `@directive` repeated in the
//!   module; `@requires` is exempt, see [`check_headers`].
//! - `E_DUPLICATE_ITEM`: two top-level items of the same kind share a name;
//!   each kind is its own namespace.
//! - `E_DUPLICATE_SIZE`: a `struct` / `def` header with two `size=`.
//! - `E_DUPLICATE_SLOT`: two `slot NAME ->` lines in a `theme` body.
//! - `E_DUPLICATE_SELECTOR`: two theme rows that select the same members
//!   and bind the same key.
//! - `E_DUPLICATE_ARG`: any other `key=` repeated in one argument list.
//! - `E_DUPLICATE_ID`: two members of one immediate body declare the same
//!   `id=`. A nested body is its own namespace, and a repeat between two
//!   top-level `place` rows of a `site` is the resolver's; see
//!   [`PlaceIdOwner`].
//! - `E_DUPLICATE_CIRCUIT`: two `circuit` lines among a `struct` / `def`
//!   body's own members.
//!
//! Each finding sits on the repeat, the token the author would edit, with a
//! note on the first declaration. `E_DUPLICATE_SELECTOR` notes the row the
//! repeat displaces instead; see [`check_theme_selectors`].

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
    // `lower` keeps one `ThemeIr` per `theme` block, including blocks whose
    // name lost the binding, so this walks the same bodies as the loop above.
    for theme in &ir.themes {
        check_theme_selectors(theme, sink);
    }
}

/// Module-header scope: a single-valued `@directive` (`@cairn`,
/// `@intended_targets`) may be declared once, because no rule says which
/// of two answers holds.
///
/// `@requires` is exempt because its floors compose: `resolve::version_axes`
/// folds them to the strictest (see `RegistryRange`), so nothing is
/// discarded.
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

/// Top-level name scope, one namespace per [`ItemKind`], matching the
/// resolver's `struct::`, `def::` and `site::NAME::` scope keys and its
/// separate theme map.
fn check_item_names(items: &[Item], sink: &mut DiagnosticSink) {
    let mut seen: IndexMap<(ItemKind, &str), Span> = IndexMap::new();
    for item in items {
        let kind = item.kind();
        let keyword = kind.keyword();
        let name = item.name();
        // The name token, not the item span, which covers the whole body.
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

/// What the author has to do about a repeated name.
///
/// A second `theme`, `def` or `struct` binds nothing, but two `site` blocks
/// share the `site::NAME::` namespace and all their places build, so "the
/// first one resolves" would invite deleting a block that is in the build.
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

/// One equivalence class of selector rows: the row that opened it, and
/// the row that most recently bound each key.
struct SelectorGroup<'a> {
    /// Class representative, compared against every later row.
    /// [`select_the_same_members`] is an equivalence relation, so agreeing
    /// with the representative is agreeing with the whole class.
    representative: &'a SelectorRule,
    /// Binding key -> span of the row that bound it last. Its order is
    /// never read.
    bound: IndexMap<&'a str, &'a Span>,
}

/// Theme-selector scope: two rows that select the same members and bind the
/// same key, where only the later row's value is read.
///
/// Read off the IR, where `ThemeIr::selectors` keeps every row in source
/// order, so the matcher's own rule ([`select_the_same_members`]) decides
/// which rows select the same members. A key repeated inside one row is
/// [`check_arg_keys`]'s.
///
/// The note points at the most recent row to bind the key, not the first:
/// a binding can be displaced more than once, and the first row's value is
/// already gone by the third.
///
/// Unlike `resolve`'s `check_unmatched_selectors`, a theme no scope applies
/// is checked too, so the finding does not surface later on the unrelated
/// edit that binds the theme.
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
    // One note per displaced row, so two keys from one row name it once.
    let mut per_row: IndexMap<&Span, Vec<String>> = IndexMap::new();
    for (key, span) in rebound {
        per_row.entry(span).or_default().push(format!("`{key}=`"));
    }
    // Source order: insertion follows this row's binding order, which can
    // put a later row's note first.
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
            "rows with the same attributes match exactly the same members, and bindings merge in source order, so every member's merged bindings keep this row's {listed} and the earlier value is lost (no pass lowers a selector's bindings yet, so neither value is built today)",
        ),
    });
    notes.push(DiagnosticNote {
        span: None,
        message: "merge the rows, or narrow one selector so they pick different members".into(),
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

/// Which arg list a repeated key sits in, which decides how it is reported.
#[derive(Clone, Copy)]
enum ArgScope {
    /// A `struct` / `def` header: repeated `size=` is `E_DUPLICATE_SIZE`,
    /// any other repeated key `E_DUPLICATE_ARG`.
    Header,
    /// Any other arg list: every repeated key is `E_DUPLICATE_ARG`.
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

/// The note every duplicate in this pass carries, pointing at the
/// occurrence that stands.
fn first_declaration_note(first_span: &Span) -> DiagnosticNote {
    DiagnosticNote {
        span: Some(first_span.clone()),
        message: "first declaration here".into(),
    }
}

/// Scope reservation: a `struct` or `def` body's own members may hold one
/// `circuit` line.
///
/// Every `circuit` member counts, well-formed or not and with or without a
/// `[...]` selector: each states the reservation, and place-and-route uses
/// only the first of a scope's [`crate::circuit_regions`]. A `circuit` line
/// under a `level` is in that level's body.
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

/// Who reports two `place` rows of one body that share an `id=`: the
/// resolver, or this pass.
///
/// The resolver does for the body its placement loop walks, a `site`'s
/// top-level `SiteIr::placements`: `usable_place_id` reports the repeat as
/// `E_DUPLICATE_PLACE_ID`, the code `spec/lint` "Sites and placements"
/// documents first-row-wins for, so `E_DUPLICATE_ID` here would bill one
/// repair twice. It compares refused ids with refused ones and accepted with
/// accepted, and acceptance depends on the id alone, so two rows sharing an
/// id are always compared.
///
/// It reads the id off `Member::id`, which `hoist_label` fills from the
/// first identifier or string `id=`, the same argument [`id_declaration`]
/// takes. The two must agree, or a repeat falls between this pass and the
/// resolver.
///
/// Only `place` rows are the resolver's: its loop ignores a `connect` row's
/// `id=` and skips a misplaced row whole. [`check_body`] stays quiet only
/// when both rows are the resolver's, so after `connect id=a`, `place
/// id=a`, `place id=a` the third row carries both `E_DUPLICATE_ID` and
/// `E_DUPLICATE_PLACE_ID`, with notes on different rows.
///
/// The predicate is the exact body, not [`crate::intent::BodyKind`], which
/// does not change with depth: a `place` row indented under any `site` row
/// stays in its parent's `Member::children` and never reaches the
/// placement loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PlaceIdOwner {
    /// The top-level body of a `site`, the list the placement loop walks.
    Resolver,
    /// A `struct` or `def` body, or any body indented under a row. No
    /// `place` row in it reaches the resolver.
    ThisPass,
}

/// The first row of a body to declare an `id=`, as [`check_body`] records
/// it.
struct FirstId {
    /// The `id=` argument, which the note on a repeat points at.
    span: Span,
    /// Whether the resolver compares this row's id; see [`PlaceIdOwner`].
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
            // A nested body is its own scope, and none of its `place` rows
            // reach the resolver.
            check_body(children, PlaceIdOwner::ThisPass, sink);
        }
    }
}

/// Pull an `id=label` *declaration* out of a generic statement.
///
/// Only the argument list (`door id=front`) declares an id; the selector
/// form (`door[id=front]`) references an existing member
/// (`Member::selector`). An `id=` whose value is not a label is left to
/// the type-mismatch pass.
fn extract_id(stmt: &Statement) -> Option<(String, Span)> {
    let Statement::Generic { args, .. } = stmt else {
        return None;
    };
    args.iter().find_map(id_declaration)
}

/// The `(label, span)` an `id=` arg declares, or `None` for any other arg.
fn id_declaration(arg: &Arg) -> Option<(String, Span)> {
    if arg.key != "id" {
        return None;
    }
    match &arg.value.kind {
        ValueKind::Ident(s) | ValueKind::Str(s) => Some((s.clone(), arg.span.clone())),
        _ => None,
    }
}
