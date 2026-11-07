//! The block a member whose geometry derives its blockstates paints, and the family that block
//! has to belong to for those states to have somewhere to go.

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::intent::{Member, ValueWithSpan};
use crate::resolve::ScopeResolution;

use crate::block_array::BlockState;
use crate::block_array::args::{diag_deferred_member_reason, member_or_slot_span};
use crate::block_array::material::is_stand_in;
use crate::block_array::roof::is_stair;

/// A block family whose blockstates a member's geometry derives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    /// The `*_stairs` blocks a sloped roof and an eave `stair` lay.
    Stair,
    /// The `*_door` blocks a `door` sets in its opening.
    Door,
    /// The `chest` and `*_chest` blocks a `chest` sets against a wall.
    Chest,
}

/// The path of `id`, without its namespace.
fn path_of(id: &str) -> &str {
    id.rsplit_once(':').map_or(id, |(_, path)| path)
}

/// Whether `id` names a door block: its path ends in `_door`.
fn is_door(id: &str) -> bool {
    path_of(id).ends_with("_door")
}

/// Whether `id` names a chest block: its path is `chest` or ends in `_chest`.
fn is_chest(id: &str) -> bool {
    let path = path_of(id);
    path == "chest" || path.ends_with("_chest")
}

impl Family {
    /// Whether `id` is a block of this family.
    fn admits(self, id: &str) -> bool {
        match self {
            Self::Stair => is_stair(id),
            Self::Door => is_door(id),
            Self::Chest => is_chest(id),
        }
    }

    /// The family as the payload's `required` names it (`spec/lint` "Machine-readable payload").
    fn name(self) -> &'static str {
        match self {
            Self::Stair => "stair",
            Self::Door => "door",
            Self::Chest => "chest",
        }
    }

    /// The states the geometry derives, as a finding lists them.
    fn derived_states(self) -> &'static str {
        match self {
            Self::Stair => "`facing` / `half` / `shape`",
            Self::Door => "`facing` / `half` / `hinge`",
            Self::Chest => "`facing` / `type`",
        }
    }

    /// The note naming the materials of this family a slot can bind.
    fn remedy(self) -> &'static str {
        match self {
            Self::Stair => {
                "bind the slot to a `*_stairs` material — the registry pack's `roof.dark_wood`, \
                 `roof.light_wood`, `roof.warm_wood`, and `roof.cool_wood` all resolve to one"
            }
            Self::Door => {
                "bind the slot to a `*_door` material — the registry pack's `door.default` \
                 resolves to one"
            }
            Self::Chest => {
                "bind the slot to a chest material — the registry pack's `chest.default` \
                 resolves to one"
            }
        }
    }
}

/// How a geometry-driven member names itself and its blockstates in a material finding.
pub(super) struct GeometryMemberDescription {
    /// How the member names itself in a message, already quoted: `` `gable` roof ``.
    pub(super) subject: String,
    /// Where its blockstates come from, as a noun phrase.
    pub(super) states_from: &'static str,
    /// The family whose blockstates it attaches, and so the family its material must be from.
    pub(super) requires: Option<Family>,
}

/// The block id a member whose geometry derives its blockstates paints, given its resolved
/// `mat_slot=` binding; `fallback` answers when the binding is absent or unusable.
///
/// A material outside [`GeometryMemberDescription::requires`] is `E_INCOMPATIBLE_MATERIAL`, and the
/// member is still built from `fallback` so the pass can finish.
pub(super) fn geometry_material_id<E>(
    member: &Member,
    scope: Option<&ScopeResolution>,
    resolved: Option<&BlockState>,
    fallback: impl FnOnce() -> Result<String, E>,
    shape: &GeometryMemberDescription,
    diagnostics: &mut Vec<Diagnostic>,
) -> Result<String, E> {
    let Some(state) = resolved else {
        let fallback = fallback()?;
        if member.mat_slot.is_some() {
            // Names the theme, so two placements under two themes write two findings.
            let under = scope
                .and_then(|scope| scope.bound_theme.as_deref())
                .map_or_else(String::new, |theme| format!(" under theme `{theme}`"));
            diagnostics.push(diag_deferred_member_reason(
                member,
                &format!(
                    "{}'s `mat_slot=` did not resolve to a block id{under}; it falls back to `{fallback}`",
                    shape.subject,
                ),
            ));
        }
        return Ok(fallback);
    };
    let slot_value = scope
        .and_then(|scope| scope.members.get(&member.span.start))
        .and_then(|binding| binding.slot_value.as_ref());
    // Whether a stand-in belongs to the family is the target's pack to say.
    if let Some(family) = shape.requires
        && !family.admits(&state.id)
        && !is_stand_in(state)
    {
        diagnostics.push(diag_incompatible_material(
            member, state, family, shape, slot_value,
        ));
        return fallback();
    }
    if !state.properties.is_empty() {
        diagnostics.push(diag_deferred_member_reason(
            member,
            &format!(
                "{} derives its blockstates from {}; the `mat_slot=` binding to `{}[...]` also carried properties and was not applied verbatim",
                shape.subject,
                shape.states_from,
                state.id,
            ),
        ));
    }
    Ok(state.id.clone())
}

/// A member whose geometry attaches blockstates was bound to a material that cannot carry them.
///
/// Anchored on the theme's slot value when there is one, since that is where the fix goes.
fn diag_incompatible_material(
    member: &Member,
    state: &BlockState,
    family: Family,
    shape: &GeometryMemberDescription,
    slot_value: Option<&ValueWithSpan>,
) -> Diagnostic {
    let mut notes = vec![DiagnosticNote {
        span: None,
        message: family.remedy().to_owned(),
    }];
    if let Some(slot) = &member.mat_slot {
        notes.push(DiagnosticNote {
            span: None,
            message: format!(
                "reached through `mat_slot={}`, so every member reading that slot has it too",
                slot.name,
            ),
        });
    }
    Diagnostic {
        code: DiagnosticCode::IncompatibleMaterial,
        span: slot_value.map_or_else(|| member.span.clone(), |v| member_or_slot_span(member, v)),
        primary: format!(
            "{} derives {} from {} and `{}` is not a {}, so those states have nowhere to go",
            shape.subject,
            family.derived_states(),
            shape.states_from,
            state.id,
            family.name(),
        ),
        notes,
        data: Some(DiagnosticData::IncompatibleMaterial {
            id: state.id.clone(),
            required: family.name().to_owned(),
            slot: member.mat_slot.as_ref().map(|slot| slot.name.clone()),
            token: slot_value.and_then(|v| match &v.value.kind {
                ValueKind::Token(token) => Some(token.clone()),
                // Anything else was refused upstream as `E_UNKNOWN_SLOT_TARGET`.
                _ => None,
            }),
        }),
    }
}
