//! What a `roof` member's line says it draws: its `kind=`, a shed's `slope_to=`, its `footprint=`
//! and `bounds=`, and the rows that shape rises above the walls.

use crate::diagnostic::Diagnostic;
use crate::intent::Member;

use crate::block_array::args::diag_invalid_member;
use crate::block_array::openings::{WallSide, Written};
use crate::block_array::roof::{
    RoofKind, flat_extra_height, gable_extra_height, hip_extra_height, shed_extra_height,
    shed_slope_span,
};

/// Every side a shed's `slope_to=` can name.
const SIDES: [WallSide; 4] = [
    WallSide::Front,
    WallSide::Back,
    WallSide::Left,
    WallSide::Right,
];

/// Resolve a `roof` member's `kind=`, or `None` after an `E_INVALID_MEMBER`.
pub(super) fn parse_roof_kind(
    member: &Member,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<RoofKind> {
    let Some(raw) = member.ident_value("kind") else {
        // A wrongly shaped value is named by its shape: `kind="shed"` spells a kind in the set.
        let reason = match member.written("kind") {
            Some(raw) => format!(
                "roof `kind=` must be an identifier naming one of {}, got {}",
                RoofKind::names(),
                raw.value.kind_name(),
            ),
            None => format!("missing `kind=` (expected one of {})", RoofKind::names()),
        };
        diagnostics.push(diag_invalid_member(member, reason, Vec::new()));
        return None;
    };
    if let Some(k) = RoofKind::from_ident(raw) {
        return Some(k);
    }
    diagnostics.push(diag_invalid_member(
        member,
        format!(
            "unknown roof `kind={raw}` (expected one of {})",
            RoofKind::names(),
        ),
        Vec::new(),
    ));
    None
}

/// Resolve a shed roof's required `slope_to=`, or `None` after an `E_INVALID_MEMBER`.
pub(super) fn shed_slope_to(
    member: &Member,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<WallSide> {
    let Some(raw) = member.ident_value("slope_to") else {
        let reason = match member.written("slope_to") {
            Some(written) => format!(
                "shed `slope_to=` must be one of front, back, left, right, not {}",
                written.value.describe(),
            ),
            None => "shed roof requires `slope_to=` (one of front, back, left, right)".to_owned(),
        };
        diagnostics.push(diag_invalid_member(member, reason, Vec::new()));
        return None;
    };
    if let Some(side) = WallSide::from_ident(raw) {
        return Some(side);
    }
    diagnostics.push(diag_invalid_member(
        member,
        format!("unknown shed `slope_to={raw}` (expected one of front, back, left, right)"),
        Vec::new(),
    ));
    None
}

/// Whether the roof's own line keeps it from drawing, after the `E_INVALID_MEMBER` that says so.
pub(super) fn roof_line_refused(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> bool {
    let refused = match parse_roof_kind(member, diagnostics) {
        None => return true,
        Some(RoofKind::Shed) => shed_slope_to(member, diagnostics).is_none(),
        Some(_) => false,
    };
    refused || extent_refused(member, diagnostics)
}

/// A roof's `footprint=` and `bounds=`, each with the one value it takes and what that value
/// builds.
const EXTENT: [(&str, &str, &str); 2] = [
    (
        "footprint",
        "struct",
        "a roof covers its struct's `size=` footprint, which `footprint=struct` names",
    ),
    (
        "bounds",
        "expand",
        "a roof's `overhang=` widens its body's block array past the footprint on every side, \
         which `bounds=expand` names",
    ),
];

/// Whether `footprint=` and `bounds=` are each absent or the value [`EXTENT`] gives it.
fn extent_readable(member: &Member) -> bool {
    EXTENT.iter().all(|&(key, value, _)| {
        member.written(key).is_none() || member.ident_value(key) == Some(value)
    })
}

/// Whether the roof's `footprint=` or `bounds=` keeps it from drawing, after the
/// `E_INVALID_MEMBER` refusing the first that does.
fn extent_refused(member: &Member, diagnostics: &mut Vec<Diagnostic>) -> bool {
    for (key, value, what) in EXTENT {
        let clause = match Written::of_ident(member, key) {
            Written::Absent => continue,
            Written::Ident(s) if s == value => continue,
            Written::Ident(s) => format!("has `{key}={s}`"),
            Written::Int(n) => format!("has `{key}={n}`"),
            Written::OtherShape(described) => format!("has a `{key}=` that is {described}"),
        };
        diagnostics.push(diag_invalid_member(
            member,
            format!("roof {clause}, not `{value}`"),
            vec![format!("{what}; write `{key}={value}` or drop `{key}=`")],
        ));
        return true;
    }
    false
}

/// The kind the roof's `kind=` names, or `None` when it names none or names a `shed` with no
/// usable `slope_to=`.
fn roof_shape(member: &Member) -> Option<RoofKind> {
    let kind = RoofKind::from_ident(member.ident_value("kind")?)?;
    if matches!(kind, RoofKind::Shed) && slope_to_of(member).is_none() {
        return None;
    }
    Some(kind)
}

/// The kind this roof will draw with, or `None` when it draws nothing, such as a `shed` with no
/// usable `slope_to=` or a roof whose `footprint=` or `bounds=` names another extent.
///
/// Both roof walks ask this, so the footprint and the height agree on which roofs are real.
pub(super) fn roof_draws(member: &Member) -> Option<RoofKind> {
    roof_shape(member).filter(|_| extent_readable(member))
}

/// The rows the roof rises above the wall top on a `roof_w` by `roof_h` box, or `None` when it
/// draws nothing.
pub(super) fn roof_rise(member: &Member, roof_w: u32, roof_h: u32) -> Option<u32> {
    roof_draws(member).map(|kind| kind_rise(kind, slope_to_of(member), roof_w, roof_h))
}

/// The rows the roof's shape rises on a `roof_w` by `roof_h` box, drawn or not, or `None` when its
/// line does not say: no readable `kind=`, or a shed with no readable `slope_to=`.
pub(super) fn shape_rise(member: &Member, roof_w: u32, roof_h: u32) -> Option<u32> {
    roof_shape(member).map(|kind| kind_rise(kind, slope_to_of(member), roof_w, roof_h))
}

/// The most rows any roof could rise on a `roof_w` by `roof_h` box, whatever its `kind=` and
/// `slope_to=` turn out to be.
pub(super) fn tallest_rise(roof_w: u32, roof_h: u32) -> u32 {
    RoofKind::ALL
        .into_iter()
        .flat_map(|kind| SIDES.map(|side| kind_rise(kind, Some(side), roof_w, roof_h)))
        .max()
        .unwrap_or(0)
}

fn slope_to_of(member: &Member) -> Option<WallSide> {
    member
        .ident_value("slope_to")
        .and_then(WallSide::from_ident)
}

/// The rise of a `kind` roof; a shed with no `slope_to` rises `0`, as it draws nothing.
fn kind_rise(kind: RoofKind, slope_to: Option<WallSide>, roof_w: u32, roof_h: u32) -> u32 {
    match kind {
        RoofKind::Gable => gable_extra_height(roof_w.min(roof_h)),
        RoofKind::Shed => slope_to.map_or(0, |side| {
            shed_extra_height(shed_slope_span(roof_w, roof_h, side))
        }),
        RoofKind::Hip => hip_extra_height(roof_w, roof_h),
        RoofKind::Flat => flat_extra_height(),
    }
}
