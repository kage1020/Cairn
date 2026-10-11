//! Readers for a member's `key=` arguments, and the findings a read raises.
//!
//! Each reader tells a key that was not written apart from one written with
//! a value it cannot use. What the unusable value costs is the caller's to
//! say: drawn with a default, it is `W_IGNORED_ARGUMENT`
//! ([`UnreadArgument`]); not drawn at all, it is `W_DEFERRED_MEMBER`
//! ([`nonneg_int_or_defer`]).
//!
//! Lowering reads its members through these, and so does `openings`, which
//! the `connect` ports read a window's rectangle through, so this module
//! sits below all three and depends on none of them.

use crate::ast::ValueKind;
use crate::check::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::error::Span;
use crate::intent::{Member, ValueWithSpan};

/// Result of reading a non-negative integer `key=` with defer semantics.
///
/// The block-array pass distinguishes three outcomes on a `key=`:
/// - `Valid(v)`: the key is present and parsed to a `u32`.
/// - `Absent`: the key was not written; the caller applies its own
///   default.
/// - `Deferred`: the key was present but did not parse to a
///   non-negative `u32`. A `W_DEFERRED_MEMBER` has already been pushed
///   and the caller must return.
///
/// Using a named tri-state keeps callers explicit about which case they
/// treat as a default vs which case aborts, and closes the
/// `y="top"`-silently-becomes-`0` gap that the plain [`Member::nonneg_u32`]
/// return type could not.
///
/// Every caller keeps the `Deferred` contract: each one returns or
/// `continue`s, and the `void=` site reads for validation alone. A caller
/// that means to draw the member anyway wants
/// [`nonneg_int_or_ignore`], whose finding says so.
pub(super) enum NonNegRead {
    Valid(u32),
    Absent,
    Deferred,
}

/// Read `key=` as a non-negative `u32` for a caller that carries on
/// without it.
///
/// [`NonNegRead::Deferred`] means "the caller must return", and
/// `W_DEFERRED_MEMBER` says the member did not lower. A caller that falls
/// back to a default and draws the member anyway needs the other report:
/// the value was unusable, and here is what was used instead.
/// `consequence` is that second half in the caller's own words, because
/// only the caller knows what its fallback does to the output — and, as
/// lowering's `max_roof_overhang` does, whether the member draws at all.
///
/// `None` covers "absent" as well as "unusable", because a caller with a
/// default treats them alike — the difference is only whether anything is
/// reported.
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
/// Every caller reports the [`UnreadArgument`], whether or not the member
/// then reaches the build: the value is unreadable wherever the member
/// ends up, and holding the finding back until a later refusal is repaired
/// only costs the author another compile to learn it. What differs is the
/// note, which [`UnreadArgument::report`] takes from the caller once it
/// knows: the default's effect on a member in the build, that the member
/// is not built either way, or that it was refused instead of given the
/// default. A member dropped before its reader runs at all (a level-scoped
/// roof, say) is not read, and so reports nothing.
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
    /// The value's own span, so the finding underlines what was written,
    /// as `check::arguments` does for the same code, and two unreadable
    /// keys on one line underline two places.
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
    /// The primary stops at "the value was ignored" because whether the
    /// member is in the build is not a fact the reader has. The note
    /// carries it instead, in the caller's words: what the default did to
    /// the output when the member is built, that it is not built either way
    /// when a finding on the same line refused it, or that the finding on
    /// the same line refused it instead of giving it the default.
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

/// Prefer the slot-binding span (which points at the `@token`) over the
/// member line so the warning underlines the exact value that could not be
/// lowered.
pub(super) fn member_or_slot_span(member: &Member, slot: &ValueWithSpan) -> Span {
    if slot.span.start == 0 && slot.span.end == 0 {
        member.span.clone()
    } else {
        slot.span.clone()
    }
}
