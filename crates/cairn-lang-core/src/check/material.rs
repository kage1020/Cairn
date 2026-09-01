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
//! - it cannot collide with the codes that own a `mat_slot=` which *is*
//!   there. That is why the question is "did the author write the key",
//!   asked of [`Member::intent_state`], and not "did a `mat_slot`
//!   field survive lowering": `intent::lower` hoists the value into
//!   [`Member::mat_slot`] only when it is label-shaped, so
//!   `mat_slot=@oak_planks` leaves that field empty while the key is
//!   plainly on the line. Reading the field alone would say "has no
//!   `mat_slot=`" about a line that has one, next to
//!   `E_TYPE_MISMATCH_LABEL` pointing at the value. Absent is this code's,
//!   present-and-unusable is `E_TYPE_MISMATCH_LABEL`'s or
//!   `E_UNRESOLVED_SLOT`'s, and no member earns two of them.
//!
//! The walk stops where [`super::member_scope`]'s does, and for the reason
//! that pass and [`super::nesting`] both give: a row the enclosing body
//! cannot read, and everything indented under it, is one mistake with one
//! repair, and a second finding inside it sends the author somewhere else.
//! Here the two repairs actively disagree — adding a `mat_slot=` to a
//! geometry row written into a `site` body silences this code and leaves
//! `E_MISPLACED_MEMBER` still saying the row produces no blocks.
//!
//! A member's material can only come from what the member itself carries
//! today. `spec/components-editing-sites.md` §9.2's `edit ... set
//! mat_slot=` patch DSL is not implemented — the grammar has no `edit`
//! item and the parser refuses one — so there is no route by which a
//! member declared bare acquires a slot later. When that lands, this pass
//! has to read the post-edit IR; it already does, in the sense that it
//! reads whatever `intent::lower` produced, so what has to hold is that
//! edits are applied there rather than downstream.

use crate::intent::{BodyKind, IntentModule, Member, MemberRole};

use super::{Diagnostic, DiagnosticCode, DiagnosticNote, DiagnosticSink};

pub(super) fn run(ir: &IntentModule, sink: &mut DiagnosticSink) {
    let has_a_theme = !ir.themes.is_empty();
    for s in &ir.structs {
        walk(&s.members, BodyKind::Geometry, false, has_a_theme, sink);
    }
    for d in &ir.defs {
        walk(&d.members, BodyKind::Geometry, false, has_a_theme, sink);
    }
    // A `site` body reads neither of the roles this pass reports, so the
    // `is_read_in` gate below empties this loop. It is here because every
    // other pass walks all three bodies and a reader should not have to
    // work out whether the omission was deliberate — and because the
    // emptiness is a fact about `MemberRole::is_read_in`, which can change,
    // rather than about this pass.
    for s in &ir.sites {
        walk(&s.placements, BodyKind::Site, false, has_a_theme, sink);
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
        // Both gates are `member_scope::walk`'s, in the same order and for
        // the same reason: an unknown keyword has no vocabulary to be
        // judged by, and a row this body cannot read is already reported
        // once, at its root, for the whole subtree.
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
/// Both halves are needed. `mat_slot` is `None` for a key that is absent
/// *and* for one whose value `intent::lower` could not hoist, and the
/// second is a different mistake with a different code — see the module
/// documentation.
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
/// A value rather than a `bool` because the reason is what a reader needs:
/// four of these five look the same from the outside — nothing is refused
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
    /// Names its material under a different key, whose absence is a
    /// different code's to report.
    NamesItsMaterialElsewhere,
    /// Puts no block of its own anywhere, whatever it carries.
    PutsNothingDown,
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
        // them. Refusing it would refuse a feature. A `door` is a carve
        // whatever it carries — `carve_door` never asks for a material at
        // all — so it reaches no painter to be starved of one.
        MemberRole::Window | MemberRole::Door => WithoutAMaterial::Carves,
        // `roof` and `stair` resolve through `geometry_material_id`, which
        // falls back to a default block; `pressure_plate` falls back to
        // the edition's plate id from the registry pack.
        MemberRole::Roof | MemberRole::Stair | MemberRole::PressurePlate => {
            WithoutAMaterial::PaintsAFallback
        }
        // A `connect` row *is* voxelised — walkway lowering lays a strip
        // between the two ports — but the material it lays is `path=@MAT`
        // rather than a `mat_slot=`, and a row without one is
        // `E_MISSING_PATH_MATERIAL`. Same shape as this code, one key
        // over, already owned.
        MemberRole::Connect => WithoutAMaterial::NamesItsMaterialElsewhere,
        // `level` groups its children, which are judged on their own;
        // `circuit` reserves a volume that the redstone phases later paint
        // into, and puts down no block itself; `place` instantiates a def,
        // whose members carry their own materials; and a keyword the role
        // table does not know is `E_UNKNOWN_KEYWORD`'s.
        MemberRole::Level | MemberRole::Circuit | MemberRole::Place | MemberRole::Other(_) => {
            WithoutAMaterial::PutsNothingDown
        }
    }
}

fn diagnose(member: &Member, has_a_theme: bool) -> Diagnostic {
    let keyword = member.role.keyword();
    // `spec/lint` asks a message for what is wrong, what would be valid,
    // and how to fix it. In a module with no `theme` at all the middle
    // term is empty — there is no slot vocabulary to name — so the advice
    // says what has to exist first rather than pointing at a set that does
    // not.
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
