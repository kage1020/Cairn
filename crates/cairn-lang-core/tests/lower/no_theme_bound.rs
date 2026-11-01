//! With no theme bound, or a theme whose slots bind no material, no `mat_slot=` resolves, and
//! each member degrades as its role does without a material.

use std::collections::BTreeSet;

use cairn_lang_core::block_array::{BlockArray, BlockArrayIr};
use cairn_lang_core::check::DiagnosticCode;

use crate::common::{
    defers, lowered, lowered_with_resolver_diagnostics, notes, only_structure, slice, state_at,
};

/// One member of each role, every slot reader naming a slot of its own.
const BODY: &str = "struct s size=5x5\n  \
                    floor mat_slot=f\n  \
                    walls mat_slot=w height=3\n  \
                    door side=front at=center\n  \
                    window side=back y=1 offset=1 size=1x1 mat_slot=g\n  \
                    roof kind=gable mat_slot=r overhang=1\n  \
                    stair kind=stairs side=left y=0 mat_slot=e\n  \
                    pressure_plate at=front.outside y=0 mat_slot=p\n";

/// Walls tall enough to hold an eave `stair` and a `pressure_plate` near their top.
const TALL: &str = "struct s size=9x5\n  \
                    walls mat_slot=w height=6\n  \
                    roof kind=gable mat_slot=r overhang=1\n  \
                    stair kind=stairs side=front y=6 mat_slot=e\n  \
                    pressure_plate at=front.outside y=5 mat_slot=p\n";

/// The `W_NO_THEME_BOUND` a themeless `struct s` earns.
const S_HAS_NO_THEME: &str =
    "struct `s` has no theme bound, so no `mat_slot=` in it resolves to a block";

/// The deferral a `door` earns with no `walls` that paints.
const DOOR_HAS_NO_WALL: &str =
    "door has no wall to cut into: no `walls` member on this struct paints";

/// The deferral `BODY`'s `window` earns with no `walls` that paints.
const WINDOW_HAS_NO_WALL: &str =
    "window at y=1 size=1x1 has no wall to cut into: no `walls` member on this struct paints";

/// Each finding of `out` as its code, the text it points at and its primary.
fn findings<'a>(src: &'a str, out: &'a BlockArrayIr) -> Vec<(DiagnosticCode, &'a str, &'a str)> {
    out.diagnostics
        .iter()
        .map(|d| (d.code, slice(src, d), d.primary.as_str()))
        .collect()
}

/// The ids `ba` paints, air included.
fn painted_ids(ba: &BlockArray) -> BTreeSet<&str> {
    ba.palette.entries.iter().map(|s| s.id.as_str()).collect()
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
                S_HAS_NO_THEME
            ),
            (
                DiagnosticCode::DeferredMember,
                "door side=front at=center",
                DOOR_HAS_NO_WALL,
            ),
            (
                DiagnosticCode::DeferredMember,
                "window side=back y=1 offset=1 size=1x1 mat_slot=g",
                WINDOW_HAS_NO_WALL,
            ),
            (
                DiagnosticCode::DeferredMember,
                "roof kind=gable mat_slot=r overhang=1",
                "`gable` roof's `mat_slot=` did not resolve to a block id; it falls back to \
                 `minecraft:spruce_stairs`",
            ),
            (
                DiagnosticCode::DeferredMember,
                "stair kind=stairs side=left y=0 mat_slot=e",
                "eave `stair`'s `mat_slot=` did not resolve to a block id; it falls back to \
                 `minecraft:spruce_stairs`",
            ),
        ],
    );
    assert_eq!(
        notes(&out.diagnostics[0]),
        [
            "a `floor` is left air; no `walls` is built or takes up rows, so no `door` or \
             `window` is cut; a `roof` is built from its default block, and so is an eave \
             `stair` or `pressure_plate` that still fits the shorter structure",
            "a scope binds a theme when the module declares exactly one logical theme, or the \
             `place` names one with `theme=`; under a `--edition` pin that theme must also have \
             a variant for that edition",
        ],
    );
}

#[test]
fn a_themeless_scope_builds_its_roof_eave_and_plate_from_their_default_blocks() {
    let out = lowered(BODY);
    let ba = only_structure(&out);
    // The floor row and the roof's four; the walls take none.
    assert_eq!((ba.dims.x, ba.dims.y, ba.dims.z), (7, 5, 7));
    assert_eq!(
        painted_ids(ba),
        BTreeSet::from([
            "minecraft:air",
            "minecraft:oak_pressure_plate",
            "minecraft:spruce_stairs",
        ]),
    );
    for z in 1..6 {
        for x in 1..6 {
            assert_eq!(
                state_at(ba, x, 0, z).id,
                "minecraft:air",
                "floor ({x}, {z})"
            );
        }
        assert_eq!(
            state_at(ba, 0, 0, z).id,
            "minecraft:spruce_stairs",
            "eave {z}"
        );
    }
    assert_eq!(state_at(ba, 1, 0, 6).id, "minecraft:oak_pressure_plate");
}

#[test]
fn a_themeless_scope_refuses_an_eave_only_its_walls_would_have_reached() {
    let out = lowered(TALL);
    assert_eq!(
        findings(TALL, &out),
        [
            (
                DiagnosticCode::NoThemeBound,
                TALL.trim_end(),
                S_HAS_NO_THEME
            ),
            (
                DiagnosticCode::DeferredMember,
                "roof kind=gable mat_slot=r overhang=1",
                "`gable` roof's `mat_slot=` did not resolve to a block id; it falls back to \
                 `minecraft:spruce_stairs`",
            ),
            (
                DiagnosticCode::DeferredMember,
                "stair kind=stairs side=front y=6 mat_slot=e",
                "stair y=6 does not fit in the struct (dims.y=5)",
            ),
            (
                DiagnosticCode::DeferredMember,
                "pressure_plate at=front.outside y=5 mat_slot=p",
                "pressure_plate y=5 does not fit in the struct (dims.y=5)",
            ),
        ],
    );
    assert_eq!(
        notes(&out.diagnostics[3]),
        [
            "a `walls` member that would reach this row paints nothing; the plate fits once that \
          member's material resolves to a block"
        ],
    );
    let themed = format!(
        "theme t:\n  slot w -> @stone\n  slot r -> @oak_stairs\n  slot e -> @oak_stairs\n  \
         slot p -> @oak_pressure_plate\n\n{TALL}"
    );
    assert_eq!(
        findings(&themed, &lowered_with_resolver_diagnostics(&themed)),
        [] as [(DiagnosticCode, &str, &str); 0],
    );
}

#[test]
fn every_roof_kind_is_built_from_its_default_block_with_no_theme() {
    for (kind, default) in [
        ("gable", "minecraft:spruce_stairs"),
        ("hip", "minecraft:spruce_stairs"),
        ("shed slope_to=back", "minecraft:spruce_stairs"),
        ("flat", "minecraft:spruce_planks"),
    ] {
        let src = format!("struct s size=5x5\n  roof kind={kind} mat_slot=r\n");
        let out = lowered(&src);
        let name = kind.split(' ').next().unwrap_or(kind);
        assert_eq!(
            defers(&out),
            [format!(
                "`{name}` roof's `mat_slot=` did not resolve to a block id; it falls back to \
                 `{default}`"
            )],
        );
        assert_eq!(
            painted_ids(only_structure(&out)),
            BTreeSet::from(["minecraft:air", default]),
            "{kind}",
        );
    }
}

#[test]
fn a_theme_whose_slots_bind_no_material_builds_what_no_theme_builds() {
    let src = format!(
        "theme t:\n  slot f -> 5\n  slot w -> \"stone\"\n  slot g -> 1\n  slot r -> 7\n  \
         slot e -> 8\n  slot p -> 9\n\n{BODY}"
    );
    let out = lowered_with_resolver_diagnostics(&src);
    let unbound: Vec<String> = ["f", "w", "g", "r", "e", "p"]
        .iter()
        .map(|slot| {
            format!(
                "slot `{slot}` in theme `t` does not bind to a canonical or abstract material \
                 token"
            )
        })
        .collect();
    let mut expected: Vec<(DiagnosticCode, &str, &str)> = ["5", "\"stone\"", "1", "7", "8", "9"]
        .into_iter()
        .zip(&unbound)
        .map(|(value, primary)| (DiagnosticCode::UnknownSlotTarget, value, primary.as_str()))
        .collect();
    expected.extend([
        (
            DiagnosticCode::DeferredMember,
            "door side=front at=center",
            DOOR_HAS_NO_WALL,
        ),
        (
            DiagnosticCode::DeferredMember,
            "window side=back y=1 offset=1 size=1x1 mat_slot=g",
            WINDOW_HAS_NO_WALL,
        ),
        (
            DiagnosticCode::DeferredMember,
            "roof kind=gable mat_slot=r overhang=1",
            "`gable` roof's `mat_slot=` did not resolve to a block id under theme `t`; it falls \
             back to `minecraft:spruce_stairs`",
        ),
        (
            DiagnosticCode::DeferredMember,
            "stair kind=stairs side=left y=0 mat_slot=e",
            "eave `stair`'s `mat_slot=` did not resolve to a block id under theme `t`; it falls \
             back to `minecraft:spruce_stairs`",
        ),
    ]);
    assert_eq!(findings(&src, &out), expected);
    assert_eq!(only_structure(&out), only_structure(&lowered(BODY)));
}

#[test]
fn a_window_whose_slot_binds_no_material_leaves_its_wall_standing() {
    let src = "theme t:\n  slot w -> @cobblestone\n  slot g -> 1\n  slot h -> @glass\n\n\
               struct s size=5x5\n  \
               walls mat_slot=w height=3\n  \
               window side=back y=1 offset=1 size=1x1 mat_slot=g\n  \
               window side=back y=1 offset=3 size=1x1 mat_slot=h\n";
    let out = lowered_with_resolver_diagnostics(src);
    assert_eq!(
        findings(src, &out),
        [(
            DiagnosticCode::UnknownSlotTarget,
            "1",
            "slot `g` in theme `t` does not bind to a canonical or abstract material token",
        )],
    );
    let ba = only_structure(&out);
    assert_eq!(state_at(ba, 3, 1, 0).id, "minecraft:cobblestone");
    assert_eq!(state_at(ba, 1, 1, 0).id, "minecraft:glass");
}
