//! Readers for a member's `key=` arguments, and the findings a read raises.

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::intent::{Member, ValueWithSpan};

/// A non-negative integer `key=` read for a caller that skips the member on an unusable value.
///
/// `Deferred`: a `W_DEFERRED_MEMBER` is already pushed and the member must not be drawn.
pub(super) enum NonNegRead {
    Valid(u32),
    Absent,
    Deferred,
}

/// Read `key=` as a non-negative `u32`, or `None` when absent or unusable; an unusable value
/// raises `W_IGNORED_ARGUMENT` with `consequence` as its note.
pub(super) fn nonneg_int_or_ignore(
    member: &Member,
    key: &'static str,
    consequence: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<u32> {
    read_or_ignore(
        member,
        key,
        |kind| match kind {
            ValueKind::Int(v) => u32::try_from(*v).ok(),
            _ => None,
        },
        NONNEG_U32,
    )
    .unwrap_or_else(|unread| {
        diagnostics.push(unread.report(consequence));
        None
    })
}

/// What [`nonneg_int_or_ignore`] accepts, completing "`key=` must be …".
const NONNEG_U32: &str = "a non-negative integer that fits in u32";

/// Read `key=` through `read`: `Ok(None)` when absent, `Err` when written but refused.
///
/// Callers report the `Err` (`spec/lint` "Error vs warning") even when the member is refused.
/// `expected` completes "`key=` must be …".
pub(super) fn read_or_ignore<'m, T>(
    member: &'m Member,
    key: &'static str,
    read: impl FnOnce(&'m ValueKind) -> Option<T>,
    expected: &'static str,
) -> Result<Option<T>, UnreadArgument> {
    let Some(raw) = member.intent_state.get(key) else {
        return Ok(None);
    };
    match read(&raw.value.kind) {
        Some(v) => Ok(Some(v)),
        None => Err(UnreadArgument {
            span: member_or_slot_span(member, raw),
            key,
            written: raw.value.describe(),
            expected,
        }),
    }
}

/// [`read_or_ignore`] for a key whose value is a bare identifier.
///
/// A non-identifier is unreadable; an identifier the caller does not support is its to defer.
pub(super) fn ident_or_ignore<'m>(
    member: &'m Member,
    key: &'static str,
    expected: &'static str,
) -> Result<Option<&'m str>, UnreadArgument> {
    read_or_ignore(
        member,
        key,
        |kind| match kind {
            ValueKind::Ident(name) => Some(name.as_str()),
            _ => None,
        },
        expected,
    )
}

/// A `key=` written with a value its reader cannot use.
#[derive(Debug)]
pub(super) struct UnreadArgument {
    span: Span,
    pub(super) key: &'static str,
    /// The value as [`crate::ast::Value::describe`] renders it.
    written: String,
    /// What the reader accepts, worded to complete "`key=` must be …".
    expected: &'static str,
}

impl UnreadArgument {
    /// The `W_IGNORED_ARGUMENT` for this value, with `consequence` as its note.
    pub(super) fn report(self, consequence: &str) -> Diagnostic {
        let Self {
            span,
            key,
            written,
            expected,
        } = self;
        Diagnostic {
            code: DiagnosticCode::IgnoredArgument,
            span,
            primary: format!("`{key}=` must be {expected}, not {written}; the value was ignored"),
            notes: vec![DiagnosticNote {
                span: None,
                message: consequence.to_owned(),
            }],
            data: None,
        }
    }
}

pub(super) fn nonneg_int_or_defer(
    member: &Member,
    key: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> NonNegRead {
    if !member.intent_state.contains_key(key) {
        return NonNegRead::Absent;
    }
    if let Some(v) = member.nonneg_u32(key) {
        NonNegRead::Valid(v)
    } else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!("`{key}=` must be a non-negative integer that fits in u32"),
        ));
        NonNegRead::Deferred
    }
}

pub(super) fn size_value(member: &Member, key: &str) -> Option<(u32, u32)> {
    let raw = member.intent_state.get(key)?;
    match &raw.value.kind {
        ValueKind::Size { w, h } => Some((w.get(), h.get())),
        _ => None,
    }
}

/// The `W_DEFERRED_MEMBER` for a member lowering does not draw, with `reason` as its primary.
pub(super) fn diag_deferred_member_reason(member: &Member, reason: &str) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::DeferredMember,
        span: member.span.clone(),
        primary: reason.to_owned(),
        notes: vec![DiagnosticNote {
            span: None,
            message: "block-array lowering currently voxelises floor, walls, door, window, \
                      roof (kind=gable|shed|hip|flat), stair (kind=stairs), pressure_plate \
                      (at=<side>.outside|inside.<side>), and level y=N grouping, and \
                      recognises circuit region=<label> void=<N> (u32, N>=1) plus \
                      door[id=<name>] opened_by=sig.<name> actuator patches; other roles \
                      will be added as their lowering rules are spec'd"
                .to_owned(),
        }],
        data: None,
    }
}

/// The slot's span, or the member's when the slot has none (`0..0`).
pub(super) fn member_or_slot_span(member: &Member, slot: &ValueWithSpan) -> Span {
    if slot.span.start == 0 && slot.span.end == 0 {
        member.span.clone()
    } else {
        slot.span.clone()
    }
}
