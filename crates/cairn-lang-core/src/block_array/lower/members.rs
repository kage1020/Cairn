//! The paint set of a body: its members, the phase that paints each, and the material each reads.

use crate::diagnostic::Diagnostic;
use crate::intent::{Member, MemberRole, ValueWithSpan};
use crate::resolve::ScopeResolution;

use crate::block_array::args::{
    diag_deferred_member_reason, diag_invalid_member, member_or_slot_span,
};
use crate::block_array::material::{MaterialDeferred, TargetRegistry, resolve_block_state};
use crate::block_array::openings::WallSide;
use crate::block_array::wall_column::WallColumn;
use crate::block_array::{BlockState, Dims, PaletteIndex};

use super::canvas::ScopePalette;
use super::phase::Phase;
use super::roof_kind::roof_line_refused;
use super::token_diag::{
    MemberFallback, TokenSite, diag_abstract_token, diag_state_literal_unchecked,
    diag_unknown_abstract_token, diag_unknown_id,
};
use super::written::Written;

/// Per-body context every member painter reads.
pub(super) struct StructCtx<'a> {
    pub(super) scope: Option<&'a ScopeResolution>,
    pub(super) registry: Option<&'a dyn TargetRegistry>,
    pub(super) theme_missing: bool,
    pub(super) dims: Dims,
    pub(super) overhang: u32,
    pub(super) interior_w: u32,
    pub(super) interior_h: u32,
    /// Highest wall row; the roof starts above it. A cut checks [`StructCtx::wall_column`] instead.
    pub(super) wall_top: u32,
    /// Rows the tallest roof that draws rises above [`Self::wall_top`].
    pub(super) rise: u32,
    /// Every row the walls members paint, gaps and all; ports read this same column.
    pub(super) wall_column: WallColumn,
    /// The struct as written, which a member's own line is judged against.
    pub(super) written: Written,
}

impl StructCtx<'_> {
    /// The note on a refusal above the struct as written when a roof whose shape does not read
    /// may raise its top row, or `None`.
    pub(super) fn unread_roof_note(&self) -> Option<String> {
        (self.written.reach > self.written.height).then(|| {
            format!(
                "a `roof` whose `kind=` or `slope_to=` does not read may raise it to row {}",
                self.written.reach - 1,
            )
        })
    }

    /// Which members keep [`Self::dims`] below the written height.
    pub(super) fn shortfall(&self) -> Shortfall {
        Shortfall {
            walls: self.written.wall_top > self.wall_top,
            roof: self.written.rise > self.rise,
        }
    }
}

/// Which members keep the struct built below the struct as written.
#[derive(Debug, Clone, Copy)]
pub(super) struct Shortfall {
    /// A `walls` member that would reach higher paints nothing.
    walls: bool,
    /// A `roof` that would rise higher draws nothing.
    roof: bool,
}

impl Shortfall {
    /// The members the missing rows wait on, completing "rows N to M wait on …".
    pub(super) fn members(self) -> &'static str {
        match self {
            Self {
                walls: true,
                roof: true,
            } => "a `walls` member that paints nothing and a `roof` that draws nothing",
            Self { roof: true, .. } => "a `roof` that draws nothing",
            Self { .. } => "a `walls` member that paints nothing",
        }
    }

    /// The note on a member deferred to a row the struct as written reaches and the struct built
    /// does not, with `outcome` saying what happens to it then: "the stair is built".
    pub(super) fn note(self, outcome: &str) -> String {
        match self {
            Self {
                walls: true,
                roof: true,
            } => format!(
                "a `walls` member that paints nothing and a `roof` that draws nothing keep the \
                 struct below this row; {outcome} once both build"
            ),
            Self { roof: true, .. } => format!(
                "a `roof` that would rise to this row draws nothing; {outcome} once that roof draws"
            ),
            Self { .. } => format!(
                "a `walls` member that would reach this row paints nothing; {outcome} once that \
                 member's material resolves to a block"
            ),
        }
    }
}

/// What the phase model does with a member.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MemberDisposition {
    /// Evaluated in this phase, and paints voxels there.
    Paints(Phase),
    /// Reserves something a later pass reads and paints nothing; its shape is still checked.
    Reserves,
    /// Nothing in this pass lowers it; it earns a `W_DEFERRED_MEMBER`.
    NotLowered,
}

/// Where a member's role puts it, per `spec/compilation` "Phase evaluation".
///
/// Every role is spelled out, so a new one fails the compile here instead of panicking later.
pub(super) fn member_disposition(role: &MemberRole) -> MemberDisposition {
    match role {
        MemberRole::Floor | MemberRole::Walls => MemberDisposition::Paints(Phase::Massing),
        MemberRole::Roof | MemberRole::Stair => MemberDisposition::Paints(Phase::Envelope),
        MemberRole::Door | MemberRole::Window => MemberDisposition::Paints(Phase::Openings),
        MemberRole::PressurePlate => MemberDisposition::Paints(Phase::Fixtures),
        MemberRole::Circuit => MemberDisposition::Reserves,
        MemberRole::Place | MemberRole::Connect | MemberRole::Other(_) => {
            MemberDisposition::NotLowered
        }
        MemberRole::Level => unreachable!(
            "`Level` members must be flattened before phase-bucketing (see `flatten_members`)"
        ),
    }
}

/// The note on an opening deferred because the `walls` member written to hold its rows paints
/// nothing.
pub(super) const UNPAINTED_COURSE: &str = "a `walls` member written to hold these rows paints \
                                           nothing; the opening is cut once that member's \
                                           material resolves to a block";

/// The note on a member deferred because a `walls` member under a `level` that is not placed may
/// hold `rows`, with `outcome` saying what happens to it then: "the door is cut".
pub(super) fn unplaced_walls_note(rows: &str, outcome: &str) -> String {
    format!(
        "a `level` that is not placed holds a `walls` member that may reach {rows}; {outcome} once \
         that `level` is placed"
    )
}

/// The note on a `level` with no `y=`, saying what one is.
const LEVEL_Y_FORM: &str = "write `level y=N`, where `N` is a non-negative integer: the members \
                            under it are measured from row `N`, as the members of the body are \
                            from row 0";

/// The `E_INVALID_MEMBER` on a `level` whose `y=` is missing or not a non-negative integer.
fn level_offset_refusal(level: &Member) -> Diagnostic {
    let primary = match level.intent_state.get("y") {
        Some(raw) => format!(
            "level `y=` must be a non-negative integer that fits in u32, not {}",
            raw.value.describe(),
        ),
        None => "level has no `y=`, so nothing says which row its members stand on".to_owned(),
    };
    diag_invalid_member(level, primary, vec![LEVEL_Y_FORM.to_owned()])
}

/// Flatten `level` blocks into the body's paint set, each member with its level's `y` offset.
///
/// Sizing reads the same list, so the volume and the paint agree on which members count.
pub(super) fn flatten_members<'a>(
    members: &'a [Member],
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<(u32, &'a Member)> {
    let mut out: Vec<(u32, &'a Member)> = Vec::new();
    for member in members {
        if matches!(member.role, MemberRole::Level) {
            let Some(y_offset) = member.nonneg_u32("y") else {
                // `check` refuses a level with members under it and no offset as
                // `E_UNSUPPORTED_NESTING`.
                if member.children.members.is_empty() {
                    diagnostics.push(level_offset_refusal(member));
                }
                continue;
            };
            for child in &member.children.members {
                // `check` refuses a level inside a level as `E_UNSUPPORTED_NESTING`.
                if matches!(child.role, MemberRole::Level) {
                    continue;
                }
                if let Some(reason) = level_offset_role_defer(&child.role, y_offset) {
                    // A roof's own line is read first, so the deferral hides none of its faults.
                    let refused = matches!(child.role, MemberRole::Roof)
                        && roof_line_refused(child, diagnostics);
                    if !refused {
                        diagnostics.push(diag_deferred_member_reason(child, reason));
                    }
                    continue;
                }
                out.push((y_offset, child));
            }
        } else {
            out.push((0, member));
        }
    }
    out
}

/// Why a role has no lowering at a non-zero `level y=`, or `None` when it lowers at any offset.
///
/// Every role is spelled out, as in [`member_disposition`].
fn level_offset_role_defer(role: &MemberRole, y_offset: u32) -> Option<&'static str> {
    if y_offset == 0 {
        return None;
    }
    match role {
        MemberRole::Floor => Some("level-scoped `floor` is not yet supported"),
        MemberRole::Roof => Some("level-scoped `roof` is not yet supported"),
        // A level-scoped `circuit` region is not shifted: its recogniser takes no `y_offset`.
        MemberRole::Walls
        | MemberRole::Door
        | MemberRole::Window
        | MemberRole::Stair
        | MemberRole::PressurePlate
        | MemberRole::Circuit
        | MemberRole::Place
        | MemberRole::Connect
        | MemberRole::Other(_) => None,
        MemberRole::Level => unreachable!(
            "a nested `level` is dropped by `flatten_members` before this function sees it"
        ),
    }
}

/// Resolve a member's `mat_slot=` binding into a [`BlockState`] without touching the palette.
///
/// `None` when there is no material, with at most one diagnostic saying why.
pub(super) fn resolve_member_state(
    member: &Member,
    scope: Option<&ScopeResolution>,
    registry: Option<&dyn TargetRegistry>,
    diagnostics: &mut Vec<Diagnostic>,
    theme_missing: bool,
) -> Option<BlockState> {
    if theme_missing {
        return None;
    }
    let scope = scope?;
    let binding = scope.members.get(&member.span.start)?;
    // INVARIANT(upstream-diagnosed): every reason `slot_value` is `None` is reported on the same
    // diagnostic stream; `ResolvedMemberBinding::slot_value` names who reports each.
    let slot_value: &ValueWithSpan = binding.slot_value.as_ref()?;
    match resolve_block_state(slot_value, registry) {
        Ok(state) => {
            diagnostics.extend(diag_state_literal_unchecked(slot_value, &state));
            Some(state)
        }
        Err(MaterialDeferred::Abstract(token)) => {
            diagnostics.push(diag_abstract_token(
                member_or_slot_span(member, slot_value),
                &token,
                TokenSite::MemberSlot(MemberFallback::for_role(&member.role)),
            ));
            None
        }
        Err(MaterialDeferred::UnknownAbstract { token, suggestion }) => {
            diagnostics.push(diag_unknown_abstract_token(
                member_or_slot_span(member, slot_value),
                &token,
                suggestion.as_deref(),
                TokenSite::MemberSlot(MemberFallback::for_role(&member.role)),
            ));
            None
        }
        Err(MaterialDeferred::UnknownId(unknown)) => {
            diagnostics.push(diag_unknown_id(slot_value.span.clone(), &unknown));
            None
        }
        Err(MaterialDeferred::AlreadyDiagnosed) => {
            // INVARIANT(upstream-diagnosed): `check_slot_targets` reports `E_UNKNOWN_SLOT_TARGET`.
            None
        }
    }
}

/// [`resolve_member_state`], interned.
pub(super) fn palette_index_for(
    member: &Member,
    scope: Option<&ScopeResolution>,
    registry: Option<&dyn TargetRegistry>,
    palette: &mut ScopePalette,
    diagnostics: &mut Vec<Diagnostic>,
    theme_missing: bool,
) -> Option<PaletteIndex> {
    resolve_member_state(member, scope, registry, diagnostics, theme_missing)
        .map(|state| palette.intern(state))
}

/// Whether this member will put a block anywhere, asked as the massing phase paints it.
pub(super) fn member_will_paint(
    member: &Member,
    scope: Option<&ScopeResolution>,
    registry: Option<&dyn TargetRegistry>,
    theme_missing: bool,
) -> bool {
    let mut ignored_diagnostics = Vec::new();
    resolve_member_state(
        member,
        scope,
        registry,
        &mut ignored_diagnostics,
        theme_missing,
    )
    .is_some()
}

pub(super) fn side_of(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> Option<WallSide> {
    let Some(raw) = member.ident_value("side") else {
        let reason = match member.intent_state.get("side") {
            Some(written) => format!(
                "`side=` must be one of front, back, left, right, not {}",
                written.value.describe(),
            ),
            None => "missing `side=` (expected one of front, back, left, right)".to_owned(),
        };
        diagnostics.push(diag_invalid_member(member, reason, Vec::new()));
        return None;
    };
    if let Some(side) = WallSide::from_ident(raw) {
        return Some(side);
    }
    diagnostics.push(diag_invalid_member(
        member,
        format!("unknown `side={raw}` (expected one of front, back, left, right)"),
        Vec::new(),
    ));
    None
}

pub(super) fn side_name(side: WallSide) -> &'static str {
    match side {
        WallSide::Front => "front",
        WallSide::Back => "back",
        WallSide::Left => "left",
        WallSide::Right => "right",
    }
}
