//! Java blockstate properties → Bedrock `states` translation.
//!
//! A Java vanilla structure carries a palette entry's state as
//! `Properties: { facing: "north", half: "top", shape: "outer_left" }` —
//! human-readable string keys shared by every edition's *tooling* but not by
//! Bedrock's on-disk `states` vocabulary. Bedrock names the same intent with
//! its own typed keys (`weirdo_direction: Int`, `upside_down_bit: Byte`), and
//! for some intents (stair `shape`) it has **no** key at all.
//!
//! Per spec versioning-editions §10.3 ("Java as the base, Bedrock as
//! overriding diffs") and §10.7 (`intent_state` neutral, `resolved_state`
//! per-edition), this module holds the hand-written Bedrock diff. It
//! currently covers the **stair family**; further block families extend
//! the same match dispatch additively as their lowering paths land. Any
//! block with properties outside a covered family is a hard error rather
//! than a silent pass-through.
//!
//! `shape` has no Bedrock equivalent (§10.7: "stairs shape — no state on
//! Bedrock"). A non-`straight` shape is **dropped with a degradation note**
//! (spec §10.3 `dropped_states: [shape]`, §10.7 `W_INTENT_DEGRADED`), never
//! silently (§10.4 forbids implicit dropping). `shape=straight` is the
//! Bedrock default, so it drops without a note.
//!
//! Numeric domains here are pinned against the Bedrock stair block-state
//! listing on `minecraft.wiki` / `wiki.bedrock.dev` (`Stairs/BS`,
//! consulted 2026-07 against Bedrock 1.21.60). The Cairn spec's §10.7
//! illustrative example uses different `weirdo_direction` values; the
//! wiki listing is authoritative for the on-disk mapping.

use cairn_lang_nbt::Compound;
use cairn_lang_nbt::tag::Tag;
use indexmap::IndexMap;
use thiserror::Error;

/// The Bedrock `states` compound for one palette entry, plus any intent that
/// could not be represented and was dropped.
#[derive(Debug, Clone, PartialEq)]
pub struct StateTranslation {
    /// Typed Bedrock `states` (e.g. `weirdo_direction: Int`,
    /// `upside_down_bit: Byte`). Empty for a bare (property-free) block.
    pub states: Compound,
    /// Human-readable degradation notes — one per intent that Bedrock cannot
    /// express (e.g. a dropped non-`straight` stair `shape`). Surfaced by the
    /// caller as `W_INTENT_DEGRADED`. Empty on a lossless translation.
    pub degraded: Vec<String>,
}

/// A Java property that the Bedrock backend cannot map. Carries the
/// self-correction triple (what is wrong / what is valid / suggested fix) so
/// the lint loop can act on the message (spec §10.4).
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BedrockStateError {
    /// A block carries blockstate properties but is not a family this backend
    /// knows how to translate. Java writes the properties verbatim; Bedrock
    /// needs an explicit per-edition mapping, which does not exist for this
    /// block yet.
    #[error(
        "block `{id}[{properties}]` carries blockstate properties the Bedrock backend cannot map \
         (only the stair family is mapped so far). Valid: a bare block id (e.g. \
         `minecraft:oak_planks`), or a `*_stairs` block. Fix: bind the member's mat_slot to a \
         property-free material, or compile with `--edition java`"
    )]
    UnmappableBlock {
        /// Offending id verbatim.
        id: String,
        /// The entry's `key=value` pairs, comma-joined for the message.
        properties: String,
    },
    /// A stair carried a property value outside the Java domain (e.g.
    /// `facing=up`). The registry pack should reject these one layer up; a
    /// leaked one is refused rather than mapped to a wrong Bedrock value.
    #[error(
        "stair `{id}` has `{key}={value}`, which is not a valid Java `{key}`. Valid `{key}`: \
         {valid}. Fix: correct the source blockstate, or compile with `--edition java`"
    )]
    UnknownStairState {
        /// Offending id verbatim.
        id: String,
        /// The offending property key (`facing` / `half`).
        key: &'static str,
        /// The offending value verbatim.
        value: String,
        /// Comma-joined valid values for `key`.
        valid: &'static str,
    },
    /// A stair carried a property key this backend does not handle. Refused
    /// (not ignored) so a future key cannot retroactively change the meaning
    /// of already-shipped output.
    #[error(
        "stair `{id}` carries unknown blockstate `{key}`. Handled: facing, half, shape. Fix: \
         remove it from the source blockstate, or compile with `--edition java`"
    )]
    UnknownStairKey {
        /// Offending id verbatim.
        id: String,
        /// The unhandled property key.
        key: String,
    },
}

/// Valid Java `facing` values, in the order the message lists them.
const FACING_VALID: &str = "east, west, south, north";

/// Translate a Java palette entry's `(id, properties)` into its Bedrock
/// `states` form.
///
/// The `id` must already be a concrete `namespace:identifier` (the backend
/// rejects abstract tokens separately); the family is keyed off the `_stairs`
/// suffix on the identifier path.
///
/// # Errors
///
/// - [`BedrockStateError::UnmappableBlock`] — a non-stair block with
///   properties.
/// - [`BedrockStateError::UnknownStairState`] — a stair `facing` / `half`
///   value outside the Java domain.
/// - [`BedrockStateError::UnknownStairKey`] — a stair property key other than
///   `facing` / `half` / `shape`.
pub fn translate_states(
    id: &str,
    properties: &IndexMap<String, String>,
) -> Result<StateTranslation, BedrockStateError> {
    if properties.is_empty() {
        return Ok(StateTranslation {
            states: Compound::new(),
            degraded: Vec::new(),
        });
    }

    if is_stair(id) {
        translate_stair(id, properties)
    } else {
        Err(BedrockStateError::UnmappableBlock {
            id: id.to_owned(),
            properties: join_properties(properties),
        })
    }
}

/// A block is a stair when its identifier path ends in `_stairs`
/// (`minecraft:oak_stairs`, `minecraft:dark_oak_stairs`, …). The whole stair
/// family shares one Bedrock state vocabulary, so the mapping is keyed off the
/// family suffix rather than each material id.
///
/// The rule itself lives in `cairn-lang-core`, which asks the same question
/// when a roof or eave decides whether it may attach stair states to a
/// material. Two copies could disagree about an id that core paints and this
/// module then has to write, which is a disagreement with no symptom until a
/// structure file reaches the game.
use cairn_lang_core::block_array::is_stair;

fn translate_stair(
    id: &str,
    properties: &IndexMap<String, String>,
) -> Result<StateTranslation, BedrockStateError> {
    let mut states = Compound::new();
    let mut degraded = Vec::new();

    for (key, value) in properties {
        match key.as_str() {
            "facing" => {
                states.insert("weirdo_direction", Tag::Int(weirdo_direction(id, value)?));
            }
            "half" => {
                states.insert("upside_down_bit", Tag::Byte(upside_down_bit(id, value)?));
            }
            "shape" => {
                // Bedrock stairs have no `shape` state (§10.7). `straight`
                // is Bedrock's default so it drops losslessly; any corner
                // shape drops with a degradation note (§10.3/§10.4).
                if value != "straight" {
                    degraded.push(format!(
                        "stair `{id}` shape={value} has no Bedrock state; Bedrock stairs render \
                         straight, so corners show visual gaps"
                    ));
                }
            }
            other => {
                return Err(BedrockStateError::UnknownStairKey {
                    id: id.to_owned(),
                    key: other.to_owned(),
                });
            }
        }
    }

    Ok(StateTranslation { states, degraded })
}

/// Java `facing` → Bedrock `weirdo_direction`. Verified against the Bedrock
/// stair block-state listing (wiki.bedrock.dev / minecraft.wiki `Stairs/BS`):
/// `0 = east, 1 = west, 2 = south, 3 = north`.
fn weirdo_direction(id: &str, facing: &str) -> Result<i32, BedrockStateError> {
    match facing {
        "east" => Ok(0),
        "west" => Ok(1),
        "south" => Ok(2),
        "north" => Ok(3),
        other => Err(BedrockStateError::UnknownStairState {
            id: id.to_owned(),
            key: "facing",
            value: other.to_owned(),
            valid: FACING_VALID,
        }),
    }
}

/// Java `half` → Bedrock `upside_down_bit`. `top` flips the stair
/// (`upside_down_bit = 1`); `bottom` is the default (`0`).
fn upside_down_bit(id: &str, half: &str) -> Result<i8, BedrockStateError> {
    match half {
        "top" => Ok(1),
        "bottom" => Ok(0),
        other => Err(BedrockStateError::UnknownStairState {
            id: id.to_owned(),
            key: "half",
            value: other.to_owned(),
            valid: "top, bottom",
        }),
    }
}

fn join_properties(properties: &IndexMap<String, String>) -> String {
    properties
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn props<const N: usize>(pairs: [(&str, &str); N]) -> IndexMap<String, String> {
        pairs
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    }

    fn stair_props(facing: &str, half: &str, shape: &str) -> IndexMap<String, String> {
        // Preserve the facing / half / shape insertion order the lowering
        // uses so `translate_states` sees the same key stream at runtime.
        props([("facing", facing), ("half", half), ("shape", shape)])
    }

    #[test]
    fn bare_block_has_empty_states_and_no_degradation() {
        let t = translate_states("minecraft:oak_planks", &IndexMap::new()).expect("bare");
        assert!(t.states.entries.is_empty());
        assert!(t.degraded.is_empty());
    }

    #[test]
    fn stair_maps_facing_and_half_straight_is_lossless() {
        let t = translate_states(
            "minecraft:dark_oak_stairs",
            &stair_props("north", "bottom", "straight"),
        )
        .expect("stair");
        assert_eq!(t.states.entries.get("weirdo_direction"), Some(&Tag::Int(3)));
        assert_eq!(t.states.entries.get("upside_down_bit"), Some(&Tag::Byte(0)));
        // `shape=straight` and no other keys leak in.
        assert_eq!(t.states.entries.len(), 2);
        assert!(t.degraded.is_empty());
    }

    #[test]
    fn all_four_facings_map_to_distinct_weirdo_directions() {
        let expected = [("east", 0), ("west", 1), ("south", 2), ("north", 3)];
        for (facing, weirdo) in expected {
            let t = translate_states(
                "minecraft:oak_stairs",
                &stair_props(facing, "bottom", "straight"),
            )
            .expect("stair");
            assert_eq!(
                t.states.entries.get("weirdo_direction"),
                Some(&Tag::Int(weirdo)),
                "facing={facing}"
            );
        }
    }

    #[test]
    fn half_top_sets_upside_down_bit() {
        let t = translate_states(
            "minecraft:spruce_stairs",
            &stair_props("east", "top", "straight"),
        )
        .expect("stair");
        assert_eq!(t.states.entries.get("upside_down_bit"), Some(&Tag::Byte(1)));
    }

    #[test]
    fn non_straight_shape_drops_with_degradation_note() {
        let t = translate_states(
            "minecraft:oak_stairs",
            &stair_props("south", "top", "outer_left"),
        )
        .expect("stair");
        // States carry only the mappable intent.
        assert_eq!(t.states.entries.get("weirdo_direction"), Some(&Tag::Int(2)));
        assert_eq!(t.states.entries.get("upside_down_bit"), Some(&Tag::Byte(1)));
        assert_eq!(t.states.entries.len(), 2);
        // One degradation note, naming the dropped intent.
        assert_eq!(t.degraded.len(), 1);
        let note = &t.degraded[0];
        assert!(note.contains("shape"), "got: {note}");
        assert!(note.contains("Bedrock"), "got: {note}");

        // `straight` produces no note.
        let straight = translate_states(
            "minecraft:oak_stairs",
            &stair_props("south", "top", "straight"),
        )
        .expect("stair");
        assert!(straight.degraded.is_empty());
    }

    #[test]
    fn non_stair_with_properties_fails_loud() {
        let err = translate_states("minecraft:oak_door", &props([("facing", "north")]))
            .expect_err("stateful non-stair");
        assert!(matches!(
            err,
            BedrockStateError::UnmappableBlock { ref id, .. } if id == "minecraft:oak_door"
        ));
        // Self-correction triple (spec §10.4): what is wrong / what is
        // valid / suggested fix. Each fragment is pinned so a message
        // reword that breaks the lint loop's expectation fails here first.
        let msg = err.to_string();
        assert!(msg.contains("minecraft:oak_door"), "wrong: {msg}");
        assert!(msg.contains("facing=north"), "wrong: {msg}");
        assert!(msg.contains("`minecraft:oak_planks`"), "valid: {msg}");
        assert!(msg.contains("`*_stairs`"), "valid: {msg}");
        assert!(msg.contains("--edition java"), "fix: {msg}");
    }

    #[test]
    fn unknown_stair_facing_value_fails_loud() {
        let err = translate_states(
            "minecraft:oak_stairs",
            &stair_props("up", "bottom", "straight"),
        )
        .expect_err("bad facing");
        assert!(matches!(
            err,
            BedrockStateError::UnknownStairState { key: "facing", ref value, .. } if value == "up"
        ));
        let msg = err.to_string();
        assert!(msg.contains("facing=up"), "wrong: {msg}");
        // Valid values (FACING_VALID) surface in the message so the lint
        // loop can steer back to the closed domain instead of guessing.
        assert!(msg.contains("east, west, south, north"), "valid: {msg}");
        assert!(msg.contains("--edition java"), "fix: {msg}");
    }

    #[test]
    fn unknown_stair_half_value_fails_loud() {
        // Mirrors the facing test to guard against a future
        // `upside_down_bit()` refactor that silently maps unknown values to
        // `0` — a silent drop that spec §10.4 forbids.
        let err = translate_states(
            "minecraft:oak_stairs",
            &stair_props("north", "middle", "straight"),
        )
        .expect_err("bad half");
        assert!(matches!(
            err,
            BedrockStateError::UnknownStairState { key: "half", ref value, .. } if value == "middle"
        ));
        let msg = err.to_string();
        assert!(msg.contains("half=middle"), "wrong: {msg}");
        assert!(msg.contains("top, bottom"), "valid: {msg}");
        assert!(msg.contains("--edition java"), "fix: {msg}");
    }

    #[test]
    fn unknown_stair_key_fails_loud() {
        let err = translate_states("minecraft:oak_stairs", &props([("waterlogged", "true")]))
            .expect_err("unknown key");
        assert!(matches!(
            err,
            BedrockStateError::UnknownStairKey { ref key, .. } if key == "waterlogged"
        ));
        let msg = err.to_string();
        assert!(msg.contains("waterlogged"), "wrong: {msg}");
        assert!(msg.contains("Handled: facing, half, shape"), "valid: {msg}");
        assert!(msg.contains("--edition java"), "fix: {msg}");
    }
}
