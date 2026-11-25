//! The block a member whose geometry derives its blockstates paints, and the family that block
//! has to belong to for those states to have somewhere to go.

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::intent::{Member, ValueWithSpan};
use crate::resolve::ScopeResolution;

use crate::block_array::args::{diag_deferred_member_reason, member_or_slot_span};
use crate::block_array::joins::JoinKind;
use crate::block_array::material::{StateDomain, UnknownId, is_stand_in, validated_token};
use crate::block_array::roof::is_stair;
use crate::block_array::{BlockState, TargetRegistry};

/// A block family whose blockstates a member's geometry derives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    /// The `*_stairs` blocks a sloped roof, an eave `stair` and an `arch` lay.
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
    /// The standing signs whose wall sign the target has, which a `sign` stands or hangs.
    Sign,
    /// The bed blocks a `bed` lays two of.
    Bed,
}

/// The path of `id`, without its namespace.
pub(super) fn path_of(id: &str) -> &str {
    id.rsplit_once(':').map_or(id, |(_, path)| path)
}

/// The wall sign of the standing sign `id`: `oak_sign`'s `oak_wall_sign`, and Bedrock's
/// `spruce_standing_sign`'s `spruce_wall_sign`.
fn wall_sign_of(id: &str) -> String {
    match id.strip_suffix("standing_sign") {
        Some(stem) => format!("{stem}wall_sign"),
        None => format!("{}_wall_sign", id.strip_suffix("_sign").unwrap_or(id)),
    }
}

/// The block the standing sign `id` hangs as: its wall sign, written as the canonical token naming
/// it is (`spec/materials-themes` "Mappings across version and edition").
///
/// # Errors
///
/// The [`UnknownId`] of the wall sign, when the pinned target neither declares nor respells it.
pub(super) fn wall_sign_at(
    id: &str,
    registry: Option<&dyn TargetRegistry>,
) -> Result<String, Box<UnknownId>> {
    validated_token(BlockState::bare(wall_sign_of(id)), registry).map(|state| state.id)
}

/// Whether `id` names a standing sign: its path ends in `_sign`, and in neither `wall_sign` nor
/// `hanging_sign`.
fn is_standing_sign(id: &str) -> bool {
    let path = path_of(id);
    path.ends_with("_sign") && !path.ends_with("wall_sign") && !path.ends_with("hanging_sign")
}

/// Why `id` is no standing sign whose wall sign the target has, or `None` when it is one.
fn sign_misfit(id: &str, registry: Option<&dyn TargetRegistry>) -> Option<Misfit> {
    if !is_standing_sign(id) {
        return Some(Misfit::Outside);
    }
    wall_sign_at(id, registry).err().map(Misfit::NoWallSign)
}

/// Whether `id` names a bed block: its path is `bed` or ends in `_bed`.
fn is_bed(id: &str) -> bool {
    let path = path_of(id);
    path == "bed" || path.ends_with("_bed")
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

/// Why a block is not of the family a member's geometry needs.
enum Misfit {
    /// The block is of another family.
    Outside,
    /// The block is a standing sign whose wall sign the pinned target does not have.
    NoWallSign(Box<UnknownId>),
}

impl Family {
    /// Why `id` is not a block of this family, or `None` when it is one.
    fn misfit(self, id: &str, registry: Option<&dyn TargetRegistry>) -> Option<Misfit> {
        let admitted = match self {
            Self::Stair => is_stair(id),
            Self::Door => is_door(id),
            Self::Chest => is_chest(id, registry),
            Self::Button => is_button(id),
            Self::Trapdoor => is_trapdoor(id),
            Self::Pillar => is_pillar(id, registry),
            Self::Sign => return sign_misfit(id, registry),
            Self::Bed => is_bed(id),
        };
        (!admitted).then_some(Misfit::Outside)
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
            Self::Sign => "sign",
            Self::Bed => "bed",
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
            Self::Sign => "`facing` / `rotation`",
            Self::Bed => "`facing` / `part`",
        }
    }

    /// The note naming the materials of this family a slot can bind.
    fn remedy(self) -> &'static str {
        match self {
            Self::Stair => {
                "bind the slot to a `*_stairs` material — the registry pack's `roof.dark_wood`, \
                 `roof.light_wood`, `roof.warm_wood`, `roof.cool_wood` and `arch.default` all \
                 resolve to one"
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
            Self::Sign => {
                "bind the slot to a standing sign whose wall sign the target has — the registry \
                 pack's `sign.oak` resolves to one"
            }
            Self::Bed => {
                "bind the slot to a bed material — the registry pack's `bed.default` and \
                 `bed.<colour>` tokens resolve to one"
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
        && !is_stand_in(state)
        && let Some(misfit) = family.misfit(&state.id, registry)
    {
        diagnostics.push(diag_incompatible_material(
            member, state, family, &misfit, shape, slot_value,
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
    misfit: &Misfit,
    shape: &GeometryMemberDescription,
    slot_value: Option<&ValueWithSpan>,
) -> Diagnostic {
    let primary = match misfit {
        Misfit::Outside => format!(
            "{} derives {} from {} and `{}` is not a {}, so those states have nowhere to go",
            shape.subject,
            family.derived_states(),
            shape.states_from,
            state.id,
            family.name(),
        ),
        Misfit::NoWallSign(wall) => format!(
            "{} is built from a standing sign whose wall sign the target has, and `{}`, the wall \
             sign of `{}`, is not a block in `{}`",
            shape.subject, wall.id, state.id, wall.registry,
        ),
    };
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
        primary,
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
