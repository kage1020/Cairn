//! `E_CONNECT_ARITY`: a `connect` row is exactly `FROM.PORT to TO.PORT
//! [path=@MATERIAL]`, with a one-dot `<place>.<port>` reference on each side of
//! `to`. Any other count or endpoint shape is reported at the positional that
//! is missing or wrong, because the resolver lays no walkway for such a row and
//! says nothing.

use cairn_lang_core::{DiagnosticCode, Severity};

use crate::common::{codes, notes, of_code, one_of_code, slice};

const PROLOGUE: &str = "struct hut size=3x3\n  \
floor\n  \
door id=entry at=center\n\n\
site h:\n  \
place id=a use=hut at=origin\n  \
place id=b use=hut east_of=a gap=4\n  ";

/// Without this, the tests below could not tell a real finding from a pass
/// that always fires.
#[test]
fn well_formed_connect_emits_no_arity_diagnostic() {
    let src = format!("{PROLOGUE}connect a.entry to b.entry path=@gravel\n");
    let arity = of_code(&src, DiagnosticCode::ConnectArity);
    assert!(
        arity.is_empty(),
        "well-formed connect must not trigger arity pass, got: {arity:#?}",
    );
}

#[test]
fn bare_connect_emits_arity_with_keyword_span() {
    let src = format!("{PROLOGUE}connect path=@gravel\n");
    let arity = of_code(&src, DiagnosticCode::ConnectArity);
    assert_eq!(arity.len(), 1, "got: {arity:#?}");
    let d = &arity[0];
    assert_eq!(d.code, DiagnosticCode::ConnectArity);
    assert_eq!(d.severity(), Severity::Error);
    assert!(
        slice(&src, d).starts_with("connect"),
        "primary span should cover the `connect` row, got: {:?}",
        slice(&src, d),
    );
    assert_eq!(
        d.primary, "`connect` requires `<from>.<port> to <to>.<port>`",
        "the primary names the keyword and the shape it takes",
    );
    assert_eq!(
        notes(d),
        ["example: `connect home1.entry to home2.entry path=@gravel`"],
        "the one note is an example of the whole shape",
    );
}

/// Zero-width, so the `file:L:C` pointer lands where the missing half goes.
#[test]
fn only_from_emits_arity_anchored_after_from() {
    let src = format!("{PROLOGUE}connect a.entry path=@gravel\n");
    let arity = of_code(&src, DiagnosticCode::ConnectArity);
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
    assert_eq!(
        d.primary, "`connect` is missing the `to <to>.<port>` half",
        "the primary names the missing `to` half",
    );
}

#[test]
fn to_without_target_emits_arity_anchored_after_to() {
    let src = format!("{PROLOGUE}connect a.entry to path=@gravel\n");
    let arity = of_code(&src, DiagnosticCode::ConnectArity);
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
    assert_eq!(
        d.primary, "`connect` is missing the `<to>.<port>` target after `to`",
        "the primary names the missing target port",
    );
}

#[test]
fn wrong_separator_emits_arity_anchored_at_middle_token() {
    let src = format!("{PROLOGUE}connect a.entry xxx b.entry path=@gravel\n");
    let arity = of_code(&src, DiagnosticCode::ConnectArity);
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

/// The separator has to be fixed first, so the missing target is not
/// reported beside it.
#[test]
fn wrong_separator_with_missing_target_prefers_separator_message() {
    let src = format!("{PROLOGUE}connect a.entry xxx path=@gravel\n");
    let arity = of_code(&src, DiagnosticCode::ConnectArity);
    assert_eq!(arity.len(), 1, "got: {arity:#?}");
    let d = &arity[0];
    assert_eq!(slice(&src, d), "xxx");
    assert!(
        d.primary.contains("`to`") || d.primary.contains("\"to\""),
        "primary message should call out the expected `to`, got: {}",
        d.primary,
    );
}

/// The resolver reads `positional[0..3]`, so without this the extra would
/// be dropped silently.
#[test]
fn over_arity_emits_arity_anchored_at_extras() {
    let src = format!("{PROLOGUE}connect a.entry to b.entry c.exit path=@gravel\n");
    let arity = of_code(&src, DiagnosticCode::ConnectArity);
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

/// One finding, because the row reads as a single mistake.
#[test]
fn over_arity_with_two_extras_underlines_the_full_run() {
    let src = format!("{PROLOGUE}connect a.entry to b.entry c.exit d.exit path=@gravel\n");
    let arity = of_code(&src, DiagnosticCode::ConnectArity);
    assert_eq!(arity.len(), 1, "got: {arity:#?}");
    let d = &arity[0];
    assert_eq!(
        slice(&src, d),
        "c.exit d.exit",
        "primary span should cover both extras as one run",
    );
}

/// Byte range of the slot that follows `prefix` on the `connect` row.
///
/// Located by the text before the slot, so a one-character endpoint such
/// as `a` does not match an earlier occurrence.
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

/// Every non-reference value the parser accepts in the from-slot, as
/// `(source text, Value::kind_name)`.
///
/// No list: a `[` after the keyword opens a selector, whose body is a
/// `key=value` list, so `connect [a.entry]` fails in the parser. The
/// to-slot covers lists.
const NON_REFERENCE_ENDPOINTS: &[(&str, &str)] = &[
    ("a", "identifier"),
    ("true", "boolean"),
    ("1", "integer"),
    ("9x7", "size"),
    ("@gravel", "token"),
    ("\"a.entry\"", "string"),
];

/// Each of these rows would otherwise lower to zero walkways, silently.
#[test]
fn from_side_non_reference_endpoints_are_flagged() {
    for (text, kind) in NON_REFERENCE_ENDPOINTS {
        let src = format!("{PROLOGUE}connect {text} to b.entry path=@gravel\n");
        let d = one_of_code(&src, DiagnosticCode::ConnectArity);
        assert_eq!(d.severity(), Severity::Error);
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
        assert!(
            d.primary.contains(&format!("`{text}`")),
            "message should quote the value as written for `{text}`, got: {}",
            d.primary,
        );
    }
}

/// Plus the list shape, which only reaches this pass from the to-slot.
/// The two slots are separate call sites.
#[test]
fn to_side_non_reference_endpoints_are_flagged() {
    let cases: Vec<(&str, &str)> = NON_REFERENCE_ENDPOINTS
        .iter()
        .copied()
        .chain(std::iter::once(("[b.entry]", "list")))
        .collect();
    for (text, kind) in cases {
        let src = format!("{PROLOGUE}connect a.entry to {text} path=@gravel\n");
        let d = one_of_code(&src, DiagnosticCode::ConnectArity);
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
        // A list has no bounded surface form, so it is named by kind alone.
        assert_eq!(
            d.primary.contains(&format!("`{text}`")),
            kind != "list",
            "surface form of `{text}` should appear iff it is bounded, got: {}",
            d.primary,
        );
    }
}

/// Unlike the separator, neither endpoint has to be fixed before the other
/// is interpretable.
#[test]
fn both_endpoints_broken_earn_one_diagnostic_each() {
    let src = format!("{PROLOGUE}connect 1 to 2 path=@gravel\n");
    let arity = of_code(&src, DiagnosticCode::ConnectArity);
    assert_eq!(arity.len(), 2, "got: {arity:#?}");
    assert_eq!(arity[0].span, from_slot(&src, "1"));
    assert_eq!(arity[1].span, to_slot(&src, "1", "2"));
    assert!(arity[0].primary.contains("`<from>.<port>`"));
    assert!(arity[1].primary.contains("`<to>.<port>`"));
}

/// Read up to its first segment, `a.entry.typo` would lay the walkway
/// `a.entry` lays, so the mistake would compile into a valid-looking build.
#[test]
fn from_side_reference_with_extra_segment_is_flagged() {
    let src = format!("{PROLOGUE}connect a.entry.x to b.entry path=@gravel\n");
    let d = one_of_code(&src, DiagnosticCode::ConnectArity);
    assert_eq!(d.span, from_slot(&src, "a.entry.x"));
    assert!(
        d.primary.contains("a.entry.x") && d.primary.contains("3 segments"),
        "message should quote the reference and its segment count, got: {}",
        d.primary,
    );
}

#[test]
fn to_side_reference_with_extra_segment_is_flagged() {
    let src = format!("{PROLOGUE}connect a.entry to b.entry.x path=@gravel\n");
    let d = one_of_code(&src, DiagnosticCode::ConnectArity);
    assert_eq!(d.span, to_slot(&src, "a.entry", "b.entry.x"));
    assert!(
        d.primary.contains("b.entry.x") && d.primary.contains("3 segments"),
        "message should quote the reference and its segment count, got: {}",
        d.primary,
    );
}

/// Removing `c.exit` does not make `1` or `2` a port reference, so
/// withholding the endpoint findings would only hide work.
#[test]
fn over_arity_also_reports_broken_endpoints() {
    let src = format!("{PROLOGUE}connect 1 to 2 c.exit path=@gravel\n");
    let arity = of_code(&src, DiagnosticCode::ConnectArity);
    assert_eq!(arity.len(), 3, "got: {arity:#?}");
    let spans: Vec<_> = arity.iter().map(|d| slice(&src, d)).collect();
    assert_eq!(spans, vec!["1", "2", "c.exit"]);
}

/// Until the author writes `to`, which positional is the target is a
/// guess.
#[test]
fn wrong_separator_suppresses_endpoint_findings() {
    let src = format!("{PROLOGUE}connect 1 xxx 2 path=@gravel\n");
    let d = one_of_code(&src, DiagnosticCode::ConnectArity);
    assert_eq!(slice(&src, &d), "xxx");
}

/// The author's next edit is the missing half, on a line not yet
/// finished.
#[test]
fn missing_half_suppresses_endpoint_findings() {
    let src = format!("{PROLOGUE}connect 1 path=@gravel\n");
    let d = one_of_code(&src, DiagnosticCode::ConnectArity);
    assert!(
        d.primary.contains("missing"),
        "expected the missing-half message, got: {}",
        d.primary,
    );
}

/// A separate arm from the missing half, reached by a different
/// positional count.
#[test]
fn to_without_target_suppresses_endpoint_findings() {
    let src = format!(
        "{PROLOGUE}connect 1 to path=@gravel
"
    );
    let d = one_of_code(&src, DiagnosticCode::ConnectArity);
    assert!(
        d.primary.contains("missing"),
        "expected the missing-target message, got: {}",
        d.primary,
    );
}

/// Otherwise the message would read ``expected `to` ... got `to` ``.
#[test]
fn string_separator_renders_distinctly_from_the_bare_keyword() {
    let quoted = format!("{PROLOGUE}connect a.entry \"to\" b.entry path=@gravel\n");
    let d = one_of_code(&quoted, DiagnosticCode::ConnectArity);
    assert!(
        d.primary.contains("\"to\""),
        "a string separator must keep its quotes in the message, got: {}",
        d.primary,
    );
    let bare = format!("{PROLOGUE}connect a.entry xxx b.entry path=@gravel\n");
    let other = one_of_code(&bare, DiagnosticCode::ConnectArity);
    assert_ne!(
        d.primary, other.primary,
        "the quoted and bare separators must not render identically",
    );
}

/// `connect` lowers to `MemberRole::Connect` in every scope, so a row in a
/// `def` body reaches this pass too.
#[test]
fn endpoint_check_reaches_connect_rows_outside_a_site() {
    let src = "def d size=3x3:\n  \
floor id=floor\n  \
connect 1 to 2 path=@gravel\n";
    let arity = of_code(src, DiagnosticCode::ConnectArity);
    assert_eq!(arity.len(), 2, "got: {arity:#?}");
}

/// Each is a single edit the author can apply from the message alone.
#[test]
fn endpoint_notes_name_the_repair_for_the_shape_at_hand() {
    let cases = [
        ("a", "a.<port>"),
        ("\"a.entry\"", "quotes"),
        ("@gravel", "path=@"),
        ("a.entry.x", "one dot"),
    ];
    for (text, expected) in cases {
        let src = format!("{PROLOGUE}connect {text} to b.entry path=@gravel\n");
        let d = one_of_code(&src, DiagnosticCode::ConnectArity);
        assert!(
            d.notes.iter().any(|n| n.message.contains(expected)),
            "endpoint `{text}` should carry a note mentioning `{expected}`, got: {:#?}",
            d.notes,
        );
    }
}

/// A prologue whose places resolve.
///
/// [`PROLOGUE`]'s `place use=` names a `struct`, not a `def`, so the
/// resolver abandons each placement before it reads a `connect` endpoint.
const RESOLVING_PROLOGUE: &str = "def hut size=3x3:\n  \
floor id=floor mat_slot=floor\n  \
walls id=walls class=outer mat_slot=wall height=3\n  \
door  id=entry side=front at=center\n\n\
theme plain:\n  \
slot floor -> @oak_planks\n  \
slot wall  -> @cobblestone\n\n\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
place id=peer   use=hut theme=plain east_of=anchor gap=4\n  ";

/// The endpoint check and the resolver's endpoint guard look at the same
/// slot, and the resolver stays silent so one mistake is not reported
/// twice.
#[test]
fn one_endpoint_mistake_earns_one_diagnostic_across_both_layers() {
    let clean = format!("{RESOLVING_PROLOGUE}connect anchor.entry to peer.entry path=@gravel\n");
    assert!(
        codes(&clean).is_empty(),
        "the resolvable prologue must be clean on a well-formed row, got: {:?}",
        codes(&clean),
    );
    for row in [
        "connect anchor to peer.entry path=@gravel",
        "connect anchor.entry to peer path=@gravel",
        "connect \"anchor.entry\" to peer.entry path=@gravel",
        "connect @gravel to peer.entry path=@gravel",
        "connect anchor.entry.x to peer.entry path=@gravel",
        "connect anchor.entry to peer.entry.x path=@gravel",
    ] {
        let src = format!("{RESOLVING_PROLOGUE}{row}\n");
        assert_eq!(
            codes(&src),
            ["E_CONNECT_ARITY"],
            "`{row}` must earn exactly one diagnostic",
        );
    }
    let both = format!("{RESOLVING_PROLOGUE}connect 1 to 2 path=@gravel\n");
    assert_eq!(codes(&both), ["E_CONNECT_ARITY", "E_CONNECT_ARITY"]);
}

/// A well-shaped endpoint that names the wrong thing is the resolver's to
/// report.
#[test]
fn well_shaped_endpoints_that_do_not_resolve_stay_with_the_resolver() {
    for (row, expected) in [
        (
            "connect nosuch.entry to peer.entry path=@gravel",
            "E_UNRESOLVED_PLACE_REF",
        ),
        (
            "connect anchor.nosuch to peer.entry path=@gravel",
            "E_UNRESOLVED_PORT",
        ),
    ] {
        let src = format!("{RESOLVING_PROLOGUE}{row}\n");
        assert_eq!(codes(&src), [expected], "for `{row}`");
    }
}
