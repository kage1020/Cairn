//! Acceptance tests for `E_DUPLICATE_SELECTOR`.
//!
//! A theme's selector rows merge into one overlay per member. `resolve`
//! walks the rows in source order and, for each one the member matches,
//! inserts every binding into the member's `selector_extras` — so a key
//! bound by two rows keeps the later value. When the two rows select the
//! *same* members there is no member left anywhere that reads the earlier
//! one: the binding the author wrote is unreachable. `check::duplicate`
//! reported a repeated `slot NAME ->` line and a key repeated inside one
//! row's brackets, and nothing about the row as a whole, so `cairn check`
//! exited 0 on it.
//!
//! Two shapes are deliberately not findings.
//!
//! - **Disjoint bindings.** Two rows with one selector and no key in common
//!   compose: every binding reaches every member both rows select and
//!   nothing is displaced. Splitting a long binding list over two lines is
//!   a thing an author means to do, and this is the same reason `@requires`
//!   is exempt from `E_DUPLICATE_HEADER` in the same pass.
//! - **Different attributes.** `window[class=small]` and
//!   `window[class=small,side=front]` overlap without coinciding — a member
//!   the first selects and the second does not still reads the first row's
//!   binding. Which of two *overlapping* rows wins is the cascade, and the
//!   cascade is source order by design; only a coincidence is a duplicate.

use cairn_lang_core::{Diagnostic, DiagnosticCode, Severity, check, lower, parse};

fn diagnose(source: &str) -> Vec<Diagnostic> {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    check(&module, &ir, None)
}

fn codes(source: &str) -> Vec<&'static str> {
    diagnose(source).iter().map(|d| d.code.as_str()).collect()
}

fn selector_only(source: &str) -> Vec<Diagnostic> {
    diagnose(source)
        .into_iter()
        .filter(|d| d.code == DiagnosticCode::DuplicateSelector)
        .collect()
}

fn one(source: &str) -> Diagnostic {
    let mut found = selector_only(source);
    assert_eq!(found.len(), 1, "expected one finding, got {found:#?}");
    found.remove(0)
}

fn notes(diag: &Diagnostic) -> Vec<&str> {
    diag.notes.iter().map(|n| n.message.as_str()).collect()
}

/// The selector rows are the variable; the struct below them carries one
/// `window` every row in this file selects. A control that reports nothing
/// is then a control whose rows *matched* and were still judged distinct,
/// rather than one whose rows reached no member at all.
fn theme_with(rows: &str) -> String {
    format!(
        "theme medieval:\n  \
         slot floor -> @oak_planks\n  \
         slot glass -> @glass_pane\n\
         {rows}\n\
         struct cottage size=9x7\n  \
         floor  mat_slot=floor\n  \
         window id=front class=small side=front offset=2 y=2 size=2x2 mat_slot=glass\n"
    )
}

/// The reported row is the later one — the line the author edits — and the
/// finding is an error, like every other duplicate this pass reports.
#[test]
fn ds_1_the_rebinding_row_is_reported_once_as_an_error() {
    let src = theme_with(
        "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@dark_oak_wood\n",
    );
    let d = one(&src);
    assert_eq!(d.severity(), Severity::Error);
    assert_eq!(
        &src[d.span.clone()],
        "window[class=small] -> frame=@dark_oak_wood",
    );
}

/// Every arity of the rebound-key list, with the sentence each renders.
/// Table-driven because the join has three branches and the serial comma is
/// a three-or-more rule: the two-key arm is the one that reads
/// `` `frame=`, and `sill=` `` when the branch is written as one `join`.
#[test]
fn ds_2_the_rebound_key_list_reads_at_every_arity() {
    for (second, listed) in [
        ("frame=@dark_oak_wood", "`frame=`"),
        (
            "frame=@dark_oak_wood sill=@stone_slab",
            "`frame=` and `sill=`",
        ),
        (
            "frame=@dark_oak_wood sill=@stone_slab trim=@brick",
            "`frame=`, `sill=`, and `trim=`",
        ),
    ] {
        let src = theme_with(&format!(
            "  window[class=small] -> frame=@spruce_wood sill=@oak_slab trim=@stone\n  \
             window[class=small] -> {second}\n"
        ));
        let d = one(&src);
        assert_eq!(
            d.primary,
            format!(
                "`window[...]` in theme `medieval` selects the same members as an earlier row and rebinds {listed}"
            ),
            "for `{second}`",
        );
    }
}

/// The notes: where the displaced binding is, why nothing can still read
/// it, and what to do. The first is anchored so an editor can jump to the
/// line whose value is being thrown away.
#[test]
fn ds_3_the_notes_point_at_the_displaced_binding_and_say_what_to_do() {
    let src = theme_with(
        "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@dark_oak_wood\n",
    );
    let d = one(&src);
    let anchored = d.notes[0]
        .span
        .as_ref()
        .expect("the first note points at the row being displaced");
    assert_eq!(
        &src[anchored.clone()],
        "window[class=small] -> frame=@spruce_wood"
    );
    assert_eq!(
        notes(&d),
        vec![
            "`frame=` bound here",
            "rows with the same attributes match exactly the same members, and bindings merge in source order, so what every member reads is this row's `frame=`",
            "merge the rows, or narrow one selector so they pick different members",
        ],
    );
    assert!(
        d.notes[1].span.is_none() && d.notes[2].span.is_none(),
        "the explanation and the repair are about the pair, not about a line: {:#?}",
        d.notes,
    );
}

/// Notes are one per displaced *row*, not one per displaced key. Two keys
/// taken from one row read as one sentence about that line; two keys taken
/// from two rows are two notes, each naming only what its own line lost.
#[test]
fn ds_3b_the_anchored_notes_are_one_per_displaced_row() {
    let from_one_row = theme_with(
        "  window[class=small] -> frame=@spruce_wood sill=@oak_slab\n  \
         window[class=small] -> frame=@dark_oak_wood sill=@stone_slab\n",
    );
    let d = one(&from_one_row);
    assert_eq!(
        notes(&d)[0],
        "`frame=` and `sill=` bound here",
        "one line lost both, so naming it twice would be two notes about one edit",
    );
    assert_eq!(d.notes.len(), 3, "and no second anchor: {:#?}", d.notes);

    let from_two_rows = theme_with(
        "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> sill=@oak_slab\n  \
         window[class=small] -> frame=@dark_oak_wood sill=@stone_slab\n",
    );
    let d = one(&from_two_rows);
    assert_eq!(notes(&d)[0], "`frame=` bound here");
    assert_eq!(notes(&d)[1], "`sill=` bound here");
    assert_eq!(
        &from_two_rows[d.notes[1].span.clone().expect("anchored note")],
        "window[class=small] -> sill=@oak_slab",
    );
}

/// Attribute order is not part of the selector. `window[class=small,side=front]`
/// and `window[side=front,class=small]` are the same filter, because the
/// matcher tests every attribute independently.
#[test]
fn ds_4_attribute_order_does_not_make_two_selectors_different() {
    let src = theme_with(
        "  window[class=small,side=front] -> frame=@spruce_wood\n  \
         window[side=front,class=small] -> frame=@dark_oak_wood\n",
    );
    assert!(one(&src).primary.contains("rebinds `frame=`"));
}

/// Whether `small` and `"small"` are the same attribute is a property of
/// the *matcher*, not of this pass, and it differs by key: `id` / `class` /
/// `mat_slot` are compared as label text, everything else by value kind.
/// Both halves check the matcher's own answer alongside the finding —
/// `E_THEME_SELECTOR_UNMATCHED` is the matcher saying, in the second half,
/// that the quoted row reaches no member the bare one reaches.
#[test]
fn ds_5_value_form_matters_exactly_where_the_matcher_says_it_does() {
    // `id` / `class` / `mat_slot` are lifted onto the member during
    // lowering and compared as label text, so the quotes are not part of
    // the filter and the rows coincide.
    for attr in ["id=front", "class=small", "mat_slot=glass"] {
        let src = theme_with(&rows_quoting(attr));
        assert_eq!(
            codes(&src),
            vec!["E_DUPLICATE_SELECTOR"],
            "a quoted `{attr}` selects the same members, and nothing went unmatched",
        );
    }
    // Every other attribute stays a generic `key=value` and is compared by
    // value kind, where the two spellings are two values. The lone
    // `E_THEME_SELECTOR_UNMATCHED` is the matcher's own answer that the
    // quoted row reaches nothing the bare one reaches.
    for attr in ["side=front", "offset=2"] {
        let src = theme_with(&rows_quoting(attr));
        assert_eq!(
            codes(&src),
            vec!["E_THEME_SELECTOR_UNMATCHED"],
            "a quoted `{attr}` selects nothing the bare one selects, so the rows are not a pair",
        );
    }
}

/// Two rows binding one key, filtering on `attr` and on `attr` with its
/// value quoted.
fn rows_quoting(attr: &str) -> String {
    let (key, value) = attr.split_once('=').expect("`key=value`");
    format!(
        "  window[{attr}] -> frame=@spruce_wood\n  \
         window[{key}=\"{value}\"] -> frame=@dark_oak_wood\n"
    )
}

/// A selector that differs in any attribute is a different selector, even
/// when one row's members are a subset of the other's: the members only the
/// wider row selects still read its binding.
#[test]
fn ds_6_rows_with_different_attributes_are_not_a_pair() {
    for second in [
        "window[class=large] -> frame=@dark_oak_wood",
        "window[class=small,side=front] -> frame=@dark_oak_wood",
        "window[] -> frame=@dark_oak_wood",
    ] {
        let src = theme_with(&format!(
            "  window[class=small] -> frame=@spruce_wood\n  {second}\n"
        ));
        assert!(
            selector_only(&src).is_empty(),
            "`{second}` selects a different set: {:#?}",
            selector_only(&src),
        );
    }
}

/// Two rows with one selector and no key in common compose. Nothing is
/// displaced, so nothing is reported — the same exemption `@requires` has
/// from `E_DUPLICATE_HEADER`.
#[test]
fn ds_7_rows_that_bind_different_keys_compose() {
    let src = theme_with(
        "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> sill=@stone_slab\n",
    );
    assert!(
        selector_only(&src).is_empty(),
        "got {:#?}",
        selector_only(&src),
    );
}

/// Only the shared keys are named. A row that rebinds one key and adds
/// another is half a duplicate, and the message has to say which half.
#[test]
fn ds_8_only_the_keys_both_rows_bind_are_listed() {
    let src = theme_with(
        "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@dark_oak_wood sill=@stone_slab\n",
    );
    let d = one(&src);
    assert!(
        d.primary.ends_with("rebinds `frame=`"),
        "`sill=` is added, not rebound: {}",
        d.primary,
    );
}

/// With three rows the note points at the row whose value is actually being
/// replaced, which is the most recent row to bind the key rather than the
/// first row of the group. Pointing at the first would name a value that
/// was already gone. The middle row displaces nothing and says nothing.
#[test]
fn ds_9_the_note_names_the_binding_that_is_actually_replaced() {
    let src = theme_with(
        "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@birch_wood\n  \
         window[class=small] -> frame=@dark_oak_wood\n",
    );
    let found = selector_only(&src);
    assert_eq!(
        found.len(),
        2,
        "rows two and three each displace one: {found:#?}"
    );
    let anchors: Vec<&str> = found
        .iter()
        .map(|d| &src[d.notes[0].span.clone().expect("anchored note")])
        .collect();
    assert_eq!(
        anchors,
        vec![
            "window[class=small] -> frame=@spruce_wood",
            "window[class=small] -> frame=@birch_wood",
        ],
    );
}

/// A row between two duplicates that binds a key neither of them binds is
/// not part of the pair, and does not become the anchor.
#[test]
fn ds_10_an_intervening_row_binding_another_key_is_not_reported() {
    let src = theme_with(
        "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> sill=@stone_slab\n  \
         window[class=small] -> frame=@dark_oak_wood\n",
    );
    let d = one(&src);
    assert_eq!(
        &src[d.notes[0].span.clone().expect("anchored note")],
        "window[class=small] -> frame=@spruce_wood",
    );
}

/// Selectors are scoped to their theme, so two themes binding the same key
/// with the same filter are two independent overlays and neither displaces
/// the other.
#[test]
fn ds_11_identical_rows_in_two_themes_are_not_a_pair() {
    let src = "theme medieval:\n  \
         slot glass -> @glass_pane\n  \
         window[class=small] -> frame=@spruce_wood\n\n\
         theme modern:\n  \
         slot glass -> @white_stained_glass\n  \
         window[class=small] -> frame=@dark_oak_wood\n\n\
         struct cottage size=9x7\n  \
         window class=small side=front offset=2 y=2 size=2x2 mat_slot=glass\n";
    assert!(
        selector_only(src).is_empty(),
        "got {:#?}",
        selector_only(src),
    );
}

/// The keyword is half the filter: two rows that agree on attributes but
/// name different roles never select one member in common.
#[test]
fn ds_12_rows_with_different_keywords_are_not_a_pair() {
    let src = theme_with(
        "  window[class=small] -> frame=@spruce_wood\n  \
         door[class=small] -> frame=@dark_oak_wood\n",
    );
    assert!(
        selector_only(&src).is_empty(),
        "got {:#?}",
        selector_only(&src),
    );
}

/// The payload carries the rebound keys, so a quick-fix does not parse the
/// sentence back apart — what `spec/lint.md` §11.2 exists to stop.
#[test]
fn ds_13_the_payload_lists_the_rebound_keys() {
    let src = theme_with(
        "  window[class=small] -> frame=@spruce_wood sill=@oak_slab\n  \
         window[class=small] -> sill=@stone_slab frame=@dark_oak_wood\n",
    );
    let d = one(&src);
    let value = serde_json::to_value(&d.data).expect("serialise payload");
    assert_eq!(
        value,
        serde_json::json!({"kind": "duplicate_selector", "rebound": ["sill", "frame"]}),
        "in the order the message lists them, which is the offending row's order",
    );
}

/// The finding coexists with the arg-level duplicate inside one row rather
/// than masking it: the two are different scopes, and both are on lines the
/// author has to edit.
#[test]
fn ds_14_it_coexists_with_a_key_repeated_inside_one_row() {
    let src = theme_with(
        "  window[class=small] -> frame=@spruce_wood\n  \
         window[class=small] -> frame=@birch_wood frame=@dark_oak_wood\n",
    );
    let mut seen = codes(&src);
    seen.sort_unstable();
    assert_eq!(seen, vec!["E_DUPLICATE_ARG", "E_DUPLICATE_SELECTOR"]);
}

/// Two coinciding rows that reach no member are still a pair. The pass is
/// syntactic — it reads the theme body, not the struct bodies — and the
/// rows are as redundant with no member as with one. `W_`-style reporting
/// of the dead rows is `E_THEME_SELECTOR_UNMATCHED`'s job, and both fire.
#[test]
fn ds_15_a_pair_that_matches_nothing_is_still_a_pair() {
    let src = theme_with(
        "  window[class=enormous] -> frame=@spruce_wood\n  \
         window[class=enormous] -> frame=@dark_oak_wood\n",
    );
    let mut seen = codes(&src);
    seen.sort_unstable();
    assert_eq!(
        seen,
        vec![
            "E_DUPLICATE_SELECTOR",
            "E_THEME_SELECTOR_UNMATCHED",
            "E_THEME_SELECTOR_UNMATCHED",
        ],
    );
    assert!(
        !one(&src).primary.contains("no member"),
        "the primary states the coincidence, not a loss it cannot know happened: {}",
        one(&src).primary,
    );
}
