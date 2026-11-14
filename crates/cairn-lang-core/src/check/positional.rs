//! The `positional` pass: bare values on a line that reads none (`spec/syntax` "Lexical"). It
//! descends through subtrees `member_scope` and `nesting` report, since dedenting a line does not
//! make its values read.

use crate::ast::{DottedRef, Item, Module, Statement, ThemeRule, Value};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::intent::{IntentModule, Member, MemberRole};

use super::DiagnosticSink;

pub(super) fn run(module: &Module, ir: &IntentModule, sink: &mut DiagnosticSink) {
    for body in ir.bodies() {
        walk(body.members, sink);
    }
    for item in &module.items {
        check_item(item, sink);
    }
}

fn walk(members: &[Member], sink: &mut DiagnosticSink) {
    for member in members {
        if !matches!(member.role, MemberRole::Other(_) | MemberRole::Connect)
            && let (Some(first), Some(last)) = (member.positional.first(), member.positional.last())
        {
            report(member, first, last, sink);
        }
        walk(&member.children.members, sink);
    }
}

fn report(member: &Member, first: &Value, last: &Value, sink: &mut DiagnosticSink) {
    let keyword = member.role.keyword();
    let count = member.positional.len();
    sink.push(Diagnostic {
        code: DiagnosticCode::UnexpectedPositional,
        span: Span {
            start: first.span.start,
            end: last.span.end,
        },
        primary: format!(
            "`{keyword}` reads only `key=value` arguments: the {count} bare {value} on this line {verb} dropped",
            value = if count == 1 { "value" } else { "values" },
            verb = if count == 1 { "is" } else { "are" },
        ),
        notes: vec![
            DiagnosticNote {
                span: None,
                message: "give each value the key it belongs to (`side=front`, `size=2x2`); a missing `=` lands here too".to_owned(),
            },
            DiagnosticNote {
                span: None,
                message: "`connect FROM.PORT to TO.PORT` is the one statement that reads positional values".to_owned(),
            },
        ],
        data: None,
    });
}

/// The header, rule, `logic` and `assert` lines of `item`, which no member carries into the IR.
fn check_item(item: &Item, sink: &mut DiagnosticSink) {
    match item {
        Item::Theme {
            positional, body, ..
        } => {
            push_run(positional, &Line::BareHeader("theme"), sink);
            for rule in body {
                match rule {
                    ThemeRule::Slot { positional, .. } => push_run(positional, &Line::Slot, sink),
                    ThemeRule::Selector {
                        keyword,
                        positional,
                        ..
                    } => push_run(positional, &Line::SelectorRule(keyword), sink),
                }
            }
        }
        Item::Def {
            positional, body, ..
        } => {
            push_run(positional, &Line::KeyedHeader("def"), sink);
            check_statements(body, sink);
        }
        Item::Struct {
            positional, body, ..
        } => {
            push_run(positional, &Line::KeyedHeader("struct"), sink);
            check_statements(body, sink);
        }
        Item::Site {
            positional, body, ..
        } => {
            push_run(positional, &Line::BareHeader("site"), sink);
            check_statements(body, sink);
        }
    }
}

fn check_statements(body: &[Statement], sink: &mut DiagnosticSink) {
    for statement in body {
        match statement {
            Statement::Generic { children, .. } | Statement::EditionGuard { children, .. } => {
                check_statements(children, sink);
            }
            Statement::Logic {
                lhs,
                binds: false,
                span,
                ..
            } => sink.push(unbound_logic(lhs, span)),
            Statement::Logic { .. } => {}
            Statement::AssertTruth { positional, .. } => {
                push_run(
                    positional,
                    &Line::Assert("truth", "the `}` of its table"),
                    sink,
                );
            }
            Statement::AssertAlways { positional, .. } => {
                push_run(positional, &Line::Assert("always", "its `)`"), sink);
            }
            Statement::AssertLatency { positional, .. } => {
                push_run(positional, &Line::Assert("latency", "its bound"), sink);
            }
        }
    }
}

/// A line other than a member line, as an `E_UNEXPECTED_POSITIONAL` on it words it.
enum Line<'a> {
    /// A `struct` or `def` header, which reads `size=`.
    KeyedHeader(&'static str),
    /// A `site` or `theme` header, which reads nothing after its name.
    BareHeader(&'static str),
    /// A `theme` selector rule on the keyword given, which reads `key=value` bindings.
    SelectorRule(&'a str),
    /// A `theme` slot row, which binds one value.
    Slot,
    /// An `assert` line of the form given, and what ends that form.
    Assert(&'static str, &'static str),
}

impl Line<'_> {
    fn primary(&self, count: usize) -> String {
        let (values, verb) = if count == 1 {
            ("bare value", "is")
        } else {
            ("bare values", "are")
        };
        match self {
            Self::KeyedHeader(keyword) => format!(
                "a `{keyword}` header reads only `key=value` arguments: the {count} {values} on \
                 this line {verb} dropped"
            ),
            Self::BareHeader(keyword) => format!(
                "a `{keyword}` header reads nothing after its name: the {count} {values} on this \
                 line {verb} dropped"
            ),
            Self::SelectorRule(keyword) => format!(
                "a `{keyword}[...]` rule binds only `key=value` arguments: the {count} {values} on \
                 this line {verb} dropped"
            ),
            Self::Slot => format!(
                "a `slot` row binds one value: the {count} {values} after it on this line {verb} \
                 dropped"
            ),
            Self::Assert(form, end) => format!(
                "an `assert {form}` line ends at {end}: the {count} {values} after it {verb} \
                 dropped"
            ),
        }
    }

    fn repair(&self, count: usize) -> String {
        let (them, each) = if count == 1 {
            ("it", "it")
        } else {
            ("them", "each")
        };
        match self {
            Self::KeyedHeader(_) => "give each value the key it belongs to (`size=3x3`); a \
                                     missing `=` lands here too"
                .to_owned(),
            Self::BareHeader(keyword) => format!(
                "delete {them}: a `{keyword}` header is its name, with an optional `:` after it"
            ),
            Self::SelectorRule(_) => "give each value the key it binds (`mat_slot=glass`); a \
                                      missing `=` lands here too"
                .to_owned(),
            Self::Slot => {
                format!("delete {them}, or bind {each} to a slot of its own on a `slot` row")
            }
            Self::Assert(..) => format!("delete {them}"),
        }
    }
}

fn push_run(positional: &[Value], line: &Line<'_>, sink: &mut DiagnosticSink) {
    let (Some(first), Some(last)) = (positional.first(), positional.last()) else {
        return;
    };
    let count = positional.len();
    sink.push(Diagnostic {
        code: DiagnosticCode::UnexpectedPositional,
        span: first.span.start..last.span.end,
        primary: line.primary(count),
        notes: vec![DiagnosticNote {
            span: None,
            message: line.repair(count),
        }],
        data: None,
    });
}

fn unbound_logic(name: &DottedRef, span: &Span) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::UnexpectedPositional,
        span: span.clone(),
        primary: format!(
            "a `logic` line binds `NAME = EXPR`, and no `=` follows `{name}`: the name and the \
             expression after it are bare values, and the line binds nothing"
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: format!("write `=` between them: `logic {name} = ...`"),
        }],
        data: None,
    }
}
