//! With no theme bound, or a theme whose slots bind no material, no `mat_slot=` resolves, and
//! each member degrades as its role does without a material.

use cairn_lang_core::block_array::BlockArrayIr;
use cairn_lang_core::check::DiagnosticCode;

use crate::common::{lowered, lowered_with_resolver_diagnostics, only_structure, state_at};

/// A floor left air, walls not built, a door refused for them, and a roof built from its default.
const BODY: &str = "struct s size=5x5\n  \
                    floor mat_slot=f\n  \
                    walls mat_slot=w height=3\n  \
                    door side=front at=center\n  \
                    roof kind=gable mat_slot=r\n";

/// The note every member deferral carries.
const LOWERED_ROLES: &str = "block-array lowering currently voxelises floor, walls, door, window, \
                             roof (kind=gable|shed|hip|flat), stair (kind=stairs), pressure_plate \
                             (at=<side>.outside|inside.<side>), and level y=N grouping, and \
                             recognises circuit region=<label> void=<N> (u32, N>=1) plus \
                             door[id=<name>] opened_by=sig.<name> actuator patches; other roles \
                             will be added as their lowering rules are spec'd";

/// Every finding of `out` as its code, the text it points at, its primary and its notes.
fn findings<'a>(
    src: &'a str,
    out: &'a BlockArrayIr,
) -> Vec<(DiagnosticCode, &'a str, &'a str, Vec<&'a str>)> {
    out.diagnostics
        .iter()
        .map(|d| {
            (
                d.code,
                &src[d.span.clone()],
                d.primary.as_str(),
                d.notes.iter().map(|n| n.message.as_str()).collect(),
            )
        })
        .collect()
}

#[test]
fn a_themeless_scope_is_told_how_each_role_degrades() {
    let out = lowered(BODY);
    assert_eq!(
        findings(BODY, &out),
        [
            (
                DiagnosticCode::NoThemeBound,
                BODY.trim_end(),
                "struct `s` has no theme bound, so no `mat_slot=` in it resolves to a block",
                vec![
                    "a `floor` is left air; no `walls` is built, so every `door` and `window` is \
                     refused for want of a wall; a `roof`, eave `stair` or `pressure_plate` is \
                     built from its default block",
                    "a scope binds a theme when the module declares exactly one logical theme, or \
                     the `place` names one with `theme=`; under a `--edition` pin that theme must \
                     also have a variant for that edition",
                ],
            ),
            (
                DiagnosticCode::DeferredMember,
                "door side=front at=center",
                "door requires a `walls` member that paints — a positive `height=` and a \
                 `mat_slot=` that resolves — to carve into",
                vec![LOWERED_ROLES],
            ),
            (
                DiagnosticCode::DeferredMember,
                "roof kind=gable mat_slot=r",
                "`gable` roof's `mat_slot=` did not resolve to a block id; it falls back to \
                 `minecraft:spruce_stairs`",
                vec![LOWERED_ROLES],
            ),
        ],
    );
}

#[test]
fn a_themeless_scope_builds_only_its_roof_from_the_default_block() {
    let out = lowered(BODY);
    let ba = only_structure(&out);
    // The floor row and the roof's three; the walls take none.
    assert_eq!((ba.dims.x, ba.dims.y, ba.dims.z), (5, 4, 5));
    let ids: Vec<&str> = ba.palette.entries.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "minecraft:air",
            "minecraft:spruce_stairs",
            "minecraft:spruce_stairs",
            "minecraft:spruce_stairs",
        ],
    );
    for z in 0..5 {
        for x in 0..5 {
            assert_eq!(
                state_at(ba, x, 0, z).id,
                "minecraft:air",
                "floor ({x}, {z})"
            );
        }
    }
}

#[test]
fn a_theme_whose_slots_bind_no_material_builds_what_no_theme_builds() {
    let src = format!("theme t:\n  slot f -> 5\n  slot w -> \"stone\"\n  slot r -> 7\n\n{BODY}");
    let out = lowered_with_resolver_diagnostics(&src);
    let found: Vec<_> = out
        .diagnostics
        .iter()
        .map(|d| (d.code, &src[d.span.clone()]))
        .collect();
    assert_eq!(
        found,
        [
            (DiagnosticCode::UnknownSlotTarget, "5"),
            (DiagnosticCode::UnknownSlotTarget, "\"stone\""),
            (DiagnosticCode::UnknownSlotTarget, "7"),
            (DiagnosticCode::DeferredMember, "door side=front at=center"),
            (DiagnosticCode::DeferredMember, "roof kind=gable mat_slot=r"),
        ],
    );
    assert_eq!(only_structure(&out), only_structure(&lowered(BODY)));
}
