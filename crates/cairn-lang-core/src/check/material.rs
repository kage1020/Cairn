//! `material` pass — flags a member whose only route to a block is
//! `mat_slot=` and which was written without one.
//!
//! `floor` and `walls` read their block from the applied theme's slot map
//! alone: a theme selector's bindings land in
//! `ResolvedMemberBinding::selector_extras`, which the block-array pass
//! never reads, and neither role has a fallback material the way `roof`,
//! `stair` and `pressure_plate` do. Without a `mat_slot=` such a member
//! contributes no voxel.
//!
//! The pass reads the surface key rather than the resolved binding, so it
//! needs no theme, `cairn check` reports it, and it cannot collide with the
//! codes for a `mat_slot=` that is there but unusable. `intent::lower`
//! hoists the value into [`Member::mat_slot`] only when it is label-shaped,
//! so the key is looked up in [`Member::intent_state`] too: absent is this
//! code's, present and unusable is `E_TYPE_MISMATCH_LABEL`'s or
//! `E_UNRESOLVED_SLOT`'s.
//!
//! The walk stops where [`super::member_scope`]'s does. Adding a
//! `mat_slot=` to a geometry row in a `site` body would silence this code
//! while `E_MISPLACED_MEMBER` still says the row produces no blocks.
//!
//! The pass reads whatever `intent::lower` produces, so the `edit ... set
//! mat_slot=` patch of `spec/components-editing-sites` "Editing model",
//! which is not implemented, has to be applied there rather than
//! downstream.

use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticNote};
use crate::intent::{BodyKind, IntentModule, Member, MemberRole};

use super::DiagnosticSink;

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    let has_a_theme = !ir.themes.is_empty();
    // Site bodies too: whether a `site` reads these roles is
    // `MemberRole::is_read_in`'s to say, not this pass's.
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
        // `member_scope::walk`'s gates: each such row is already reported
        // once, for its whole subtree.
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

/// Did the author leave this member's material unsaid?
///
/// `mat_slot` is also `None` for a value `intent::lower` could not hoist,
/// which is another code's mistake, so the key is checked too.
fn names_no_material(member: &Member) -> bool {
    member.mat_slot.is_none()
        && !member.intent_state.contains_key("mat_slot")
        && matches!(
            without_a_material(&member.role),
            WithoutAMaterial::PaintsNothing
        )
}

/// What a member of one role does to the build when it carries no
/// `mat_slot=`.
///
/// An enum rather than a `bool`: four of the five refuse nothing, for
/// different reasons, and a role that changes category says so here.
enum WithoutAMaterial {
    /// Reaches the palette through the theme's slot map and has no default
    /// block, so it contributes no voxel at all.
    PaintsNothing,
    /// Takes blocks away rather than putting them down, which needs no
    /// material of its own.
    Carves,
    /// Paints a default block and needs no `mat_slot=` to do it.
    PaintsAFallback,
    /// Names its material under a different key, whose absence is a
    /// different code's to report.
    NamesItsMaterialElsewhere,
    /// Puts no block of its own anywhere, whatever it carries.
    PutsNothingDown,
}

/// Which of those a role is.
///
/// Exhaustive, so a new role has to say. `block_array::lower`'s
/// `member_will_paint` must agree: a role that grows a fallback material
/// moves to [`WithoutAMaterial::PaintsAFallback`] in both, and
/// `tests/check/missing_material.rs` checks the pairing against the lowered
/// structure. `MemberRole::unread_arguments` relies on the `level`,
/// `circuit`, `place` and `connect` reasons below for their `mat_slot`.
fn without_a_material(role: &MemberRole) -> WithoutAMaterial {
    match role {
        MemberRole::Floor | MemberRole::Walls => WithoutAMaterial::PaintsNothing,
        // A `window` without `mat_slot=` is an opening carved to air (the
        // arrow slits of `examples/themed-tower.crn`); a `door` is carved
        // whatever it carries.
        MemberRole::Window | MemberRole::Door => WithoutAMaterial::Carves,
        // `roof` and `stair` resolve through `geometry_material_id`, which
        // falls back to a default block; `pressure_plate` falls back to
        // the edition's plate id from the registry pack.
        MemberRole::Roof | MemberRole::Stair | MemberRole::PressurePlate => {
            WithoutAMaterial::PaintsAFallback
        }
        // A walkway lays `path=@MAT`; a row without one is
        // `E_MISSING_PATH_MATERIAL`.
        MemberRole::Connect => WithoutAMaterial::NamesItsMaterialElsewhere,
        // `level` groups children judged on their own; `circuit` reserves a
        // volume the redstone phases paint into; `place` instantiates a def
        // whose members carry their own materials; an unknown keyword is
        // `E_UNKNOWN_KEYWORD`'s.
        MemberRole::Level | MemberRole::Circuit | MemberRole::Place | MemberRole::Other(_) => {
            WithoutAMaterial::PutsNothingDown
        }
    }
}

fn diagnose(member: &Member, has_a_theme: bool) -> Diagnostic {
    let keyword = member.role.keyword();
    // With no `theme` there is no slot to name, so the advice says what has
    // to exist first.
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
