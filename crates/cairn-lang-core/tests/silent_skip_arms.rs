//! Regression check for the silent-skip arms in
//! `resolve::resolver::resolve_site_placements` and the cascade
//! `W_DEFERRED_CONNECT` warning in `validate_port`.
//!
//! Three `place`-level arms intentionally `continue` without emitting a
//! diagnostic when a row lacks `id=`, `use=`, or `theme=`. A fourth arm
//! in `resolve_connect_row` returns when the `connect` line is missing
//! its `to` positional. The risk that previously sat on these arms was
//! that a downstream `connect` referencing the silent place would drop
//! its walkway with no visible signal at all — exactly the failure
//! mode `W_DEFERRED_MEMBER` already covers for endpoint cascades in
//! `block_array::lower`.
//!
//! Coverage matrix:
//!
//! 1. **Baseline** — a well-formed `connect` lays exactly one walkway
//!    and emits zero diagnostics. Pins the negative space so the
//!    silent-arm cases are interpretable.
//! 2. **Missing `use=`** — the cascade emits exactly one
//!    `W_DEFERRED_CONNECT`, no walkway, no errors. Pins the
//!    refactor-safety net: a future bug that drops a normal-path
//!    `place_def.insert` would surface as a wave of these warnings
//!    instead of silently breaking every walkway.
//! 3. **Missing `theme=`** — same cascade shape; multi-theme files
//!    where `theme=` is currently a known silent-gap input still
//!    surface their downstream `connect` failures through this code.
//! 4. **Missing `id=`** — unreachable from any `connect` by
//!    construction (no name to dot-ref), so the only contract worth
//!    pinning is "no spurious diagnostics from the silent arm itself".
//! 5. **`connect` rows with a non-`FROM.PORT to TO.PORT` shape** —
//!    three entries to the same silent return at the resolver, all
//!    pinned together so a future guard refactor cannot split their
//!    behaviour without showing up here: the missing-`to`-half case
//!    (`connect anchor.entry`), the missing-target case
//!    (`connect anchor.entry to`), and the wrong-separator case
//!    (`connect anchor.entry xxx peer.entry`). The cross-layer signal
//!    is owned by `check::connect_arity` (`E_CONNECT_ARITY`), which
//!    runs ahead of `resolve` inside `check`; these tests pin the
//!    resolver-only contract so library callers that bypass `check`
//!    keep the defensive behaviour, while
//!    `tests/check_connect_arity.rs` pins the user-facing diagnostic
//!    on the full pipeline.

use cairn_lang_core::block_array::{BlockArrayIr, lower_to_block_array};
use cairn_lang_core::check::{Diagnostic, DiagnosticCode, Severity};
use cairn_lang_core::{lower, parse, resolve};

struct Outcome {
    walkways: usize,
    diagnostics: Vec<Diagnostic>,
}

fn run(src: &str) -> Outcome {
    let module = parse(src).expect("parse");
    let ir = lower(&module);
    let resolution = resolve(&ir);
    let mut out: BlockArrayIr = lower_to_block_array(&ir, &resolution, None);
    let mut combined = resolution.diagnostics;
    combined.append(&mut out.diagnostics);
    Outcome {
        walkways: out.walkways.len(),
        diagnostics: combined,
    }
}

fn count(diagnostics: &[Diagnostic], code: DiagnosticCode) -> usize {
    diagnostics.iter().filter(|d| d.code == code).count()
}

fn errors(diagnostics: &[Diagnostic]) -> usize {
    diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .count()
}

/// Site prologue with a single def and theme. Names are test-local to
/// avoid collision with `examples/cottage.crn` (which declares its own
/// `cottage` struct under a slightly different role schema).
const PROLOGUE: &str = "@cairn 2026.06\n\n\
def hut size=3x3:\n  \
floor id=floor mat_slot=floor\n  \
walls id=walls class=outer mat_slot=wall height=3\n  \
door  id=entry side=front at=center\n\n\
theme plain:\n  \
slot floor -> @oak_planks\n  \
slot wall  -> @cobblestone\n\n";

/// Baseline: a fully-resolved `connect` produces exactly one walkway
/// and zero diagnostics. Anchors the negative-space assertions in the
/// silent-arm tests below — without this, "0 walkways, 0 errors" is
/// indistinguishable from a pipeline that no-ops on every input.
#[test]
fn fully_resolved_connect_lays_one_walkway_with_no_diagnostics() {
    let src = format!(
        "{PROLOGUE}\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
place id=peer   use=hut theme=plain east_of=anchor gap=4\n  \
connect anchor.entry to peer.entry path=@gravel\n"
    );
    let outcome = run(&src);
    assert_eq!(
        outcome.walkways, 1,
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

/// `place` missing `use=` registers in `seen_place_ids` but never
/// reaches `place_def.insert`. The downstream `connect` therefore
/// passes the `port_ref_from_value` gate and reaches `validate_port`'s
/// `place_def` miss arm, where the new cascade emits one
/// `W_DEFERRED_CONNECT`.
#[test]
fn missing_use_cascades_w_deferred_connect_with_no_walkway() {
    let src = format!(
        "{PROLOGUE}\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
place id=silent         theme=plain east_of=anchor gap=4\n  \
connect anchor.entry to silent.entry path=@gravel\n"
    );
    let outcome = run(&src);
    assert_eq!(
        outcome.walkways, 0,
        "a connect targeting a `use=`-less place must drop its walkway",
    );
    assert_eq!(
        errors(&outcome.diagnostics),
        0,
        "missing `use=` is a known silent input; no errors should surface, got: {:#?}",
        outcome.diagnostics,
    );
    assert_eq!(
        count(&outcome.diagnostics, DiagnosticCode::DeferredConnect),
        1,
        "exactly one `W_DEFERRED_CONNECT` must cascade from the missing `use=`",
    );
}

/// Mirror test for missing `theme=`. The multi-theme case is the known
/// silent gap; this single-theme fixture exercises the cascade shape
/// identically by suppressing the per-site heuristic (no theme name on
/// the placement is treated as a structural skip regardless of how
/// many themes the file declares — see the resolver doc).
#[test]
fn missing_theme_cascades_w_deferred_connect_with_no_walkway() {
    let src = format!(
        "{PROLOGUE}\
site duo:\n  \
place id=anchor    use=hut theme=plain at=origin\n  \
place id=themeless use=hut             east_of=anchor gap=4\n  \
connect anchor.entry to themeless.entry path=@gravel\n"
    );
    let outcome = run(&src);
    assert_eq!(
        outcome.walkways, 0,
        "a connect targeting a `theme=`-less place must drop its walkway",
    );
    assert_eq!(
        errors(&outcome.diagnostics),
        0,
        "missing `theme=` is a known silent input; no errors should surface, got: {:#?}",
        outcome.diagnostics,
    );
    assert_eq!(
        count(&outcome.diagnostics, DiagnosticCode::DeferredConnect),
        1,
        "exactly one `W_DEFERRED_CONNECT` must cascade from the missing `theme=`",
    );
}

/// An unnamed `place` row is unreachable from any `connect` row by
/// construction (no `id=` for the dot-ref left side), so the silent
/// arm cannot cascade. The contract worth pinning is that the silent
/// arm itself does not produce diagnostics and the surrounding site
/// still lowers without errors.
#[test]
fn place_without_id_skips_silently_without_diagnostics() {
    let src = format!(
        "{PROLOGUE}\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
place           use=hut theme=plain east_of=anchor gap=4\n"
    );
    let outcome = run(&src);
    assert_eq!(
        errors(&outcome.diagnostics),
        0,
        "an unnamed `place` must not push any error, got: {:#?}",
        outcome.diagnostics,
    );
    assert_eq!(
        count(&outcome.diagnostics, DiagnosticCode::DeferredConnect),
        0,
        "no `connect` references the unnamed place, so the cascade must not fire",
    );
}

/// A `connect` row missing its `to` half short-circuits silently at the
/// resolver: the cross-layer diagnostic (`E_CONNECT_ARITY`) is owned by
/// `check::connect_arity`, which runs ahead of `resolve` inside the
/// top-level `check` pipeline. This test exercises `resolve(ir)`
/// directly, so it pins the resolver-only contract that library
/// callers bypassing `check` continue to see the defensive return
/// rather than a panic. The user-facing diagnostic is covered in
/// `tests/check_connect_arity.rs`.
#[test]
fn connect_with_missing_positional_silently_returns_without_diagnostics() {
    let src = format!(
        "{PROLOGUE}\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
connect anchor.entry\n"
    );
    let outcome = run(&src);
    assert_eq!(
        outcome.walkways, 0,
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
        "the cascade fires from `validate_port`, which is never reached when the `to` half is absent",
    );
}

/// `connect FROM.PORT to` (2 positionals: the `to` keyword present but
/// no target port) is the other entry to the same INVARIANT'd silent
/// arm. The resolver's guard inspects `positional.get(2)` — `None`
/// here — so it returns silently identically to the 1-positional
/// case. Pin both shapes so a future resolver refactor that splits
/// them (e.g. panic when `to` is present but target is missing) shows
/// up as a deliberate test change.
#[test]
fn connect_with_to_keyword_only_silently_returns_without_diagnostics() {
    let src = format!(
        "{PROLOGUE}\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
connect anchor.entry to\n"
    );
    let outcome = run(&src);
    assert_eq!(
        outcome.walkways, 0,
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
        "the cascade fires from `validate_port`, which is never reached when the target port is absent",
    );
}

/// `connect FROM.PORT xxx TO.PORT` (3 positionals but the middle slot
/// is not the literal `to` keyword) used to slip through the resolver
/// guard because `positional.get(2)` is `Some`. The strengthened
/// guard now rejects it on shape-mismatch; this test pins that
/// behaviour so a regression that softens the guard re-introduces
/// the silent-misinterpretation hole the C1 review caught.
#[test]
fn connect_with_wrong_separator_silently_returns_without_diagnostics() {
    let src = format!(
        "{PROLOGUE}\
site duo:\n  \
place id=anchor use=hut theme=plain at=origin\n  \
place id=peer   use=hut theme=plain east_of=anchor gap=4\n  \
connect anchor.entry xxx peer.entry path=@gravel\n"
    );
    let outcome = run(&src);
    assert_eq!(
        outcome.walkways, 0,
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
        "the cascade fires from `validate_port`, which is never reached when the guard rejects the shape upstream",
    );
}
