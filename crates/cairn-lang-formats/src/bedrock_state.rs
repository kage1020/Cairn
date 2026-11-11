//! A palette entry's Bedrock block and `states`, through the registry pack's state map for the
//! target version (`spec/versioning-editions` "Java / Bedrock portability"). The entry's
//! properties are the canonical vocabulary's; a state Bedrock does not carry drops, with a
//! degradation note when that loses what the entry asks for, and one the map has no form for is
//! refused.

use cairn_lang_core::block_array::BlockState;
use cairn_lang_core::resolve::DroppedIntent;
use cairn_lang_nbt::Compound;
use cairn_lang_nbt::tag::Tag;
use thiserror::Error;

use crate::registry::{BedrockForm, DroppedState, StateRefusal, StateValue, VersionStates};

/// One palette entry as Bedrock writes it, and the intents it had to drop.
#[derive(Debug, Clone, PartialEq)]
pub struct StateTranslation {
    /// The Bedrock id, which a state can spell as another block (`oak_slab[type=double]` is
    /// `oak_double_slab`).
    pub id: String,
    /// The typed Bedrock states, empty for an entry written bare.
    pub states: Compound,
    /// Whether the cell also holds water, which the `.mcstructure` carries as its second layer.
    pub waterlogged: bool,
    /// One entry per intent Bedrock cannot express, each worded by its `reason`: a state it does
    /// not carry at a value other than the Java default, or one it works out itself that the
    /// lowering did not derive.
    pub degraded: Vec<DroppedIntent>,
}

/// A palette entry the Bedrock backend cannot write.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum BedrockStateError {
    /// The version's state map has no form for the entry's states.
    #[error(
        "block `{id}[{properties}]` has no form at bedrock {mc_version}: {reason}. Fix: pass a \
         `--target` whose state map carries these states, bind the member's material to another \
         block, or compile with `--edition java`"
    )]
    Unmapped {
        /// The block id.
        id: String,
        /// The entry's `key=value` pairs, comma-joined.
        properties: String,
        /// The version.
        mc_version: String,
        /// Why, as the state map says it.
        reason: String,
    },
    /// A property the Java block does not have.
    #[error(
        "block `{id}` carries blockstate `{key}`, which the Java block does not have. Valid: \
         {valid}. Fix: remove it from the source blockstate; `--edition java` would write it \
         unchanged, since Java has no such state either"
    )]
    UnknownProperty {
        /// The block id.
        id: String,
        /// The property.
        key: String,
        /// The block's properties, comma-joined.
        valid: String,
    },
    /// A value outside the Java property's domain, such as a stair's `facing=up`.
    #[error(
        "block `{id}` has `{key}={value}`, which is not a valid Java `{key}`. Valid `{key}`: \
         {valid}. Fix: correct the source blockstate to one of those; `--edition java` would \
         write the same value unchanged, since Java has no such `{key}` either"
    )]
    UnknownValue {
        /// The block id.
        id: String,
        /// The property.
        key: String,
        /// The value.
        value: String,
        /// The values the property takes, comma-joined.
        valid: String,
    },
}

/// How `state`, whose id is concrete, is written at `map`'s version, `derived` naming the keys the
/// lowering derived for it rather than a member writing them.
///
/// # Errors
///
/// [`BedrockStateError`] when the map has no form for the states, or they are not the block's.
pub fn translate_states(
    map: &VersionStates<'_>,
    state: &BlockState,
    derived: &[String],
) -> Result<StateTranslation, BedrockStateError> {
    let BedrockForm {
        id,
        states,
        waterlogged,
        dropped,
        rederived,
    } = map.resolve(state).map_err(|refusal| match refusal {
        StateRefusal::Unmapped { reason } => BedrockStateError::Unmapped {
            id: state.id.clone(),
            properties: state.properties_text(),
            mc_version: map.mc_version().to_owned(),
            reason,
        },
        StateRefusal::UnknownProperty { key, valid } => BedrockStateError::UnknownProperty {
            id: state.id.clone(),
            key,
            valid: valid.join(", "),
        },
        StateRefusal::UnknownValue { key, value, valid } => BedrockStateError::UnknownValue {
            id: state.id.clone(),
            key,
            value,
            valid: valid.join(", "),
        },
    })?;
    let mut compound = Compound::new();
    for (name, value) in states {
        compound.insert(name, tag(value));
    }
    let written = rederived
        .into_iter()
        .filter(|state| !derived.contains(&state.key))
        .map(|state| DroppedState {
            reason: format!(
                "the written `{}={}` is not kept: {}",
                state.key, state.value, state.reason
            ),
            ..state
        });
    Ok(StateTranslation {
        id,
        states: compound,
        waterlogged,
        degraded: dropped
            .into_iter()
            .chain(written)
            .map(|dropped| DroppedIntent {
                key: dropped.key,
                value: dropped.value,
                mc_versions: vec![map.mc_version().to_owned()],
                reason: dropped.reason,
            })
            .collect(),
    })
}

/// A palette value as its NBT tag: Bedrock's booleans are bytes.
fn tag(value: StateValue) -> Tag {
    match value {
        StateValue::Bool(value) => Tag::Byte(i8::from(value)),
        StateValue::Int(value) => Tag::Int(
            i32::try_from(value).expect("the loader refuses a state map int past `TAG_Int`"),
        ),
        StateValue::Str(value) => Tag::String(value),
    }
}

#[cfg(test)]
mod tests;
