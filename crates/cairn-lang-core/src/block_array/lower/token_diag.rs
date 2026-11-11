//! Findings about a material token or state literal, worded for the site
//! that wrote it.

use crate::ast::ValueKind;
use crate::diagnostic::{Diagnostic, DiagnosticCode, DiagnosticData, DiagnosticNote};
use crate::error::Span;
use crate::intent::{MemberRole, ValueWithSpan};
use crate::suggest::{candidate_list, did_you_mean_note, nearest_match};

use crate::block_array::BlockState;
use crate::block_array::material::{
    AbsentConcept, DeclaredElsewhere, DomainFault, IdOrigin, OutOfDomain, Sides, StateDomain,
    TargetRegistry, UnknownId, UnmappedState,
};

/// What becomes of a member whose `mat_slot=` resolves to no material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MemberFallback {
    /// A `floor`: `fill_floor` is not reached, so its cells stay air.
    Air,
    /// A `fill`: it paints none of its box, which keeps what the other members paint there.
    Unpainted,
    /// A `walls`: it paints no row and claims none in the volume, so a cut into it is refused.
    Absent,
    /// A `window`: [`fill_window`] refuses it, so the wall stays and a port on it is refused.
    ///
    /// [`fill_window`]: super::window::fill_window
    WallStays,
    /// A `roof`, an eave `stair`, a `door`, a `pressure_plate`, a `button` or a `chest`: built
    /// from its default block.
    DefaultBlock,
}

impl MemberFallback {
    /// The consequence for a member of this role, each spelled out as in [`paint_member`].
    ///
    /// [`paint_member`]: super::paint::paint_member
    pub(super) fn for_role(role: &MemberRole) -> Self {
        match role {
            MemberRole::Floor => Self::Air,
            MemberRole::Fill => Self::Unpainted,
            MemberRole::Walls => Self::Absent,
            MemberRole::Window => Self::WallStays,
            MemberRole::Roof
            | MemberRole::Stair
            | MemberRole::Door
            | MemberRole::PressurePlate
            | MemberRole::Button
            | MemberRole::Chest => Self::DefaultBlock,
            MemberRole::Level
            | MemberRole::Lever
            | MemberRole::Daylight
            | MemberRole::Observer
            | MemberRole::Lamp
            | MemberRole::Piston
            | MemberRole::Dispenser
            | MemberRole::Repeater
            | MemberRole::Comparator
            | MemberRole::RedstoneTorch
            | MemberRole::RedstoneDust
            | MemberRole::Circuit
            | MemberRole::RawBlock
            | MemberRole::RawFill
            | MemberRole::Place
            | MemberRole::Connect
            | MemberRole::Other(_) => {
                unreachable!(
                    "no painter resolves the `mat_slot=` of a `{}`",
                    MemberRole::keyword(role)
                )
            }
        }
    }
}

/// Where an `@token` was read from, for the prose of the abstract-token diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TokenSite {
    /// A `connect` row's `path=`.
    WalkwayPath,
    /// A member's `mat_slot=` binding, with what becomes of the member without it.
    MemberSlot(MemberFallback),
}

impl TokenSite {
    fn token_noun(self) -> &'static str {
        match self {
            Self::WalkwayPath => "abstract path token",
            Self::MemberSlot(_) => "abstract token",
        }
    }

    /// What the build does instead, following "cannot be lowered without the registry pack; ".
    fn consequence(self) -> &'static str {
        match self {
            Self::WalkwayPath => "the walkway falls back to air",
            Self::MemberSlot(MemberFallback::Air) => "the cell falls back to air",
            Self::MemberSlot(MemberFallback::Unpainted) => "the fill paints none of its box",
            Self::MemberSlot(MemberFallback::Absent) => {
                "the walls are not built and take up no rows, so a door or window cut into them \
                 is refused"
            }
            Self::MemberSlot(MemberFallback::WallStays) => {
                "the window is not cut, and the wall stays"
            }
            Self::MemberSlot(MemberFallback::DefaultBlock) => {
                "the member is built from its default block"
            }
        }
    }

    fn canonical_example(self) -> &'static str {
        match self {
            Self::WalkwayPath => "path=@gravel",
            Self::MemberSlot(_) => "@oak_planks",
        }
    }

    fn catalog_note(self) -> &'static str {
        match self {
            Self::WalkwayPath => {
                "abstract path tokens must be declared in the pack's `materials` catalog"
            }
            Self::MemberSlot(_) => {
                "abstract material tokens must be declared in the pack's `materials` catalog \
                 (see `spec/materials-themes` \"Canonical vocabulary\")"
            }
        }
    }
}

pub(super) fn diag_abstract_token(span: Span, token: &str, site: TokenSite) -> Diagnostic {
    Diagnostic {
        code: DiagnosticCode::AbstractTokenDeferred,
        span,
        primary: format!(
            "{} `@{token}` cannot be lowered without the registry pack; {}",
            site.token_noun(),
            site.consequence(),
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: format!(
                "use a canonical block token (e.g. `{}`) until the registry pack ships",
                site.canonical_example(),
            ),
        }],
        data: None,
    }
}

pub(super) fn diag_unknown_abstract_token(
    span: Span,
    token: &str,
    suggestion: Option<&str>,
    site: TokenSite,
) -> Diagnostic {
    let mut notes = Vec::with_capacity(2);
    notes.extend(suggestion.map(|s| did_you_mean_note(&format!("@{s}"))));
    notes.push(DiagnosticNote {
        span: None,
        message: site.catalog_note().to_owned(),
    });
    Diagnostic {
        code: DiagnosticCode::UnknownAbstractToken,
        span,
        primary: format!(
            "{} `@{token}` is not declared by the registry pack's materials catalog",
            site.token_noun(),
        ),
        notes,
        data: None,
    }
}

/// Check the state literal `value` carries against the pinned target, reporting each pair outside
/// its block's states, or the literal as unchecked where the target records none; `false` when a
/// pair is outside, and the member or walkway binding it is then not built.
pub(super) fn check_state_literal(
    value: &ValueWithSpan,
    state: &BlockState,
    registry: Option<&dyn TargetRegistry>,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let (ValueKind::Token(text) | ValueKind::BlockId(text), Some(registry)) =
        (&value.value.kind, registry)
    else {
        return true;
    };
    if state.properties.is_empty() || !text.contains('[') {
        return true;
    }
    match registry.state_domain(state) {
        StateDomain::Unpinned | StateDomain::AnsweredElsewhere => true,
        StateDomain::Unrecorded => {
            diagnostics.push(diag_state_literal_unchecked(
                value.span.clone(),
                state,
                &target_label(registry),
            ));
            true
        }
        StateDomain::Checked(faults) => {
            let target = target_label(registry);
            diagnostics.extend(faults.iter().map(|fault| {
                diag_state_domain(
                    value.span.clone(),
                    &state.id,
                    fault,
                    &target,
                    StateOrigin::Literal,
                )
            }));
            faults.is_empty()
        }
    }
}

/// Report each pair of `state`, which lowering built for the member at `span`, outside its
/// block's states at the pinned target.
pub(super) fn diag_derived_state_domain(
    span: &Span,
    state: &BlockState,
    registry: &dyn TargetRegistry,
) -> Vec<Diagnostic> {
    let StateDomain::Checked(faults) = registry.state_domain(state) else {
        return Vec::new();
    };
    let target = target_label(registry);
    faults
        .iter()
        .map(|fault| {
            diag_state_domain(
                span.clone(),
                &state.id,
                fault,
                &target,
                StateOrigin::Derived,
            )
        })
        .collect()
}

/// How the pinned target reads in a finding.
fn target_label(registry: &dyn TargetRegistry) -> String {
    registry
        .block_ids()
        .map_or_else(|| "the target".to_owned(), |ids| ids.label().to_owned())
}

/// Who wrote a state an `E_STATE_DOMAIN` is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StateOrigin {
    /// A state literal in the source.
    Literal,
    /// Lowering, for the member's geometry.
    Derived,
}

impl StateOrigin {
    fn kind(self) -> &'static str {
        match self {
            Self::Literal => "literal",
            Self::Derived => "derived",
        }
    }
}

/// One pair of a state on block `id` that `target` does not take.
fn diag_state_domain(
    span: Span,
    id: &str,
    fault: &OutOfDomain,
    target: &str,
    origin: StateOrigin,
) -> Diagnostic {
    let OutOfDomain {
        key,
        value,
        fault: kind,
        valid_at,
    } = fault;
    let valid = kind.valid();
    let listed = valid.join(", ");
    let (primary, suggestion) = match kind {
        DomainFault::UnknownProperty { .. } => {
            let takes = if valid.is_empty() {
                "it takes no state".to_owned()
            } else {
                format!("its states are {listed}")
            };
            (
                format!("`{id}` has no state `{key}` at {target}; {takes}"),
                nearest_match(key, valid.iter().map(String::as_str))
                    .map(|near| (near.to_owned(), format!("{near}={value}"))),
            )
        }
        DomainFault::ValueOutside { .. } => (
            format!("`{id}` takes no `{key}={value}` at {target}; `{key}` takes {listed}"),
            nearest_match(value, valid.iter().map(String::as_str))
                .map(|near| (near.to_owned(), format!("{key}={near}"))),
        ),
    };
    let mut notes = Vec::with_capacity(2);
    // A derived pair is lowering's, so the author has no literal to correct.
    if origin == StateOrigin::Literal {
        notes.extend(suggestion.as_ref().map(|(_, pair)| did_you_mean_note(pair)));
    }
    let elsewhere = (!valid_at.is_empty())
        .then(|| format!("`{key}={value}` is valid at {}", valid_at.join(", ")));
    let correct = match kind {
        DomainFault::UnknownProperty { .. } if valid.is_empty() => {
            format!("remove `{key}` from the state literal")
        }
        DomainFault::UnknownProperty { .. } => {
            format!("correct the state literal to a state {target} takes")
        }
        DomainFault::ValueOutside { .. } => {
            format!("correct the state literal to a value {target} takes")
        }
    };
    let fix = match (origin, elsewhere, kind) {
        (StateOrigin::Derived, elsewhere, _) => format!(
            "{}lowering derived this state for the member, so Cairn's derivation or the registry \
             pack's table for the block is wrong; bind the member's material to another block",
            elsewhere.map_or_else(String::new, |at| format!("{at}, but ")),
        ),
        (StateOrigin::Literal, Some(at), _) => format!(
            "{at}: pass {} as `--target`, or {correct}",
            if valid_at.len() == 1 {
                "that version"
            } else {
                "one of them"
            },
        ),
        (StateOrigin::Literal, None, DomainFault::UnknownProperty { .. }) if valid.is_empty() => {
            correct
        }
        (StateOrigin::Literal, None, DomainFault::UnknownProperty { .. }) => format!(
            "remove `{key}` from the state literal, or correct it to a state {target} takes"
        ),
        (StateOrigin::Literal, None, DomainFault::ValueOutside { .. }) => {
            format!("{correct}, or leave `{key}` out for the block's default")
        }
    };
    notes.push(DiagnosticNote {
        span: None,
        message: fix,
    });
    Diagnostic {
        code: DiagnosticCode::StateDomain,
        span,
        primary,
        notes,
        data: Some(DiagnosticData::StateDomain {
            id: id.to_owned(),
            key: key.clone(),
            value: value.clone(),
            registry: target.to_owned(),
            reason: kind.kind().to_owned(),
            valid: valid.to_vec(),
            origin: origin.kind().to_owned(),
            suggestion: suggestion.map(|(near, _)| near),
            valid_at: valid_at.clone(),
        }),
    }
}

/// Warn that a state literal reaches the output unchecked, since `target` records no states for
/// its block.
fn diag_state_literal_unchecked(span: Span, state: &BlockState, target: &str) -> Diagnostic {
    let pairs = state.properties_text();
    Diagnostic {
        code: DiagnosticCode::StateLiteralUnchecked,
        span,
        primary: format!(
            "`{id}` is written with `{pairs}` unchecked: {target}'s registry pack records no \
             states for the block",
            id = state.id
        ),
        notes: vec![DiagnosticNote {
            span: None,
            message: "a property the block does not have, or a value outside its domain, \
                      reaches the structure file unchanged; check each one against the \
                      block's states in the target edition and version"
                .to_owned(),
        }],
        data: None,
    }
}

/// What named a material, which says how to name another.
#[derive(Debug, Clone, Copy)]
pub(super) enum NamedBy {
    /// The theme binding a member's `mat_slot=`, or the member's default.
    Binding,
    /// A raw member's `mat=`.
    RawMat,
    /// A walkway's `path=`.
    Path,
}

/// Report `state`, built at `span`, when the pinned target's state map has no form for it.
pub(super) fn diag_state_unmapped(
    span: Span,
    state: &BlockState,
    registry: &dyn TargetRegistry,
    named_by: NamedBy,
) -> Option<Diagnostic> {
    let UnmappedState {
        registry: target,
        reason,
        mapped_at,
    } = registry.unmapped_state(state)?;
    let states = state.properties_text();
    let (entry, carried) = if states.is_empty() {
        (state.id.clone(), "the block")
    } else {
        (format!("{}[{states}]", state.id), "these states")
    };
    let pinned = match mapped_at.as_slice() {
        [] => String::new(),
        [one] => format!("pass `--target {one}`, whose state map carries {carried}, "),
        [init @ .., last] => format!(
            "pass `--target` {} or {last}, whose state maps carry {carried}, ",
            init.join(", ")
        ),
    };
    let choose = match named_by {
        NamedBy::Binding => "bind the material to",
        NamedBy::RawMat => "set `mat=` to",
        NamedBy::Path => "set `path=` to",
    };
    Some(Diagnostic {
        code: DiagnosticCode::StateUnmapped,
        span,
        primary: format!("`{entry}` has no form at {target}: {reason}"),
        notes: vec![DiagnosticNote {
            span: None,
            message: format!(
                "{pinned}{choose} a block {target} has a form for, or build for the other edition"
            ),
        }],
        data: Some(DiagnosticData::StateUnmapped {
            id: state.id.clone(),
            states,
            registry: target,
        }),
    })
}

/// Where a block id was written, which decides what its finding may advise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum IdSite {
    /// A `raw_block` or `raw_fill`'s `mat=`, which names an edition's own block.
    Raw,
    /// A semantic member's block, or a `connect` row's `path=`.
    Semantic(TokenSite),
}

/// Report a block id the compile's target does not declare: `E_PARITY_UNSUPPORTED` when a semantic
/// line named a block only the other edition has, and `E_UNKNOWN_ID` otherwise.
pub(super) fn diag_undeclared_id(span: Span, unknown: &UnknownId, site: IdSite) -> Diagnostic {
    match (site, &unknown.origin, &unknown.absent) {
        (IdSite::Semantic(token_site), IdOrigin::Authored, Some(absent)) => {
            diag_parity_unsupported(span, unknown, absent, token_site)
        }
        _ => diag_unknown_id(span, unknown, site),
    }
}

/// Report a semantic line's block that only `absent.declared_by` has, with what the line can name
/// instead (`spec/versioning-editions` "A block only the other edition has").
fn diag_parity_unsupported(
    span: Span,
    unknown: &UnknownId,
    absent: &AbsentConcept,
    site: TokenSite,
) -> Diagnostic {
    let UnknownId { id, registry, .. } = unknown;
    let has = absent.declared_by;
    let lacks = has.other();
    let instead = match site {
        TokenSite::MemberSlot(_) => format!(
            "the semantic layer is edition-neutral: bind this slot from a `theme NAME_{has}` and \
             give `theme NAME_{lacks}` a block {} has, or place the block with a `raw_block` under \
             `@edition {has}`, which only a {} build keeps",
            lacks.title(),
            has.title(),
        ),
        TokenSite::WalkwayPath => "a walkway is laid alike for every edition, and its `site` body \
             binds no slot and builds nothing under a guard: set `path=` to a block both editions \
             have, or to an abstract token each edition's materials catalog maps"
            .to_owned(),
    };
    Diagnostic {
        code: DiagnosticCode::ParityUnsupported,
        span,
        primary: format!(
            "`{id}` is a block only {} has, so `{registry}` cannot build it",
            has.title()
        ),
        notes: vec![
            DiagnosticNote {
                span: None,
                message: absent.reason.clone(),
            },
            DiagnosticNote {
                span: None,
                message: instead,
            },
        ],
        data: Some(DiagnosticData::ParityUnsupported {
            id: id.clone(),
            registry: registry.clone(),
            declared_by: has.as_str().to_owned(),
            reason: absent.reason.clone(),
        }),
    }
}

/// Report a block id the compile's target does not declare, worded for who chose the id.
fn diag_unknown_id(span: Span, unknown: &UnknownId, site: IdSite) -> Diagnostic {
    let UnknownId {
        id,
        registry,
        origin,
        suggestion,
        aliases,
        elsewhere,
        absent,
    } = unknown;
    let mut notes = Vec::with_capacity(3);
    match origin {
        IdOrigin::Authored => {}
        IdOrigin::Catalog { token } => notes.push(DiagnosticNote {
            span: None,
            message: format!(
                "the registry pack maps `@{token}` onto it; the token is fine and the pack's mapping is not, so this is not a fix you make here",
            ),
        }),
        IdOrigin::Builtin { token } => notes.push(DiagnosticNote {
            span: None,
            message: format!(
                "the registry pack declares no `{token}`, so lowering fell back to the id compiled into the compiler; the pack needs that row",
            ),
        }),
    }
    let versions = elsewhere
        .as_ref()
        .map(|elsewhere| version_note(id, registry, origin, elsewhere));
    // An alias note wins; without one, the version note stands in for the nearest spelling.
    let candidate = match (aliases.as_slice(), &versions, suggestion) {
        _ if absent.is_some() => None,
        ([], Some(_), _) => None,
        ([], None, None) => Some(format!(
            "no block in `{registry}` is near enough to suggest; compile against a target that declares `{id}`, or pick a block this one has",
        )),
        ([], None, Some(candidate)) => Some(format!(
            "`{registry}` spells the nearest block `{candidate}`"
        )),
        (spellings, ..) => Some(format!(
            "`{registry}` spells this block {}, per the registry pack's alias table",
            candidate_list(spellings),
        )),
    };
    let absence = absent.as_ref().map(|absent| {
        let has = absent.declared_by;
        let keep = match site {
            IdSite::Raw => format!(
                "; put this line under `@edition {has}`, so that only a {} build keeps it",
                has.title()
            ),
            IdSite::Semantic(_) => String::new(),
        };
        format!(
            "`{id}` is a block only {} has: {}{keep}",
            has.title(),
            absent.reason
        )
    });
    notes.extend(
        candidate
            .into_iter()
            .chain(versions)
            .chain(absence)
            .map(|message| DiagnosticNote {
                span: None,
                message,
            }),
    );
    Diagnostic {
        code: DiagnosticCode::UnknownId,
        span,
        primary: format!("`{id}` is not a block in `{registry}`"),
        notes,
        data: Some(DiagnosticData::UnknownId {
            id: id.clone(),
            registry: registry.clone(),
            origin: origin.kind().to_owned(),
            token: origin.token().map(str::to_owned),
            suggestion: suggestion.clone(),
            aliases: aliases.clone(),
            since: elsewhere
                .as_ref()
                .and_then(|e| e.since().map(str::to_owned)),
            until: elsewhere
                .as_ref()
                .and_then(|e| e.until().map(str::to_owned)),
            declared_by: absent
                .as_ref()
                .map(|absent| absent.declared_by.as_str().to_owned()),
        }),
    }
}

/// Which targets of the edition declare `id`, which `registry` does not, and how to pick one; the
/// `@requires` floor is offered only for an id the author named.
fn version_note(
    id: &str,
    registry: &str,
    origin: &IdOrigin,
    elsewhere: &DeclaredElsewhere,
) -> String {
    let edition = elsewhere.edition().as_str();
    match elsewhere.sides() {
        Sides::Since(since) => {
            let floor = if matches!(origin, IdOrigin::Authored) {
                format!(
                    ", or declare `@requires {edition} version>=V`, with V the release that added \
                     it ({since} at the latest), so a target without it is refused at the floor"
                )
            } else {
                String::new()
            };
            format!(
                "the first target that declares `{id}` is {edition} {since}: build with \
                 `--target {since}` or another target that declares it{floor}"
            )
        }
        Sides::Until(until) => format!(
            "the last target that declares `{id}` is {edition} {until}: build with \
             `--target {until}` or another target that declares it, or pick a block `{registry}` \
             has"
        ),
        Sides::Both { until, since } => format!(
            "{edition} {until} declares `{id}` before this target and {since} after it: build \
             with one of them as `--target`, or pick a block `{registry}` has"
        ),
    }
}
