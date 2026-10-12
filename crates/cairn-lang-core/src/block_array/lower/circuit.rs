//! The members block-array lowering checks but does not paint: a
//! `circuit` region and a door's actuator patch.

use crate::ast::{SIGNAL_HEAD, ValueKind};
use crate::diagnostic::{Diagnostic, DiagnosticNote};
use crate::intent::{Member, MemberRole, PatchTargetError, actuator_patch_target};

use crate::block_array::args::{NonNegRead, diag_deferred_member_reason, nonneg_int_or_defer};

/// Check a `circuit region=<label> void=<N>` line's shape without emitting voxels
/// (`spec/redstone` "Place-and-route").
///
/// `void=0` defers too: an author who wants no headroom drops the line.
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

/// The intent-state keys an actuator patch reads.
const ACTUATOR_PATCH_INTENT_KEYS: &[&str] = &["opened_by"];

/// Whether a `door` line is an actuator patch: the selector form `door[…]`, which binds a signal
/// to a door declared elsewhere (`spec/redstone` "Signal binding").
pub(super) fn is_actuator_patch(member: &Member) -> bool {
    matches!(member.role, MemberRole::Door) && member.selector.is_some()
}

/// Check a `door[id=X] opened_by=sig.Y` actuator patch's shape without emitting voxels.
///
/// The `id=` must name exactly one physical `door` in `siblings`, the flattened paint set.
pub(super) fn recognize_actuator_patch(
    member: &Member,
    siblings: &[(u32, &Member)],
    diagnostics: &mut Vec<Diagnostic>,
) {
    debug_assert!(
        member.selector.is_some(),
        "recognize_actuator_patch called on a member without a selector; \
         is_actuator_patch invariant broken",
    );
    let Some(selector) = member.selector.as_ref() else {
        return;
    };
    // Only keys a `door` accepts; `check::arguments` reports the rest (`E_UNKNOWN_ARGUMENT`).
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
    // The redstone front end picks the door through this too, so the two agree.
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

/// The repair for a patch whose selector picks no single member, where the reason names none.
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
