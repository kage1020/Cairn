//! Java blockstate properties → Bedrock `states` translation.
//!
//! A Java vanilla structure carries a palette entry's state as
//! `Properties: { facing: "north", half: "top", shape: "outer_left" }` —
//! human-readable string keys shared by every edition's *tooling* but not by
//! Bedrock's on-disk `states` vocabulary. Bedrock names the same intent with
//! its own typed keys (`weirdo_direction: Int`, `upside_down_bit: Byte`), and
//! for some intents (stair `shape`) it has **no** key at all.
//!
//! Per `spec/versioning-editions` "Folding the `(edition, version)` matrix"
//! and "Java / Bedrock portability", Java is the base and Bedrock the
//! overriding diff; `intent_state` is neutral and `resolved_state`
//! per-edition. This module holds the hand-written Bedrock diff for the
//! **stair family**. A block with properties outside a covered family is a
//! hard error rather than a silent pass-through.
//!
//! `shape` has no Bedrock equivalent (the portability section lists "stairs
//! `shape` (no such state on Bedrock)"). A non-`straight` shape is **dropped
//! with a degradation note** (`dropped_states: [shape]` in the matrix
//! section, `W_INTENT_DEGRADED` in the portability one), never silently, as
//! `spec/versioning-editions` "Fail-loud and minimum-version inference"
//! requires. `shape=straight` is the Bedrock default, so it drops without a
//! note.
//!
//! Numeric domains here are pinned against the Bedrock stair block-state
//! listing on `minecraft.wiki` / `wiki.bedrock.dev` (`Stairs/BS`,
//! consulted 2026-07 against Bedrock 1.21.60). The Cairn spec's own
//! portability example uses different `weirdo_direction` values; the
//! wiki listing is authoritative for the on-disk mapping.

use cairn_lang_core::block_array::{BlockState, is_stair};
use cairn_lang_core::resolve::DroppedIntent;
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
    /// One entry per intent Bedrock cannot express (e.g. a dropped
    /// non-`straight` stair `shape`). Empty on a lossless translation.
    ///
    /// Structured rather than a sentence, so a caller can act on the dropped
    /// property; [`degradation_detail`] words it.
    pub degraded: Vec<DroppedIntent>,
}

/// A Java property that the Bedrock backend cannot map. Carries the
/// self-correction triple (what is wrong / what is valid / suggested fix) so
/// the lint loop can act on the message, per `spec/versioning-editions`
/// "Fail-loud and minimum-version inference".
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BedrockStateError {
    /// A block carries blockstate properties but is not a family this backend
    /// knows how to translate. Java writes the properties verbatim; Bedrock
    /// needs an explicit per-edition mapping, which this block lacks.
    #[error(
        "block `{id}[{properties}]` carries blockstate properties the Bedrock backend cannot map \
         (only {mapped} is mapped so far). Valid: a bare block id (e.g. \
         `minecraft:oak_planks`), or a `*_stairs` block. Fix: bind the member's mat_slot to a \
         property-free material, or compile with `--edition java`"
    )]
    UnmappableBlock {
        /// Offending id verbatim.
        id: String,
        /// The entry's `key=value` pairs, comma-joined for the message.
        properties: String,
        /// The block families this backend does map states for.
        mapped: &'static str,
    },
    /// A stair carried a property value outside the Java domain (e.g.
    /// `facing=up`). The registry pack should reject these one layer up; a
    /// leaked one is refused rather than mapped to a wrong Bedrock value.
    #[error(
        "stair `{id}` has `{key}={value}`, which is not a valid Java `{key}`. Valid `{key}`: \
         {valid}. Fix: correct the source blockstate to one of those; `--edition java` would \
         write the same value unchanged, since Java has no such `{key}` either"
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
        "stair `{id}` carries unknown blockstate `{key}`. Handled: {handled}. Fix: remove it \
         from the source blockstate, or compile with `--edition java`"
    )]
    UnknownStairKey {
        /// Offending id verbatim.
        id: String,
        /// The unhandled property key.
        key: String,
        /// Comma-joined keys this backend does handle.
        handled: &'static str,
    },
}

/// Valid Java `facing` values, in the order the message lists them.
const FACING_VALID: &str = "east, west, south, north";

/// Block families [`translate_states`] has a state mapping for.
///
/// Carried on the error rather than spelled in each message, so every
/// message naming the set follows it.
const MAPPED_FAMILIES: &str = "the stair family";

/// Stair property keys [`translate_stair`] reads, in the order its match
/// arms take them. Carried on the error for the same reason as
/// [`MAPPED_FAMILIES`].
const STAIR_KEYS_HANDLED: &str = "facing, half, shape";

/// Translate a Java palette entry's `(id, properties)` into its Bedrock
/// `states` form.
///
/// The `id` must already be a concrete `namespace:identifier` (the backend
/// rejects abstract tokens separately). The whole stair family shares one
/// Bedrock state vocabulary, so the mapping is keyed off the family through
/// `cairn-lang-core`'s [`is_stair`], the predicate core uses to decide
/// whether a roof or eave may attach stair states. A second copy here could
/// disagree with core, with no symptom until a structure file reaches the
/// game.
///
/// # Errors
///
/// - [`BedrockStateError::UnmappableBlock`] — a non-stair block with
///   properties.
/// - [`BedrockStateError::UnknownStairState`] — a stair `facing` / `half`
///   value outside the Java domain.
/// - [`BedrockStateError::UnknownStairKey`] — a stair property key other than
///   `facing` / `half` / `shape`.
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

/// What one dropped intent cost, without naming what carried it.
///
/// The one wording shared by `W_INTENT_DEGRADED` and the `degraded:` note
/// `cairn info` prints. It leaves the entry out because `cairn info` has
/// already printed `id[states]` as its line's subject;
/// [`degradation_message`] adds it for a
/// [`ParityNote`](crate::bedrock_structure::ParityNote), which has to read
/// on its own.
///
/// A `match` rather than a format over the pieces, because this sentence
/// is about stairs: a new [`DroppedIntent`] variant fails to compile here
/// until its own loss is put into words.
#[must_use]
pub fn degradation_detail(dropped: &DroppedIntent) -> String {
    match dropped {
        DroppedIntent::Shape { value } => format!(
            "shape={value} has no Bedrock state; Bedrock stairs render straight, so corners show \
             visual gaps"
        ),
    }
}

/// [`degradation_detail`] with the entry that carried the intent, as
/// `W_INTENT_DEGRADED` reports it.
///
/// `state` names the entry, quoted as given. The `.mcstructure` writer
/// passes the full Java state,
/// `minecraft:spruce_stairs[facing=north,half=bottom,shape=outer_left]`,
/// because two states of one id can drop the same value (`roof-hip`'s north
/// and south corners both drop `shape=outer_left`), and a warning naming
/// the id alone would repeat itself word for word.
///
/// The family word comes from a `match` for the same reason the sentence
/// does: `stair` is true of this variant, not of the type.
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

/// Java `facing` → Bedrock `weirdo_direction`, per the `Stairs/BS` listing
/// the module doc cites.
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

/// Java `half` → Bedrock `upside_down_bit`.
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

/// A Java palette entry named as its state, `id[key=value,...]`: the
/// spelling `cairn info` prints a degraded entry in. A bare block comes out
/// as `id[]`, as `info`'s spelling would give it.
pub(crate) fn java_state(state: &BlockState) -> String {
    format!("{}[{}]", state.id, state.properties_text())
}

#[cfg(test)]
mod tests;
