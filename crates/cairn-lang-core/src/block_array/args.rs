//! Readers for a member's `key=` arguments, and the findings a read raises.
//!
//! Each reader tells a key that was not written apart from one written with
//! a value it cannot use. What the unusable value costs is the caller's to
//! say: drawn with a default, it is `W_IGNORED_ARGUMENT`
//! ([`UnreadArgument`]); not drawn at all, it is `W_DEFERRED_MEMBER`
//! ([`nonneg_int_or_defer`]).
//!
//! Lowering, `openings` and the `connect` ports all read through these, so
//! this module depends on none of them.

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::intent::{Member, ValueWithSpan};

/// Result of reading a non-negative integer `key=` for a caller that does
/// not draw the member when the value is unusable.
///
/// - `Valid(v)`: the key parsed to a `u32`.
/// - `Absent`: the key was not written; the caller applies its own
///   default.
/// - `Deferred`: the key did not parse to a `u32`. A `W_DEFERRED_MEMBER`
///   has been pushed, and the caller must not draw the member.
///
/// Three cases rather than [`Member::nonneg_u32`]'s `Option`, so an
/// unusable value such as `y="top"` cannot pass for an absent one. A caller
/// that draws the member anyway wants [`nonneg_int_or_ignore`].
pub(super) enum NonNegRead {
    Valid(u32),
    Absent,
    Deferred,
}

/// Read `key=` as a non-negative `u32` for a caller that falls back to a
/// default and draws the member anyway.
///
/// An unusable value raises `W_IGNORED_ARGUMENT` with `consequence` as its
/// note: what the fallback does to the output, which only the caller knows
/// (and, as lowering's `max_roof_overhang` shows, whether the member draws
/// at all). `None` covers absent and unusable alike; only the unusable one
/// is reported.
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

/// What [`nonneg_int_or_ignore`] accepts, worded to complete "`key=` must
/// be …".
const NONNEG_U32: &str = "a non-negative integer that fits in u32";

/// Read `key=` through `read` for a caller that falls back to a default,
/// telling "absent" apart from "unreadable".
///
/// - `Ok(Some(v))`: the key is present and `read` accepted its value.
/// - `Ok(None)`: the key was not written; the caller applies its default
///   and nothing is reported.
/// - `Err(unread)`: the key was written and `read` refused it. This is the
///   unreadable value `spec/lint` "Error vs warning" reports as
///   `W_IGNORED_ARGUMENT`. The caller applies its default, or, where the
///   key decides whether the member is built at all, refuses the member
///   instead.
///
/// Every caller reports the [`UnreadArgument`] whether or not the member
/// reaches the build, because holding it back until a later refusal is
/// fixed costs the author another compile; only the note differs (see
/// [`UnreadArgument::report`]). A member dropped before its reader runs (a
/// level-scoped roof, say) reports nothing.
///
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
/// Which identifiers the caller accepts is still the caller's to say:
/// this only separates "not an identifier at all" (`half="bottom"`,
/// `facing=1`), which is an unreadable value, from an identifier the
/// caller does not support (`half=sideways`), which defers the member.
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

/// A `key=` written with a value its reader cannot use — the third outcome
/// of [`read_or_ignore`], beside "read" and "not written".
///
/// Not yet a [`Diagnostic`]: its note says what became of the member, and
/// only the caller knows whether it reached the build.
#[derive(Debug)]
pub(super) struct UnreadArgument {
    /// The value's own span, so two unreadable keys on one line underline
    /// two places, as `check::arguments` does for the same code.
    span: Span,
    pub(super) key: &'static str,
    /// The value as [`crate::ast::Value::describe`] renders it, so a quoted
    /// `"true"` reads as the string it is rather than as the word the
    /// author meant.
    written: String,
    /// What the reader accepts, worded to complete "`key=` must be …".
    expected: &'static str,
}

impl UnreadArgument {
    /// The `W_IGNORED_ARGUMENT` for this value, with `consequence` as its
    /// note.
    ///
    /// The primary stops at "the value was ignored" because the reader
    /// cannot know whether the member is built. `consequence` says, in the
    /// caller's words: what the default did to a built member, that a
    /// finding on the same line refused the member anyway, or that it was
    /// refused instead of given the default.
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

/// The `W_DEFERRED_MEMBER` for a member lowering does not draw, with
/// `reason` as its primary.
///
/// Here rather than with lowering's other builders because
/// [`nonneg_int_or_defer`] raises it too.
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

/// The slot's span, so a warning underlines the value itself, or the
/// member's when the slot has none (`0..0`).
pub(super) fn member_or_slot_span(member: &Member, slot: &ValueWithSpan) -> Span {
    if slot.span.start == 0 && slot.span.end == 0 {
        member.span.clone()
    } else {
        slot.span.clone()
    }
}
