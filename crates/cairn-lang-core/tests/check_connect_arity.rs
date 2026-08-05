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
//!
//! "Shape" covers the two endpoint slots as well as the count: the
//! grammar admits exactly a one-dot `<place>.<port>` reference on each
//! side of `to`. Every other value the parser accepts in a positional
//! slot — a bare identifier, a literal, a material token, a quoted
//! string, a list, a reference with a second dot — reaches the resolver,
//! which drops it and lays no walkway. The endpoint cases below pin the
//! diagnostic that keeps that drop from being silent.

use cairn_lang_core::{Diagnostic, DiagnosticCode, Severity, check, lower, parse};

fn diagnose(source: &str) -> Vec<Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}"));
    let ir = lower(&module);
    check(&module, &ir, None)
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

/// Byte range of the slot that follows `prefix` on the `connect` row.
///
/// Locating by the text that precedes the slot rather than by the slot
/// text itself keeps a one-character endpoint (`a`, `1`) from matching
/// an earlier occurrence — `a` appears in `place id=a` and again inside
/// the from-side `a.entry` before it appears as the to-side endpoint.
/// The `starts_with` assertion makes a mis-specified prefix a test
/// failure rather than a silently misplaced expectation.
fn slot_span(src: &str, prefix: &str, text: &str) -> std::ops::Range<usize> {
    let start = src
        .rfind(prefix)
        .unwrap_or_else(|| panic!("`{prefix}` is not in:\n{src}"))
        + prefix.len();
    assert!(
        src[start..].starts_with(text),
        "expected `{text}` right after `{prefix}`, found `{}`",
        &src[start..src.len().min(start + text.len() + 8)],
    );
    start..start + text.len()
}

fn from_slot(src: &str, text: &str) -> std::ops::Range<usize> {
    slot_span(src, "connect ", text)
}

fn to_slot(src: &str, from: &str, text: &str) -> std::ops::Range<usize> {
    slot_span(src, &format!("connect {from} to "), text)
}

fn only_endpoint_diag(src: &str) -> Diagnostic {
    let mut arity = arity_only(diagnose(src));
    assert_eq!(arity.len(), 1, "expected one diagnostic, got: {arity:#?}");
    arity.remove(0)
}

/// Every non-reference value the parser accepts in the from-slot. Each
/// entry is `(source text, expected kind word)`; the kind word is
/// `Value::kind_name`, which the message quotes so the author can tell
/// `1` (integer) from `"1"` (string) without opening the file.
///
/// A list is absent here on purpose: `connect [a.entry] to b.entry`
/// fails in the parser before this pass runs, because the `[` in the
/// first positional slot is read as the start of a `key=[...]`
/// argument. The to-slot covers the list shape instead — see
/// `ca_11_to_side_non_reference_endpoints_are_flagged`.
const NON_REFERENCE_ENDPOINTS: &[(&str, &str)] = &[
    ("a", "identifier"),
    ("true", "boolean"),
    ("1", "integer"),
    ("9x7", "size"),
    ("@gravel", "token"),
    ("\"a.entry\"", "string"),
];

/// A from-slot that is not a `<place>.<port>` reference earns one
/// `E_CONNECT_ARITY` anchored on the offending value. Before this pass
/// grew endpoint validation, every one of these rows checked clean and
/// lowered to zero walkways — the exact silent-drop `E_CONNECT_ARITY`
/// exists to prevent, arrived at through the endpoint slot rather than
/// the positional count.
#[test]
fn ca_10_from_side_non_reference_endpoints_are_flagged() {
    for (text, kind) in NON_REFERENCE_ENDPOINTS {
        let src = format!("{PROLOGUE}connect {text} to b.entry path=@gravel\n");
        let d = only_endpoint_diag(&src);
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(
            d.span,
            from_slot(&src, text),
            "span should underline the offending endpoint `{text}`, got {:?}",
            slice(&src, &d),
        );
        assert!(
            d.primary.contains("`<from>.<port>`"),
            "message should name the from-side shape for `{text}`, got: {}",
            d.primary,
        );
        assert!(
            d.primary.contains(kind),
            "message should name the actual kind `{kind}` for `{text}`, got: {}",
            d.primary,
        );
    }
}

/// Mirror of the from-slot matrix on the to-slot, plus the list shape
/// that only reaches this pass from the to-slot. The two slots are
/// separate call sites, so a fix applied to one and not the other would
/// leave half the silent drops in place.
#[test]
fn ca_11_to_side_non_reference_endpoints_are_flagged() {
    let cases: Vec<(&str, &str)> = NON_REFERENCE_ENDPOINTS
        .iter()
        .copied()
        .chain(std::iter::once(("[b.entry]", "list")))
        .collect();
    for (text, kind) in cases {
        let src = format!("{PROLOGUE}connect a.entry to {text} path=@gravel\n");
        let d = only_endpoint_diag(&src);
        assert_eq!(
            d.span,
            to_slot(&src, "a.entry", text),
            "span should underline the offending endpoint `{text}`, got {:?}",
            slice(&src, &d),
        );
        assert!(
            d.primary.contains("`<to>.<port>`"),
            "message should name the to-side shape for `{text}`, got: {}",
            d.primary,
        );
        assert!(
            d.primary.contains(kind),
            "message should name the actual kind `{kind}` for `{text}`, got: {}",
            d.primary,
        );
    }
}

/// Both halves broken earns one diagnostic per half, in source order.
/// The two endpoints are independent fix sites — unlike the separator
/// case, neither has to be corrected before the other is interpretable
/// — so reporting only the first would send the author round the
/// edit-check loop twice for one line.
#[test]
fn ca_12_both_endpoints_broken_earn_one_diagnostic_each() {
    let src = format!("{PROLOGUE}connect 1 to 2 path=@gravel\n");
    let arity = arity_only(diagnose(&src));
    assert_eq!(arity.len(), 2, "got: {arity:#?}");
    assert_eq!(arity[0].span, from_slot(&src, "1"));
    assert_eq!(arity[1].span, to_slot(&src, "1", "2"));
    assert!(arity[0].primary.contains("`<from>.<port>`"));
    assert!(arity[1].primary.contains("`<to>.<port>`"));
}

/// A reference with a second dot is worse than a dropped row: the
/// resolver used to read `tail()[0]` and ignore the rest, so
/// `a.entry.typo` laid the walkway `a.entry` would have laid. The
/// author's mistake compiled into a valid-looking build. The message
/// names the segment count so the extra dot is visible in the log
/// without the source alongside.
#[test]
fn ca_13_from_side_reference_with_extra_segment_is_flagged() {
    let src = format!("{PROLOGUE}connect a.entry.x to b.entry path=@gravel\n");
    let d = only_endpoint_diag(&src);
    assert_eq!(d.span, from_slot(&src, "a.entry.x"));
    assert!(
        d.primary.contains("a.entry.x") && d.primary.contains('3'),
        "message should quote the reference and its segment count, got: {}",
        d.primary,
    );
}

/// The to-slot mirror of the extra-segment case.
#[test]
fn ca_14_to_side_reference_with_extra_segment_is_flagged() {
    let src = format!("{PROLOGUE}connect a.entry to b.entry.x path=@gravel\n");
    let d = only_endpoint_diag(&src);
    assert_eq!(d.span, to_slot(&src, "a.entry", "b.entry.x"));
    assert!(
        d.primary.contains("b.entry.x") && d.primary.contains('3'),
        "message should quote the reference and its segment count, got: {}",
        d.primary,
    );
}

/// Trailing extras and broken endpoints are independent mistakes, so
/// the row reports all three. This is the one arm where the pass emits
/// more than one finding for a row without the halves being separate
/// slots: removing `c.exit` does not make `1` or `2` a port reference,
/// so withholding the endpoint findings would only hide work the author
/// still has to do.
#[test]
fn ca_15_over_arity_also_reports_broken_endpoints() {
    let src = format!("{PROLOGUE}connect 1 to 2 c.exit path=@gravel\n");
    let arity = arity_only(diagnose(&src));
    assert_eq!(arity.len(), 3, "got: {arity:#?}");
    let spans: Vec<_> = arity.iter().map(|d| slice(&src, d)).collect();
    assert_eq!(spans, vec!["1", "2", "c.exit"]);
}

/// A wrong separator suppresses the endpoint findings: until the
/// author writes `to`, which positional is the target is a guess, and
/// the pass would be reporting on slots it cannot yet identify. Pins
/// the boundary of the previous test's "report everything" rule.
#[test]
fn ca_16_wrong_separator_suppresses_endpoint_findings() {
    let src = format!("{PROLOGUE}connect 1 xxx 2 path=@gravel\n");
    let d = only_endpoint_diag(&src);
    assert_eq!(slice(&src, &d), "xxx");
}

/// A row missing its second half reports the missing half only. The
/// same reasoning as the separator case: the author's next edit adds
/// the `to <to>.<port>` text, and an endpoint complaint about the half
/// that is already written would be answered by a line they have not
/// finished typing.
#[test]
fn ca_17_missing_half_suppresses_endpoint_findings() {
    let src = format!("{PROLOGUE}connect 1 path=@gravel\n");
    let d = only_endpoint_diag(&src);
    assert!(
        d.primary.contains("missing"),
        "expected the missing-half message, got: {}",
        d.primary,
    );
}

/// `connect a.entry "to" b.entry` used to render as ``expected `to`
/// ... got `to` `` because the message printed a string literal's
/// contents verbatim. `spec/lint.md` requires messages an author can
/// act on without re-reading the source; a message that asks for the
/// thing it says it received cannot be acted on at all. The quotes now
/// survive into the message, and the bare-keyword rendering stays
/// distinct.
#[test]
fn ca_18_string_separator_renders_distinctly_from_the_bare_keyword() {
    let quoted = format!("{PROLOGUE}connect a.entry \"to\" b.entry path=@gravel\n");
    let d = only_endpoint_diag(&quoted);
    assert!(
        d.primary.contains("\"to\""),
        "a string separator must keep its quotes in the message, got: {}",
        d.primary,
    );
    let bare = format!("{PROLOGUE}connect a.entry xxx b.entry path=@gravel\n");
    let other = only_endpoint_diag(&bare);
    assert_ne!(
        d.primary, other.primary,
        "the quoted and bare separators must not render identically",
    );
}

/// `connect` lowers to `MemberRole::Connect` in every scope, so a row
/// inside a `def` body reaches this pass too (see the pass doc). The
/// endpoint check must fire there as well — a stray `connect` with a
/// broken endpoint is no more useful than a well-formed one in the
/// wrong scope, and the arity arms already report at this position.
#[test]
fn ca_19_endpoint_check_reaches_connect_rows_outside_a_site() {
    let src = "def d size=3x3:\n  \
floor id=floor\n  \
connect 1 to 2 path=@gravel\n";
    let arity = arity_only(diagnose(src));
    assert_eq!(arity.len(), 2, "got: {arity:#?}");
}

/// The notes carry the shape-specific repair, not just the generic
/// example: a bare identifier is missing its port, a quoted reference
/// only has to lose its quotes, and an extra dot has to go. Each is a
/// single edit the author can apply from the message alone, which is
/// what `spec/lint.md` asks of a diagnostic.
#[test]
fn ca_20_endpoint_notes_name_the_repair_for_the_shape_at_hand() {
    let cases = [
        ("a", "a.<port>"),
        ("\"a.entry\"", "quotes"),
        ("a.entry.x", "one dot"),
    ];
    for (text, expected) in cases {
        let src = format!("{PROLOGUE}connect {text} to b.entry path=@gravel\n");
        let d = only_endpoint_diag(&src);
        assert!(
            d.notes.iter().any(|n| n.message.contains(expected)),
            "endpoint `{text}` should carry a note mentioning `{expected}`, got: {:#?}",
            d.notes,
        );
    }
}
