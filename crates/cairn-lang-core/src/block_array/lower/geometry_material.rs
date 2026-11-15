//! The block a member whose geometry derives its blockstates paints, and the family that block
//! has to belong to for those states to have somewhere to go.

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::intent::{Member, ValueWithSpan};
use crate::resolve::ScopeResolution;

use crate::block_array::args::{diag_deferred_member_reason, member_or_slot_span};
use crate::block_array::joins::JoinKind;
use crate::block_array::material::{StateDomain, is_stand_in};
use crate::block_array::roof::is_stair;
use crate::block_array::{BlockState, TargetRegistry};

/// A block family whose blockstates a member's geometry derives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    /// The `*_stairs` blocks a sloped roof and an eave `stair` lay.
    Stair,
    /// The `*_door` blocks a `door` sets in its opening.
    Door,
    /// The blocks the registry pack reads as chests, which a `chest` sets against a wall.
    Chest,
    /// The `*_button` blocks a `button` hangs on a wall.
    Button,
    /// The trapdoor blocks a `trapdoor` hangs by a wall or sets at a triple.
    Trapdoor,
    /// The blocks with an `axis` taking `x`, `y` and `z`, which a `beam` lays on their side.
    Pillar,
}

/// The path of `id`, without its namespace.
fn path_of(id: &str) -> &str {
    id.rsplit_once(':').map_or(id, |(_, path)| path)
}

/// Whether `id` names a door block: its path ends in `_door`.
fn is_door(id: &str) -> bool {
    path_of(id).ends_with("_door")
}

/// Whether `id` names a button block: its path ends in `_button`.
fn is_button(id: &str) -> bool {
    path_of(id).ends_with("_button")
}

/// Whether `id` names a trapdoor block: its path is `trapdoor` or ends in `_trapdoor`.
fn is_trapdoor(id: &str) -> bool {
    let path = path_of(id);
    path == "trapdoor" || path.ends_with("_trapdoor")
}

/// Whether `id` names a pillar block: one whose `axis` takes `x`, `y` and `z` at the pinned
/// target, or, where the target cannot say, any block (`spec/blockstate` "Derive by default,
/// promote on override").
fn is_pillar(id: &str, registry: Option<&dyn TargetRegistry>) -> bool {
    let Some(registry) = registry else {
        return true;
    };
    ["x", "y", "z"].into_iter().all(|axis| {
        let mut state = BlockState::bare(id);
        state.properties.insert("axis".to_owned(), axis.to_owned());
        match registry.state_domain(&state) {
            StateDomain::Checked(faults) => faults.is_empty(),
            StateDomain::Unpinned | StateDomain::Unrecorded | StateDomain::AnsweredElsewhere => {
                true
            }
        }
    })
}

/// Whether `id` names a chest block: one the pack reads as a chest, or, where it cannot say, one
/// whose path is `chest` or ends in `_chest` (`spec/blockstate` "Derive by default, promote on
/// override").
fn is_chest(id: &str, registry: Option<&dyn TargetRegistry>) -> bool {
    if let Some(joins) = registry.and_then(|registry| registry.joins(&BlockState::bare(id))) {
        return matches!(joins.kind, Some(JoinKind::Chest { .. }));
    }
    let path = path_of(id);
    path == "chest" || path.ends_with("_chest")
}

impl Family {
    /// Whether `id` is a block of this family.
    fn admits(self, id: &str, registry: Option<&dyn TargetRegistry>) -> bool {
        match self {
            Self::Stair => is_stair(id),
            Self::Door => is_door(id),
            Self::Chest => is_chest(id, registry),
            Self::Button => is_button(id),
            Self::Trapdoor => is_trapdoor(id),
            Self::Pillar => is_pillar(id, registry),
        }
    }

    /// The family as the payload's `required` names it (`spec/lint` "Machine-readable payload").
    fn name(self) -> &'static str {
        match self {
            Self::Stair => "stair",
            Self::Door => "door",
            Self::Chest => "chest",
            Self::Button => "button",
            Self::Trapdoor => "trapdoor",
            Self::Pillar => "pillar",
        }
    }

    /// The states the geometry derives, as a finding lists them.
    fn derived_states(self) -> &'static str {
        match self {
            Self::Stair => "`facing` / `half` / `shape`",
            Self::Door => "`facing` / `half` / `hinge`",
            Self::Chest => "`facing` / `type`",
            Self::Button => "`face` / `facing`",
            Self::Trapdoor => "`facing` / `half` / `open`",
            Self::Pillar => "`axis`",
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
            Self::Button => {
                "bind the slot to a `*_button` material — the registry pack's `button.default` \
                 resolves to one"
            }
            Self::Trapdoor => {
                "bind the slot to a `*_trapdoor` material — the registry pack's \
                 `trapdoor.default` resolves to one"
            }
            Self::Pillar => {
                "bind the slot to a block whose `axis` takes `x`, `y` and `z`, such as a log — \
                 the registry pack's `beam.default` resolves to one"
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
/// `mat_slot=` binding; `fallback` answers when the binding is absent or unusable, and when it
/// names a material outside [`GeometryMemberDescription::requires`] (`E_INCOMPATIBLE_MATERIAL`).
///
/// # Errors
///
/// `fallback`'s error, when the member falls back to it.
pub(super) fn geometry_material_id<E>(
    member: &Member,
    scope: Option<&ScopeResolution>,
    registry: Option<&dyn TargetRegistry>,
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
        && !family.admits(&state.id, registry)
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
