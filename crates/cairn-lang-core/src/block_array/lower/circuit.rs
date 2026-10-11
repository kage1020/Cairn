//! The members block-array lowering checks but does not paint: a
//! `circuit` region and a door's actuator patch.

use crate::ast::{SIGNAL_HEAD, ValueKind};
use crate::diagnostic::{Diagnostic, DiagnosticNote};
use crate::intent::{Member, MemberRole, PatchTargetError, actuator_patch_target};

use crate::block_array::args::{NonNegRead, diag_deferred_member_reason, nonneg_int_or_defer};

/// Recognise a `circuit region=<label> void=<N>` fixture without emitting
/// voxels.
///
/// `circuit` reserves a routing region for place-and-route
/// (`spec/redstone` "Place-and-route" and "Connection to the IR and
/// phases"). Its dust, repeaters and cells are the logic layer's, which
/// reads the intent IR itself, so nothing reaches the [`BlockArray`], not
/// even the region name. Checking the shape here, rather than reporting
/// `W_DEFERRED_MEMBER`, keeps a valid `circuit` line quiet.
///
/// The accepted surface:
/// - `region=<label>`: any `Ident` or `Str`; another kind defers, naming
///   it. Whether the label names a routable region is the routing pass's
///   question.
/// - `void=<N>`: a `u32` service-layer height above zero. An invalid or
///   overflowing value defers through `nonneg_int_or_defer`, and `void=0`
///   defers too: reserving no headroom is almost always a typo, since an
///   author who wants none drops the line.
///
/// A malformed shape defers with a reason naming the missing or invalid
/// key.
///
/// [`BlockArray`]: crate::block_array::BlockArray
pub(super) fn recognize_circuit_region(member: &Member, diagnostics: &mut Vec<Diagnostic>) {
    let Some(raw_region) = member.intent_state.get("region") else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "circuit requires `region=<label>` (e.g. `region=floor`, `region=basement`)",
        ));
        return;
    };
    let Some(region) = raw_region.value.as_label_str() else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "circuit `region=` must be an identifier or string label, got {}",
                raw_region.value.kind_name(),
            ),
        ));
        return;
    };
    if region.is_empty() {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "circuit `region=` must be a non-empty label",
        ));
        return;
    }
    // `nonneg_int_or_defer` has already reported a `Deferred` value.
    match nonneg_int_or_defer(member, "void", diagnostics) {
        NonNegRead::Valid(0) => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                "circuit `void=0` reserves no service layer; use a `u32` value >= 1",
            ));
        }
        NonNegRead::Absent => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                "circuit requires `void=<N>` (a `u32` service-layer height >= 1)",
            ));
        }
        NonNegRead::Valid(_) | NonNegRead::Deferred => {}
    }
}

/// The selector attributes an actuator patch reads.
const ACTUATOR_PATCH_SELECTOR_KEYS: &[&str] = &["id"];

/// The intent-state keys an actuator patch reads. `lit_by`, `powered_by`
/// and `fired_by` (`spec/redstone` "Signal binding") need their keywords
/// in the role table before they can join.
const ACTUATOR_PATCH_INTENT_KEYS: &[&str] = &["opened_by"];

/// A `door` member is an **actuator patch** when its line uses the
/// selector form (`door[…] …`): the brackets pick a door declared
/// elsewhere by `id=`, and the patch adds only an `opened_by=` signal
/// binding (`spec/redstone` "Signal binding"). Bucketing peels patches off
/// before the openings phase, where `side_of` would report a missing
/// `side=`. Stray `side=` / `at=` keys are the recogniser's to report.
pub(super) fn is_actuator_patch(member: &Member) -> bool {
    matches!(member.role, MemberRole::Door) && member.selector.is_some()
}

/// Recognise a `door[id=X] opened_by=sig.Y` actuator patch without
/// emitting voxels.
///
/// This pass owns the patch's surface shape, so a malformed patch defers
/// with a targeted `W_DEFERRED_MEMBER` and a well-formed one stays quiet;
/// the signal binding is the redstone pass's, which reads the intent IR
/// itself.
///
/// The accepted surface:
/// - The `[selector]` holds `id=<label>`, an `Ident` or `Str`.
/// - `id=` is its only key ([`ACTUATOR_PATCH_SELECTOR_KEYS`]). Another key
///   a `door` accepts (`door[side=front]`) defers, naming it.
/// - The label names exactly one physical `door` in `siblings`, the
///   flattened paint set, so one under `level y=N` counts. A missing or
///   repeated id defers with `actuator_patch_target`'s reason, plus a fix
///   where that reason names none.
/// - `opened_by=` is present, and is the only intent-state key
///   ([`ACTUATOR_PATCH_INTENT_KEYS`]). Another key defers, naming it, so
///   a reserved `powered_by=` never builds with no meaning.
/// - The `opened_by=` value is a two-segment `sig.<name>` `DotRef`;
///   anything else defers, quoting the path or naming the kind.
///
/// The signal *name* is not checked: the signal graph does not exist
/// here, and the redstone pass reports an unknown one.
pub(super) fn recognize_actuator_patch(
    member: &Member,
    siblings: &[(u32, &Member)],
    diagnostics: &mut Vec<Diagnostic>,
) {
    // `is_actuator_patch` guarantees a selector; the `let-else` keeps a
    // release build going should that break.
    debug_assert!(
        member.selector.is_some(),
        "recognize_actuator_patch called on a member without a selector; \
         is_actuator_patch invariant broken",
    );
    let Some(selector) = member.selector.as_ref() else {
        return;
    };
    // Only keys a `door` accepts. One outside its vocabulary is
    // `check::arguments`' `E_UNKNOWN_ARGUMENT`, which offers the likely
    // typo and reaches a `cairn check` that runs no lowering; repeating it
    // here would bill one repair twice.
    let vocabulary = member.role.accepted_arguments().unwrap_or_default();
    let unknown_selector_keys: Vec<&str> = selector
        .keys()
        .map(String::as_str)
        .filter(|k| !ACTUATOR_PATCH_SELECTOR_KEYS.contains(k) && vocabulary.contains(k))
        .collect();
    if !unknown_selector_keys.is_empty() {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "door actuator patch `[selector]` accepts only `id=<label>`; unknown attribute(s): {}",
                unknown_selector_keys.join(", "),
            ),
        ));
        return;
    }
    // The redstone front end picks the door through this too, so the
    // patch deferred here is the one that gets no port there.
    if let Err(reason) = actuator_patch_target(member, siblings.iter().map(|&(_, m)| m)) {
        let mut diagnostic = diag_deferred_member_reason(member, &reason.to_string());
        if let Some(fix) = patch_target_fix(&reason) {
            diagnostic.notes.push(DiagnosticNote {
                span: None,
                message: fix,
            });
        }
        diagnostics.push(diagnostic);
        return;
    }
    let unknown_intent_keys: Vec<&str> = member
        .intent_state
        .keys()
        .map(String::as_str)
        .filter(|k| !ACTUATOR_PATCH_INTENT_KEYS.contains(k))
        .collect();
    if !unknown_intent_keys.is_empty() {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "door actuator patch accepts only `opened_by=sig.<name>` today; unknown \
                 attribute(s): {} (spec/redstone \"Signal binding\" reserves `lit_by=` / \
                 `powered_by=` / `fired_by=` for future keywords)",
                unknown_intent_keys.join(", "),
            ),
        ));
        return;
    }
    let Some(opened_by) = member.intent_state.get("opened_by") else {
        diagnostics.push(diag_deferred_member_reason(
            member,
            "door actuator patch requires an `opened_by=sig.<name>` binding (only `opened_by=` is recognised on doors today)",
        ));
        return;
    };
    match &opened_by.value.kind {
        ValueKind::DotRef(dotref) if dotref.head() == SIGNAL_HEAD && dotref.tail().len() == 1 => {}
        ValueKind::DotRef(dotref) if dotref.head() == SIGNAL_HEAD => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "door actuator patch `opened_by=` must be a two-segment signal reference `sig.<name>`, got `{dotref}`",
                ),
            ));
        }
        ValueKind::DotRef(dotref) => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "door actuator patch `opened_by=` must be a signal reference `sig.<name>` (head must be `sig`), got `{dotref}`",
                ),
            ));
        }
        _ => {
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "door actuator patch `opened_by=` must be a signal reference `sig.<name>`, got {}",
                    opened_by.value.kind_name(),
                ),
            ));
        }
    }
}

/// The repair for a patch whose selector picks no single member, where
/// the reason alone does not point at one.
///
/// [`PatchTargetError`]'s sentence states the fault and stops, so the
/// repair is said here. A missing or malformed `id=` and an id beside the
/// ones the reason lists already name the edit; an id that two members
/// carry, or a scope whose members carry none, does not.
fn patch_target_fix(reason: &PatchTargetError) -> Option<String> {
    let keyword = reason.keyword();
    match reason {
        PatchTargetError::Ambiguous { count, .. } => Some(format!(
            "Fix: give the {count} `{keyword}` members distinct ids, so the selector names one."
        )),
        PatchTargetError::NoSuchId {
            id,
            known,
            unlabelled,
            ..
        } if known.is_empty() && *unlabelled > 0 => Some(format!(
            "Fix: add `id={id}` to the `{keyword}` this patch is meant to bind."
        )),
        _ => None,
    }
}
