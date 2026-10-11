//! Java blockstate properties to Bedrock `states`, for the stair family only
//! (`spec/versioning-editions` "Java / Bedrock portability"). A non-`straight` stair `shape` has
//! no Bedrock state and drops with a degradation note; another block with properties is refused.

use cairn_lang_core::block_array::{BlockState, is_stair};
use cairn_lang_core::resolve::DroppedIntent;
use cairn_lang_nbt::Compound;
use cairn_lang_nbt::tag::Tag;
use indexmap::IndexMap;
use thiserror::Error;

/// One palette entry's Bedrock `states`, and the intents it had to drop.
#[derive(Debug, Clone, PartialEq)]
pub struct StateTranslation {
    /// The typed Bedrock states, empty for a bare block.
    pub states: Compound,
    /// One entry per intent Bedrock cannot express; [`degradation_detail`] words it.
    pub degraded: Vec<DroppedIntent>,
}

/// A Java property the Bedrock backend cannot map.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BedrockStateError {
    /// A block outside the mapped families carries properties.
    #[error(
        "block `{id}[{properties}]` carries blockstate properties the Bedrock backend cannot map \
         (only {mapped} is mapped so far). Valid: a bare block id (e.g. \
         `minecraft:oak_planks`), or a `*_stairs` block. Fix: bind the member's mat_slot to a \
         property-free material, or compile with `--edition java`"
    )]
    UnmappableBlock {
        /// The block id.
        id: String,
        /// The entry's `key=value` pairs, comma-joined.
        properties: String,
        /// The families this backend maps.
        mapped: &'static str,
    },
    /// A stair property value outside the Java domain, such as `facing=up`.
    #[error(
        "stair `{id}` has `{key}={value}`, which is not a valid Java `{key}`. Valid `{key}`: \
         {valid}. Fix: correct the source blockstate to one of those; `--edition java` would \
         write the same value unchanged, since Java has no such `{key}` either"
    )]
    UnknownStairState {
        /// The block id.
        id: String,
        /// The key, `facing` or `half`.
        key: &'static str,
        /// The value.
        value: String,
        /// The valid values, comma-joined.
        valid: &'static str,
    },
    /// A stair key this backend does not handle, refused so mapping it later changes no output.
    #[error(
        "stair `{id}` carries unknown blockstate `{key}`. Handled: {handled}. Fix: remove it \
         from the source blockstate, or compile with `--edition java`"
    )]
    UnknownStairKey {
        /// The block id.
        id: String,
        /// The key.
        key: String,
        /// The keys this backend handles, comma-joined.
        handled: &'static str,
    },
}

const FACING_VALID: &str = "east, west, south, north";

const MAPPED_FAMILIES: &str = "the stair family";

/// The keys [`translate_stair`] matches, in its order.
const STAIR_KEYS_HANDLED: &str = "facing, half, shape";

/// The Bedrock `states` for a Java palette entry whose id is concrete.
///
/// # Errors
///
/// [`BedrockStateError`] for a non-stair block with properties, or a stair state it cannot map.
pub fn translate_states(state: &BlockState) -> Result<StateTranslation, BedrockStateError> {
    let (id, properties) = (state.id.as_str(), &state.properties);
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
            properties: state.properties_text(),
            mapped: MAPPED_FAMILIES,
        })
    }
}

/// What one dropped intent cost, worded without the entry that carried it.
#[must_use]
pub fn degradation_detail(dropped: &DroppedIntent) -> String {
    match dropped {
        DroppedIntent::Shape { value } => format!(
            "shape={value} has no Bedrock state; Bedrock stairs render straight, so corners show \
             visual gaps"
        ),
    }
}

/// [`degradation_detail`] after the entry `state` that carried the intent, as
/// `W_INTENT_DEGRADED` reports it.
#[must_use]
pub fn degradation_message(state: &str, dropped: &DroppedIntent) -> String {
    let family = match dropped {
        DroppedIntent::Shape { .. } => "stair",
    };
    format!("{family} `{state}` {}", degradation_detail(dropped))
}

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
                // No Bedrock `shape` state; `straight` is its default.
                if value != "straight" {
                    degraded.push(DroppedIntent::Shape {
                        value: value.clone(),
                    });
                }
            }
            other => {
                return Err(BedrockStateError::UnknownStairKey {
                    id: id.to_owned(),
                    key: other.to_owned(),
                    handled: STAIR_KEYS_HANDLED,
                });
            }
        }
    }

    Ok(StateTranslation { states, degraded })
}

/// Java `facing` to `weirdo_direction`, as Bedrock 1.21.60's stair states number them.
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

/// A palette entry as `cairn info` spells it, `id[key=value,...]`, or `id[]` for a bare block.
pub(crate) fn java_state(state: &BlockState) -> String {
    format!("{}[{}]", state.id, state.properties_text())
}

#[cfg(test)]
mod tests;
