//! `connect_arity` pass — flags every `connect` member whose positional
//! shape is not `FROM.PORT to TO.PORT`.
//!
//! The surface grammar of `connect` (spec §9.3.5) is fixed at three
//! positionals: the from-side dotted reference, the literal `to`
//! keyword, and the to-side dotted reference. The line-based parser
//! (`parse`'s `parse_command`) accepts any number of
//! positionals up to the next newline without enforcing arity, and
//! [`crate::intent::lower`] carries them through verbatim. Without this
//! pass, broken rows like `connect a.entry` would reach the resolver,
//! whose `resolve_connect_row` arm short-circuits with no diagnostic
//! and leaves the walkway silently absent from the build.
//!
//! "Shape" is not only the count. The grammar admits exactly a one-dot
//! `<place>.<port>` reference in each endpoint slot, and the parser
//! accepts any [`Value`] in a positional — a bare identifier, a
//! literal, a `@material` token, a quoted string, a list, or a
//! reference carrying a second dot. None of those can be lifted into a
//! `PortRef`, so the resolver drops the row and its walkway never
//! appears in the build.
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
//!   * 3+ positional with a `to` middle → underline each endpoint that
//!     is not a one-dot reference, plus the run of trailing extras.
//!     These are independent fix sites: deleting the extras does not
//!     turn `1` into a port reference, and correcting one endpoint says
//!     nothing about the other, so one finding each spares the author a
//!     round of the edit-check loop per mistake.
//!
//! Resolver-side note: the silent return arms in
//! `resolve::resolver`'s `resolve_connect_row` and
//! `port_ref_from_value` stay in place so library callers that invoke
//! `resolve(ir)` directly without going through `check` still see the
//! same defensive behaviour. Their guards accept exactly the rows this
//! pass leaves unreported — the same positional count, the same
//! separator, the same endpoint shapes — so the two layers cannot
//! disagree on which rows are well-formed, only on how loudly they say
//! so.

use crate::ast::{Value, ValueKind};
use crate::error::Span;
use crate::intent::{ConnectEnd, IntentModule, Member, MemberRole};

use super::{Diagnostic, DiagnosticCode, DiagnosticNote, DiagnosticSink};

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    // `connect` carries semantic meaning only at site placement scope,
    // but `intent::keyword_table::role_of` treats `connect` as global
    // and lowers any occurrence to [`MemberRole::Connect`] regardless
    // of the surrounding body. `check::member_scope` reports a `connect`
    // written into a `struct` or `def` body as `E_MISPLACED_MEMBER`, but
    // that finding is about the body rather than the row: it says nothing
    // about whether the row would have been well-formed where it belongs.
    // Walk every scope here so a stray row gets both halves at once,
    // instead of an arity error appearing only after the first is fixed.
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
                "expected `to` between `<from>.<port>` and `<to>.<port>`, got {}",
                describe(mid),
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
                describe(mid),
            ),
            vec![example_note()],
        ),
        // Three positionals with `to` in the middle is the well-formed
        // count. What remains to check is the two endpoint slots, and —
        // when the row runs long — the trailing extras.
        [from, _to_kw, to_port, extras @ ..] => {
            validate_endpoint(ConnectEnd::From, from, sink);
            validate_endpoint(ConnectEnd::To, to_port, sink);
            if let (Some(first), Some(last)) = (extras.first(), extras.last()) {
                // Over-arity. The grammar caps `connect` at three
                // positionals; everything beyond `to TO.PORT` is `args=`
                // territory (notably `path=@MATERIAL`). Without this arm
                // the resolver would read `positional[0..3]` and drop
                // every trailing slot on the floor — a user who wrote
                // `connect a.entry to b.entry c.exit path=@gravel`
                // thinking the row could carry two destinations would
                // see one walkway lay and the other vanish silently.
                // Underline the run of extras together so the fix
                // surface is the whole offending suffix rather than each
                // token in isolation.
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

/// Flag `value` when it is not a one-dot `<place>.<port>` reference.
///
/// The accepted shape is exactly what `resolve::resolver`'s
/// `port_ref_from_value` can lift into a `PortRef`; anything else
/// leaves the row without a walkway, so this is the layer that has to
/// say so.
///
/// `end` comes first so the call site reads in the order the row does.
/// It cannot be made unswappable — the two call sites below could hand
/// over the wrong slot and still typecheck — so the pairing is pinned
/// by the per-slot message assertions in `tests/check_connect_arity.rs`
/// instead of by the signature.
fn validate_endpoint(end: ConnectEnd, value: &Value, sink: &mut DiagnosticSink) {
    let got = match &value.kind {
        ValueKind::DotRef(dot) if dot.tail().len() == 1 => return,
        // A reference with the wrong number of segments. Naming the
        // count makes the extra dot visible in a message quoted without
        // the source beside it, where `a.entry.x` and `a.entry` differ
        // by one easily-missed character.
        ValueKind::DotRef(dot) => format!("{} with {} segments", describe(value), dot.len()),
        _ => describe(value),
    };
    // The placeholder is spelled as the other arms of this pass and the
    // spec's 9.3.5 grammar line spell it; the position keeps two
    // endpoint findings on one row apart in a plain-text log, which
    // renders no spans.
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
/// there is one. `spec/lint.md` asks for messages an author can act on
/// from the message alone; the generic example note shows the target
/// shape but not which character to change.
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

/// Name a value by kind and, where the reconstruction is bounded, by
/// the text the author wrote.
///
/// The surface form matters most for the shapes that render
/// identically under a bare `kind_name`: `connect a.entry "to" b.entry`
/// reported ``expected `to` … got `to` `` for as long as this printed a
/// string's contents unquoted, which reads as the pass rejecting the
/// very keyword it asked for.
fn describe(value: &Value) -> String {
    value.describe()
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
