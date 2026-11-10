//! `E_STATE_DOMAIN`: a property or value of a block state its block does not take at the pinned
//! target (`spec/versioning-editions` "A state is checked against its block's states at the
//! target").

use cairn_lang_core::block_array::{
    BlockArrayIr, BlockIdSet, BlockState, DomainFault, OutOfDomain, StateDomain, TargetRegistry,
    lower_to_block_array,
};
use cairn_lang_core::{DiagnosticCode, lower, parse, resolve};
use serde_json::json;

use crate::common::{duo_with, findings, lowered, only_structure, solid_cells};

/// A block's states as a test table lists them.
type Table = &'static [(
    &'static str,
    &'static [(&'static str, &'static [&'static str])],
)];

/// The states each block takes at the pinned target; a block missing here is unrecorded.
const PINNED: Table = &[
    (
        "minecraft:chest",
        &[
            ("facing", &["north", "south", "west", "east"]),
            ("type", &["single", "left", "right"]),
            ("waterlogged", &["true", "false"]),
        ],
    ),
    ("minecraft:cobblestone", &[]),
    (
        "minecraft:creaking_heart",
        &[
            ("axis", &["x", "y", "z"]),
            ("creaking_heart_state", &["uprooted", "dormant", "awake"]),
            ("natural", &["true", "false"]),
        ],
    ),
    (
        "minecraft:oak_door",
        &[
            ("facing", &["north", "south", "west", "east"]),
            ("half", &["upper", "lower"]),
            ("hinge", &["left", "right"]),
            ("open", &["true", "false"]),
            ("powered", &["true", "false"]),
        ],
    ),
    ("minecraft:oak_log", &[("axis", &["x", "y", "z"])]),
    ("minecraft:oak_planks", &[]),
    (
        "minecraft:oak_stairs",
        &[
            ("facing", &["north", "south", "west", "east"]),
            ("half", &["top", "bottom"]),
            ("waterlogged", &["true", "false"]),
        ],
    ),
    (
        "minecraft:spruce_stairs",
        &[
            ("facing", &["north", "south", "west", "east"]),
            ("half", &["top", "bottom"]),
        ],
    ),
];

/// The edition's other version, which the pinned one dropped `active` and a stair's `shape` from.
const OLDER: Table = &[
    (
        "minecraft:creaking_heart",
        &[
            ("active", &["true", "false"]),
            ("axis", &["x", "y", "z"]),
            ("natural", &["true", "false"]),
        ],
    ),
    (
        "minecraft:spruce_stairs",
        &[
            ("facing", &["north", "south", "west", "east"]),
            ("half", &["top", "bottom"]),
            ("shape", &["straight"]),
        ],
    ),
];

/// A target whose tables are [`PINNED`] and [`OLDER`], or no version at all.
struct Tables {
    ids: Vec<String>,
    pinned: bool,
}

impl Tables {
    fn pinned() -> Self {
        let mut ids: Vec<String> = PINNED.iter().map(|(id, _)| (*id).to_owned()).collect();
        ids.push("minecraft:gravel".to_owned());
        ids.sort();
        Self { ids, pinned: true }
    }
}

fn domain(table: Table, id: &str) -> Option<&'static [(&'static str, &'static [&'static str])]> {
    table
        .iter()
        .find(|(listed, _)| *listed == id)
        .map(|(_, states)| *states)
}

impl TargetRegistry for Tables {
    fn lookup(&self, _token: &str) -> Option<BlockState> {
        None
    }

    fn known_tokens(&self) -> Vec<String> {
        Vec::new()
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        self.pinned.then(|| BlockIdSet::new("test 2.0", &self.ids))
    }

    fn state_domain(&self, state: &BlockState) -> StateDomain {
        if !self.pinned {
            return StateDomain::Unpinned;
        }
        let Some(states) = domain(PINNED, &state.id) else {
            return StateDomain::Unrecorded;
        };
        let owned = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect();
        let outside = state.properties.iter().filter_map(|(key, value)| {
            let fault = match states.iter().find(|(property, _)| property == key) {
                None => DomainFault::UnknownProperty {
                    properties: owned(&states.iter().map(|(p, _)| *p).collect::<Vec<_>>()),
                },
                Some((_, values)) if !values.contains(&value.as_str()) => {
                    DomainFault::ValueOutside {
                        values: owned(values),
                    }
                }
                Some(_) => return None,
            };
            let older = domain(OLDER, &state.id).is_some_and(|older| {
                older
                    .iter()
                    .any(|(property, values)| property == key && values.contains(&value.as_str()))
            });
            Some(OutOfDomain {
                key: key.clone(),
                value: value.clone(),
                fault,
                valid_at: if older {
                    vec!["1.0".to_owned()]
                } else {
                    Vec::new()
                },
            })
        });
        StateDomain::Checked(outside.collect())
    }
}

fn lowered_at(source: &str, registry: &Tables) -> BlockArrayIr {
    let module = parse(source).unwrap_or_else(|e| panic!("parse failed: {e}\nsource:\n{source}"));
    let ir = lower(&module);
    lower_to_block_array(&ir, &resolve(&ir, None), Some(registry))
}

fn floored(target: &str) -> String {
    format!("theme t:\n  slot f -> {target}\n\nstruct s size=3x3\n  floor mat_slot=f\n")
}

/// Whether any structure's palette holds `state`, as the palette prints it.
fn painted(out: &BlockArrayIr, state: &str) -> bool {
    out.structures.values().any(|array| {
        array.palette.entries.iter().any(|entry| {
            let printed = if entry.properties.is_empty() {
                entry.id.clone()
            } else {
                format!("{}[{}]", entry.id, entry.properties_text())
            };
            printed == state
        })
    })
}

#[test]
fn a_property_the_block_lacks_is_refused_at_the_literal() {
    let source = floored("@oak_log[axs=x]");
    let out = lowered_at(&source, &Tables::pinned());
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::StateDomain,
            "@oak_log[axs=x]",
            "`minecraft:oak_log` has no state `axs` at test 2.0; its states are axis",
            vec![
                "did you mean `axis=x`?",
                "remove `axs` from the state literal, or correct it to a state test 2.0 takes",
            ],
        )],
    );
    // `spec/lint` "Machine-readable payload".
    assert_eq!(
        serde_json::to_value(&out.diagnostics[0].data).expect("the payload serialises"),
        json!({
            "kind": "state_domain",
            "id": "minecraft:oak_log",
            "key": "axs",
            "value": "x",
            "registry": "test 2.0",
            "reason": "unknown_property",
            "valid": ["axis"],
            "origin": "literal",
            "suggestion": "axis",
        }),
    );
    assert_eq!(
        solid_cells(only_structure(&out)),
        0,
        "the floor is not built"
    );
}

#[test]
fn a_property_on_a_block_with_no_states_is_only_removed() {
    let source = floored("@oak_planks[axis=x]");
    let out = lowered_at(&source, &Tables::pinned());
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::StateDomain,
            "@oak_planks[axis=x]",
            "`minecraft:oak_planks` has no state `axis` at test 2.0; it takes no state",
            vec!["remove `axis` from the state literal"],
        )],
    );
    assert_eq!(
        solid_cells(only_structure(&out)),
        0,
        "the floor is not built"
    );
}

/// The findings of `source`, which binds `@oak_stairs[facing=up]` once, when the literal is all
/// it reports.
fn the_stair_literal_alone(source: &str, out: &BlockArrayIr) {
    assert_eq!(
        findings(source, out),
        [(
            DiagnosticCode::StateDomain,
            "@oak_stairs[facing=up]",
            "`minecraft:oak_stairs` takes no `facing=up` at test 2.0; `facing` takes north, \
             south, west, east",
            vec![
                "correct the state literal to a value test 2.0 takes, or leave `facing` out for \
                 the block's default",
            ],
        )],
    );
}

#[test]
fn a_roof_whose_material_is_outside_its_block_is_not_built() {
    let source = "theme t:\n  slot r -> @oak_stairs[facing=up]\n\nstruct s size=3x3\n  roof kind=gable mat_slot=r\n";
    let out = lowered_at(source, &Tables::pinned());
    the_stair_literal_alone(source, &out);
    assert_eq!(solid_cells(only_structure(&out)), 0, "no roof falls back");
}

#[test]
fn an_eave_stair_whose_material_is_outside_its_block_is_not_built() {
    let roofed = "theme t:\n  slot wall -> @cobblestone\n  slot eave -> @oak_stairs[facing=up]\n\n\
                  struct s size=3x1\n  walls mat_slot=wall height=1\n  \
                  roof kind=flat mat_slot=wall overhang=1\n";
    let source = format!(
        "{roofed}  stair id=eave kind=stairs mat_slot=eave side=front half=top facing=out\n"
    );
    let out = lowered_at(&source, &Tables::pinned());
    the_stair_literal_alone(&source, &out);
    assert_eq!(
        only_structure(&out),
        only_structure(&lowered_at(roofed, &Tables::pinned())),
        "no eave band falls back",
    );
}

#[test]
fn a_pressure_plate_whose_material_is_outside_its_block_is_not_placed() {
    let plated = |state: &str| {
        format!(
            "theme t:\n  slot p -> {state}\n\nstruct s size=3x3\n  \
             pressure_plate at=inside.front offset=1 mat_slot=p\n"
        )
    };
    let source = plated("@oak_log[axis=q]");
    let out = lowered_at(&source, &Tables::pinned());
    let found: Vec<(DiagnosticCode, &str)> = findings(&source, &out)
        .into_iter()
        .map(|(code, at, ..)| (code, at))
        .collect();
    assert_eq!(found, [(DiagnosticCode::StateDomain, "@oak_log[axis=q]")]);
    assert_eq!(solid_cells(only_structure(&out)), 0, "no default plate");

    // A literal the target takes still defers, since a plate's states are its own.
    let source = plated("@oak_log[axis=x]");
    let out = lowered_at(&source, &Tables::pinned());
    let found: Vec<(DiagnosticCode, &str)> = findings(&source, &out)
        .into_iter()
        .map(|(code, at, ..)| (code, at))
        .collect();
    assert_eq!(
        found,
        [(
            DiagnosticCode::DeferredMember,
            "pressure_plate at=inside.front offset=1 mat_slot=p"
        )]
    );
    assert_eq!(solid_cells(only_structure(&out)), 0);
}

#[test]
fn a_door_whose_material_is_outside_its_block_is_not_set_in_the_wall() {
    let walled = "theme t:\n  slot wall -> @cobblestone\n  slot d -> @oak_door[hinge=up]\n\n\
                  struct s size=3x5\n  walls mat_slot=wall height=3\n";
    let source = format!("{walled}  door side=front at=center mat_slot=d\n");
    let out = lowered_at(&source, &Tables::pinned());
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::StateDomain,
            "@oak_door[hinge=up]",
            "`minecraft:oak_door` takes no `hinge=up` at test 2.0; `hinge` takes left, right",
            vec![
                "correct the state literal to a value test 2.0 takes, or leave `hinge` out for \
                 the block's default",
            ],
        )],
    );
    assert_eq!(
        only_structure(&out),
        only_structure(&lowered_at(walled, &Tables::pinned())),
        "no default door stands in the wall",
    );
}

#[test]
fn a_chest_whose_material_is_outside_its_block_is_not_set_against_the_wall() {
    let walled = "theme t:\n  slot wall -> @cobblestone\n  slot c -> @chest[type=double]\n\n\
                  struct s size=5x5\n  walls mat_slot=wall height=3\n";
    let source = format!("{walled}  chest at=inside.back offset=2 y=1 mat_slot=c\n");
    let out = lowered_at(&source, &Tables::pinned());
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::StateDomain,
            "@chest[type=double]",
            "`minecraft:chest` takes no `type=double` at test 2.0; `type` takes single, left, right",
            vec![
                "correct the state literal to a value test 2.0 takes, or leave `type` out for \
                 the block's default",
            ],
        )],
    );
    assert_eq!(
        only_structure(&out),
        only_structure(&lowered_at(walled, &Tables::pinned())),
        "no default chest stands against the wall",
    );
}

#[test]
fn a_value_outside_the_property_names_the_values_it_takes() {
    let source = floored("@oak_log[axis=q]");
    let out = lowered_at(&source, &Tables::pinned());
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::StateDomain,
            "@oak_log[axis=q]",
            "`minecraft:oak_log` takes no `axis=q` at test 2.0; `axis` takes x, y, z",
            vec![
                "did you mean `axis=x`?",
                "correct the state literal to a value test 2.0 takes, or leave `axis` out for \
                 the block's default",
            ],
        )],
    );
    assert_eq!(
        serde_json::to_value(&out.diagnostics[0].data).expect("the payload serialises"),
        json!({
            "kind": "state_domain",
            "id": "minecraft:oak_log",
            "key": "axis",
            "value": "q",
            "registry": "test 2.0",
            "reason": "value_outside",
            "valid": ["x", "y", "z"],
            "origin": "literal",
            "suggestion": "x",
        }),
    );
}

#[test]
fn a_pair_another_version_takes_offers_that_version_as_the_target() {
    let source = floored("@creaking_heart[active=true]");
    let out = lowered_at(&source, &Tables::pinned());
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::StateDomain,
            "@creaking_heart[active=true]",
            "`minecraft:creaking_heart` has no state `active` at test 2.0; its states are axis, \
             creaking_heart_state, natural",
            vec![
                "`active=true` is valid at 1.0: pass that version as `--target`, or correct the \
                 state literal to a state test 2.0 takes",
            ],
        )],
    );
    assert_eq!(
        serde_json::to_value(&out.diagnostics[0].data).expect("the payload serialises"),
        json!({
            "kind": "state_domain",
            "id": "minecraft:creaking_heart",
            "key": "active",
            "value": "true",
            "registry": "test 2.0",
            "reason": "unknown_property",
            "valid": ["axis", "creaking_heart_state", "natural"],
            "origin": "literal",
            "valid_at": ["1.0"],
        }),
    );
}

#[test]
fn each_pair_outside_is_its_own_finding() {
    let source = floored("@oak_log[lit=true,axis=q]");
    let out = lowered_at(&source, &Tables::pinned());
    let found: Vec<(DiagnosticCode, &str, &str)> = findings(&source, &out)
        .into_iter()
        .map(|(code, at, primary, _)| (code, at, primary))
        .collect();
    assert_eq!(
        found,
        [
            (
                DiagnosticCode::StateDomain,
                "@oak_log[lit=true,axis=q]",
                "`minecraft:oak_log` takes no `axis=q` at test 2.0; `axis` takes x, y, z",
            ),
            (
                DiagnosticCode::StateDomain,
                "@oak_log[lit=true,axis=q]",
                "`minecraft:oak_log` has no state `lit` at test 2.0; its states are axis",
            ),
        ],
    );
}

#[test]
fn a_walkway_whose_path_is_outside_its_block_is_not_laid() {
    let duo = |axis: &str| {
        duo_with(&format!(
            "connect anchor.entry to peer.entry path=@oak_log[axis={axis}]"
        ))
    };
    let source = duo("q");
    let out = lowered_at(&source, &Tables::pinned());
    let found: Vec<(DiagnosticCode, &str)> = findings(&source, &out)
        .into_iter()
        .map(|(code, at, ..)| (code, at))
        .collect();
    assert_eq!(found, [(DiagnosticCode::StateDomain, "@oak_log[axis=q]")]);
    assert!(out.walkways.is_empty(), "{:?}", out.walkways.keys());

    let laid = lowered_at(&duo("z"), &Tables::pinned());
    assert_eq!(laid.walkways.len(), 1, "{:?}", laid.diagnostics);
    assert_eq!(laid.diagnostics, []);
}

#[test]
fn a_state_lowering_derives_outside_its_block_is_refused_at_the_member() {
    let source = "theme t:\n  slot wall -> @cobblestone\n  slot eave -> @oak_stairs\n\n\
                  struct s size=3x1\n  walls mat_slot=wall height=1\n  \
                  roof kind=flat mat_slot=wall overhang=1\n  \
                  stair id=eave kind=stairs mat_slot=eave side=front half=top facing=out\n";
    let out = lowered_at(source, &Tables::pinned());
    assert_eq!(
        findings(source, &out),
        [(
            DiagnosticCode::StateDomain,
            "stair id=eave kind=stairs mat_slot=eave side=front half=top facing=out",
            "`minecraft:oak_stairs` has no state `shape` at test 2.0; its states are facing, \
             half, waterlogged",
            vec![
                "lowering derived this state for the member, so Cairn's derivation or the \
                 registry pack's table for the block is wrong; bind the member's material to \
                 another block",
            ],
        )],
    );
    assert_eq!(
        serde_json::to_value(&out.diagnostics[0].data).expect("the payload serialises"),
        json!({
            "kind": "state_domain",
            "id": "minecraft:oak_stairs",
            "key": "shape",
            "value": "straight",
            "registry": "test 2.0",
            "reason": "unknown_property",
            "valid": ["facing", "half", "waterlogged"],
            "origin": "derived",
        }),
    );
}

#[test]
fn a_derived_pair_another_version_takes_names_it_and_offers_no_literal() {
    let source = "theme t:\n  slot wall -> @cobblestone\n  slot eave -> @spruce_stairs\n\n\
                  struct s size=3x1\n  walls mat_slot=wall height=1\n  \
                  roof kind=flat mat_slot=wall overhang=1\n  \
                  stair id=eave kind=stairs mat_slot=eave side=front half=top facing=out\n";
    let out = lowered_at(source, &Tables::pinned());
    assert_eq!(
        findings(source, &out),
        [(
            DiagnosticCode::StateDomain,
            "stair id=eave kind=stairs mat_slot=eave side=front half=top facing=out",
            "`minecraft:spruce_stairs` has no state `shape` at test 2.0; its states are facing, \
             half",
            vec![
                "`shape=straight` is valid at 1.0, but lowering derived this state for the \
                 member, so Cairn's derivation or the registry pack's table for the block is \
                 wrong; bind the member's material to another block",
            ],
        )],
    );
}

#[test]
fn a_walkway_whose_path_the_target_records_no_states_for_is_laid_unchecked() {
    let source = duo_with("connect anchor.entry to peer.entry path=@gravel[waterlogged=true]");
    let out = lowered_at(&source, &Tables::pinned());
    let found: Vec<(DiagnosticCode, &str)> = findings(&source, &out)
        .into_iter()
        .map(|(code, at, ..)| (code, at))
        .collect();
    assert_eq!(
        found,
        [(
            DiagnosticCode::StateLiteralUnchecked,
            "@gravel[waterlogged=true]"
        )]
    );
    assert_eq!(out.walkways.len(), 1, "{:?}", out.walkways.keys());
}

#[test]
fn a_block_the_target_records_no_states_for_is_written_unchecked() {
    let source = floored("@gravel[waterlogged=true]");
    let out = lowered_at(&source, &Tables::pinned());
    assert_eq!(
        findings(&source, &out),
        [(
            DiagnosticCode::StateLiteralUnchecked,
            "@gravel[waterlogged=true]",
            "`minecraft:gravel` is written with `waterlogged=true` unchecked: test 2.0's \
             registry pack records no states for the block",
            vec![
                "a property the block does not have, or a value outside its domain, reaches the \
                 structure file unchanged; check each one against the block's states in the \
                 target edition and version",
            ],
        )],
    );
    assert!(painted(&out, "minecraft:gravel[waterlogged=true]"));
}

#[test]
fn a_lowering_with_no_version_pinned_leaves_the_literal_to_one_that_pins() {
    let source = floored("@oak_log[axis=q]");
    let unpinned = Tables {
        pinned: false,
        ..Tables::pinned()
    };
    for out in [lowered(&source), lowered_at(&source, &unpinned)] {
        assert_eq!(out.diagnostics, []);
        assert!(painted(&out, "minecraft:oak_log[axis=q]"));
    }
}

#[test]
fn a_raw_blocks_id_and_states_are_checked_as_any_blocks_are() {
    // `spec/materials-themes` "Raw blocks".
    let source = "struct s size=3x3\n  raw_block mat=minecraft:oak_log[axs=x] at=1,0,1\n  \
                  raw_fill mat=minecraft:stonee from=0,0,0 to=0,0,2\n  \
                  raw_block mat=minecraft:oak_log[axis=z] at=2,0,1\n";
    let out = lowered_at(source, &Tables::pinned());
    assert_eq!(
        findings(source, &out),
        [
            (
                DiagnosticCode::StateDomain,
                "minecraft:oak_log[axs=x]",
                "`minecraft:oak_log` has no state `axs` at test 2.0; its states are axis",
                vec![
                    "did you mean `axis=x`?",
                    "remove `axs` from the state literal, or correct it to a state test 2.0 takes",
                ],
            ),
            (
                DiagnosticCode::UnknownId,
                "minecraft:stonee",
                "`minecraft:stonee` is not a block in `test 2.0`",
                vec![
                    "no block in `test 2.0` is near enough to suggest; compile against a target \
                     that declares `minecraft:stonee`, or pick a block this one has",
                ],
            ),
        ],
    );
    assert!(painted(&out, "minecraft:oak_log[axis=z]"));
    assert!(!painted(&out, "minecraft:oak_log[axs=x]"));
}

#[test]
fn a_raw_member_whose_cells_wait_is_still_checked_against_the_pinned_target() {
    // `spec/materials-themes` "Raw blocks": no theme binds the walls, so the rows above 0 wait.
    let source = "struct s size=3x3\n  walls height=3 mat_slot=w\n  \
                  raw_block mat=minecraft:oak_log[axs=x] at=1,1,1\n  \
                  raw_fill mat=minecraft:stonee from=0,1,0 to=0,1,2\n";
    let out = lowered_at(source, &Tables::pinned());
    let mut found = findings(source, &out);
    assert_eq!(found.remove(0).0, DiagnosticCode::NoThemeBound);
    assert_eq!(
        found,
        [
            (
                DiagnosticCode::DeferredMember,
                "raw_block mat=minecraft:oak_log[axs=x] at=1,1,1",
                "raw_block `at=1,1,1` does not fit in the struct (dims.y=1)",
                vec![
                    "a `walls` member that would reach this row paints nothing; the block is \
                     placed once that member's material resolves to a block",
                ],
            ),
            (
                DiagnosticCode::StateDomain,
                "minecraft:oak_log[axs=x]",
                "`minecraft:oak_log` has no state `axs` at test 2.0; its states are axis",
                vec![
                    "did you mean `axis=x`?",
                    "remove `axs` from the state literal, or correct it to a state test 2.0 takes",
                ],
            ),
            (
                DiagnosticCode::DeferredMember,
                "raw_fill mat=minecraft:stonee from=0,1,0 to=0,1,2",
                "raw_fill `to=0,1,2` does not fit in the struct (dims.y=1)",
                vec![
                    "a `walls` member that would reach these rows paints nothing; the box is \
                     filled once that member's material resolves to a block",
                ],
            ),
            (
                DiagnosticCode::UnknownId,
                "minecraft:stonee",
                "`minecraft:stonee` is not a block in `test 2.0`",
                vec![
                    "no block in `test 2.0` is near enough to suggest; compile against a target \
                     that declares `minecraft:stonee`, or pick a block this one has",
                ],
            ),
        ],
    );
}
