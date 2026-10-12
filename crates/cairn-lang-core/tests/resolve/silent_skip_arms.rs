//! Rows the resolver drops without a finding because a pass ahead of it in `check` reports them,
//! each pinned through `resolve` next to the signal it relies on.

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::{Diagnostic, DiagnosticCode, Severity};
use cairn_lang_core::{check, lower, parse, resolve};

use crate::common::{duo_with, lowered_with_resolver_diagnostics};

fn count(diagnostics: &[Diagnostic], code: DiagnosticCode) -> usize {
    diagnostics.iter().filter(|d| d.code == code).count()
}

fn errors(diagnostics: &[Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|d| d.severity() == Severity::Error)
        .count()
}

/// Site prologue with a single def and theme.
const PROLOGUE: &str = "@cairn 2026.06\n\n\
def hut size=3x3:\n  \
floor id=floor mat_slot=floor\n  \
walls id=walls class=outer mat_slot=wall height=3\n  \
door  id=entry side=front at=center\n\n\
theme plain:\n  \
slot floor -> @oak_planks\n  \
slot wall  -> @cobblestone\n\n";

/// Baseline for the silent arms below, so "no walkway, no finding" is not a pipeline no-op.
#[test]
fn fully_resolved_connect_lays_one_walkway_with_no_diagnostics() {
    let src = format!(
        "{PROLOGUE}\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
place id=peer   use=hut theme=plain east_of=anchor gap=4\n  \
connect anchor.entry to peer.entry path=@gravel\n"
    );
    let outcome = lowered_with_resolver_diagnostics(&src);
    assert_eq!(
        outcome.walkways.len(),
        1,
        "baseline: a well-formed connect must emit exactly one walkway",
    );
    assert_eq!(
        errors(&outcome.diagnostics),
        0,
        "baseline: no errors expected, got: {:#?}",
        outcome.diagnostics,
    );
    assert_eq!(
        count(&outcome.diagnostics, DiagnosticCode::DeferredConnect),
        0,
        "baseline: no `W_DEFERRED_CONNECT` expected for a fully-resolved row",
    );
}

#[test]
fn missing_use_cascades_w_deferred_connect_with_no_walkway() {
    let src = format!(
        "{PROLOGUE}\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
place id=silent         theme=plain east_of=anchor gap=4\n  \
connect anchor.entry to silent.entry path=@gravel\n"
    );
    let outcome = lowered_with_resolver_diagnostics(&src);
    assert_eq!(
        outcome.walkways.len(),
        0,
        "a connect targeting a `use=`-less place must drop its walkway",
    );
    assert_eq!(
        count(&outcome.diagnostics, DiagnosticCode::IncompletePlace),
        1,
        "the `place` row names its own problem, got: {:#?}",
        outcome.diagnostics,
    );
    assert_eq!(
        count(&outcome.diagnostics, DiagnosticCode::DeferredConnect),
        1,
        "and the `connect` row still says which walkway went with it",
    );
}

/// The file's single theme does not stand in for the missing key.
#[test]
fn missing_theme_cascades_w_deferred_connect_with_no_walkway() {
    let src = format!(
        "{PROLOGUE}\
site duo:\n  \
place id=anchor    use=hut theme=plain at=origin\n  \
place id=themeless use=hut             east_of=anchor gap=4\n  \
connect anchor.entry to themeless.entry path=@gravel\n"
    );
    let outcome = lowered_with_resolver_diagnostics(&src);
    assert_eq!(
        outcome.walkways.len(),
        0,
        "a connect targeting a `theme=`-less place must drop its walkway",
    );
    assert_eq!(
        count(&outcome.diagnostics, DiagnosticCode::IncompletePlace),
        1,
        "the `place` row names its own problem, got: {:#?}",
        outcome.diagnostics,
    );
    assert_eq!(
        count(&outcome.diagnostics, DiagnosticCode::DeferredConnect),
        1,
        "and the `connect` row still says which walkway went with it",
    );
}

#[test]
fn place_without_id_is_reported_once_and_cannot_cascade() {
    let src = format!(
        "{PROLOGUE}\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
place           use=hut theme=plain east_of=anchor gap=4\n"
    );
    let outcome = lowered_with_resolver_diagnostics(&src);
    assert_eq!(
        count(&outcome.diagnostics, DiagnosticCode::IncompletePlace),
        1,
        "an unnamed `place` is reported once, got: {:#?}",
        outcome.diagnostics,
    );
    assert_eq!(
        count(&outcome.diagnostics, DiagnosticCode::DeferredConnect),
        0,
        "no `connect` references the unnamed place, so the cascade must not fire",
    );
}

/// `E_CONNECT_ARITY` is `check::connect_arity`'s, which runs ahead of `resolve`.
#[test]
fn connect_with_missing_positional_silently_returns_without_diagnostics() {
    let src = format!(
        "{PROLOGUE}\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
connect anchor.entry\n"
    );
    let outcome = lowered_with_resolver_diagnostics(&src);
    assert_eq!(
        outcome.walkways.len(),
        0,
        "a `connect` row missing the `to` half must not lay a walkway",
    );
    assert_eq!(
        errors(&outcome.diagnostics),
        0,
        "arity enforcement is owned by `check::connect_arity`; the resolver-only path stays silent, got: {:#?}",
        outcome.diagnostics,
    );
    assert_eq!(
        count(&outcome.diagnostics, DiagnosticCode::DeferredConnect),
        0,
        "the cascade needs an endpoint read off the row, and there is none when the `to` half is absent",
    );
}

#[test]
fn connect_with_to_keyword_only_silently_returns_without_diagnostics() {
    let src = format!(
        "{PROLOGUE}\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
connect anchor.entry to\n"
    );
    let outcome = lowered_with_resolver_diagnostics(&src);
    assert_eq!(
        outcome.walkways.len(),
        0,
        "a `connect` row missing the target port must not lay a walkway",
    );
    assert_eq!(
        errors(&outcome.diagnostics),
        0,
        "arity enforcement is owned by `check::connect_arity`; the resolver-only path stays silent, got: {:#?}",
        outcome.diagnostics,
    );
    assert_eq!(
        count(&outcome.diagnostics, DiagnosticCode::DeferredConnect),
        0,
        "the cascade needs an endpoint read off the row, and there is none when the target port is absent",
    );
}

#[test]
fn connect_with_wrong_separator_silently_returns_without_diagnostics() {
    let src = format!(
        "{PROLOGUE}\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
place id=peer   use=hut theme=plain east_of=anchor gap=4\n  \
connect anchor.entry xxx peer.entry path=@gravel\n"
    );
    let outcome = lowered_with_resolver_diagnostics(&src);
    assert_eq!(
        outcome.walkways.len(),
        0,
        "a `connect` row with a non-`to` separator must not lay a walkway through the resolver-only path",
    );
    assert_eq!(
        errors(&outcome.diagnostics),
        0,
        "arity enforcement is owned by `check::connect_arity`; the resolver-only path stays silent, got: {:#?}",
        outcome.diagnostics,
    );
    assert_eq!(
        count(&outcome.diagnostics, DiagnosticCode::DeferredConnect),
        0,
        "the cascade needs an endpoint read off the row, and there is none when the separator is not `to`",
    );
}

#[test]
fn connect_with_non_reference_endpoint_silently_returns_without_diagnostics() {
    for row in [
        "connect anchor to peer.entry path=@gravel",
        "connect anchor.entry to peer path=@gravel",
        "connect \"anchor.entry\" to peer.entry path=@gravel",
        "connect @gravel to peer.entry path=@gravel",
        "connect 1 to 2 path=@gravel",
    ] {
        let outcome = lowered_with_resolver_diagnostics(&duo_with(row));
        assert_eq!(
            outcome.walkways.len(),
            0,
            "`{row}` must not lay a walkway through the resolver-only path",
        );
        assert_eq!(
            errors(&outcome.diagnostics),
            0,
            "endpoint enforcement is owned by `check::connect_arity`; the resolver-only path stays silent for `{row}`, got: {:#?}",
            outcome.diagnostics,
        );
    }
}

#[test]
fn connect_with_extra_positionals_lays_no_walkway() {
    let outcome = lowered_with_resolver_diagnostics(&duo_with(
        "connect anchor.entry to peer.entry c.exit path=@gravel",
    ));
    assert_eq!(
        outcome.walkways.len(),
        0,
        "an over-arity row must not lay a walkway through the resolver-only path",
    );
    assert_eq!(
        errors(&outcome.diagnostics),
        0,
        "arity enforcement is owned by `check::connect_arity`; the resolver-only path stays silent, got: {:#?}",
        outcome.diagnostics,
    );
}

#[test]
fn connect_with_extra_segment_endpoint_lays_no_walkway() {
    for row in [
        "connect anchor.entry.x to peer.entry path=@gravel",
        "connect anchor.entry to peer.entry.x path=@gravel",
        "connect anchor.entry.x to peer.entry.y path=@gravel",
    ] {
        let outcome = lowered_with_resolver_diagnostics(&duo_with(row));
        assert_eq!(
            outcome.walkways.len(),
            0,
            "`{row}` must not lay the walkway that truncating the extra segment would produce",
        );
        assert_eq!(
            errors(&outcome.diagnostics),
            0,
            "endpoint enforcement is owned by `check::connect_arity`; the resolver-only path stays silent for `{row}`, got: {:#?}",
            outcome.diagnostics,
        );
    }
}

/// `check::duplicate` owns the diagnostic.
#[test]
fn duplicate_item_names_bind_once_and_stay_silent_through_resolve() {
    let src = format!(
        "{PROLOGUE}def lodge size=9x9:
  floor id=floor mat_slot=floor

def lodge size=3x3:
  floor id=floor mat_slot=floor

site duo:
  place id=anchor use=lodge theme=plain at=origin
"
    );
    let module = parse(&src).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    assert_eq!(
        errors(&resolution.diagnostics),
        0,
        "name collisions are owned by `check::duplicate`; the resolver-only path stays silent, got: {:#?}",
        resolution.diagnostics,
    );
    assert_eq!(
        resolution
            .scopes
            .keys()
            .filter(|k| *k == "def::lodge")
            .count(),
        1,
        "the name must bind exactly once",
    );
    let out: BlockArrayIr = lower_to_block_array(&ir, &resolution, None);
    assert_eq!(
        errors(&out.diagnostics),
        0,
        "lowering must not report the collision either, got: {:#?}",
        out.diagnostics,
    );
    let placement = out
        .placements
        .get("site::duo::anchor")
        .expect("the placement resolves");
    assert_eq!(
        (placement.dims.x, placement.dims.z),
        (9, 9),
        "the first declaration is the one that binds",
    );
}

#[test]
fn an_origin_naming_a_row_refused_for_its_id_reports_the_row_and_cascades() {
    let src = format!(
        "{PROLOGUE}\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
place id=\"a/b\" use=hut theme=plain east_of=anchor gap=4\n  \
place id=peer   use=hut theme=plain east_of=\"a/b\" gap=4\n  \
connect anchor.entry to peer.entry path=@gravel\n"
    );
    let outcome = lowered_with_resolver_diagnostics(&src);
    let codes: Vec<&str> = outcome
        .diagnostics
        .iter()
        .map(|d| d.code.as_str())
        .collect();
    assert_eq!(
        codes,
        [
            "E_INVALID_PLACE_ID",
            "W_DEFERRED_PLACE",
            "W_DEFERRED_CONNECT"
        ],
        "one finding per row of the chain and nothing else, got: {:#?}",
        outcome.diagnostics,
    );
    assert_eq!(
        outcome.walkways.len(),
        0,
        "the walkway goes with the row that is not placed",
    );

    let deferred = &outcome.diagnostics[1];
    let peer_row = src.find("place id=peer").expect("fixture has the row");
    assert!(
        deferred.span.start > peer_row && &src[deferred.span.clone()] == "\"a/b\"",
        "the cascade sits on the reference, got {:?}",
        &src[deferred.span.clone()],
    );
    let refused_row = src.find("place id=\"a/b\"").expect("fixture has the row");
    let note = deferred
        .notes
        .first()
        .and_then(|note| note.span.clone())
        .expect("the note carries the refused row's span");
    assert_eq!(
        note.start, refused_row,
        "the note points at the row that has the repair",
    );
    assert_eq!(
        outcome.diagnostics[0].span.start, refused_row,
        "and that row is the one carrying the error",
    );
}

/// `Value::as_label_str` reads nothing from either, so the resolver drops both rows alike. Each
/// row carries the other keys, so a stray `E_INCOMPLETE_PLACE` would mean the check reads values.
#[test]
fn non_label_use_or_theme_takes_the_same_silent_arm_as_an_absent_one() {
    for row in [
        "place id=silent use=3    theme=plain east_of=anchor gap=4",
        "place id=silent use=hut  theme=7     east_of=anchor gap=4",
        "place id=silent use=@oak theme=plain east_of=anchor gap=4",
    ] {
        let src = format!(
            "{PROLOGUE}\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
{row}\n  \
connect anchor.entry to silent.entry path=@gravel\n"
        );
        let outcome = lowered_with_resolver_diagnostics(&src);
        assert_eq!(
            outcome.walkways.len(),
            0,
            "`{row}` must drop the placement, and the walkway with it",
        );
        assert_eq!(
            errors(&outcome.diagnostics),
            0,
            "the mistyped value is owned by `check::type_mismatch`; the \
             resolver-only path stays silent for `{row}`, got: {:#?}",
            outcome.diagnostics,
        );
        assert_eq!(
            count(&outcome.diagnostics, DiagnosticCode::IncompletePlace),
            0,
            "and it is not reported as an absent key for `{row}` — the key is \
             on the line, so asking for it again would be advice the author \
             has already followed",
        );
        assert_eq!(
            count(&outcome.diagnostics, DiagnosticCode::DeferredConnect),
            1,
            "the same cascade an absent `use=` produces must fire for `{row}`",
        );
    }
}

/// `check::requires` reports the refused header, which every CLI path runs first.
#[test]
fn a_refused_requirement_is_skipped_silently_by_the_floor_derivation() {
    let source = "@requires version<1.20\n@requires nonsense\nstruct s size=2x2\n";
    let module = cairn_lang_core::parse(source).expect("the headers parse");

    assert_eq!(
        cairn_lang_core::resolve::unscoped_version_floors(&module),
        Vec::new(),
        "neither expression is a floor, so none is derived",
    );

    // The signal the skip relies on.
    let ir = cairn_lang_core::lower(&module);
    let reported = cairn_lang_core::check(&module, &ir, None);
    let invalid: Vec<_> = reported
        .iter()
        .filter(|d| d.code == DiagnosticCode::InvalidRequires)
        .collect();
    assert_eq!(invalid.len(), 2, "{reported:#?}");
    for diagnostic in invalid {
        assert_eq!(
            diagnostic.code.severity(),
            cairn_lang_core::Severity::Error,
            "a warning would let a compile proceed with no floor at all",
        );
    }
}

/// `check::arguments` owns its `W_IGNORED_ARGUMENT`.
#[test]
fn a_slot_on_a_role_that_reads_none_is_skipped_silently_through_resolve() {
    let src = "theme plain:\n  slot wall -> @cobblestone\n\n\
               struct s size=5x5\n  floor mat_slot=wall\n  walls mat_slot=wall height=3\n  \
               door side=front at=center mat_slot=nosuch\n";
    let module = parse(src).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir, None);
    assert_eq!(
        resolution.diagnostics.len(),
        0,
        "the slot is not looked up, so no `E_UNRESOLVED_SLOT`, got: {:#?}",
        resolution.diagnostics,
    );
    let door = src.find("door side").expect("the door is in the source");
    let binding = resolution.scopes["struct::s"]
        .members
        .get(&door)
        .expect("the door is resolved");
    assert!(
        binding.slot_value.is_none(),
        "nothing was looked up, so nothing is bound",
    );
    let out: BlockArrayIr = lower_to_block_array(&ir, &resolution, None);
    assert_eq!(
        out.diagnostics.len(),
        0,
        "lowering has nothing to say about it either, got: {:#?}",
        out.diagnostics,
    );

    // The signal the skip relies on.
    let reported = check(&module, &ir, None);
    assert_eq!(
        reported.iter().map(|d| d.code).collect::<Vec<_>>(),
        [DiagnosticCode::IgnoredArgument],
        "{reported:#?}",
    );
    assert_eq!(&src[reported[0].span.clone()], "nosuch");
}
