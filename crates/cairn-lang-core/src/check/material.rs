//! The `material` pass: a `floor` or `walls` without `mat_slot=`, which reads its block only from
//! the theme's slot map and so contributes no voxel.

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::intent::{BodyKind, IntentModule, Member, MemberRole};

use super::DiagnosticSink;

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    let has_a_theme = !ir.themes.is_empty();
    for body in ir.bodies() {
        walk(body.members, body.kind, false, has_a_theme, sink);
    }
}

fn walk(
    members: &[Member],
    body: BodyKind,
    inside_level: bool,
    has_a_theme: bool,
    sink: &mut DiagnosticSink,
) {
    for member in members {
        // `member_scope::walk`'s gates: such a row is already reported, subtree and all.
        if matches!(member.role, MemberRole::Other(_)) || !member.role.is_read_in(body) {
            continue;
        }
        if names_no_material(member) {
            sink.push(diagnose(member, has_a_theme));
        }
        if super::nesting::body_reaches_the_build(member, inside_level) {
            walk(&member.children.members, body, true, has_a_theme, sink);
        }
    }
}

/// `mat_slot` is also `None` for a value that failed to hoist, which another code reports.
fn names_no_material(member: &Member) -> bool {
    member.mat_slot.is_none()
        && !member.intent_state.contains_key("mat_slot")
        && matches!(
            without_a_material(&member.role),
            WithoutAMaterial::PaintsNothing
        )
}

/// What a member of one role does to the build when it carries no `mat_slot=`.
enum WithoutAMaterial {
    PaintsNothing,
    Carves,
    PaintsAFallback,
    NamesItsMaterialElsewhere,
    PutsNothingDown,
}

/// Must agree with `block_array`'s `member_will_paint`; `MemberRole::unread_arguments` relies on
/// the `level`, `circuit`, `place` and `connect` arms.
fn without_a_material(role: &MemberRole) -> WithoutAMaterial {
    match role {
        MemberRole::Floor | MemberRole::Walls => WithoutAMaterial::PaintsNothing,
        // A `window` without `mat_slot=` is an opening carved to air.
        MemberRole::Window | MemberRole::Door => WithoutAMaterial::Carves,
        MemberRole::Roof | MemberRole::Stair | MemberRole::PressurePlate => {
            WithoutAMaterial::PaintsAFallback
        }
        // `path=@MAT`, whose absence is `E_MISSING_PATH_MATERIAL`.
        MemberRole::Connect => WithoutAMaterial::NamesItsMaterialElsewhere,
        MemberRole::Level | MemberRole::Circuit | MemberRole::Place | MemberRole::Other(_) => {
            WithoutAMaterial::PutsNothingDown
        }
    }
}

fn diagnose(member: &Member, has_a_theme: bool) -> Diagnostic {
    let keyword = member.role.keyword();
    let advice = if has_a_theme {
        format!("add `mat_slot=NAME` to the `{keyword}`, naming a slot the applied theme declares")
    } else {
        format!(
            "this module declares no `theme`, so there is no slot for a `mat_slot=` to name: \
             declare one, then give the `{keyword}` a `mat_slot=NAME` from it",
        )
    };
    Diagnostic {
        code: DiagnosticCode::MissingMaterial,
        span: member.span.clone(),
        primary: format!("`{keyword}` has no `mat_slot=`, so it paints nothing"),
        notes: vec![DiagnosticNote {
            span: None,
            message: advice,
        }],
        data: None,
    }
}
