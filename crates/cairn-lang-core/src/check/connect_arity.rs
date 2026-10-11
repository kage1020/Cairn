//! `connect_arity` pass: flags every `connect` member whose positional
//! shape is not `FROM.PORT to TO.PORT`.
//!
//! `spec/components-editing-sites` "Ports and `connect`" fixes that shape:
//! three positionals, each endpoint a one-dot `<place>.<port>` reference.
//! The parser accepts any [`Value`]s up to the newline and
//! [`crate::intent::lower`] carries them through, while the resolver drops
//! a row it cannot lift into a `PortRef` without a word, so without this
//! pass the walkway would be silently absent from the build.
//!
//! Anchoring:
//!   * 0 positionals: the whole `connect` row.
//!   * 1 positional: a zero-width cursor after it, where `to TO.PORT` goes.
//!   * 2 positionals with `to` in the middle: a zero-width cursor after `to`.
//!   * 2 or more with another middle: that token alone, since the target
//!     slot cannot be read until the keyword is fixed.
//!   * 3 or more with `to` in the middle: each endpoint that is not a
//!     one-dot reference, plus the run of trailing extras, one finding each
//!     because they are independent fixes.
//!
//! The resolver's silent arms (`resolve::connect`'s `resolve_connect_row`
//! and `port_ref_from_value`) stay for callers that resolve without
//! checking, and accept exactly the rows this pass leaves unreported.

use crate::ast::{Value, ValueKind};
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::intent::{ConnectEnd, IntentModule, Member, MemberRole};

use super::DiagnosticSink;

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    // Every body, not only sites: a `connect` in a `struct` or `def` gets
    // `E_MISPLACED_MEMBER` from `check::member_scope`, and its arity finding
    // should arrive with that one rather than after it is fixed.
    for body in ir.bodies() {
        walk(body.members, sink);
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
                "expected `to` between `<from>.<port>` and `<to>.<port>`, got {}",
                // Quoted, so `"to"` does not read as the keyword it asks for.
                mid.describe(),
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
                "expected `to` between `<from>.<port>` and `<to>.<port>`, got {}",
                mid.describe(),
            ),
            vec![example_note()],
        ),
        [from, _to_kw, to_port, extras @ ..] => {
            validate_endpoint(ConnectEnd::From, from, sink);
            validate_endpoint(ConnectEnd::To, to_port, sink);
            if let (Some(first), Some(last)) = (extras.first(), extras.last()) {
                // The resolver reads only `positional[0..3]`, so a second
                // destination would vanish silently. One span covers the
                // extras because the fix is the whole suffix.
                let span = Span {
                    start: first.span.start,
                    end: last.span.end,
                };
                push(
                    sink,
                    span,
                    format!(
                        "`connect` accepts exactly `<from>.<port> to <to>.<port>`; {} extra positional{} after `to`",
                        extras.len(),
                        if extras.len() == 1 { "" } else { "s" },
                    ),
                    vec![example_note(), DiagnosticNote {
                        span: None,
                        message: "additional inputs belong in `key=value` arguments (e.g. `path=@gravel`)".to_string(),
                    }],
                );
            }
        }
    }
}

/// Flag `value` when it is not a one-dot `<place>.<port>` reference, the
/// shape `resolve::connect`'s `port_ref_from_value` lifts into a `PortRef`.
///
/// Swapping the slots at the two call sites would still typecheck; the
/// per-slot messages in `tests/check/connect_arity.rs` pin the pairing.
fn validate_endpoint(end: ConnectEnd, value: &Value, sink: &mut DiagnosticSink) {
    let got = match &value.kind {
        ValueKind::DotRef(dot) if dot.tail().len() == 1 => return,
        // The count makes an extra dot visible in a message read without
        // the source beside it.
        ValueKind::DotRef(dot) => format!("{} with {} segments", value.describe(), dot.len()),
        _ => value.describe(),
    };
    // The position tells two endpoint findings on one row apart in a
    // plain-text log, which renders no spans.
    let (shape, position) = match end {
        ConnectEnd::From => ("`<from>.<port>`", "before `to`"),
        ConnectEnd::To => ("`<to>.<port>`", "after `to`"),
    };
    let mut notes = vec![example_note()];
    if let Some(repair) = repair_note(value) {
        notes.push(DiagnosticNote {
            span: None,
            message: repair,
        });
    }
    push(
        sink,
        value.span.clone(),
        format!("`connect` needs {shape} {position}, got {got}"),
        notes,
    );
}

/// The single edit that turns this value into a port reference, when
/// there is one; the example note shows the shape but not what to change.
fn repair_note(value: &Value) -> Option<String> {
    match &value.kind {
        ValueKind::Ident(name) => Some(format!(
            "name the port as well: `{name}.<port>`, where `<port>` is an `id=` on a member of the placed def",
        )),
        ValueKind::Str(_) => Some(
            "drop the quotes: a port reference is bare source syntax, not a string literal"
                .to_string(),
        ),
        ValueKind::Token(_) => Some(
            "`@` marks a material token; the walkway material belongs in `path=@…`, not in an endpoint"
                .to_string(),
        ),
        ValueKind::DotRef(_) => Some(
            "a port reference carries exactly one dot: the place id, then the port id".to_string(),
        ),
        _ => None,
    }
}

fn is_to_keyword(value: &Value) -> bool {
    matches!(&value.kind, ValueKind::Ident(s) if s == "to")
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
        span,
        primary,
        notes,
        data: None,
    });
}
