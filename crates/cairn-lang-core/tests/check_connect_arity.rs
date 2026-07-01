//! Acceptance tests for the `connect_arity` pass of
//! `cairn_lang_core::check`.
//!
//! `connect` rows have a fixed surface grammar: `connect FROM.PORT to
//! TO.PORT [path=@MATERIAL]` (spec §9.3.5). The line-based parser
//! accepts any number of positional values up to the next newline
//! without enforcing the arity, and `intent::lower` carries the
//! positional list through verbatim. The resolver's `connect` arm
//! short-circuits with no diagnostic when the row is incomplete, so a
//! typo would otherwise leave the user with a silently-vanished walkway
//! and no signal. This pass anchors `E_CONNECT_ARITY` at the missing
//! positional's parse position before the resolver runs.

use cairn_lang_core::{Diagnostic, DiagnosticCode, Severity, check, lower, parse};

fn diagnose(source: &str) -> Vec<Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}"));
    let ir = lower(&module);
    check(&module, &ir)
}

fn arity_only(diagnostics: Vec<Diagnostic>) -> Vec<Diagnostic> {
    diagnostics
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::ConnectArity)
        .collect()
}

fn slice<'a>(source: &'a str, diag: &Diagnostic) -> &'a str {
    &source[diag.span.clone()]
}

const PROLOGUE: &str = "struct hut size=3x3\n  \
floor\n  \
door id=entry at=center\n\n\
site h:\n  \
place id=a use=hut at=origin\n  \
place id=b use=hut east_of=a gap=4\n  ";

/// A fully-formed `connect` row passes the arity pass silently. Anchors
/// the negative space for the broken-row assertions below — without this
/// the "exactly one `E_CONNECT_ARITY`" claims could not distinguish a real
/// signal from a pass that always fires.
#[test]
fn ca_1_well_formed_connect_emits_no_arity_diagnostic() {
    let src = format!("{PROLOGUE}connect a.entry to b.entry path=@gravel\n");
    let arity = arity_only(diagnose(&src));
    assert!(
        arity.is_empty(),
        "well-formed connect must not trigger arity pass, got: {arity:#?}",
    );
}

/// `connect` with zero positional arguments has no `from.port` and no
/// `to.port`. The pass anchors the diagnostic at the keyword span (the
/// member-level span) so the user sees the whole row underlined.
#[test]
fn ca_2_bare_connect_emits_arity_with_keyword_span() {
    let src = format!("{PROLOGUE}connect path=@gravel\n");
    let arity = arity_only(diagnose(&src));
    assert_eq!(arity.len(), 1, "got: {arity:#?}");
    let d = &arity[0];
    assert_eq!(d.code, DiagnosticCode::ConnectArity);
    assert_eq!(d.severity, Severity::Error);
    assert!(
        slice(&src, d).starts_with("connect"),
        "primary span should cover the `connect` row, got: {:?}",
        slice(&src, d),
    );
    assert!(
        d.primary.contains("connect"),
        "primary message should name the keyword, got: {}",
        d.primary,
    );
    assert!(
        d.notes
            .iter()
            .any(|n| n.message.contains("connect") && n.message.contains("to")),
        "should include an example shape note, got: {:#?}",
        d.notes,
    );
}

/// `connect FROM.PORT` is missing the `to TO.PORT` half. The diagnostic
/// is anchored at a zero-width span right after the from value so the
/// `file:L:C` pointer lands at the cursor position where the missing
/// half should go.
#[test]
fn ca_3_only_from_emits_arity_anchored_after_from() {
    let src = format!("{PROLOGUE}connect a.entry path=@gravel\n");
    let arity = arity_only(diagnose(&src));
    assert_eq!(arity.len(), 1, "got: {arity:#?}");
    let d = &arity[0];
    let from_end = src.find("a.entry").expect("from value present") + "a.entry".len();
    assert_eq!(
        d.span.start, from_end,
        "primary span should start at the byte after `a.entry`",
    );
    assert_eq!(
        d.span.end, from_end,
        "primary span should be zero-width (a cursor, not a range)",
    );
    assert!(
        d.primary.contains("to"),
        "primary message should mention the missing `to` half, got: {}",
        d.primary,
    );
}

/// `connect FROM.PORT to` has the `to` keyword but no `TO.PORT`. The
/// diagnostic is anchored at a zero-width span right after the `to`
/// token, mirroring the missing-from-half cursor placement.
#[test]
fn ca_4_to_without_target_emits_arity_anchored_after_to() {
    let src = format!("{PROLOGUE}connect a.entry to path=@gravel\n");
    let arity = arity_only(diagnose(&src));
    assert_eq!(arity.len(), 1, "got: {arity:#?}");
    let d = &arity[0];
    let to_end = src.find("a.entry to").expect("from + to present") + "a.entry to".len();
    assert_eq!(
        d.span.start, to_end,
        "primary span should start at the byte after the `to` token",
    );
    assert_eq!(
        d.span.end, to_end,
        "primary span should be zero-width (a cursor, not a range)",
    );
    assert!(
        d.primary.to_lowercase().contains("port"),
        "primary message should mention the missing port, got: {}",
        d.primary,
    );
}

/// `connect FROM.PORT xxx TO.PORT` has all three positional slots filled
/// but the middle keyword is not the literal `to`. The diagnostic is
/// anchored at the offending token, and the message names the expected
/// keyword so the user can correct it without re-reading the spec.
#[test]
fn ca_5_wrong_separator_emits_arity_anchored_at_middle_token() {
    let src = format!("{PROLOGUE}connect a.entry xxx b.entry path=@gravel\n");
    let arity = arity_only(diagnose(&src));
    assert_eq!(arity.len(), 1, "got: {arity:#?}");
    let d = &arity[0];
    assert_eq!(
        slice(&src, d),
        "xxx",
        "primary span should cover the offending separator",
    );
    assert!(
        d.primary.contains("`to`") || d.primary.contains("\"to\""),
        "primary message should call out the expected `to`, got: {}",
        d.primary,
    );
}

/// `connect FROM.PORT xxx` (only two positionals and the middle one is
/// not `to`) reports the wrong-separator diagnostic rather than the
/// missing-target one — the user needs to fix the separator first, and
/// surfacing two findings for what reads as a single row would be
/// noise.
#[test]
fn ca_6_wrong_separator_with_missing_target_prefers_separator_message() {
    let src = format!("{PROLOGUE}connect a.entry xxx path=@gravel\n");
    let arity = arity_only(diagnose(&src));
    assert_eq!(arity.len(), 1, "got: {arity:#?}");
    let d = &arity[0];
    assert_eq!(slice(&src, d), "xxx");
    assert!(
        d.primary.contains("`to`") || d.primary.contains("\"to\""),
        "primary message should call out the expected `to`, got: {}",
        d.primary,
    );
}

/// `connect FROM.PORT to TO.PORT EXTRA` carries an extra positional
/// past the `to TO.PORT` shape. The grammar caps at three positionals
/// (anything else belongs in `key=value` arguments), so this is a
/// silent-failure shape: without the diagnostic the resolver would
/// read `positional[0..3]` and drop `EXTRA` on the floor, leaving the
/// author with one less walkway than they wrote. The pass underlines
/// the run of extras as a single span so the fix surface is the whole
/// offending suffix.
#[test]
fn ca_7_over_arity_emits_arity_anchored_at_extras() {
    let src = format!("{PROLOGUE}connect a.entry to b.entry c.exit path=@gravel\n");
    let arity = arity_only(diagnose(&src));
    assert_eq!(arity.len(), 1, "got: {arity:#?}");
    let d = &arity[0];
    assert_eq!(
        slice(&src, d),
        "c.exit",
        "primary span should cover the trailing extra(s)",
    );
    assert!(
        d.primary.contains("extra"),
        "primary message should call out the over-arity, got: {}",
        d.primary,
    );
}

/// Two trailing extras land in one diagnostic spanning the whole
/// suffix. Surfacing one diagnostic per extra would noise up a row
/// that reads as a single mistake.
#[test]
fn ca_8_over_arity_with_two_extras_underlines_the_full_run() {
    let src = format!("{PROLOGUE}connect a.entry to b.entry c.exit d.exit path=@gravel\n");
    let arity = arity_only(diagnose(&src));
    assert_eq!(arity.len(), 1, "got: {arity:#?}");
    let d = &arity[0];
    assert_eq!(
        slice(&src, d),
        "c.exit d.exit",
        "primary span should cover both extras as one run",
    );
}

/// Existing examples must continue to pass the arity check. Pins the
/// regression surface — a future change to the pass that over-fires
/// would break `cargo run -p cairn-lang-cli -- check examples/...`.
#[test]
fn ca_9_examples_village_and_l_walkway_are_arity_clean() {
    for path in ["examples/village.crn", "examples/l-walkway.crn"] {
        let src = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(path),
        )
        .unwrap_or_else(|e| panic!("read {path}: {e}"));
        let arity = arity_only(diagnose(&src));
        assert!(
            arity.is_empty(),
            "{path} must be arity-clean, got: {arity:#?}",
        );
    }
}
