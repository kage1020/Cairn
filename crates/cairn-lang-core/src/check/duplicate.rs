//! `duplicate` pass — flags a `key=` / `slot` / `id=` / item name repeated
//! in the same scope, and a `theme` selector row repeated as a whole.
//!
//! Walks the surface AST rather than the IR because the IR's
//! [`IntentState`](crate::intent::IntentState) and `args` maps are
//! last-write-wins; the surface form is the only place where both the first
//! and second occurrences are still visible. `E_DUPLICATE_SELECTOR` is the
//! exception, and for the reason the rule states rather than against it —
//! `ThemeIr::selectors` is a `Vec` that keeps every row, and it is the
//! shape the matcher those rows feed actually reads. See
//! [`check_theme_selectors`].
//!
//! The codes emitted here use distinct scopes:
//! - `E_DUPLICATE_HEADER` — a single-valued `@directive` (`@cairn`,
//!   `@intended_targets`) appears more than once in the module.
//!   `@requires` is exempt; see [`check_headers`].
//! - `E_DUPLICATE_ITEM` — two top-level items *of the same kind* share a
//!   name. The four kinds are separate namespaces, so this is four
//!   independent scopes rather than one.
//! - `E_DUPLICATE_SIZE` — a struct/def header has more than one `size=`.
//! - `E_DUPLICATE_SLOT` — a `theme` body has two `slot NAME ->` lines for
//!   the same `NAME`.
//! - `E_DUPLICATE_SELECTOR` — a `theme` body has two selector rows that
//!   select the same members and bind the same key.
//! - `E_DUPLICATE_ARG`  — any other duplicate `key=` inside the same
//!   argument list (header excluding `size=`, statement args, selector
//!   attrs, selector bindings, header args of struct/def excluding size).
//! - `E_DUPLICATE_ID`   — two members in the same immediate body scope
//!   declare `id=NAME` for the same `NAME` (per-body scope; nested `level`
//!   blocks have their own namespace). Two `place` rows of one `site` body
//!   are left to the resolver, which compares refused ids as well as
//!   accepted ones and reports a repeat as `E_DUPLICATE_PLACE_ID`. A `place`
//!   row indented under another row is not in that body and stays here. See
//!   [`PlaceIdOwner`].
//! - `E_DUPLICATE_CIRCUIT` — a `struct` / `def` body has two `circuit`
//!   lines among its own members, the ones place-and-route reads a
//!   scope's reservation from.
//!
//! Every scope here reports the *repeat* and points a note at the first
//! declaration, so the anchor is the token the author would edit and the
//! note is the one they would keep. `E_DUPLICATE_SELECTOR` is the
//! exception: it names the row the repeat actually takes the value from,
//! which past two rows is not the first one. See [`check_theme_selectors`]
//! for why that scope needs the distinction and the others do not.

use indexmap::IndexMap;

use crate::ast::{Arg, Header, Item, ItemKind, Module, Statement, ThemeRule, ValueKind};
use crate::error::Span;
use crate::intent::{IntentModule, MemberRole, SelectorRule, ThemeIr, role_of};
use crate::prose::{and_list, selector_text};
use crate::resolve::select_the_same_members;

use super::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote, DiagnosticSink};

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
    // The one scope read off the IR rather than the surface AST, walked in
    // its own loop because it needs a different tree. `lower` visits items
    // in order and keeps one `ThemeIr` per `theme` block, including the
    // blocks whose name lost the binding, so this is the same set of
    // bodies the loop above walks; the sink sorts by span at the end, so
    // splitting the theme work across two passes does not reorder anything
    // the user sees.
    for theme in &ir.themes {
        check_theme_selectors(theme, sink);
    }
}

/// Module-header scope: a single-valued `@directive` may be declared
/// once.
///
/// `@cairn` and `@intended_targets` are single-valued by construction —
/// one language version the file was written against, one list of
/// intended targets. Two of either state two answers to a question that
/// has one, and no rule anywhere says which holds. Neither has a
/// consumer in the compiler today (both are provenance for a future
/// reader), which is precisely why a repeat has to be reported rather
/// than resolved: there is no reader to prefer one over the other, and
/// picking silently later would be worse than saying so now.
///
/// `@requires` is exempt because it *composes*. `resolve::version_axes`
/// folds every `version>=X` floor to the strictest, which is the
/// conjunction of the constraints, not a choice between them — nothing
/// is discarded, and the behaviour is documented on `RegistryRange` and
/// pinned by that module's tests. Reporting it here would make an error
/// out of a shape the rest of the crate defines as meaningful.
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

/// Top-level name scope, one namespace per item kind.
///
/// [`ItemKind`] is the namespace: the resolver keys scopes `struct::`,
/// `def::`, and `site::NAME::` and holds themes in a map of their own,
/// so the same name on two different kinds never collides and must not
/// be reported. Keying the seen-map by the kind rather than by a
/// keyword string is what keeps this pass and the resolver reading the
/// same definition of "namespace".
fn check_item_names(items: &[Item], sink: &mut DiagnosticSink) {
    let mut seen: IndexMap<(ItemKind, &str), Span> = IndexMap::new();
    for item in items {
        let kind = item.kind();
        let keyword = kind.keyword();
        let name = item.name();
        // Anchor on the name token, not the item's span: the block span
        // covers the indented body, and underlining a whole `def` says
        // nothing about which word to change.
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

/// What the author has to do about a repeated name, which differs by
/// kind because what the repeat costs differs by kind.
///
/// For `theme` / `def` / `struct` the name *is* the binding key, so the
/// second declaration binds nothing and its body reaches no artifact.
/// For `site` the binding key is `site::NAME::PLACE_ID`, so two blocks
/// of one name do not shadow each other — their places land in one
/// shared namespace and all of them build. Telling that author "the
/// first one resolves" would invite them to delete a block whose
/// placements were in the build.
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
    /// Binding key -> span of the row that bound it last. Only ever
    /// `get` and `insert`, so the ordering is not load-bearing here;
    /// `IndexMap` for consistency with the rest of the pass. The map in
    /// [`duplicate_selector_diag`] is the one whose order is read.
    bound: IndexMap<&'a str, &'a Span>,
}

/// Theme-selector scope: two rows that select the same members and bind the
/// same key, where the second row's value is the only one any member reads.
///
/// This is the one scope in the pass that reads the IR. The module's reason
/// for preferring the surface AST — the IR's maps are last-write-wins, so
/// the earlier occurrence is already gone — does not apply here:
/// `ThemeIr::selectors` is a `Vec` in source order with every row intact.
/// What the IR adds is the shape `resolve` matches on, so "these two rows
/// select the same members" is answered by the matcher's own rule instead
/// of a second copy of it written over `Vec<Arg>`. A key repeated *inside*
/// one row is a different scope and stays with [`check_arg_keys`], which
/// runs over the same rows from the AST side.
///
/// Where the rest of the pass points its note at the *first* declaration,
/// this one points at the most recent row to bind the key. The other
/// scopes have nothing between the two occurrences that took anything
/// away, so "first" and "displaced by this repeat" name the same row; a
/// selector binding can be displaced more than once, and naming the first
/// row would send the author to a value that was already gone.
///
/// Unlike `resolve`'s `check_unmatched_selectors`, no theme is skipped for
/// not being applied to a scope. Two rows saying two things about one
/// filter are redundant whether or not anything is bound to the theme
/// today, and suppressing the finding would surface it later, on the
/// unrelated edit that binds it.
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
        // Record this row against every key it binds, displacing whatever
        // was there. The map then holds the binding a *later* row would
        // actually replace, which is what its note has to point at: with
        // three rows on one key, the first row's value was gone before the
        // third was reached, and naming it would send the author to a line
        // whose value nothing was reading either.
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
    // One note per displaced *row*, not per displaced key: two keys can
    // come from two different rows, and two from one row would otherwise
    // name the same line twice.
    let mut per_row: IndexMap<&Span, Vec<String>> = IndexMap::new();
    for (key, span) in rebound {
        per_row.entry(span).or_default().push(format!("`{key}=`"));
    }
    // Notes in source order. Insertion order is the offending row's binding
    // order, which can put the note about a later line ahead of an earlier
    // one when two rows are displaced at once.
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

/// Scope reservation: a `struct` or `def` body may hold one `circuit`
/// line.
///
/// Every `circuit` member of the body counts, well-formed or not and
/// with or without a `[...]` selector: each states the scope's
/// reservation, and [`crate::circuit_regions`] keeps every well-formed
/// one, of which place-and-route uses the first. Only the body's own
/// members: [`crate::circuit_regions`] does not descend into a `level`,
/// so a `circuit` there is not a second reservation for this check to
/// report.
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
                notes: vec![first_declaration_note(first_span)],
                data: None,
            }),
            None => first = Some(span),
        }
    }
}

/// Who reports two `place` rows of one body that share an `id=`: the
/// resolver, or this pass.
///
/// The resolver does, for the one body its placement loop walks: the
/// top-level body of a `site`, lowered into `SiteIr::placements`. There
/// `usable_place_id` compares each `place` row's id with the `place` rows
/// above it and reports a repeat as `E_DUPLICATE_PLACE_ID`. That code
/// names the site, and it is the one `spec/lint` "Sites and placements"
/// documents the first-row-wins rule for. Reporting `E_DUPLICATE_ID` here
/// as well would bill one repair twice.
///
/// The resolver compares refused ids as well as accepted ones: an id
/// `PlaceId` accepts against the accepted ids above it, and an id it
/// refuses against the refused ones, after reporting the row as
/// `E_INVALID_PLACE_ID`. Whether `PlaceId` accepts an id depends on the id
/// alone, so two rows sharing one land in the same ledger and are always
/// compared.
///
/// The resolver reads the id off `Member::id`, which `hoist_label` fills
/// from the first `id=` whose value is an identifier or a string. That is
/// the same argument [`id_declaration`] takes here, and the two have to
/// agree. Were `hoist_label` to narrow, a `place id="a"` row left to the
/// resolver would reach it with no id to compare; were `id_declaration` to
/// narrow, the row would drop out of this pass's ledger, and a `connect`
/// row repeating its id would go unreported.
///
/// Only `place` rows are the resolver's, because its ledgers hold nothing
/// else. The placement loop reads a `connect` row but not its `id=`, and
/// skips a row the body has no reader for (`E_MISPLACED_MEMBER`) whole.
/// So a repeat between one of those and any other row, a `place` row
/// included, is reported here. [`check_body`] records every row's id, and
/// stays quiet about a repeat only when it and the row it repeats are
/// both the resolver's.
///
/// The row it repeats is the first one to declare the id. That leaves one
/// arrangement reported twice: after `connect id=a`, `place id=a`, `place
/// id=a`, the third row repeats the `connect` row here and the second row
/// in the resolver, so it carries `E_DUPLICATE_ID` and
/// `E_DUPLICATE_PLACE_ID`, with notes on different rows.
///
/// The predicate is the exact body the placement loop walks, not the body
/// kind. [`crate::intent::BodyKind`] says which roles a body has a reader
/// for, and it does not change with depth: `check::member_scope` recurses
/// with it unchanged, because nesting does not open a new body. Who
/// compares a `place` row's id does change with depth. `lower_body` keeps
/// an indented row in its parent's `Member::children`, and the placement
/// loop walks `SiteIr::placements` one level deep, so a `place` row
/// indented under any row of a `site` never reaches the resolver. Keyed on
/// `BodyKind`, that row would be left to a pass that never sees it, and its
/// repeat would go unreported.
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
    // Per immediate body: collect `id=` values declared by `Statement::Generic`
    // at this depth, plus the `key=` arg list of each statement and selector.
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
            // Take the id this row declares out of its args, and report it
            // if an earlier row of this body declared it too, unless both
            // rows are the resolver's. A selector `id=` references a member
            // rather than declaring one, so it is not read; see `extract_id`.
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
            // A nested body has its own scope, both for `id=` and for args,
            // and none of its `place` rows reach the resolver.
            check_body(children, PlaceIdOwner::ThisPass, sink);
        }
    }
}

/// Pull an `id=label` *declaration* out of a generic statement.
///
/// Only the dedicated arg list (`door id=front ...`) declares a fresh id;
/// the selector form (`door[id=front] ...`) references an existing member
/// per the surface grammar — see `Member::selector` in `intent::member`.
/// Treating selector ids as declarations would falsely flag the
/// `redstone-door` example's `logic` rebind line.
///
/// `id=` whose value is not a label is left to the type-mismatch pass.
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
