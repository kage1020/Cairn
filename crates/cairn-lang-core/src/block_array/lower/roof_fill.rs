//! Painting a `roof` member, and the material rule it shares with the
//! eave `stair`.

use crate::ast::ValueKind;
use crate::check::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::intent::{Member, ValueWithSpan};
use crate::resolve::ScopeResolution;

use crate::block_array::args::{diag_deferred_member_reason, member_or_slot_span};
use crate::block_array::openings::WallSide;
use crate::block_array::roof::{
    RoofKind, RoofVoxel, flat_block_state, flat_voxels, gable_ridge_axis, gable_stair_state,
    gable_voxels, hip_stair_state, hip_voxels, is_stair, shed_stair_state, shed_voxels,
};
use crate::block_array::{BlockState, PaletteIndex};

use super::canvas::{MemberCanvas, ScopePalette};
use super::members::{StructCtx, resolve_member_state};
use super::sizing::roof_draws;

pub(super) fn fill_roof(
    member: &Member,
    ctx: &StructCtx<'_>,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(kind) = parse_roof_kind(member, diagnostics) else {
        return;
    };
    // Resolved behind [`roof_draws`], the gate the volume goes through, and
    // not before it. A `shed` with no usable `slope_to=` draws nothing, and
    // resolving it anyway reported a material for a roof that is not there:
    // `W_ABSTRACT_TOKEN_DEFERRED` saying it is built from its default block,
    // and `geometry_material_id`'s `W_DEFERRED_MEMBER` naming that block.
    // `parse_roof_kind` has read a kind, so that shed is the one roof turned
    // away here, and [`shed_slope_to`] is what says why.
    if roof_draws(member).is_none() {
        shed_slope_to(member, diagnostics);
        return;
    }
    let resolved = resolve_member_state(
        member,
        ctx.scope,
        ctx.registry,
        diagnostics,
        ctx.theme_missing,
    );
    let base_id = geometry_material_id(
        member,
        ctx.scope,
        resolved.as_ref(),
        kind.base_block_id(),
        &GeometryMemberDescription {
            subject: format!("`{}` roof", kind.name()),
            states_from: "the geometry",
            requires_stair: kind.paints_stairs(),
        },
        diagnostics,
    );

    let (roof_w, roof_h, wall_top) = (ctx.dims.x, ctx.dims.z, ctx.wall_top);
    match kind {
        RoofKind::Gable => {
            let ridge_axis = gable_ridge_axis(roof_w, roof_h);
            paint_roof(
                gable_voxels(roof_w, roof_h, wall_top),
                |face| gable_stair_state(ridge_axis, face),
                base_id,
                palette,
                canvas,
            );
        }
        RoofKind::Shed => {
            let Some(slope_to) = shed_slope_to(member, diagnostics) else {
                return;
            };
            paint_roof(
                shed_voxels(roof_w, roof_h, wall_top, slope_to),
                |face| shed_stair_state(slope_to, face),
                base_id,
                palette,
                canvas,
            );
        }
        RoofKind::Hip => {
            // Hip and gable share the same long-axis-wins-with-x-tiebreak
            // ridge rule (`spec/compilation` "Hip roof voxel rules" falls
            // through to "Gable roof voxel rules"). Reusing
            // `gable_ridge_axis` keeps the two paths from drifting if the
            // tiebreak rule ever changes.
            let ridge_axis = gable_ridge_axis(roof_w, roof_h);
            paint_roof(
                hip_voxels(roof_w, roof_h, wall_top),
                |face| hip_stair_state(ridge_axis, face),
                base_id,
                palette,
                canvas,
            );
        }
        // A deck is one material, so its voxels share the one face `()`.
        RoofKind::Flat => paint_roof(
            flat_voxels(roof_w, roof_h, wall_top)
                .into_iter()
                .map(|pos| RoofVoxel { pos, face: () }),
            |()| flat_block_state(),
            base_id,
            palette,
            canvas,
        ),
    }
}

/// Paint a roof's voxels, each with its face's blockstate under the id
/// `base_id`.
///
/// One `palette.intern` per face rather than one per voxel — a 99-voxel
/// cottage roof needs four — but claimed on first use, not up front. A roof
/// need not emit every face its kind has: an odd ridge span converges on a
/// single row, so a gable that has one (a 3x3 struct) emits three of the
/// five faces and never the apex pair; and a shed one deep is a single apex
/// row with no slope under it, since the layer count is the slope span
/// floored at one and the topmost layer is always the apex. An entry no
/// voxel references is not free: it reaches the `.nbt` palette, the
/// `resolved_ir_hash`, and the per-entry counts `cairn info` reports. A flat
/// deck's one slot is claimed from inside its first write the same way, so
/// the rule "no entry without a voxel" holds at every generator.
///
/// Claiming on demand also means the palette lays out in the order the
/// generator first visits each face, which is fixed by its layer
/// iteration, so the layout stays deterministic. A claim is looked up by
/// the face itself rather than through a face → slot table, so a face enum
/// that grows or reorders cannot send one face to another's slot.
fn paint_roof<F: Copy + PartialEq>(
    voxels: impl IntoIterator<Item = RoofVoxel<F>>,
    state_of: impl Fn(F) -> BlockState,
    base_id: &str,
    palette: &mut ScopePalette,
    canvas: &mut MemberCanvas<'_>,
) {
    let mut claimed: Vec<(F, PaletteIndex)> = Vec::new();
    for RoofVoxel { pos, face } in voxels {
        canvas.paint(pos, || {
            if let Some(&(_, idx)) = claimed.iter().find(|(seen, _)| *seen == face) {
                return idx;
            }
            let mut state = state_of(face);
            base_id.clone_into(&mut state.id);
            let idx = palette.intern(state);
            claimed.push((face, idx));
            idx
        });
    }
}

/// What a geometry-driven member does with the material it is given.
///
/// Lets one function serve every such member: it can name the member and
/// say where its blockstates came from without knowing which one it is.
pub(super) struct GeometryMemberDescription {
    /// How the member names itself in a message, already quoted: a roof
    /// renders as `` `gable` roof ``, an eave as `` eave `stair` ``.
    pub(super) subject: String,
    /// Where its blockstates come from, as a noun phrase: a roof reads the
    /// geometry, an eave stair reads its own arguments.
    pub(super) states_from: &'static str,
    /// Whether it attaches stair blockstates, and therefore needs a
    /// material from the stair family.
    pub(super) requires_stair: bool,
}

/// The block id a geometry-driven member paints, given its resolved
/// `mat_slot=` binding.
///
/// One function for the roof and the eave stair: they take the same three
/// decisions on the same grounds, and a member that attaches blockstates
/// has the same obligations wherever it appears. `fallback` is the kind's
/// own id, used when the binding supplies nothing usable:
///
/// - no `mat_slot=` at all — silent, the member asked for nothing;
/// - a `mat_slot=` that resolved to nothing — the resolver already said why
///   (`E_UNRESOLVED_SLOT`, `E_UNKNOWN_ABSTRACT_TOKEN`, …) or the theme is
///   missing entirely (`W_NO_THEME_BOUND` fired against the scope), and the
///   `W_DEFERRED_MEMBER` here anchors the consequence to the member that
///   wears the fallback;
/// - a `mat_slot=` that resolved outside the stair family, when this member
///   needs one. `E_INCOMPATIBLE_MATERIAL`, which stops the build: a whole
///   block has nowhere to put `facing` / `half` / `shape`, and neither
///   answer available without the author is acceptable — attaching them
///   anyway writes a blockstate that does not exist, and substituting the
///   fallback builds the member out of a material nobody chose. The
///   fallback is still returned so the rest of the pass has a coherent
///   palette to finish with; nothing reaches disk, because the finding is
///   an error.
///
/// A resolved state that carries `properties` is reported and its id still
/// used — the member derives its own states and drops the binding's, which
/// is a smaller thing than the material being the wrong shape. Checked
/// after the family, because properties on an id the member is not going to
/// paint are nothing the author needs to hear about.
pub(super) fn geometry_material_id<'a>(
    member: &Member,
    scope: Option<&ScopeResolution>,
    resolved: Option<&'a BlockState>,
    fallback: &'a str,
    shape: &GeometryMemberDescription,
    diagnostics: &mut Vec<Diagnostic>,
) -> &'a str {
    let Some(state) = resolved else {
        if member.mat_slot.is_some() {
            // The theme is named because it is the thing the author edits,
            // and because without it two placements under two themes write
            // the same sentence. This is the arm sibling-variant softening
            // reaches: with no `--edition` pin the resolver stays silent on
            // a slot only the sibling variant declares, so lowering is the
            // only reporter, and the member line is shared by every
            // placement of the `def`. Two broken themes then differ in
            // nothing at all, and fixing one of them changes no output.
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
        return fallback;
    };
    let slot_value = scope
        .and_then(|scope| scope.members.get(&member.span.start))
        .and_then(|binding| binding.slot_value.as_ref());
    if shape.requires_stair && !is_stair(&state.id) {
        diagnostics.push(diag_incompatible_material(member, state, shape, slot_value));
        return fallback;
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
    &state.id
}

/// A member whose geometry attaches blockstates was bound to a material
/// that cannot carry them.
///
/// Anchored on the theme's slot value when there is one, because that line
/// is where the fix goes — the member itself only names a slot, and every
/// member reading that slot has the same problem for the same reason.
fn diag_incompatible_material(
    member: &Member,
    state: &BlockState,
    shape: &GeometryMemberDescription,
    slot_value: Option<&ValueWithSpan>,
) -> Diagnostic {
    let mut notes = vec![DiagnosticNote {
        span: None,
        message: "bind the slot to a `*_stairs` material — the registry pack's \
                  `roof.dark_wood`, `roof.light_wood`, `roof.warm_wood`, and \
                  `roof.cool_wood` all resolve to one"
            .to_owned(),
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
            "{} derives `facing` / `half` / `shape` from {} and `{}` is not a stair, so those states have nowhere to go",
            shape.subject, shape.states_from, state.id,
        ),
        notes,
        data: Some(DiagnosticData::IncompatibleMaterial {
            id: state.id.clone(),
            required: "stair".to_owned(),
            slot: member.mat_slot.as_ref().map(|slot| slot.name.clone()),
            token: slot_value.and_then(|v| match &v.value.kind {
                ValueKind::Token(token) => Some(token.clone()),
                // Anything else here was already refused upstream
                // (`E_UNKNOWN_SLOT_TARGET`), so there is no token to name.
                _ => None,
            }),
        }),
    }
}

/// Resolve a `roof` member's `kind=` to a [`RoofKind`].
///
/// Pushes a `W_DEFERRED_MEMBER` warning and returns `None` when the
/// `kind=` is missing, typed wrong, or names a kind outside the supported
/// set. Keeping the dispatch table in [`RoofKind::from_ident`] and the
/// diagnostic phrasing here lets each side stay self-contained.
fn parse_roof_kind(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<RoofKind> {
    let Some(raw) = member.ident_value("kind") else {
        // A value of the wrong shape is named by its shape, not by the
        // closed set: `kind="shed"` spells a kind that is in the set, and
        // being told the set again answers a question the author did not
        // ask. `recognize_circuit_region` words the same case the same way.
        // It matters more since `check::arguments` began deferring to this
        // finding — a `kind=` naming no rule is where the conditional
        // argument check stays quiet, so this is the whole repair.
        let reason = match member.intent_state.get("kind") {
            Some(raw) => format!(
                "roof `kind=` must be an identifier naming one of {}, got {}",
                RoofKind::names(),
                raw.value.kind_name(),
            ),
            None => format!("missing `kind=` (expected one of {})", RoofKind::names()),
        };
        diagnostics.push(diag_deferred_member_reason(member, &reason));
        return None;
    };
    if let Some(k) = RoofKind::from_ident(raw) {
        return Some(k);
    }
    diagnostics.push(diag_deferred_member_reason(
        member,
        &format!(
            "unknown roof `kind={raw}` (expected one of {})",
            RoofKind::names(),
        ),
    ));
    None
}

/// Resolve a shed roof's `slope_to=` argument.
///
/// Required for `kind=shed` because the slope direction has no sensible
/// default — picking one silently would let a typo emit a roof that
/// peaks on the wrong wall. Missing or mis-typed `slope_to=` therefore
/// surfaces a `W_DEFERRED_MEMBER` warning.
pub(super) fn shed_slope_to(
    member: &Member,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<WallSide> {
    let Some(raw) = member.ident_value("slope_to") else {
        let reason = match member.intent_state.get("slope_to") {
            Some(written) => format!(
                "shed `slope_to=` must be one of front, back, left, right, not {}",
                written.value.describe(),
            ),
            None => "shed roof requires `slope_to=` (one of front, back, left, right)".to_owned(),
        };
        diagnostics.push(diag_deferred_member_reason(member, &reason));
        return None;
    };
    if let Some(side) = WallSide::from_ident(raw) {
        return Some(side);
    }
    diagnostics.push(diag_deferred_member_reason(
        member,
        &format!("unknown shed `slope_to={raw}` (expected one of front, back, left, right)"),
    ));
    None
}
