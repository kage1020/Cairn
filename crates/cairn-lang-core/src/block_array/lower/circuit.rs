//! The members block-array lowering checks but does not paint: a `circuit` region and an
//! actuator patch.

use indexmap::IndexMap;

use crate::ast::{SIGNAL_HEAD, Value, ValueKind};
use crate::diagnostic::{Diagnostic, DiagnosticNote};
use crate::intent::{
    Actuator, Member, PatchTargetError, SENSOR_HOSTS, ValueWithSpan, actuator_patch_target,
};
use crate::prose::or_list;

use crate::block_array::args::{
    NonNegRead, diag_deferred_member_noted, diag_invalid_member, nonneg_int_or_refuse,
};

/// Check a `circuit region=<label> void=<N>` line's shape without emitting voxels
/// (`spec/redstone` "Place-and-route").
///
/// `void=0` is refused too: an author who wants no headroom drops the line.
pub(super) fn recognize_circuit_region(member: &Member, diagnostics: &mut Vec<Diagnostic>) {
    let Some(raw_region) = member.arguments.get("region") else {
        diagnostics.push(diag_invalid_member(
            member,
            "circuit requires `region=<label>` (e.g. `region=floor`, `region=basement`)".to_owned(),
            Vec::new(),
        ));
        return;
    };
    let Some(region) = raw_region.value.as_label_str() else {
        diagnostics.push(diag_invalid_member(
            member,
            format!(
                "circuit `region=` must be an identifier or string label, got {}",
                raw_region.value.kind_name(),
            ),
            Vec::new(),
        ));
        return;
    };
    if region.is_empty() {
        diagnostics.push(diag_invalid_member(
            member,
            "circuit `region=` must be a non-empty label".to_owned(),
            Vec::new(),
        ));
        return;
    }
    // `nonneg_int_or_refuse` has already reported a `Refused` value.
    match nonneg_int_or_refuse(member, "void", diagnostics) {
        NonNegRead::Valid(0) => {
            diagnostics.push(diag_invalid_member(
                member,
                "circuit `void=0` reserves no service layer; use a `u32` value >= 1".to_owned(),
                Vec::new(),
            ));
        }
        NonNegRead::Absent => {
            diagnostics.push(diag_invalid_member(
                member,
                "circuit requires `void=<N>` (a `u32` service-layer height >= 1)".to_owned(),
                Vec::new(),
            ));
        }
        NonNegRead::Valid(_) | NonNegRead::Refused => {}
    }
}

/// The selector attributes an actuator patch reads.
const ACTUATOR_PATCH_SELECTOR_KEYS: &[&str] = &["id"];

/// The actuator a line patches: the selector form of an actuator, such as `door[…]`, which binds a
/// signal to an actuator declared elsewhere (`spec/redstone` "Signal binding"); `None` for any
/// other line.
pub(super) fn patched_actuator(
    member: &Member,
) -> Option<(Actuator, &IndexMap<String, ValueWithSpan>)> {
    Some((member.role.actuator()?, member.selector.as_ref()?))
}

/// The refusal of a sensor's line in the selector form, which places nothing, since only an
/// actuator takes a patch (`spec/redstone` "Signal binding"); `None` for any other line.
pub(super) fn refused_sensor_selector(member: &Member) -> Option<Diagnostic> {
    let keyword = member.role.keyword();
    if member.selector.is_none() || !SENSOR_HOSTS.contains(&keyword) {
        return None;
    }
    let actuators: Vec<String> = Actuator::ALL
        .iter()
        .map(|actuator| format!("`{}`", actuator.keyword()))
        .collect();
    Some(diag_invalid_member(
        member,
        format!(
            "`{keyword}[…]` places nothing: the brackets pick a member declared elsewhere, and \
             only an actuator, {}, takes a patch",
            or_list(&actuators).unwrap_or_default(),
        ),
        vec![format!(
            "declare the `{keyword}` on a line of its own, without brackets, as `{keyword} \
             id=<label> at=… -> sig.<name>`"
        )],
    ))
}

/// Check the shape of a patch of `actuator`, as `door[id=X] opened_by=sig.Y`, without emitting
/// voxels; the member it binds a signal to when that shape holds.
///
/// The `id=` must name exactly one physical member of the patch's keyword in `written`, each
/// member with its level's offset, `None` under a `level` that is not placed.
pub(super) fn recognize_actuator_patch<'w>(
    member: &Member,
    (actuator, selector): (Actuator, &IndexMap<String, ValueWithSpan>),
    written: &[(Option<u32>, &'w Member)],
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<&'w Member> {
    let (keyword, key) = (actuator.keyword(), actuator.key());
    let (does, to_do) = actuator.verbs();
    // Only keys the keyword accepts; `check::arguments` reports the rest (`E_UNKNOWN_ARGUMENT`).
    let vocabulary = member.role.accepted_arguments().unwrap_or_default();
    let unknown_selector_keys: Vec<&str> = selector
        .keys()
        .map(String::as_str)
        .filter(|k| !ACTUATOR_PATCH_SELECTOR_KEYS.contains(k) && vocabulary.contains(k))
        .collect();
    if !unknown_selector_keys.is_empty() {
        diagnostics.push(diag_invalid_member(
            member,
            format!(
                "{keyword} actuator patch `[selector]` accepts only `id=<label>`; unknown attribute(s): {}",
                unknown_selector_keys.join(", "),
            ),
            Vec::new(),
        ));
        return None;
    }
    // The redstone front end picks the actuator through this too, so the two agree.
    let target = match actuator_patch_target(member, written.iter().map(|&(_, m)| m)) {
        Ok(target) => target,
        Err(reason) => {
            let mut diagnostic = diag_invalid_member(member, reason.to_string(), Vec::new());
            if let Some(fix) = patch_target_fix(&reason) {
                diagnostic.notes.push(DiagnosticNote {
                    span: None,
                    message: fix,
                });
            }
            diagnostics.push(diagnostic);
            return None;
        }
    };
    let unknown_keys: Vec<&str> = member
        .written_fields()
        .map(|(written, _)| written)
        .filter(|written| *written != key)
        .collect();
    if !unknown_keys.is_empty() {
        diagnostics.push(diag_invalid_member(
            member,
            format!(
                "{keyword} actuator patch takes only `{key}=sig.<name>`; unknown attribute(s): {}",
                unknown_keys.join(", "),
            ),
            vec![format!(
                "move these onto the {keyword}'s own line, or drop them"
            )],
        ));
        return None;
    }
    let Some(bound) = member.arguments.get(key) else {
        diagnostics.push(diag_invalid_member(
            member,
            format!(
                "{keyword} actuator patch has no `{key}=`, so it binds no signal to the {keyword}"
            ),
            vec![format!(
                "write `{key}=sig.<name>`, the signal that {does} the {keyword}"
            )],
        ));
        return None;
    };
    if let Some(primary) = not_a_signal(keyword, key, &bound.value) {
        diagnostics.push(diag_invalid_member(member, primary, Vec::new()));
        return None;
    }
    let unplaced = written
        .iter()
        .any(|&(y_offset, m)| y_offset.is_none() && std::ptr::eq(m, target));
    if unplaced {
        diagnostics.push(diag_deferred_member_noted(
            member,
            &format!(
                "{keyword} actuator patch binds a `{keyword}` under a `level` that is not placed, \
                 so no {keyword} is built for it to {to_do}"
            ),
            &format!("the {keyword} is built once that `level` is placed"),
        ));
    }
    Some(target)
}

/// Whether `value` names a signal, as `sig.<name>` (`spec/redstone` "Signal binding").
pub(super) fn names_a_signal(value: &Value) -> bool {
    matches!(&value.kind, ValueKind::DotRef(dotref) if dotref.head() == SIGNAL_HEAD && dotref.tail().len() == 1)
}

/// Why an actuator patch's `key=` value names no signal, or `None` when it names one.
fn not_a_signal(keyword: &str, key: &str, value: &Value) -> Option<String> {
    match &value.kind {
        _ if names_a_signal(value) => None,
        ValueKind::DotRef(dotref) if dotref.head() == SIGNAL_HEAD => Some(format!(
            "{keyword} actuator patch `{key}=` must be a two-segment signal reference `sig.<name>`, got `{dotref}`",
        )),
        ValueKind::DotRef(dotref) => Some(format!(
            "{keyword} actuator patch `{key}=` must be a signal reference `sig.<name>` (head must be `sig`), got `{dotref}`",
        )),
        _ => Some(format!(
            "{keyword} actuator patch `{key}=` must be a signal reference `sig.<name>`, got {}",
            value.kind_name(),
        )),
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
