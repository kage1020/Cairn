//! `material` pass — flags a member whose only route to a block is
//! `mat_slot=` and which was written without one.
//!
//! `floor` and `walls` read their block from the applied theme's slot map
//! and from nowhere else: a theme selector's bindings land in
//! `ResolvedMemberBinding::selector_extras`, which the block-array pass
//! never reads, and neither role has a fallback material the way `roof`,
//! `stair` and `pressure_plate` do. A member written without a `mat_slot=`
//! therefore contributes no voxel and — before this pass — no diagnostic
//! either: `cairn check` exited 0 and the structure came out empty.
//!
//! The pass reads the surface key rather than the resolved binding, which
//! is what keeps it here rather than in the resolver or in lowering. Three
//! things follow from that:
//!
//! - it needs no theme, so a module that declares none is still told which
//!   members name no material;
//! - `cairn check` reports it, which the lowering-side codes
//!   (`E_UNKNOWN_ID`, `E_INCOMPATIBLE_MATERIAL`) are not;
//! - it cannot collide with `E_UNRESOLVED_SLOT`. That code needs the key to
//!   be *present* and unusable; this one needs it absent. The same split
//!   `E_INCOMPLETE_PLACE` and `E_TYPE_MISMATCH_LABEL` draw on a `place`
//!   row, for the same reason: the two have different repairs.
//!
//! A member's material can only come from what the member itself carries
//! today. `spec/components-editing-sites.md` §9.2's `edit ... set
//! mat_slot=` patch DSL is not implemented — the grammar has no `edit`
//! item and the parser refuses one — so there is no route by which a
//! member declared bare acquires a slot later. When that lands, this pass
//! has to read the post-edit IR; it already does, in the sense that it
//! reads whatever `intent::lower` produced, so what has to hold is that
//! edits are applied there rather than downstream.

use crate::intent::{IntentModule, Member, MemberRole};

use super::{Diagnostic, DiagnosticCode, DiagnosticNote, DiagnosticSink};

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    for s in &ir.structs {
        walk(&s.members, sink);
    }
    for d in &ir.defs {
        walk(&d.members, sink);
    }
    for s in &ir.sites {
        walk(&s.placements, sink);
    }
}

fn walk(members: &[Member], sink: &mut DiagnosticSink) {
    for member in members {
        if member.mat_slot.is_none()
            && matches!(
                without_a_material(&member.role),
                WithoutAMaterial::PaintsNothing
            )
        {
            sink.push(diagnose(member));
        }
        walk(&member.children.members, sink);
    }
}

/// What a member of one role does to the build when it carries no
/// `mat_slot=`.
///
/// A value rather than a `bool` because the reason is what a reader needs:
/// three of these four look the same from the outside — nothing is refused
/// — and they are not the same fact, so a role that changes category
/// changes it here rather than in a comment.
enum WithoutAMaterial {
    /// Reaches the palette through the theme's slot map and has no default
    /// block, so it contributes no voxel at all.
    PaintsNothing,
    /// Takes blocks away rather than putting them down, which needs no
    /// material of its own.
    Carves,
    /// Paints a default block and needs no `mat_slot=` to do it.
    PaintsAFallback,
    /// Never reaches a painter.
    ReachesNoPainter,
}

/// Which of those a role is.
///
/// One exhaustive match rather than a wildcard, because the answer is a
/// property of that role's painter and a new role has to say which it is.
/// `block_array::lower`'s `member_will_paint` documents the same tie from
/// the other side: a role that grows a fallback material moves from
/// [`WithoutAMaterial::PaintsNothing`] to
/// [`WithoutAMaterial::PaintsAFallback`], and
/// `tests/check_missing_material.rs` measures the pairing against the
/// lowered structure rather than trusting either list.
fn without_a_material(role: &MemberRole) -> WithoutAMaterial {
    match role {
        MemberRole::Floor | MemberRole::Walls => WithoutAMaterial::PaintsNothing,
        // A `window` with no `mat_slot=` is an *opening*: the rectangle is
        // carved to air, which is how `examples/themed-tower.crn` punches
        // arrow slits through a stone wall without choosing a species for
        // them. Refusing it would refuse a feature. A `door` carves its
        // doorway the same way.
        MemberRole::Window | MemberRole::Door => WithoutAMaterial::Carves,
        // `roof` and `stair` resolve through `geometry_material_id`, which
        // falls back to a default block; `pressure_plate` falls back to
        // the edition's plate id from the registry pack.
        MemberRole::Roof | MemberRole::Stair | MemberRole::PressurePlate => {
            WithoutAMaterial::PaintsAFallback
        }
        // `level` groups its children, which are judged on their own;
        // `circuit` reserves a volume for the redstone passes; `place` and
        // `connect` are site rows, refused inside a struct body by
        // `check::member_scope` and naming no material inside a `site`;
        // and a keyword the role table does not know is
        // `E_UNKNOWN_KEYWORD`'s, with no painter reached.
        MemberRole::Level
        | MemberRole::Circuit
        | MemberRole::Place
        | MemberRole::Connect
        | MemberRole::Other(_) => WithoutAMaterial::ReachesNoPainter,
    }
}

fn diagnose(member: &Member) -> Diagnostic {
    let keyword = MemberRole::keyword(&member.role);
    Diagnostic {
        code: DiagnosticCode::MissingMaterial,
        span: member.span.clone(),
        primary: format!("`{keyword}` has no `mat_slot=`, so it paints nothing"),
        notes: vec![DiagnosticNote {
            span: None,
            message: format!(
                "add `mat_slot=NAME` to the `{keyword}`, naming a slot the applied theme declares",
            ),
        }],
        data: None,
    }
}
