//! A selector row's binding the pinned target cannot build earns the finding a slot's value would,
//! at the binding, and its member is built without the part (`spec/materials-themes` "Selector
//! bindings").

use cairn_lang_core::block_array::{
    AbsentConcept, BlockArray, BlockIdSet, BlockState, DomainFault, OutOfDomain, StateDomain,
    TargetRegistry, UnmappedState,
};
use cairn_lang_core::{DiagnosticCode, Edition};

use crate::common::{findings, lowered_with, only_structure, state_at};

/// A pinned `test 1.0`: its catalog maps `@wood.frame` onto spruce wood, it lacks Bedrock's border
/// block, its state map has no form for a log lying along `x`, and it records the states of a
/// spruce log alone.
struct Target {
    ids: Vec<String>,
}

impl Target {
    fn new() -> Self {
        let mut ids: Vec<String> = ["cobblestone", "spruce_log", "spruce_wood"]
            .iter()
            .map(|id| format!("minecraft:{id}"))
            .collect();
        ids.sort();
        Self { ids }
    }
}

impl TargetRegistry for Target {
    fn lookup(&self, token: &str) -> Option<BlockState> {
        (token == "wood.frame").then(|| BlockState::bare("minecraft:spruce_wood"))
    }

    fn known_tokens(&self) -> Vec<String> {
        vec!["wood.frame".to_owned()]
    }

    fn block_ids(&self) -> Option<BlockIdSet<'_>> {
        Some(BlockIdSet::new("test 1.0", &self.ids))
    }

    fn absent_concept(&self, id: &str) -> Option<AbsentConcept> {
        (id == "minecraft:border_block").then(|| AbsentConcept {
            declared_by: Edition::Bedrock,
            reason: "Bedrock's border block; Java has no such block".to_owned(),
        })
    }

    fn unmapped_state(&self, state: &BlockState) -> Option<UnmappedState> {
        state
            .properties
            .get("axis")
            .is_some_and(|axis| axis == "x")
            .then(|| UnmappedState {
                registry: "test 1.0".to_owned(),
                reason: "its map has no sideways log".to_owned(),
                mapped_at: Vec::new(),
            })
    }

    fn state_domain(&self, state: &BlockState) -> StateDomain {
        if state.id != "minecraft:spruce_log" {
            return StateDomain::Unrecorded;
        }
        let axes = ["x", "y", "z"];
        StateDomain::Checked(
            state
                .properties
                .iter()
                .filter(|(key, value)| *key != "axis" || !axes.contains(&value.as_str()))
                .map(|(key, value)| OutOfDomain {
                    key: key.clone(),
                    value: value.clone(),
                    fault: DomainFault::ValueOutside {
                        values: axes.map(str::to_owned).to_vec(),
                    },
                    valid_at: Vec::new(),
                })
                .collect(),
        )
    }
}

/// A 3x3 struct of one-row walls whose corners `binding` trims.
fn trimmed(binding: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n  walls[class=outer] -> trim={binding}\n\n\
         struct s size=3x3\n  walls class=outer mat_slot=wall height=1\n"
    )
}

/// A 5x5 struct whose front window, cut to air, `binding` frames.
fn framed(binding: &str) -> String {
    format!(
        "theme t:\n  slot wall -> @cobblestone\n  window[class=small] -> frame={binding}\n\n\
         struct s size=5x5\n  walls mat_slot=wall height=3\n  \
         window class=small side=front offset=2 y=2 size=1x1\n"
    )
}

/// The ids along row 1 of the struct's front wall (`+z`), from low `x`.
fn front_row(array: &BlockArray, y: u32) -> Vec<&str> {
    (0..array.dims.x)
        .map(|x| state_at(array, x, y, array.dims.z - 1).id.as_str())
        .collect()
}

const COBBLE: &str = "minecraft:cobblestone";
const LOG: &str = "minecraft:spruce_log";
const WOOD: &str = "minecraft:spruce_wood";
const AIR: &str = "minecraft:air";

#[test]
fn a_binding_whose_state_its_block_does_not_take_lays_no_part() {
    let src = trimmed("@spruce_log[axis=q]");
    let out = lowered_with(&src, &Target::new());
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::StateDomain,
            "@spruce_log[axis=q]",
            "`minecraft:spruce_log` takes no `axis=q` at test 1.0; `axis` takes x, y, z",
            vec![
                "did you mean `axis=x`?",
                "correct the state literal to a value test 1.0 takes, or leave `axis` out for the \
                 block's default",
            ],
        )],
    );
    assert_eq!(front_row(only_structure(&out), 1), [COBBLE; 3]);
}

#[test]
fn a_binding_whose_states_the_target_records_none_of_is_laid_unchecked() {
    let src = framed("@spruce_wood[axis=y]");
    let out = lowered_with(&src, &Target::new());
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::StateLiteralUnchecked,
            "@spruce_wood[axis=y]",
            "`minecraft:spruce_wood` is written with `axis=y` unchecked: test 1.0's registry pack \
             records no states for the block",
            vec![
                "a property the block does not have, or a value outside its domain, reaches the \
                 structure file unchanged; check each one against the block's states in the \
                 target edition and version",
            ],
        )],
    );
    assert_eq!(
        front_row(only_structure(&out), 2),
        [COBBLE, WOOD, AIR, WOOD, COBBLE]
    );
}

#[test]
fn a_binding_the_target_has_no_form_for_is_refused_at_the_binding() {
    let src = trimmed("@spruce_log[axis=x]");
    let out = lowered_with(&src, &Target::new());
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::StateUnmapped,
            "@spruce_log[axis=x]",
            "`minecraft:spruce_log[axis=x]` has no form at test 1.0: its map has no sideways log",
            vec!["bind `trim=` to a block test 1.0 has a form for, or build for the other edition"],
        )],
        "reported once, at the binding, and not on the walls' line",
    );
    assert_eq!(front_row(only_structure(&out), 1), [COBBLE; 3]);
    let src = trimmed("@spruce_log[axis=y]");
    let out = lowered_with(&src, &Target::new());
    assert_eq!(findings(&src, &out), []);
    assert_eq!(front_row(only_structure(&out), 1), [LOG, COBBLE, LOG]);
}

#[test]
fn a_binding_to_a_block_only_the_other_edition_has_is_a_parity_finding() {
    let src = framed("@border_block");
    let out = lowered_with(&src, &Target::new());
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::ParityUnsupported,
            "@border_block",
            "`minecraft:border_block` is a block only Bedrock has, so `test 1.0` cannot build it",
            vec![
                "Bedrock's border block; Java has no such block",
                "the semantic layer is edition-neutral: write this `frame=` row in a `theme \
                 NAME_bedrock` and give the same row in `theme NAME_java` a block Java has, or \
                 bind a block both editions have",
            ],
        )],
    );
    assert_eq!(
        front_row(only_structure(&out), 2),
        [COBBLE, COBBLE, AIR, COBBLE, COBBLE]
    );
}

#[test]
fn a_binding_the_catalog_does_not_declare_names_the_nearest_token() {
    let src = framed("@wood.fram");
    let out = lowered_with(&src, &Target::new());
    assert_eq!(
        findings(&src, &out),
        [(
            DiagnosticCode::UnknownAbstractToken,
            "@wood.fram",
            "abstract token `@wood.fram` is not declared by the registry pack's materials catalog",
            vec![
                "did you mean `@wood.frame`?",
                "abstract material tokens must be declared in the pack's `materials` catalog (see \
                 `spec/materials-themes` \"Canonical vocabulary\")",
            ],
        )],
    );
    assert_eq!(
        front_row(only_structure(&out), 2),
        [COBBLE, COBBLE, AIR, COBBLE, COBBLE]
    );
    let src = framed("@wood.frame");
    let out = lowered_with(&src, &Target::new());
    assert_eq!(findings(&src, &out), []);
    assert_eq!(
        front_row(only_structure(&out), 2),
        [COBBLE, WOOD, AIR, WOOD, COBBLE],
        "the catalog's token resolves to its block",
    );
}
