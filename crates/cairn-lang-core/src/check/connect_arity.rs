//! `connect_arity` pass — flags every `connect` member whose positional
//! shape is not `FROM.PORT to TO.PORT`.
//!
//! The surface grammar of `connect` (spec §9.3.5) is fixed at three
//! positionals: the from-side dotted reference, the literal `to`
//! keyword, and the to-side dotted reference. The line-based parser
//! ([`crate::parse::Parser::parse_command`]) accepts any number of
//! positionals up to the next newline without enforcing arity, and
//! [`crate::intent::lower`] carries them through verbatim. Without this
//! pass, broken rows like `connect a.entry` would reach the resolver,
//! whose `resolve_connect_row` arm short-circuits with no diagnostic
//! and leaves the walkway silently absent from the build.
//!
//! Anchoring strategy:
//!   * 0 positional → underline the whole `connect` row.
//!   * 1 positional → zero-width cursor right after the from value, so
//!     the rendered `file:L:C` points at where `to TO.PORT` should go.
//!   * 2 positional with `to` middle → zero-width cursor right after the
//!     `to` keyword.
//!   * 2 or 3+ positional with a non-`to` middle → underline the
//!     offending separator token; the user must fix the wrong keyword
//!     before the trailing target slot is interpretable, so surfacing
//!     two findings for one row would be noise.
//!
//! Resolver-side TODO: the silent return arms in
//! [`crate::resolve::resolver`] stay in place so library callers that
//! invoke `resolve(ir)` directly without going through `check` still
//! see the same defensive behaviour, but their comments reference this
//! pass as the upstream signal source.

use crate::ast::{Value, ValueKind};
use crate::error::Span;
use crate::intent::{IntentModule, Member, MemberRole};

use super::{Diagnostic, DiagnosticCode, DiagnosticNote, DiagnosticSink};

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    // `connect` only carries semantic meaning at site placement scope —
    // the keyword_allowlist pass already flags `connect` rows that
    // appear under a `struct` or `def` body (their role would have
    // lowered to `Other("connect")` if the keyword table excluded them
    // there). The role table currently treats `connect` as global, so
    // the walk has to cover struct/def/site bodies and their nested
    // sub-members to stay consistent if a stray `connect` slips through.
    for s in &ir.structs {
        walk(&s.members, sink);
    }
    for d in &ir.defs {
        walk(&d.members, sink);
    }
    for s in &ir.sites {
        walk(&s.placements, sink);
    }
}

fn walk(members: &[Member], sink: &mut DiagnosticSink) {
    for m in members {
        if matches!(m.role, MemberRole::Connect) {
            validate(m, sink);
        }
        walk(&m.children.members, sink);
    }
}

fn validate(member: &Member, sink: &mut DiagnosticSink) {
    match member.positional.as_slice() {
        [] => push(
            sink,
            member.span.clone(),
            "`connect` requires `<from>.<port> to <to>.<port>`".into(),
            vec![example_note()],
        ),
        [from] => push(
            sink,
            zero_width_after(&from.span),
            "`connect` is missing the `to <to>.<port>` half".into(),
            vec![example_note()],
        ),
        [_from, mid] if !is_to_keyword(mid) => push(
            sink,
            mid.span.clone(),
            format!(
                "expected `to` between `<from>.<port>` and `<to>.<port>`, got `{}`",
                render_value(mid),
            ),
            vec![example_note()],
        ),
        [_from, mid] => push(
            sink,
            zero_width_after(&mid.span),
            "`connect` is missing the `<to>.<port>` target after `to`".into(),
            vec![example_note()],
        ),
        [_from, mid, ..] if !is_to_keyword(mid) => push(
            sink,
            mid.span.clone(),
            format!(
                "expected `to` between `<from>.<port>` and `<to>.<port>`, got `{}`",
                render_value(mid),
            ),
            vec![example_note()],
        ),
        _ => {
            // 3+ positionals with `to` in the middle slot is the
            // well-formed shape. The downstream resolver still verifies
            // each side is a dotted reference and that the place/port
            // ids resolve, so port-shape and resolution errors keep
            // their own dedicated codes.
        }
    }
}

fn is_to_keyword(value: &Value) -> bool {
    matches!(&value.kind, ValueKind::Ident(s) if s == "to")
}

fn render_value(value: &Value) -> String {
    match &value.kind {
        ValueKind::Ident(s) | ValueKind::Str(s) => s.clone(),
        _ => value.kind_name().to_string(),
    }
}

fn zero_width_after(span: &Span) -> Span {
    Span {
        start: span.end,
        end: span.end,
    }
}

fn example_note() -> DiagnosticNote {
    DiagnosticNote {
        span: None,
        message: "example: `connect home1.entry to home2.entry path=@gravel`".to_string(),
    }
}

fn push(sink: &mut DiagnosticSink, span: Span, primary: String, notes: Vec<DiagnosticNote>) {
    sink.push(Diagnostic {
        code: DiagnosticCode::ConnectArity,
        severity: DiagnosticCode::ConnectArity.severity(),
        span,
        primary,
        notes,
        data: None,
    });
}
