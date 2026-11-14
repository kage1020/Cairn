//! The `material` pass: a `floor`, `walls` or material `fill` without `mat_slot=`, which reads its
//! block only from the theme's slot map and so contributes no voxel.

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
            for child in &member.children.members {
                if super::nesting::child_reaches_the_build(member, child) {
                    walk(std::slice::from_ref(child), body, true, has_a_theme, sink);
                }
            }
        }
    }
}

/// `mat_slot` is also `None` for a value that failed to hoist, which another code reports.
fn names_no_material(member: &Member) -> bool {
    member.mat_slot.is_none()
        && !member.arguments.contains_key("mat_slot")
        && matches!(without_a_material(member), WithoutAMaterial::PaintsNothing)
}

/// What a member does to the build when it carries no `mat_slot=`.
enum WithoutAMaterial {
    PaintsNothing,
    Carves,
    PaintsAFallback,
    BuildsItsOwnBlock,
    NamesItsMaterialElsewhere,
    PutsNothingDown,
}

/// Must agree with `block_array`'s `member_will_paint`; `MemberRole::unread_arguments` relies on
/// the `level`, `raw_repeat`, `circuit`, `place` and `connect` arms.
fn without_a_material(member: &Member) -> WithoutAMaterial {
    match &member.role {
        MemberRole::Fill if member.pours_fluid() => WithoutAMaterial::NamesItsMaterialElsewhere,
        MemberRole::Floor | MemberRole::Walls | MemberRole::Fill => WithoutAMaterial::PaintsNothing,
        // A `window` without `mat_slot=` is an opening carved to air.
        MemberRole::Window => WithoutAMaterial::Carves,
        MemberRole::Roof
        | MemberRole::Stair
        | MemberRole::Door
        | MemberRole::PressurePlate
        | MemberRole::Button
        | MemberRole::Chest => WithoutAMaterial::PaintsAFallback,
        // Each is built from its own tokens, which no `mat_slot=` chooses among.
        MemberRole::Lever
        | MemberRole::Daylight
        | MemberRole::Observer
        | MemberRole::Lamp
        | MemberRole::Piston
        | MemberRole::Dispenser
        | MemberRole::Repeater
        | MemberRole::Comparator
        | MemberRole::RedstoneTorch
        | MemberRole::RedstoneDust => WithoutAMaterial::BuildsItsOwnBlock,
        // `path=@MAT`, whose absence is `E_MISSING_PATH_MATERIAL`, and `mat=`, whose absence is
        // `E_INVALID_MEMBER`.
        MemberRole::Connect | MemberRole::RawBlock | MemberRole::RawFill => {
            WithoutAMaterial::NamesItsMaterialElsewhere
        }
        MemberRole::Level
        | MemberRole::RawRepeat
        | MemberRole::Circuit
        | MemberRole::Place
        | MemberRole::Other(_) => WithoutAMaterial::PutsNothingDown,
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
    let mut notes = vec![DiagnosticNote {
        span: None,
        message: advice,
    }];
    if matches!(member.role, MemberRole::Fill) {
        notes.push(DiagnosticNote {
            span: None,
            message: "or write `fluid=water` to pour water into the box, which needs no \
                      `mat_slot=`"
                .to_owned(),
        });
    }
    Diagnostic {
        code: DiagnosticCode::MissingMaterial,
        span: member.span.clone(),
        primary: format!("`{keyword}` has no `mat_slot=`, so it paints nothing"),
        notes,
        data: None,
    }
}
