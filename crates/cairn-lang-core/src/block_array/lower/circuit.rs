//! The members block-array lowering checks but does not paint: a
//! `circuit` region and a door's actuator patch.

use crate::ast::{SIGNAL_HEAD, ValueKind};
use crate::diagnostic::{Diagnostic, DiagnosticNote};
use crate::intent::{Member, MemberRole, PatchTargetError, actuator_patch_target};

use crate::block_array::args::{NonNegRead, diag_deferred_member_reason, nonneg_int_or_defer};

/// Recognise a `circuit region=<label> void=<N>` fixture without emitting
/// voxels.
///
/// `circuit` reserves a routing region for the future
/// `logic_synth → logic_place → logic_route` passes (`spec/redstone`
/// "Place-and-route" and "Connection to the IR and phases"). Nothing lands in
/// the block array at this stage — the physical dust / repeater / cell tiles
/// are decided by the logic layer, which is not part of block-array lowering
/// yet. Recognising the shape
/// here (rather than defaulting to `W_DEFERRED_MEMBER`) keeps
/// `redstone-door.crn` from firing a per-source-line warning while the
/// downstream passes are still under construction, mirroring how
/// `logic` / `assert` items never reach this function at all.
///
/// The recognised region name is intentionally NOT threaded onto the
/// [`BlockArray`] today: the receiver is the future logic pipeline,
/// which walks the intent IR directly and does not consume block-array
/// side-channels. When that pipeline lands the hand-off will be a fresh
/// intent-IR walk rather than an extension of this recogniser.
///
/// The surface contract accepted today:
/// - `region=<label>` — the region name a later logic pass will look up
///   (`floor`, `basement`, …). Accepts any `Ident` or `Str` value; other
///   value kinds (integers, booleans, dotted refs, …) defer with a
///   kind-mismatch primary that names the offending kind. The
///   block-array pass does not yet validate that the label matches an
///   existing member kind on the struct — that check belongs to the
///   routing pass, which owns the catalogue of routable regions.
/// - `void=<N>` — a `u32` service-layer height greater than zero. A
///   present-but-invalid value defers via `nonneg_int_or_defer` (which
///   also catches values that overflow `u32`), and `void=0` explicitly
///   defers because reserving zero blocks of routing headroom is almost
///   always a typo (an author who wants no reserved layer just drops
///   the `circuit` line).
///
/// Malformed shapes fall back to `diag_deferred_member_reason` so the
/// author still sees a targeted warning naming the missing / invalid
/// key rather than the generic "not yet handled" message.
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
    // `NonNegRead::Deferred` already pushed its own `void=` primary via
    // `nonneg_int_or_defer` (covering both non-integer values and
    // integers that overflow `u32`), so it shares the "no extra
    // diagnostic" arm with the valid-positive case.
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

/// The only selector attribute an actuator patch recognises today.
const ACTUATOR_PATCH_SELECTOR_KEYS: &[&str] = &["id"];

/// The only intent-state key an actuator patch recognises today. Extending
/// this list to `lit_by` / `powered_by` / `fired_by` requires landing the
/// matching keyword in the role table first (`spec/redstone` "Signal
/// binding").
const ACTUATOR_PATCH_INTENT_KEYS: &[&str] = &["opened_by"];

/// A door member is an **actuator patch** when its surface line uses the
/// selector form (`door[…] …`). The bracketed selector references an
/// already-declared physical door by `id=`; its role in block-array
/// lowering is pure metadata (an `opened_by=` signal binding for the
/// future redstone lowering pipeline, `spec/redstone` "Signal binding").
/// Routing patch lines through `carve_door` would false-positive `side_of`'s
/// "missing `side=`" guard, so `lower_body_to_block_array` peels them off
/// before the phase-bucketing match. Whether the patch also carries
/// stray `side=` / `at=` keys is checked inside the recogniser, not
/// here, so the classifier stays a one-liner and the diagnostic path
/// owns key-allowlist enforcement.
pub(super) fn is_actuator_patch(member: &Member) -> bool {
    matches!(member.role, MemberRole::Door) && member.selector.is_some()
}

/// Recognise a `door[id=X] opened_by=sig.Y` actuator patch without
/// emitting voxels.
///
/// The block-array pass owns the *surface shape* of the patch — the
/// selector allowlist, the `opened_by=` presence, and the `sig.<name>`
/// signal-reference well-formedness — so a malformed patch fails loud
/// with a targeted `W_DEFERRED_MEMBER` while a well-formed one stays
/// quiet. Threading the signal binding onto the future logic pipeline
/// happens in the redstone lowering pass that consumes the intent IR
/// directly (mirroring how `recognize_circuit_region` leaves its
/// recognised region name off the block array).
///
/// The surface contract accepted today:
/// - The `[selector]` must contain an `id=<label>` — an `Ident` or
///   `Str` value. Other value kinds defer with a kind-mismatch primary.
/// - The selector's only recognised key is `id=` (see
///   [`ACTUATOR_PATCH_SELECTOR_KEYS`]). Any other selector attribute
///   defers with a primary that names the unknown key so a stray
///   `class=main` or `foo=bar` does not slip through silently.
/// - The label must match exactly one physical `door` member in the
///   same `flatten_members` view (so a door authored under `level y=N`
///   is still selectable). Absence defers with a primary that lists
///   the ids that ARE declared; ambiguity (the same `id=` declared in
///   two scopes the flattener merges) defers separately with an
///   "ambiguous" primary so the author is never silently rebinding the
///   first hit.
/// - `opened_by=` must be present. `spec/redstone` "Signal binding" also
///   defines `lit_by=` on lamps, `powered_by=` on pistons, and
///   `fired_by=` on dispensers, but those keywords are not yet in the
///   role table — door + `opened_by` is the only shape
///   `redstone-door.crn` exercises today. Any other intent-state key
///   defers with a primary that names the offending key so a future
///   `powered_by=` implementation cannot silently change the meaning
///   of existing source (see [`ACTUATOR_PATCH_INTENT_KEYS`]).
/// - The `opened_by=` value must be a two-segment `sig.<name>`
///   `DotRef`. Non-`DotRef` values defer with a "got `<kind>`" primary;
///   a `DotRef` whose head is not `sig` or whose segment count is not
///   2 defers with the offending path rendered verbatim.
///
/// The signal *name* on the RHS of `sig.` is intentionally not
/// validated against a namespace here — the signal graph does not
/// exist at block-array lowering time, so a `sig.does_not_exist`
/// binding surfaces later in the redstone lowering pass when the
/// graph is walked. The surface recogniser only owns the syntactic
/// shape.
///
/// `siblings` is the same flattened `(y_offset, &Member)` list the
/// phase-bucketing loop iterates over. Passed as a slice so the
/// recogniser can look up physical doors without a second walk of the
/// intent IR.
pub(super) fn recognize_actuator_patch(
    member: &Member,
    siblings: &[(u32, &Member)],
    diagnostics: &mut Vec<Diagnostic>,
) {
    // `is_actuator_patch` guarantees `selector.is_some()` at the call
    // site. Pinning the invariant with a `debug_assert!` fails loud in
    // tests if a future refactor of the classifier drops the selector
    // check without also revisiting this recogniser; the `let-else`
    // below then keeps release builds usable rather than panicking.
    debug_assert!(
        member.selector.is_some(),
        "recognize_actuator_patch called on a member without a selector; \
         is_actuator_patch invariant broken",
    );
    let Some(selector) = member.selector.as_ref() else {
        return;
    };
    // Only keys a `door` does carry. One outside the role's vocabulary is
    // `check::arguments`' `E_UNKNOWN_ARGUMENT`, which names it, offers the
    // word it may be a typo for, and reaches an unpinned `cairn check`
    // that runs no lowering at all — so repeating it here would bill one
    // repair twice. What is left for this recogniser is the key that
    // passes a `door`'s vocabulary and still has no reader in the patch,
    // `door[side=front]` being the shape.
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
    // Which door the brackets pick is read from the one place the
    // redstone front end reads it too, so the patch this defers is the
    // patch that gets no port there.
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
